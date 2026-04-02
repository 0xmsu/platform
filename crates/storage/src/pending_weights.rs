//! Weight submission queue for managing pending weight submissions.
//!
//! This module provides durable storage for weights that need to be submitted
//! to the Bittensor chain. It handles:
//! - Enqueueing weights for future submission
//! - Tracking submission attempts
//! - Preventing duplicate epoch submissions
//! - Crash recovery via sled persistence

use platform_core::{MiniChainError, Result};
use serde::{Deserialize, Serialize};
use sled::{Db, Tree};

/// Represents a pending weight submission.
///
/// Contains all data needed to submit weights to Bittensor,
/// including tracking for retry attempts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWeight {
    /// The epoch these weights are for
    pub epoch: u64,
    /// Weight tuples: (mechanism_id, uids, values)
    pub weights: Vec<(u8, Vec<u16>, Vec<u16>)>,
    /// Number of submission attempts
    pub attempt: u32,
    /// Block number when this was enqueued
    pub enqueued_at: u64,
}

/// Durable queue for weight submissions using sled.
///
/// Maintains two trees:
/// - `pending_weights`: epoch -> serialized PendingWeight (active queue)
/// - `submitted_epochs`: epoch -> () (prevents duplicates)
///
/// All write operations immediately flush to disk for crash safety.
pub struct WeightSubmissionQueue {
    pending_tree: Tree,
    submitted_tree: Tree,
    db: Db,
}

impl WeightSubmissionQueue {
    /// Create a new weight submission queue.
    ///
    /// Opens or creates the required sled trees for persistence.
    pub fn new(db: &Db) -> Result<Self> {
        let pending_tree = db.open_tree("pending_weights").map_err(|e| {
            MiniChainError::Storage(format!("Failed to open pending_weights tree: {}", e))
        })?;

        let submitted_tree = db.open_tree("submitted_epochs").map_err(|e| {
            MiniChainError::Storage(format!("Failed to open submitted_epochs tree: {}", e))
        })?;

        Ok(Self {
            pending_tree,
            submitted_tree,
            db: db.clone(),
        })
    }

    /// Add a new weight submission to the queue.
    ///
    /// Returns error if the epoch was already enqueued or submitted.
    /// Immediately flushes to disk for durability.
    pub fn enqueue(&self, weight: PendingWeight) -> Result<()> {
        let epoch_key = weight.epoch.to_be_bytes();

        // Check if epoch exists in pending queue
        if self
            .pending_tree
            .contains_key(&epoch_key)
            .map_err(|e| MiniChainError::Storage(format!("Failed to check pending tree: {}", e)))?
        {
            return Err(MiniChainError::Storage(format!(
                "Duplicate epoch {}",
                weight.epoch
            )));
        }

        // Check if epoch was already submitted
        if self.submitted_tree.contains_key(&epoch_key).map_err(|e| {
            MiniChainError::Storage(format!("Failed to check submitted tree: {}", e))
        })? {
            return Err(MiniChainError::Storage(format!(
                "Duplicate epoch {}",
                weight.epoch
            )));
        }

        // Serialize and insert
        let data = bincode::serialize(&weight)
            .map_err(|e| MiniChainError::Storage(format!("Failed to serialize weight: {}", e)))?;

        self.pending_tree
            .insert(&epoch_key, data)
            .map_err(|e| MiniChainError::Storage(format!("Failed to insert weight: {}", e)))?;

        // Immediate durability
        self.pending_tree
            .flush()
            .map_err(|e| MiniChainError::Storage(format!("Failed to flush pending tree: {}", e)))?;

        Ok(())
    }

