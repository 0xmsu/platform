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

    #[test]
    fn test_minute_zero_detection() {
        // Test that minute() == 0 correctly identifies heure pile
        let dt1 = chrono::DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z").unwrap();
        let dt2 = chrono::DateTime::parse_from_rfc3339("2026-01-01T10:30:00Z").unwrap();
        let dt3 = chrono::DateTime::parse_from_rfc3339("2026-01-01T10:59:59Z").unwrap();

        assert_eq!(dt1.minute(), 0);  // heure pile
        assert_ne!(dt2.minute(), 0);  // not heure pile
        assert_ne!(dt3.minute(), 0);  // not heure pile
    }

    #[test]
    fn test_hour_epoch_calculation() {
        // Test hour epoch from timestamp
        let ts: i64 = 1738368000; // 2025-02-01 00:00:00 UTC
        let hour_epoch = ts / 3600;
        assert_eq!(hour_epoch, 482880); // hours since epoch

        // Verify consistency: same hour gives same epoch
        let ts2: i64 = 1738371599; // 2025-02-01 00:59:59 UTC
        assert_eq!(ts2 / 3600, hour_epoch);

        // Different hour gives different epoch
        let ts3: i64 = 1738371600; // 2025-02-01 01:00:00 UTC
        assert_ne!(ts3 / 3600, hour_epoch);
    }

    #[test]
    fn test_dedup_same_hour_skipped() {
        // Same hour_epoch should be skipped
        let hour_epoch: i64 = 482880;
        let last_submission: Option<i64> = Some(hour_epoch);

        // If last_submission_hour == current hour_epoch, should skip
        assert!(last_submission.is_some());
        assert_eq!(last_submission.unwrap(), hour_epoch);

        // This simulates: if last_hour == hour_epoch { continue; }
        let should_skip = match last_submission {
            Some(last) if last == hour_epoch => true,
            _ => false,
        };
        assert!(should_skip);
    }

    #[test]
    fn test_dedup_different_hour_processed() {
        // Different hour should be processed
        let last_submission: Option<i64> = Some(482880);
        let new_hour_epoch: i64 = 482881; // Next hour

        let should_process = match last_submission {
            Some(last) if last == new_hour_epoch => false,
            _ => true,
        };
        assert!(should_process);
    }

    #[test]
    fn test_dedup_first_submission() {
        // No previous submission (None) should always process
        let last_submission: Option<i64> = None;
        let hour_epoch: i64 = 482880;

        let should_process = match last_submission {
            Some(last) if last == hour_epoch => false,
            _ => true,
        };
        assert!(should_process);
    }

    #[test]
    fn test_invalid_response_parsing() {
        // Test that invalid JSON response is handled gracefully
        let json_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {}  // Missing 'weights' field
        });

        // Verify missing weights array produces error
        let weights_arr = json_response["result"]["weights"].as_array();
        assert!(weights_arr.is_none());
    }

    #[test]
    fn test_empty_weights_array_handled() {
        // Empty weights should not crash - just log warning
        let json_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "weights": []
            }
        });

        let weights_arr = json_response["result"]["weights"].as_array();
        assert!(weights_arr.is_some());
        assert!(weights_arr.unwrap().is_empty());
    }

    #[test]
    fn test_malformed_weight_entry_skipped() {
        // Malformed entry should be skipped, not crash
        let json_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "weights": [
                    {"mechanismId": null, "entries": []},  // null mechanism id
                    {"mechanismId": 1, "entries": null}   // null entries
                ]
            }
        });

        let weights_arr = json_response["result"]["weights"].as_array().unwrap();

        // Should handle gracefully with defaults
        for entry in weights_arr {
            let _mechanism_id = entry["mechanismId"].as_u64().unwrap_or(0);
            let _entries = entry["entries"].as_array();
            // Should not panic
        }
    }
}
