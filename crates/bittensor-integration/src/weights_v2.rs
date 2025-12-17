//! Weight submission to Bittensor - V2 using high-level Subtensor API
//!
//! This module provides a simplified interface for submitting weights using
//! bittensor-rs's `Subtensor` struct which automatically handles:
//! - CRv4 timelock encryption (auto-reveal by chain)
//! - Legacy commit-reveal (with persistence)
//! - Direct set_weights (when commit-reveal disabled)
//!
//! ## Usage
//! ```ignore
//! use platform_bittensor_integration::WeightSubmitterV2;
//!
//! let submitter = WeightSubmitterV2::new(endpoint, netuid, signer, data_dir).await?;
//!
//! // Automatically uses the correct method based on chain config
//! let response = submitter.submit_mechanism_weights(mechanism_id, &weights).await?;
//! ```

use anyhow::Result;
use bittensor_rs::chain::{BittensorSigner, ExtrinsicWait};
use bittensor_rs::subtensor::{Subtensor, SubtensorBuilder, WeightResponse};
use bittensor_rs::utils::weights::normalize_weights;
use platform_challenge_sdk::WeightAssignment;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Weight submitter using bittensor-rs Subtensor API
///
/// Automatically handles:
/// - CRv4 (timelock encryption) - chain auto-reveals
/// - Legacy commit-reveal (with persistence)
/// - Direct set_weights
pub struct WeightSubmitterV2 {
    /// High-level Subtensor client
    subtensor: Arc<Subtensor>,
    /// Wallet signer
    signer: Arc<BittensorSigner>,
    /// Subnet ID
    netuid: u16,
    /// Version key for weights
    version_key: u64,
    /// Current epoch (updated externally)
    current_epoch: RwLock<u64>,
    /// Cache of hotkey -> UID mappings
    uid_cache: RwLock<HashMap<String, u16>>,
}

