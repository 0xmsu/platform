//! Background task for weight submission with retry logic.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use platform_core::MiniChainError;
use platform_storage::{PendingWeight, WeightSubmissionQueue};

use bittensor_rs::chain::{BittensorSigner, ExtrinsicWait};
use bittensor_rs::Subtensor;

/// Maximum delay between retries in seconds.
const MAX_RETRY_DELAY_SECS: u64 = 60;

/// Base delay between retries in seconds (multiplied by 2^attempt).
const BASE_RETRY_DELAY_SECS: u64 = 10;

/// Commands that can be sent to the weight submission task.
#[derive(Debug)]
pub enum WeightCommand {
    /// Submit weights for an epoch.
    Submit {
        epoch: u64,
        weights: Vec<(u8, Vec<u16>, Vec<u16>)>,
    },
    /// Hotkey changed, clear pending weights.
    HotkeyChanged,
    /// Shut down the task.
    Shutdown,
}

/// Handle for controlling the weight submission task.
#[derive(Clone)]
pub struct WeightTaskHandle {
    tx: mpsc::Sender<WeightCommand>,
    queue: Arc<WeightSubmissionQueue>,
}

impl WeightTaskHandle {
    /// Enqueue weights for submission.
    pub async fn enqueue(&self, epoch: u64, weights: Vec<(u8, Vec<u16>, Vec<u16>)>) -> Result<(), MiniChainError> {
        let weight = PendingWeight {
            epoch,
            weights: weights.clone(),
            attempt: 0,
            enqueued_at: 0,
        };
        self.queue.enqueue(weight)?;

        if let Err(e) = self.tx.send(WeightCommand::Submit { epoch, weights }).await {
            warn!("Failed to send weight command: {}", e);
        }

        Ok(())
    }

    /// Notify the task that the hotkey has changed.
    pub async fn notify_hotkey_changed(&self) {
        if let Err(e) = self.tx.send(WeightCommand::HotkeyChanged).await {
            warn!("Failed to send hotkey changed command: {}", e);
        }
    }

    /// Shut down the weight submission task gracefully.
    pub async fn shutdown(&self) {
        if let Err(e) = self.tx.send(WeightCommand::Shutdown).await {
            warn!("Failed to send shutdown command: {}", e);
        }
    }

    /// Get a reference to the underlying queue.
    pub fn queue(&self) -> &WeightSubmissionQueue {
        &self.queue
    }
}

/// Spawn the weight submission background task.
pub fn spawn_weight_task(
    queue: Arc<WeightSubmissionQueue>,
    subtensor: Arc<Subtensor>,
    signer: Arc<BittensorSigner>,
    netuid: u16,
    version_key: u64,
    uid: u16,
) -> WeightTaskHandle {
    let (tx, mut rx) = mpsc::channel::<WeightCommand>(32);
    let queue_clone = queue.clone();
    let subtensor_clone = subtensor;
    let signer_clone = signer;

    tokio::spawn(async move {
        info!("Weight submission task started");

        while let Some(cmd) = rx.recv().await {
            match cmd {
                WeightCommand::Submit { epoch, weights } => {
                    if let Err(e) = submit_with_retry(
                        &queue_clone,
                        &subtensor_clone,
                        &signer_clone,
                        netuid,
                        version_key,
                        uid,
                        epoch,
                        weights,
                    )
                    .await
                    {
                        error!(epoch = epoch, error = %e, "Weight submission failed after retries");
                    }
                }
                WeightCommand::HotkeyChanged => {
                    info!("Hotkey changed, clearing pending weights");
                    if let Err(e) = queue_clone.clear() {
                        error!(error = %e, "Failed to clear weight queue");
                    }
                }
                WeightCommand::Shutdown => {
                    info!("Weight task received shutdown command");
                    break;
                }
            }
        }

        info!("Weight submission task shutting down");
    });

    WeightTaskHandle { tx, queue }
}

/// Calculate exponential backoff duration.
fn calculate_backoff(attempt: u32) -> Duration {
    let delay = BASE_RETRY_DELAY_SECS * 2u64.pow(attempt.min(6));
    Duration::from_secs(delay.min(MAX_RETRY_DELAY_SECS))
}

/// Check if an error is a transport/connection error that should trigger retry.
fn is_transport_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("connection closed")
        || lower.contains("restart required")
        || lower.contains("background task")
        || lower.contains("transport")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("websocket")
        || lower.contains("eof")
        || lower.contains("channel closed")
        || lower.contains("timed out")
        || lower.contains("timeout")
}

