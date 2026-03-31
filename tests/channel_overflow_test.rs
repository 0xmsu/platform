//! Channel Overflow Tests
//!
//! Tests documenting the P2P channel overflow bug.
//!
//! BUG: When storage proposals burst (1000+ in 100ms), the channel fills
//! and try_send() fails with "no available capacity" (TrySendError::Full).
//! Heartbeat messages and other critical broadcasts get dropped.
//!
//! Production channel: 4096 capacity (main.rs:500-501)
//! Issue: All broadcast sites use try_send() which fails immediately.

use platform_core::ChallengeId;
use platform_p2p_consensus::{
    HeartbeatMessage, P2PCommand, P2PMessage, StorageProposalMessage, ValidatorCapabilities,
};
use tokio::sync::mpsc;

mod channel_overflow_tests {
    use super::*;

    /// Test: Channel should handle 1000 proposals in 100ms burst
    ///
    /// Current Behavior (BUG): try_send() fails with TrySendError::Full
    /// when channel buffer is exhausted.
    ///
    /// Expected Behavior (FIX): Channel should either:
    /// - Use backpressure/blocking for critical messages
    /// - Have separate high-priority channel for heartbeats
    /// - Implement priority queuing
    #[tokio::test]
    async fn test_channel_overflow_on_proposal_burst() {
        let (tx, _rx) = mpsc::channel::<P2PCommand>(10);

        let total_proposals = 1000;
        let mut failed_sends = 0;

        for i in 0..total_proposals {
            let proposal = create_test_storage_proposal(i);
            let cmd = P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal));

            if tx.try_send(cmd).is_err() {
                failed_sends += 1;
            }
        }

        // EXPECTED: All proposals should be handleable
        // ACTUAL (BUG): Channel overflows, many proposals dropped
        // This test FAILS until the bug is fixed
        assert_eq!(
            failed_sends, 0,
            "BUG: Channel overflow detected. {} out of {} proposals were dropped. \
             try_send() returns 'no available capacity' (TrySendError::Full).",
            failed_sends,
            total_proposals
        );
    }

    /// Test: Heartbeats should succeed even during proposal flood
    ///
    /// Current Behavior (BUG): Heartbeat messages are dropped when channel
    /// is congested with storage proposals, using same try_send() path.
    ///
    /// Expected Behavior (FIX): Heartbeats should use priority mechanism
    /// or separate channel to ensure delivery.
    #[tokio::test]
    async fn test_heartbeat_fails_during_proposal_flood() {
        let (tx, _rx) = mpsc::channel::<P2PCommand>(10);

        // Flood channel with proposals
        for i in 0..15 {
            let proposal = create_test_storage_proposal(i);
            let _ = tx.try_send(P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal)));
        }

        // Attempt to send heartbeat - critical for network health
        let heartbeat = create_test_heartbeat();
        let heartbeat_cmd = P2PCommand::Broadcast(P2PMessage::Heartbeat(heartbeat));
        let result = tx.try_send(heartbeat_cmd);

        // EXPECTED: Heartbeat should succeed (priority message)
        // ACTUAL (BUG): Heartbeat fails with TrySendError::Full
        // This test FAILS until the bug is fixed
        assert!(
            result.is_ok(),
            "BUG: Heartbeat failed to send during proposal flood. \
             Critical network health messages are dropped when channel is congested. \
             Error: {:?}",
            result.err()
        );
    }

    /// Test: Burst pattern should allow interleaved heartbeats
    ///
    /// Current Behavior (BUG): When proposals burst, heartbeats stuck
    /// behind proposals in queue, may be dropped.
    ///
    /// Expected Behavior (FIX): Heartbeats should have priority.
    #[tokio::test]
    async fn test_burst_pattern_blocks_heartbeats() {
        let (tx, _rx) = mpsc::channel::<P2PCommand>(100);

        let total_heartbeats = 50;
        let mut heartbeats_failed = 0;

        for batch in 0..total_heartbeats {
            // Send 20 proposals per heartbeat
            for i in 0..20 {
                let proposal = create_test_storage_proposal(batch * 100 + i);
                let _ = tx.try_send(P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal)));
            }

            // Send heartbeat - should succeed even under load
            let heartbeat = create_test_heartbeat();
            let cmd = P2PCommand::Broadcast(P2PMessage::Heartbeat(heartbeat));
            if tx.try_send(cmd).is_err() {
                heartbeats_failed += 1;
            }
        }

        // EXPECTED: No heartbeats should fail
        // ACTUAL (BUG): Many heartbeats dropped due to channel congestion
        // This test FAILS until the bug is fixed
        assert_eq!(
            heartbeats_failed, 0,
            "BUG: {} out of {} heartbeats were dropped during proposal burst. \
             Critical messages should not be dropped.",
            heartbeats_failed,
            total_heartbeats
        );
    }
}

fn create_test_storage_proposal(id: usize) -> StorageProposalMessage {
    use platform_core::Keypair;
    use std::time::{SystemTime, UNIX_EPOCH};

    let keypair = Keypair::generate();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as i64;

    let mut proposal_id = [0u8; 32];
    let id_bytes = id.to_le_bytes();
    proposal_id[..8].copy_from_slice(&id_bytes);

    let challenge_id = ChallengeId::new(&format!("test-challenge-{}", id % 10));

    StorageProposalMessage {
        proposal_id,
        challenge_id,
        proposer: keypair.hotkey(),
        key: format!("key-{}", id).into_bytes(),
        value: format!("value-{}", id).into_bytes(),
        timestamp,
        signature: vec![0; 64],
    }
}

fn create_test_heartbeat() -> HeartbeatMessage {
    use platform_core::Keypair;
    use std::time::{SystemTime, UNIX_EPOCH};

    let keypair = Keypair::generate();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as i64;

    HeartbeatMessage {
        validator: keypair.hotkey(),
        state_hash: [0u8; 32],
        core_state_hash: [0u8; 32],
        sequence: 1,
        stake: 10_000_000,
        timestamp,
        signature: vec![0; 64],
        capabilities: ValidatorCapabilities::default(),
    }
}
