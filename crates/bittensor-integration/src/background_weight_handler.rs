//! Background handler for hourly weight submission.
//!
//! Periodically fetches weights from RPC at the top of each hour
//! and enqueues them for submission via WeightTaskHandle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{Timelike, Utc};
use reqwest::Client;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::weight_task::WeightTaskHandle;

/// RPC endpoint for fetching weights
const WEIGHT_RPC_URL: &str = "https://chain.platform.zip/rpc";

/// Timeout for RPC requests in seconds
const RPC_TIMEOUT_SECS: u64 = 30;

/// Handler for the background weight submission timer.
pub struct BackgroundWeightHandler {
    weight_task_handle: WeightTaskHandle,
    http_client: Client,
    last_submission_hour: Arc<Mutex<Option<i64>>>,
    running: Arc<AtomicBool>,
}

impl BackgroundWeightHandler {
    /// Shut down the background handler gracefully.
    pub async fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!("Background weight handler shutdown requested");
    }
}

/// Spawn the background weight handler with hourly timer.
///
/// Checks every 60 seconds, triggering submission at minute() == 0.
pub fn spawn_background_weight_handler(
    weight_task_handle: WeightTaskHandle,
    http_client: Client,
) -> BackgroundWeightHandler {
    let last_submission_hour = Arc::new(Mutex::new(None::<i64>));
    let running = Arc::new(AtomicBool::new(true));

    let handler = BackgroundWeightHandler {
        weight_task_handle: weight_task_handle.clone(),
        http_client: http_client.clone(),
        last_submission_hour: last_submission_hour.clone(),
        running: running.clone(),
    };

    let handle_clone = weight_task_handle.clone();
    let client_clone = http_client;
    let last_hour_clone = last_submission_hour;
    let running_clone = running.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        info!("Background weight handler started");

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !running_clone.load(Ordering::SeqCst) {
                        break;
                    }

                    let now = Utc::now();

                    // Check if we're at the top of the hour
                    if now.minute() == 0 {
                        let hour_epoch = now.timestamp() / 3600;

                        // Dedup check - only submit once per hour
                        let mut last = last_hour_clone.lock().await;
                        if let Some(last_hour) = *last {
                            if last_hour == hour_epoch {
                                continue; // Already submitted this hour
                            }
                        }

                        // Fetch weights from RPC
                        match fetch_weights_via_http(&client_clone).await {
                            Ok(weights) if !weights.is_empty() => {
                                // Compute epoch from time (like Bittensor)
                                let epoch = (now.timestamp() as u64) / 3600;

                                if let Err(e) = handle_clone.enqueue(epoch, weights).await {
                                    error!("Failed to enqueue weights: {}", e);
                                } else {
                                    *last = Some(hour_epoch);
                                    info!("Enqueued weights for hour {}", hour_epoch);
                                }
                            }
                            Ok(_) => warn!("Empty weights from RPC"),
                            Err(e) => error!("RPC fetch failed: {}", e),
                        }
                    }
                }
            }
        }

        info!("Background weight handler stopped");
    });

    handler
}

/// Fetch pre-computed weights from the primary validator's RPC endpoint.
/// Returns Vec<(mechanism_id, uids, weights)> ready for submission.
pub async fn fetch_weights_via_http(client: &Client) -> Result<Vec<(u8, Vec<u16>, Vec<u16>)>> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "subnet_getWeights",
        "params": {},
        "id": 1
    });

    let response = client
        .post(WEIGHT_RPC_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(RPC_TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("RPC request failed: {}", response.status());
    }

    let json: serde_json::Value = response.json().await?;

    // Parse response
    let weights_arr = json["result"]["weights"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid response: missing weights array"))?;

    let mut result = Vec::new();
    for entry in weights_arr {
        let mechanism_id = entry["mechanismId"].as_u64().unwrap_or(0) as u8;
        let entries = entry["entries"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing entries array"))?;

        let mut uids = Vec::new();
        let mut weights = Vec::new();
        for e in entries {
            uids.push(e["uid"].as_u64().unwrap_or(0) as u16);
            weights.push(e["weight"].as_u64().unwrap_or(0) as u16);
        }

        if !uids.is_empty() {
            result.push((mechanism_id, uids, weights));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert!(!WEIGHT_RPC_URL.is_empty());
        assert_eq!(RPC_TIMEOUT_SECS, 30);
    }
}