    /// Remove and return the oldest weight from the queue.
    ///
    /// Returns None if queue is empty.
    /// Immediately flushes to disk after removal.
    pub fn dequeue(&self) -> Result<Option<PendingWeight>> {
        // Get first entry (oldest by epoch due to BE byte ordering)
        match self.pending_tree.first() {
            Ok(Some((key, value))) => {
                self.pending_tree.remove(&key).map_err(|e| {
                    MiniChainError::Storage(format!("Failed to remove weight: {}", e))
                })?;

                self.pending_tree.flush().map_err(|e| {
                    MiniChainError::Storage(format!("Failed to flush after dequeue: {}", e))
                })?;

                let weight: PendingWeight = bincode::deserialize(&value).map_err(|e| {
                    MiniChainError::Storage(format!("Failed to deserialize weight: {}", e))
                })?;

                Ok(Some(weight))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(MiniChainError::Storage(format!(
                "Failed to get first weight: {}",
                e
            ))),
        }
    }

    /// Mark an epoch as successfully submitted.
    ///
    /// Adds the epoch to the submitted set to prevent re-submission.
    /// Immediately flushes to disk.
    pub fn mark_submitted(&self, epoch: u64) -> Result<()> {
        let epoch_key = epoch.to_be_bytes();

        self.submitted_tree
            .insert(&epoch_key, vec![])
            .map_err(|e| {
                MiniChainError::Storage(format!("Failed to mark epoch submitted: {}", e))
            })?;

        self.submitted_tree.flush().map_err(|e| {
            MiniChainError::Storage(format!("Failed to flush submitted tree: {}", e))
        })?;

        Ok(())
    }

    /// Increment the attempt counter for an epoch.
    ///
    /// Returns the new attempt count, or error if epoch not found.
    /// Immediately flushes to disk.
    pub fn increment_attempt(&self, epoch: u64) -> Result<u32> {
        let epoch_key = epoch.to_be_bytes();

        match self.pending_tree.get(&epoch_key) {
            Ok(Some(bytes)) => {
                let mut weight: PendingWeight = bincode::deserialize(&bytes).map_err(|e| {
                    MiniChainError::Storage(format!("Failed to deserialize weight: {}", e))
                })?;

                weight.attempt += 1;
                let new_attempt = weight.attempt;

                let data = bincode::serialize(&weight).map_err(|e| {
                    MiniChainError::Storage(format!("Failed to serialize weight: {}", e))
                })?;

                self.pending_tree.insert(&epoch_key, data).map_err(|e| {
                    MiniChainError::Storage(format!("Failed to update weight: {}", e))
                })?;

                self.pending_tree.flush().map_err(|e| {
                    MiniChainError::Storage(format!("Failed to flush after increment: {}", e))
                })?;

                Ok(new_attempt)
            }
            Ok(None) => Err(MiniChainError::Storage(format!(
                "Epoch {} not found",
                epoch
            ))),
            Err(e) => Err(MiniChainError::Storage(format!(
                "Failed to get epoch: {}",
                e
            ))),
        }
    }

    /// Clear all pending and submitted weights.
    ///
    /// Used for testing or resetting state.
    /// Immediately flushes to disk.
    pub fn clear(&self) -> Result<()> {
        self.pending_tree
            .clear()
            .map_err(|e| MiniChainError::Storage(format!("Failed to clear pending tree: {}", e)))?;

        self.submitted_tree.clear().map_err(|e| {
            MiniChainError::Storage(format!("Failed to clear submitted tree: {}", e))
        })?;

        self.db
            .flush()
            .map_err(|e| MiniChainError::Storage(format!("Failed to flush database: {}", e)))?;

        Ok(())
    }

    /// Get the number of pending weights in the queue.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.pending_tree.len()
    }

