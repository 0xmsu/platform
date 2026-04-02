//! E2E tests for weight submission queue.
//!
//! Tests for WeightSubmissionQueue durability and state management.

use platform_storage::{PendingWeight, Storage, WeightSubmissionQueue};
use tempfile::TempDir;

fn create_test_queue() -> (WeightSubmissionQueue, Storage, TempDir) {
    let dir = tempfile::tempdir().expect("Failed to create tempdir");
    let storage = Storage::open(dir.path()).expect("Failed to open storage");
    let queue = WeightSubmissionQueue::new(storage.db()).expect("Failed to create queue");
    (queue, storage, dir)
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
fn test_duplicate_epoch_rejected() {
    let (queue, _storage, _dir) = create_test_queue();

    let weight = create_test_weight(10);
    queue.enqueue(weight).expect("First enqueue should succeed");

    let duplicate = create_test_weight(10);
    let result = queue.enqueue(duplicate);

    assert!(result.is_err(), "Second enqueue should fail");
    let err = result.expect_err("Should have error");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("Duplicate epoch"),
        "Error should contain 'Duplicate epoch', got: {}",
        err_msg
    );
}

#[test]
fn test_crash_recovery_persistence() {
    let dir = tempfile::tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().to_path_buf();

    {
        let storage = Storage::open(&db_path).expect("Failed to open storage");
        let queue = WeightSubmissionQueue::new(storage.db()).expect("Failed to create queue");
        let weight = create_test_weight(10);
        queue.enqueue(weight).expect("Enqueue should succeed");
    }

    {
        let storage = Storage::open(&db_path).expect("Failed to reopen storage");
        let queue = WeightSubmissionQueue::new(storage.db()).expect("Failed to recreate queue");

        let recovered = queue
            .dequeue()
            .expect("Dequeue should succeed")
            .expect("Should have recovered weight");
        assert_eq!(recovered.epoch, 10, "Should recover epoch 10");
    }
}

#[test]
fn test_hotkey_rotation_clears_queue() {
    let (queue, _storage, _dir) = create_test_queue();

    queue
        .enqueue(create_test_weight(10))
        .expect("Enqueue epoch 10 should succeed");
    queue
        .enqueue(create_test_weight(20))
        .expect("Enqueue epoch 20 should succeed");

    queue.clear().expect("Clear should succeed");

    let dequeued = queue.dequeue().expect("Dequeue after clear should succeed");
    assert!(dequeued.is_none(), "Queue should be empty after clear");

    let result = queue.enqueue(create_test_weight(10));
    assert!(
        result.is_ok(),
        "Enqueue epoch 10 after clear should succeed"
    );
}
