//! Channel Overflow Tests
//!
//! Tests for P2P channel handling with send_timeout backpressure.
//! With send_timeout, messages wait up to the configured timeout for channel capacity.
//! Critical messages (consensus) use longer timeouts (500ms) while low-priority
//! messages (storage) use shorter timeouts (10ms).

use platform_core::ChallengeId;
use platform_p2p_consensus::{
    HeartbeatMessage, P2PCommand, P2PMessage, StorageProposalMessage, ValidatorCapabilities,
    MessagePriority, ConsensusProposal, ProposalContent, StateChangeType,
};
use std::time::Duration;
use tokio::sync::mpsc;

mod channel_overflow_tests {
    use super::*;

    /// Test: Channel handles burst with backpressure when consumer is active
    ///
    /// With a consumer actively draining, send_timeout provides backpressure
    /// and all proposals should succeed within their timeout window.
    #[tokio::test]
    async fn test_channel_overflow_on_proposal_burst() {
        let (tx, mut rx) = mpsc::channel::<P2PCommand>(10);

        // Spawn consumer that drains the channel
        let consumer = tokio::spawn(async move {
            while let Some(_cmd) = rx.recv().await {
                // Drain messages
            }
        });

        let total_proposals = 1000;
        let mut failed_sends = 0;

        for i in 0..total_proposals {
            let proposal = create_test_storage_proposal(i);
            let cmd = P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal));