/// Submit weights with retry logic.
async fn submit_with_retry(
    queue: &WeightSubmissionQueue,
    subtensor: &Subtensor,
    signer: &BittensorSigner,
    netuid: u16,
    version_key: u64,
    uid: u16,
    epoch: u64,
    weights: Vec<(u8, Vec<u16>, Vec<u16>)>,
) -> Result<(), MiniChainError> {
    let pending = queue.peek()?;
    let attempt = pending
        .as_ref()
        .filter(|w| w.epoch == epoch)
        .map(|w| w.attempt)
        .unwrap_or(0);

    for (mechanism_id, uids, values) in &weights {
        loop {
            let can_submit = subtensor
                .can_set_weights(netuid, uid, *mechanism_id)
                .await
                .map_err(|e| MiniChainError::Network(e.to_string()))?;

            if !can_submit {
                info!(
                    epoch = epoch,
                    mechanism = mechanism_id,
                    "Commit window closed, waiting 12s"
                );
                tokio::time::sleep(Duration::from_secs(12)).await;
                continue;
            }

            let result = subtensor
                .set_mechanism_weights(
                    signer,
                    netuid,
                    *mechanism_id,
                    uids,
                    values,
                    version_key,
                    ExtrinsicWait::Finalized,
                )
                .await;

            match result {
                Ok(resp) if resp.success => {
                    info!(
                        epoch = epoch,
                        mechanism = mechanism_id,
                        tx_hash = ?resp.tx_hash,
                        uid_count = uids.len(),
                        "Mechanism weight submission SUCCESS"
                    );
                    break;
                }
                Ok(resp) => {
                    warn!(
                        epoch = epoch,
                        mechanism = mechanism_id,
                        message = %resp.message,
                        "Weight submission returned non-success, will retry next window"
                    );
                    tokio::time::sleep(Duration::from_secs(12)).await;
                }
                Err(e) => {
                    let err_str = format!("{}", e);

                    if err_str.contains("Priority is too low")
                        || err_str.contains("1010")
                        || err_str.contains("CommittingWeightsTooFast")
                    {
                        warn!(
                            epoch = epoch,
                            mechanism = mechanism_id,
                            error = %err_str,
                            "Transaction priority/rate limit conflict, will retry next window"
                        );
                        tokio::time::sleep(Duration::from_secs(12)).await;
                        continue;
                    }

                    if err_str.contains("HotKeyNotRegisteredInSubNet") {
                        error!(
                            epoch = epoch,
                            mechanism = mechanism_id,
                            error = %err_str,
                            "Hotkey not registered on subnet"
                        );
                        return Err(MiniChainError::Network(format!(
                            "Hotkey not registered: {}",
                            err_str
                        )));
                    }

                    if is_transport_error(&err_str) {
                        let new_attempt = queue.increment_attempt(epoch)?;
                        let backoff = calculate_backoff(new_attempt);

                        warn!(
                            epoch = epoch,
                            mechanism = mechanism_id,
                            attempt = new_attempt,
                            backoff_secs = backoff.as_secs(),
                            error = %err_str,
                            "Transport error, will retry after backoff"
                        );

                        tokio::time::sleep(backoff).await;
                        continue;
                    }

                    error!(
                        epoch = epoch,
                        mechanism = mechanism_id,
                        error = %err_str,
                        "Unknown error, marking as failed"
                    );
                    return Err(MiniChainError::Network(err_str));
                }
            }
        }
    }

    queue.mark_submitted(epoch)?;
    info!(epoch = epoch, "All mechanism weights submitted successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_backoff_zero_attempt() {
        let backoff = calculate_backoff(0);
        assert_eq!(backoff, Duration::from_secs(10));
    }

    #[test]
    fn test_calculate_backoff_first_attempt() {
        let backoff = calculate_backoff(1);
        assert_eq!(backoff, Duration::from_secs(20));
    }

    #[test]
    fn test_calculate_backoff_second_attempt() {
        let backoff = calculate_backoff(2);
        assert_eq!(backoff, Duration::from_secs(40));
    }

    #[test]
    fn test_calculate_backoff_caps_at_max() {
        let backoff = calculate_backoff(3);
        assert_eq!(backoff, Duration::from_secs(60));
    }

    #[test]
    fn test_calculate_backoff_high_attempt() {
        let backoff = calculate_backoff(10);
        assert_eq!(backoff, Duration::from_secs(60));
    }

    #[test]
    fn test_is_transport_error_connection_closed() {
        assert!(is_transport_error("connection closed by peer"));
    }

    #[test]
    fn test_is_transport_error_restart_required() {
        assert!(is_transport_error("restart required"));
    }

    #[test]
    fn test_is_transport_error_broken_pipe() {
        assert!(is_transport_error("Broken pipe"));
    }

    #[test]
    fn test_is_transport_error_timeout() {
        assert!(is_transport_error("operation timed out"));
    }

    #[test]
    fn test_is_transport_error_websocket() {
        assert!(is_transport_error("WebSocket connection reset"));
    }

    #[test]
    fn test_is_transport_error_not_permanent() {
        assert!(!is_transport_error("InvalidSignature"));
        assert!(!is_transport_error("InsufficientBalance"));
    }
}
