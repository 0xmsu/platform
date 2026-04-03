//! Standalone weight submission thread.
//! Completely decoupled from P2P rate limiting and block sync.
//! Fetches weights from chain.platform.zip and submits directly to Bittensor.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{Timelike, Utc};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use platform_bittensor::{BittensorSigner, ExtrinsicWait, Subtensor};

/// URL for fetching pre-computed weights
const WEIGHT_RPC_URL: &str = "https://chain.platform.zip/rpc";

/// How often to check the time (60 seconds)
const CHECK_INTERVAL_SECS: u64 = 60;

/// Retry delays for RPC fetch (seconds)
const RETRY_DELAYS: [u64; 3] = [5, 10, 30];

/// Response from subnet_getWeights RPC
#[derive(Debug, Deserialize)]
struct WeightResponse {
    result: WeightResult,
}

#[derive(Debug, Deserialize)]
struct WeightResult {
    weights: Vec<MechanismWeights>,
}

#[derive(Debug, Deserialize)]
struct MechanismWeights {
    #[serde(rename = "mechanismId")]
    mechanism_id: u8,
    entries: Vec<WeightEntry>,
}

#[derive(Debug, Deserialize)]
struct WeightEntry {
    uid: u16,
    weight: u16,
}

/// Standalone weight submitter - no P2P dependencies
pub struct StandaloneWeightSubmitter {
    subtensor: Arc<Subtensor>,
    signer: BittensorSigner,
    netuid: u16,
    http_client: Client,
    last_submission_hour: Arc<tokio::sync::Mutex<Option<i64>>>,
    running: Arc<AtomicBool>,
}

impl StandaloneWeightSubmitter {
    /// Create a new standalone weight submitter
    pub fn new(
        subtensor: Arc<Subtensor>,
        signer: BittensorSigner,
        netuid: u16,
    ) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            subtensor,
            signer,
            netuid,
            http_client,
            last_submission_hour: Arc::new(tokio::sync::Mutex::new(None)),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Shutdown gracefully
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!("Standalone weight submitter shutdown requested");
    }

    /// Run the submission loop (call in a spawned task)
    pub async fn run(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(CHECK_INTERVAL_SECS));
        
        info!(
            url = WEIGHT_RPC_URL,
            netuid = self.netuid,
            "Standalone weight submitter started (hourly at :00)"
        );
        
        while self.running.load(Ordering::SeqCst) {
            interval.tick().await;
            
            let now = Utc::now();
            
            // Check if we're at the top of the hour (minute 0)
            if now.minute() == 0 {
                let hour_epoch = now.timestamp() / 3600;
                
                // Dedup: only submit once per hour
                let mut last = self.last_submission_hour.lock().await;
                if let Some(last_hour) = *last {
                    if last_hour == hour_epoch {
                        debug!("Already submitted this hour, skipping");
                        continue;
                    }
                }
                
                // Try to submit with retries
                match self.submit_with_retry().await {
                    Ok(()) => {
                        *last = Some(hour_epoch);
                        info!(hour = hour_epoch, "Weights submitted successfully");
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to submit weights after all retries");
                    }
                }
            }
        }
        
        info!("Standalone weight submitter stopped");
    }

    /// Submit weights with retry logic
    async fn submit_with_retry(&self) -> Result<()> {
        let mut last_error = None;
        
        for (attempt, delay) in RETRY_DELAYS.iter().enumerate() {
            match self.fetch_and_submit().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = RETRY_DELAYS.len(),
                        delay_secs = delay,
                        error = %e,
                        "Weight submission failed, retrying..."
                    );
                    last_error = Some(e);
                    if attempt < RETRY_DELAYS.len() - 1 {
                        tokio::time::sleep(Duration::from_secs(*delay)).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
    }

    /// Fetch weights from RPC and submit to chain
    async fn fetch_and_submit(&self) -> Result<()> {
        // 1. Fetch from chain.platform.zip
        let weights = self.fetch_weights().await?;
        
        if weights.is_empty() {
            warn!("Empty weights from RPC");
            return Ok(());
        }
        
        // 2. Submit each mechanism
        for mw in weights {
            let uids: Vec<u16> = mw.entries.iter().map(|e| e.uid).collect();
            let weights: Vec<u16> = mw.entries.iter().map(|e| e.weight).collect();
            
            info!(
                mechanism_id = mw.mechanism_id,
                uid_count = uids.len(),
                total_weight = weights.iter().map(|w| *w as u64).sum::<u64>(),
                "Submitting weights to Bittensor"
            );
            
            self.subtensor
                .set_weights(
                    &self.signer,
                    self.netuid,
                    &uids,
                    &weights,
                    mw.mechanism_id as u64,
                    ExtrinsicWait::Finalized,
                )
                .await
                .map_err(|e| anyhow::anyhow!("set_weights failed: {}", e))?;
        }
        
        Ok(())
    }

    /// Fetch weights from chain.platform.zip/rpc
    async fn fetch_weights(&self) -> Result<Vec<MechanismWeights>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "subnet_getWeights",
            "params": {},
            "id": 1
        });
        
        let response = self.http_client
            .post(WEIGHT_RPC_URL)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP request failed: {}", e))?;
        
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("RPC HTTP error: {} {}", status.as_u16(), status.canonical_reason().unwrap_or(""));
        }
        
        let parsed: WeightResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("JSON parse failed: {}", e))?;
        
        Ok(parsed.result.weights)
    }
}

/// Spawn the standalone weight submitter in a dedicated task.
pub fn spawn_standalone_weight_submitter(
    subtensor: Arc<Subtensor>,
    signer: BittensorSigner,
    netuid: u16,
) -> Arc<StandaloneWeightSubmitter> {
    let submitter = Arc::new(StandaloneWeightSubmitter::new(subtensor, signer, netuid));
    let submitter_clone = submitter.clone();
    
    tokio::spawn(async move {
        submitter_clone.run().await;
    });
    
    submitter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert!(!WEIGHT_RPC_URL.is_empty());
        assert_eq!(CHECK_INTERVAL_SECS, 60);
        assert_eq!(RETRY_DELAYS, [5, 10, 30]);
    }

    #[test]
    fn test_json_parsing() {
        let json = serde_json::json!({
            "result": {
                "weights": [
                    {
                        "mechanismId": 0,
                        "entries": [
                            {"uid": 1, "weight": 100},
                            {"uid": 2, "weight": 200}
                        ]
                    }
                ]
            }
        });
        
        let parsed: WeightResponse = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.result.weights.len(), 1);
        assert_eq!(parsed.result.weights[0].mechanism_id, 0);
        assert_eq!(parsed.result.weights[0].entries.len(), 2);
    }
}
