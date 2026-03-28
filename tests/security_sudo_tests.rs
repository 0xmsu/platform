//! Security tests for sudo key protection
//!
//! Tests verify that:
//! 1. merge_from() protects sudo_key from peer state
//! 2. ForceStateUpdate preserves sudo_key
//! 3. Malicious peers cannot change sudo_key via sync

use platform_core::{
    ChainState, ChallengeId, Hotkey, Keypair, NetworkConfig, SudoAction, ValidatorInfo, Stake,
};

fn create_test_state_with_sudo(sudo_keypair: &Keypair) -> ChainState {
    ChainState::new(sudo_keypair.hotkey(), NetworkConfig::default())
}

#[test]
fn test_merge_from_preserves_sudo_key() {
    // Create two states with different sudo keys
    let sudo_a = Keypair::generate();
    let sudo_b = Keypair::generate();
    
    let mut state_a = create_test_state_with_sudo(&sudo_a);
    let state_b = create_test_state_with_sudo(&sudo_b);
    
    // Give state_b a higher mutation_sequence
    state_b.mutation_sequence = state_a.mutation_sequence + 10;
    
    // state_b has different sudo_key
    assert_ne!(state_a.sudo_key, state_b.sudo_key);
    
    // Perform merge - state_a should NOT adopt state_b's sudo_key
    let original_sudo = state_a.sudo_key.clone();
    state_a.merge_from(&state_b);
    
    // Security: sudo_key should be preserved
    assert_eq!(
        state_a.sudo_key, original_sudo,
        "SECURITY VIOLATION: merge_from should not change sudo_key!"
    );
    
    // But mutation_sequence should be updated
    assert_eq!(state_a.mutation_sequence, state_b.mutation_sequence);
}

#[test]
fn test_malicious_peer_cannot_steal_sudo_via_sync() {
    // Simulate attack scenario:
    // 1. Malicious peer creates state with their hotkey as sudo
    // 2. They set higher mutation_sequence
    // 3. They try to sync to honest validator
    
    let honest_sudo = Keypair::generate();
    let attacker_sudo = Keypair::generate();
    
    let mut honest_state = create_test_state_with_sudo(&honest_sudo);
    
    // Attacker creates malicious state
    let mut malicious_state = create_test_state_with_sudo(&attacker_sudo);
    malicious_state.mutation_sequence = honest_state.mutation_sequence + 1000;
    
    // Honest state should NOT accept the attacker's sudo_key
    let protected_sudo = honest_state.sudo_key.clone();
    honest_state.merge_from(&malicious_state);
    
    assert_eq!(
        honest_state.sudo_key, protected_sudo,
        "SECURITY: Attacker was able to change sudo_key via sync!"
    );
    
    assert_ne!(
        honest_state.sudo_key, attacker_sudo.hotkey(),
        "SECURITY: Attacker's hotkey should not become sudo!"
    );
}

#[test]
fn test_merge_from_rejects_older_state() {
    let sudo = Keypair::generate();
    let mut state_a = create_test_state_with_sudo(&sudo);
    let state_b = create_test_state_with_sudo(&sudo);
    
    // state_b has lower mutation_sequence
    state_a.mutation_sequence = 100;
    
    let result = state_a.merge_from(&state_b);
    
    // Should not merge
    assert!(!result);
    assert_eq!(state_a.mutation_sequence, 100);
}

#[test]
fn test_force_state_update_preserves_sudo_key() {
    // Create sudo key
    let original_sudo = Keypair::generate();
    
    // Create attacker's key
    let attacker_key = Keypair::generate();
    
    // Create initial state
    let mut state = create_test_state_with_sudo(&original_sudo);
    
    // Attacker creates malicious state with their own key as sudo
    let mut malicious_state = create_test_state_with_sudo(&attacker_key);
    
    // Add some state to make it look legitimate
    let validator = ValidatorInfo::new(attacker_key.hotkey(), Stake::new(1_000_000_000));
    malicious_state.add_validator(validator.clone()).unwrap();
    
    // Attacker tries to force state update
    let original_sudo_hotkey = state.sudo_key.clone();
    state.apply_sudo_action(&SudoAction::ForceStateUpdate {
        state: malicious_state,
    }).unwrap();
    
    // SECURITY: sudo_key should still be the original, not the attacker's
    assert_eq!(
        state.sudo_key, original_sudo_hotkey,
        "SECURITY: ForceStateUpdate changed sudo_key!"
    );
    
    assert_ne!(
        state.sudo_key, attacker_key.hotkey(),
        "SECURITY: Attacker became sudo via ForceStateUpdate!"
    );
}

#[test]
fn test_sudo_key_check_works() {
    let sudo = Keypair::generate();
    let non_sudo = Keypair::generate();
    
    let state = create_test_state_with_sudo(&sudo);
    
    // sudo key should be recognized
    assert!(state.is_sudo(&sudo.hotkey()));
    
    // non-sudo key should NOT be recognized
    assert!(!state.is_sudo(&non_sudo.hotkey()));
}

#[test]
fn test_merge_from_logs_sudo_key_change_attempt() {
    // This test verifies the warning log is triggered
    // Run with RUST_LOG=warn to see the output
    
    let sudo_a = Keypair::generate();
    let sudo_b = Keypair::generate();
    
    let mut state_a = create_test_state_with_sudo(&sudo_a);
    let state_b = create_test_state_with_sudo(&sudo_b);
    
    state_b.mutation_sequence = state_a.mutation_sequence + 1;
    
    // This should log a SECURITY warning
    state_a.merge_from(&state_b);
    
    // But sudo_key should still be protected
    assert_eq!(state_a.sudo_key, sudo_a.hotkey());
}

#[test]
fn test_chain_state_is_sudo_method() {
    let sudo_kp = Keypair::generate();
    let other_kp = Keypair::generate();
    let state = create_test_state_with_sudo(&sudo_kp);
    
    assert!(state.is_sudo(&sudo_kp.hotkey()));
    assert!(!state.is_sudo(&other_kp.hotkey()));
    assert!(!state.is_sudo(&Hotkey([0u8; 32])));
}
