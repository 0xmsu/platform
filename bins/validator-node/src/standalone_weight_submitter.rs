//! Standalone weight submission thread.
//! Completely decoupled from P2P rate limiting and block sync.
//! Fetches weights from chain.platform.zip and submits directly to Bittensor.
//!
//! Uses CRv4 commit-reveal for weight submission:
//! - Commit: Encrypt weights with timelock (DRAND-based)
//! - Reveal: Automatic when DRAND pulse becomes available

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{Timelike, Utc};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use platform_bittensor::{
    BittensorConfig, SubtensorClient, WeightSubmitter,
};

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
/// Uses WeightSubmitter internally for CRv4 commit-reveal support
pub struct StandaloneWeightSubmitter {
    // Configuration stored to create fresh connections
    endpoint: String,
    netuid: u16,
    signer_seed: String,
    http_client: Client,
    last_submission_hour: Arc<Mutex<Option<i64>>>,
    running: Arc<AtomicBool>,
}

impl StandaloneWeightSubmitter {
    pub async fn new(
        endpoint: &str,
        netuid: u16,
        signer_seed: &str,
    ) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        
        info!(
            endpoint = %endpoint,
            netuid,
            "Standalone weight submitter initialized (will create fresh connection for each submission)"
        );
        
        Ok(Self {
            endpoint: endpoint.to_string(),
            netuid,
            signer_seed: signer_seed.to_string(),
            http_client,
            last_submission_hour: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(true)),
        })
    }

    /// Shutdown gracefully
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!("Standalone weight submitter shutdown requested");
    }

    pub async fn run(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(CHECK_INTERVAL_SECS));
        
        info!(
            url = WEIGHT_RPC_URL,
            "Standalone weight submitter started (hourly at :00 + startup)"
        );
        
        info!("Submitting weights at startup...");
        match self.submit_with_retry().await {
            Ok(()) => {
                let now = Utc::now();
                let hour_epoch = now.timestamp() / 3600;
                let mut last = self.last_submission_hour.lock().await;
                *last = Some(hour_epoch);
                info!(hour = hour_epoch, "Startup weights submitted successfully");
            }
            Err(e) => {
                error!(error = %e, "Failed to submit weights at startup (will retry at next hourly checkpoint)");
            }
        }
        
        loop {
            interval.tick().await;
            
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            
            let now = Utc::now();
            
            if now.minute() == 0 {
                let hour_epoch = now.timestamp() / 3600;
                
                let mut last = self.last_submission_hour.lock().await;
                if let Some(last_hour) = *last {
                    if last_hour == hour_epoch {
                        debug!("Already submitted this hour, skipping");
                        continue;
                    }
                }
                
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

    async fn fetch_and_submit(&self) -> Result<()> {
        let weights = self.fetch_weights().await?;
        
        if weights.is_empty() {
            warn!("Empty weights from RPC");
            return Ok(());
        }
        
        let mechanism_weights: Vec<(u8, Vec<u16>, Vec<u16>)> = weights
            .into_iter()
            .map(|mw| {
                let uids: Vec<u16> = mw.entries.iter().map(|e| e.uid).collect();
                let weights: Vec<u16> = mw.entries.iter().map(|e| e.weight).collect();
                info!(
                    mechanism_id = mw.mechanism_id,
                    uid_count = uids.len(),
                    total_weight = weights.iter().map(|w| *w as u64).sum::<u64>(),
                    "Submitting weights to Bittensor via CRv4"
                );
                (mw.mechanism_id, uids, weights)
            })
            .collect();
        
        // Create FRESH Bittensor connection for this submission
        info!(endpoint = %self.endpoint, "Creating fresh Bittensor connection...");
        
        let config = BittensorConfig {
            endpoint: self.endpoint.clone(),
            netuid: self.netuid,
            use_commit_reveal: true,
            version_key: 1,
        };
        
        let mut client = SubtensorClient::new(config);
        client.connect().await
            .map_err(|e| anyhow::anyhow!("Bittensor connection failed to {}: {}", self.endpoint, e))?;
        client.set_signer(&self.signer_seed)
            .map_err(|e| anyhow::anyhow!("Failed to set signer: {}", e))?;
        
        let mut weight_submitter = WeightSubmitter::new(client, None);
        
        weight_submitter.submit_mechanism_weights_batch(&mechanism_weights).await?;
        
        info!("Weights submitted successfully, connection will be dropped");
        
        // Connection is automatically dropped when weight_submitter goes out of scope
        
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

pub async fn spawn_standalone_weight_submitter(
    endpoint: &str,
    netuid: u16,
    signer_seed: &str,
) -> Result<Arc<StandaloneWeightSubmitter>> {
    let submitter = Arc::new(StandaloneWeightSubmitter::new(endpoint, netuid, signer_seed).await?);
    let submitter_clone = submitter.clone();
    
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime for weight submitter");
        rt.block_on(submitter_clone.run());
    });
    
    Ok(submitter)
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