    /// Check if the queue is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.pending_tree.is_empty()
    }

    /// Peek at the oldest weight without removing it.
    #[allow(dead_code)]
    pub fn peek(&self) -> Result<Option<PendingWeight>> {
        match self.pending_tree.first() {
            Ok(Some((_, value))) => {
                let weight: PendingWeight = bincode::deserialize(&value).map_err(|e| {
                    MiniChainError::Storage(format!("Failed to deserialize weight: {}", e))
                })?;
                Ok(Some(weight))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(MiniChainError::Storage(format!("Failed to peek: {}", e))),
        }
    }

    /// Check if an epoch has been submitted.
    #[allow(dead_code)]
    pub fn is_submitted(&self, epoch: u64) -> Result<bool> {
        self.submitted_tree
            .contains_key(epoch.to_be_bytes())
            .map_err(|e| MiniChainError::Storage(format!("Failed to check submitted: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_queue() -> (WeightSubmissionQueue, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = sled::open(dir.path()).unwrap();
        let queue = WeightSubmissionQueue::new(&db).unwrap();
        (queue, dir)
    }

    fn create_test_weight(epoch: u64) -> PendingWeight {
        PendingWeight {
            epoch,
            weights: vec![(0, vec![1, 2, 3], vec![100, 200, 300])],
            attempt: 0,
            enqueued_at: 1000,
        }
    }

    #[test]
    fn test_enqueue_dequeue() {
        let (queue, _dir) = create_test_queue();

        let weight = create_test_weight(42);
        queue.enqueue(weight.clone()).unwrap();

        let dequeued = queue.dequeue().unwrap().expect("Should have a weight");
        assert_eq!(dequeued.epoch, 42);
        assert_eq!(dequeued.weights, weight.weights);
        assert_eq!(dequeued.attempt, 0);

        // Queue should now be empty
        assert!(queue.dequeue().unwrap().is_none());
    }

    #[test]
    fn test_duplicate_rejected() {
        let (queue, _dir) = create_test_queue();

        let weight = create_test_weight(42);
        queue.enqueue(weight.clone()).unwrap();

        // Enqueuing same epoch should fail
        let result = queue.enqueue(create_test_weight(42));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Duplicate epoch"));

        // Mark as submitted
        queue.mark_submitted(42).unwrap();

        // Enqueuing submitted epoch should also fail
        let result = queue.enqueue(create_test_weight(42));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate epoch"));
    }

    #[test]
    fn test_crash_recovery() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().to_path_buf();

        // Create queue and add weight
        {
            let db = sled::open(&db_path).unwrap();
            let queue = WeightSubmissionQueue::new(&db).unwrap();
            let weight = create_test_weight(100);
            queue.enqueue(weight).unwrap();
            queue.mark_submitted(50).unwrap();
            // db goes out of scope and closes
        }

        // Reopen and verify persistence
        {
            let db = sled::open(&db_path).unwrap();
            let queue = WeightSubmissionQueue::new(&db).unwrap();

            // Pending weight should persist
            let weight = queue.dequeue().unwrap().expect("Should have weight");
            assert_eq!(weight.epoch, 100);

            // Submitted epoch should persist
            assert!(queue.is_submitted(50).unwrap());

            // Duplicate epoch check should work
            let result = queue.enqueue(create_test_weight(50));
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_mark_submitted_removes() {
        let (queue, _dir) = create_test_queue();

        // Mark epoch 42 as submitted without ever enqueuing
        queue.mark_submitted(42).unwrap();

        // Attempting to enqueue that epoch should fail
        let result = queue.enqueue(create_test_weight(42));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate epoch"));
    }

    #[test]
    fn test_clear() {
        let (queue, _dir) = create_test_queue();

        queue.enqueue(create_test_weight(1)).unwrap();
        queue.enqueue(create_test_weight(2)).unwrap();
        queue.mark_submitted(10).unwrap();

        assert_eq!(queue.len(), 2);
        assert!(queue.is_submitted(10).unwrap());

        queue.clear().unwrap();

        assert_eq!(queue.len(), 0);
        assert!(!queue.is_submitted(10).unwrap());
    }

    #[test]
    fn test_increment_attempt() {
        let (queue, _dir) = create_test_queue();

        queue.enqueue(create_test_weight(42)).unwrap();

        // First increment
        let attempt = queue.increment_attempt(42).unwrap();
        assert_eq!(attempt, 1);

        // Second increment
        let attempt = queue.increment_attempt(42).unwrap();
        assert_eq!(attempt, 2);

        // Verify persisted
        let weight = queue.peek().unwrap().unwrap();
        assert_eq!(weight.attempt, 2);

        // Non-existent epoch should error
        let result = queue.increment_attempt(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_fifo_order() {
        let (queue, _dir) = create_test_queue();

        queue.enqueue(create_test_weight(30)).unwrap();
        queue.enqueue(create_test_weight(10)).unwrap();
        queue.enqueue(create_test_weight(20)).unwrap();

        // Should dequeue in epoch order (sled orders by key)
        let w1 = queue.dequeue().unwrap().unwrap();
        assert_eq!(w1.epoch, 10);

        let w2 = queue.dequeue().unwrap().unwrap();
        assert_eq!(w2.epoch, 20);

        let w3 = queue.dequeue().unwrap().unwrap();
        assert_eq!(w3.epoch, 30);

        assert!(queue.is_empty());
    }
}