            // Use LOW priority timeout for storage proposals
            let timeout = MessagePriority::Low.timeout();
            match tokio::time::timeout(timeout, tx.send(cmd)).await {
                Ok(_) => {}
                Err(_) => {
                    failed_sends += 1;
                }
            }
        }

        // Drop sender to stop consumer
        drop(tx);
        let _ = consumer.await;

        // With backpressure and active consumer, most proposals should succeed
        // Allow up to 5% failures due to timing in burst scenarios
        let acceptable_failures = (total_proposals as f64 * 0.05) as usize;
        assert!(
            failed_sends <= acceptable_failures,
            "Too many failures: {} out of {} proposals failed (max acceptable: {})",
            failed_sends,
            total_proposals,
            acceptable_failures
        );
    }

    /// Test: Heartbeats succeed during proposal flood with backpressure
    ///
    /// Heartbeats use MEDIUM priority (100ms timeout) which is longer than
    /// LOW priority (10ms), giving them more time to get through.
    #[tokio::test]
    async fn test_heartbeat_succeeds_during_proposal_flood() {
        let (tx, mut rx) = mpsc::channel::<P2PCommand>(10);

        // Spawn consumer that drains the channel (simulates real P2P processing)
        let consumer = tokio::spawn(async move {
            while let Some(_cmd) = rx.recv().await {
                // Drain messages
            }
        });

        // Flood channel with proposals using LOW priority
        for i in 0..50 {
            let proposal = create_test_storage_proposal(i);
            let cmd = P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal));
            let timeout = MessagePriority::Low.timeout();
            let _ = tokio::time::timeout(timeout, tx.send(cmd)).await;
        }

        // Send heartbeat with MEDIUM priority (longer timeout)
        let heartbeat = create_test_heartbeat();
        let heartbeat_cmd = P2PCommand::Broadcast(P2PMessage::Heartbeat(heartbeat));
        let heartbeat_timeout = MessagePriority::Medium.timeout();
        let result = tokio::time::timeout(heartbeat_timeout, tx.send(heartbeat_cmd)).await;

        // Cleanup
        drop(tx);
        let _ = consumer.await;

        assert!(
            result.is_ok(),
            "Heartbeat should succeed with backpressure (MEDIUM priority 100ms timeout). \
             Error: {:?}",
            result.err()
        );
    }

    /// Test: Burst pattern allows heartbeats through with backpressure
    ///
    /// When channel has active consumer, heartbeats should not be blocked
    /// by proposal bursts due to backpressure working.
    #[tokio::test]
    async fn test_burst_pattern_allows_heartbeats() {
        let (tx, mut rx) = mpsc::channel::<P2PCommand>(100);

        // Spawn consumer that actively drains the channel
        let consumer = tokio::spawn(async move {
            while let Some(_cmd) = rx.recv().await {
                // Drain messages
            }
        });

        let total_heartbeats = 50;
        let mut heartbeats_failed = 0;

        for batch in 0..total_heartbeats {
            // Send 20 proposals per heartbeat using LOW priority
            for i in 0..20 {
                let proposal = create_test_storage_proposal(batch * 100 + i);
                let cmd = P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal));
                let timeout = MessagePriority::Low.timeout();
                let _ = tokio::time::timeout(timeout, tx.send(cmd)).await;
            }

            // Send heartbeat with MEDIUM priority - should succeed
            let heartbeat = create_test_heartbeat();
            let cmd = P2PCommand::Broadcast(P2PMessage::Heartbeat(heartbeat));
            let heartbeat_timeout = MessagePriority::Medium.timeout();
            if tokio::time::timeout(heartbeat_timeout, tx.send(cmd)).await.is_err() {
                heartbeats_failed += 1;
            }
        }

        // Cleanup
        drop(tx);
        let _ = consumer.await;

        // Allow up to 2% failures due to timing
        let acceptable_failures = (total_heartbeats as f64 * 0.02) as usize;
        assert!(
            heartbeats_failed <= acceptable_failures,
            "Too many heartbeat failures: {} out of {} failed (max acceptable: {})",
            heartbeats_failed,
            total_heartbeats,
            acceptable_failures
        );
    }

    /// Test: CRITICAL priority messages get longer timeout
    ///
    /// Proposal messages use CRITICAL priority with 500ms timeout,
    /// ensuring consensus-critical messages have more time to get through.
    #[tokio::test]
    async fn test_critical_messages_have_longer_timeout() {
        let (tx, mut rx) = mpsc::channel::<P2PCommand>(10);

        // Spawn consumer that drains slowly
        let consumer = tokio::spawn(async move {
            while let Some(_cmd) = rx.recv().await {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        // Flood with LOW priority messages
        for i in 0..30 {
            let proposal = create_test_storage_proposal(i);
            let cmd = P2PCommand::Broadcast(P2PMessage::StorageProposal(proposal));
            let _ = tokio::time::timeout(MessagePriority::Low.timeout(), tx.send(cmd)).await;
        }

        // CRITICAL message should succeed with longer timeout
        let critical_msg = create_test_proposal();
        let critical_cmd = P2PCommand::Broadcast(P2PMessage::Proposal(critical_msg));
        let critical_timeout = MessagePriority::Critical.timeout();
        let result = tokio::time::timeout(critical_timeout, tx.send(critical_cmd)).await;

        // Cleanup
        drop(tx);
        let _ = consumer.await;

        assert!(
            result.is_ok(),
            "CRITICAL priority message should succeed with 500ms timeout"
        );
    }

    /// Test: verify timeout values match spec
    #[test]
    fn test_priority_timeouts_match_spec() {
        assert_eq!(MessagePriority::Critical.timeout(), Duration::from_millis(500));
        assert_eq!(MessagePriority::High.timeout(), Duration::from_millis(200));
        assert_eq!(MessagePriority::Medium.timeout(), Duration::from_millis(100));
        assert_eq!(MessagePriority::Low.timeout(), Duration::from_millis(10));
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

fn create_test_proposal() -> ConsensusProposal {
    use platform_core::Keypair;
    use std::time::{SystemTime, UNIX_EPOCH};

    let keypair = Keypair::generate();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as i64;

    ConsensusProposal {
        view: 1,
        sequence: 1,
        proposal: ProposalContent {
            change_type: StateChangeType::ChallengeSubmission,
            data: vec![],
            data_hash: [0u8; 32],
        },
        proposer: keypair.hotkey(),
        signature: vec![0; 64],
        timestamp,
    }
}