impl WeightSubmitterV2 {
    /// Create a new weight submitter
    ///
    /// # Arguments
    /// * `endpoint` - RPC endpoint URL
    /// * `netuid` - Subnet ID
    /// * `signer` - Wallet signer (hotkey)
    /// * `version_key` - Network version key
    /// * `data_dir` - Optional directory for state persistence
    pub async fn new(
        endpoint: &str,
        netuid: u16,
        signer: BittensorSigner,
        version_key: u64,
        data_dir: Option<PathBuf>,
    ) -> Result<Self> {
        let subtensor = if let Some(dir) = data_dir {
            let state_path = dir.join("subtensor_state.json");
            SubtensorBuilder::new(endpoint)
                .with_persistence(state_path)
                .build()
                .await?
        } else {
            Subtensor::new(endpoint).await?
        };

        Ok(Self {
            subtensor: Arc::new(subtensor),
            signer: Arc::new(signer),
            netuid,
            version_key,
            current_epoch: RwLock::new(0),
            uid_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Create from existing Subtensor instance
    pub fn from_subtensor(
        subtensor: Arc<Subtensor>,
        signer: BittensorSigner,
        netuid: u16,
        version_key: u64,
    ) -> Self {
        Self {
            subtensor,
            signer: Arc::new(signer),
            netuid,
            version_key,
            current_epoch: RwLock::new(0),
            uid_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Update current epoch
    pub async fn set_epoch(&self, epoch: u64) {
        let mut current = self.current_epoch.write().await;
        if *current != epoch {
            info!("Epoch updated: {} -> {}", *current, epoch);
            *current = epoch;
        }
    }

    /// Get current epoch
    pub async fn epoch(&self) -> u64 {
        *self.current_epoch.read().await
    }

    /// Update UID cache from metagraph
    pub async fn update_uid_cache(&self, hotkey_to_uid: HashMap<String, u16>) {
        let mut cache = self.uid_cache.write().await;
        *cache = hotkey_to_uid;
    }

    /// Get UID for a hotkey from cache
    pub async fn get_uid(&self, hotkey: &str) -> Option<u16> {
        let cache = self.uid_cache.read().await;
        cache.get(hotkey).copied()
    }

    /// Check if commit-reveal is enabled
    pub async fn commit_reveal_enabled(&self) -> Result<bool> {
        self.subtensor.commit_reveal_enabled(self.netuid).await
    }

    /// Check if CRv4 (timelock encryption) is being used
    pub async fn is_crv4(&self) -> bool {
        self.subtensor.is_crv4_enabled().await
    }

    /// Submit weights for main mechanism (mechanism_id = 0)
    ///
    /// Automatically uses the correct submission method based on chain config:
    /// - CRv4: Timelock encryption (chain auto-reveals)
    /// - Legacy: Hash-based commit-reveal
    /// - Direct: No commit-reveal
    pub async fn submit_weights(&self, weights: &[WeightAssignment]) -> Result<WeightResponse> {
        self.submit_mechanism_weights(0, weights).await
    }

    /// Submit weights for a specific mechanism
    ///
    /// This is the main entry point for weight submission.
    pub async fn submit_mechanism_weights(
        &self,
        mechanism_id: u8,
        weights: &[WeightAssignment],
    ) -> Result<WeightResponse> {
        // Convert WeightAssignment to (uids, weights)
        let (uids, weight_values) = self.prepare_weights(weights).await?;

        if uids.is_empty() {
            return Err(anyhow::anyhow!("No valid weights to submit"));
        }

        info!(
            "Submitting {} weights for mechanism {} (netuid={})",
            uids.len(),
            mechanism_id,
            self.netuid
        );

        // Use the high-level API that handles everything
        self.subtensor
            .set_mechanism_weights(
                &self.signer,
                self.netuid,
                mechanism_id,
                &uids,
                &weight_values,
                self.version_key,
                ExtrinsicWait::Finalized,
            )
            .await
    }

    /// Submit weights for multiple mechanisms in batch
    ///
    /// Each mechanism's weights are submitted separately with the correct
    /// commit-reveal handling.
    pub async fn submit_mechanism_weights_batch(
        &self,
        mechanism_weights: &[(u8, Vec<WeightAssignment>)],
    ) -> Result<Vec<(u8, WeightResponse)>> {
        let mut results = Vec::new();

        for (mechanism_id, weights) in mechanism_weights {
            match self.submit_mechanism_weights(*mechanism_id, weights).await {
                Ok(response) => results.push((*mechanism_id, response)),
                Err(e) => {
                    error!("Failed to submit weights for mechanism {}: {}", mechanism_id, e);
                    results.push((
                        *mechanism_id,
                        WeightResponse::failure(&format!("Error: {}", e)),
                    ));
                }
            }
        }

        Ok(results)
    }

    /// Submit pre-prepared weights (already in u16 format)
    pub async fn submit_raw_mechanism_weights(
        &self,
        mechanism_id: u8,
        uids: &[u16],
        weights: &[u16],
    ) -> Result<WeightResponse> {
        self.subtensor
            .set_mechanism_weights(
                &self.signer,
                self.netuid,
                mechanism_id,
                uids,
                weights,
                self.version_key,
                ExtrinsicWait::Finalized,
            )
            .await
    }

    /// Check if there are pending commits (for legacy commit-reveal)
    pub async fn has_pending_commits(&self) -> bool {
        self.subtensor.has_pending_commits().await
    }

    /// Force reveal all pending commits (for legacy commit-reveal)
    pub async fn reveal_pending(&self) -> Result<Vec<WeightResponse>> {
        self.subtensor
            .reveal_all_pending(&self.signer, ExtrinsicWait::Finalized)
            .await
    }

    /// Prepare weights: convert WeightAssignment to (Vec<u16>, Vec<u16>)
    async fn prepare_weights(&self, weights: &[WeightAssignment]) -> Result<(Vec<u16>, Vec<u16>)> {
        let cache = self.uid_cache.read().await;

        let mut uids = Vec::new();
        let mut values = Vec::new();

        for weight in weights {
            if let Some(uid) = cache.get(&weight.agent_hash) {
                uids.push(*uid);
                // Convert f64 weight (0-1) to u16 (0-65535)
                let w_u16 = (weight.weight.clamp(0.0, 1.0) * 65535.0) as u16;
                values.push(w_u16);
            } else {
                warn!("No UID found for hotkey: {}", weight.agent_hash);
            }
        }

        // If no valid weights, add fallback to UID 0
        if uids.is_empty() {
            info!("No valid UIDs found, defaulting to UID 0");
            return Ok((vec![0], vec![65535]));
        }

        // Ensure weights sum to ~65535 (fill remainder to UID 0 if needed)
        let sum: u32 = values.iter().map(|w| *w as u32).sum();
        if sum < 65535 {
            let remaining = (65535 - sum) as u16;
            if let Some(idx) = uids.iter().position(|&u| u == 0) {
                values[idx] = values[idx].saturating_add(remaining);
            } else {
                uids.push(0);
                values.push(remaining);
                debug!("Added {} weight to UID 0 to fill sum", remaining);
            }
        }

        Ok((uids, values))
    }

    /// Get info about pending commits
    pub async fn pending_info(&self) -> String {
        let pending = self.subtensor.pending_commits().await;
        if pending.is_empty() {
            "none".to_string()
        } else {
            format!(
                "{} pending commits: {:?}",
                pending.len(),
                pending
                    .iter()
                    .map(|p| (p.netuid, p.mechanism_id))
                    .collect::<Vec<_>>()
            )
        }
    }

    /// Cleanup old pending commits
    pub async fn cleanup(&self, max_age_epochs: u64) {
        let current_epoch = self.epoch().await;
        self.subtensor
            .cleanup_old_commits(current_epoch, max_age_epochs)
            .await;
    }

    /// Get underlying Subtensor reference
    pub fn subtensor(&self) -> &Subtensor {
        &self.subtensor
    }
}

/// Utility: Convert f64 weights to normalized u16
pub fn normalize_weights_f64(weights: &[f64]) -> Vec<u16> {
    let sum: f64 = weights.iter().sum();
    if sum == 0.0 {
        return vec![0; weights.len()];
    }

    weights
        .iter()
        .map(|w| ((w / sum) * 65535.0) as u16)
        .collect()
}

/// Utility: Convert f32 weights to normalized u16 using bittensor-rs
pub fn normalize_weights_f32(uids: &[u64], weights: &[f32]) -> Result<(Vec<u16>, Vec<u16>)> {
    normalize_weights(uids, weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_weights_f64() {
        let weights = vec![0.5, 0.3, 0.2];
        let normalized = normalize_weights_f64(&weights);

        // Sum should be approximately 65535
        let sum: u32 = normalized.iter().map(|w| *w as u32).sum();
        assert!(sum >= 65530 && sum <= 65540);
    }

    #[test]
    fn test_normalize_empty() {
        let weights: Vec<f64> = vec![];
        let normalized = normalize_weights_f64(&weights);
        assert!(normalized.is_empty());
    }

    #[test]
    fn test_normalize_zero_sum() {
        let weights = vec![0.0, 0.0, 0.0];
        let normalized = normalize_weights_f64(&weights);
        assert_eq!(normalized, vec![0, 0, 0]);
    }
}
