# Audit Report: Consensus & P2P Sync — Storage, Leaderboard, and State Replication

## Executive Summary

The platform uses a multi-layered consensus system:
1. **PBFT consensus** (`p2p-consensus/consensus.rs`) for major state changes (submissions, evaluations, weight updates, epoch transitions)
2. **Storage proposal/vote mechanism** (`p2p-consensus/state.rs` + `validator-node/main.rs`) for challenge storage writes
3. **State root consensus** (`distributed-storage/state_consensus.rs`) for cross-validator state verification with fraud proofs
4. **Validated storage** (`distributed-storage/validated_storage.rs`) as a layered consensus-before-write abstraction

**Critical Finding**: WASM route handlers bypass consensus entirely for storage writes, creating a fundamental divergence risk between validators.

---

## 1. Consensus Mechanism Architecture

### 1.1 PBFT Consensus Engine
- **File**: `crates/p2p-consensus/src/consensus.rs`
- Classic PBFT: PrePrepare → Prepare → Commit phases
- **Quorum**: 2f+1 count-based quorum **AND** 2/3 stake-weighted threshold
- View changes on timeout (30s round timeout, 60s view change timeout)
- All messages are cryptographically signed and verified
- Leader election is view-based, rotating through validators

### 1.2 State Change Types via PBFT
```
ChallengeSubmission, EvaluationResult, WeightUpdate,
ValidatorChange, ConfigUpdate (sudo only), EpochTransition
```

### 1.3 P2P Network Layer
- **File**: `crates/p2p-consensus/src/network.rs`
- libp2p gossipsub + Kademlia DHT
- All messages wrapped in `SignedP2PMessage` with replay protection (nonce tracking with 5-min expiry) and rate limiting (100 msg/s per signer)
- Validator-only enforcement for consensus traffic
- Signer identity verification against message sender field

---

## 2. Storage Write Path Analysis

### 2.1 When WASM calls `host_storage_set()` — **CRITICAL FINDING**

**Path**: `challenge-sdk-wasm/host_functions.rs` → `wasm-runtime-interface/storage.rs:handle_storage_set()` → `StorageBackend::propose_write()`

**Finding**: In the validator node's `wasm_executor.rs`, all WASM execution contexts are configured with:
```rust
storage_host_config: StorageHostConfig {
    allow_direct_writes: true,
    require_consensus: false,
    ..
}
```
This is set at lines 204-205, 332-333, 811-812, and 969-970 of `wasm_executor.rs`.

**Consequence**: When WASM code calls `host_storage_set()`, the `handle_storage_set` function checks:
```rust
if storage.config.require_consensus && !storage.config.allow_direct_writes {
    return StorageHostStatus::ConsensusRequired;
}
```
Since `allow_direct_writes=true` and `require_consensus=false`, this check passes and the write goes directly to the local `ChallengeStorageBackend` — **NO consensus, NO P2P proposal, NO replication to other validators**.

The `ChallengeStorageBackend` (`challenge_storage.rs`) directly writes to the local sled database via `LocalStorage::put()`.

### Severity: **HIGH**
**Impact**: If two validators process the same WASM route handler request, each writes to its own local storage independently. There is no mechanism to reconcile these writes across validators. This means different validators can have completely different storage states for the same challenge.

---

### 2.2 P2P Storage Proposal/Vote Mechanism

**Files**: `p2p-consensus/src/state.rs:StorageProposal`, `validator-node/main.rs:1912-2035`

A separate consensus mechanism exists for storage writes propagated via P2P:

1. A validator broadcasts `P2PMessage::StorageProposal` 
2. Other validators **auto-approve** the proposal (line ~1948 in main.rs):
   ```rust
   // Auto-vote approve (validator trusts other validators)
   // In production, could verify via WASM validate_storage_write
   ```
3. Votes are tallied using simple majority: `threshold = (total_validators / 2) + 1`
4. On consensus approval, data is written to distributed storage

### Severity: **MEDIUM**
**Issue 1 — Auto-approval**: Validators auto-approve all storage proposals from known validators without running WASM validation. The comment explicitly says "In production, could verify via WASM validate_storage_write" — this validation is not implemented.

**Issue 2 — Disconnected from WASM path**: The P2P storage proposal mechanism is separate from the WASM `host_storage_set()` path. WASM writes go directly to local storage; the P2P proposal path exists but is not invoked by WASM host functions.

**Issue 3 — Simple majority vs PBFT**: Storage proposals use simple majority voting `(n/2)+1` in `ChainState::vote_storage_proposal()`, not the full PBFT 2f+1 + stake-weighted quorum used for main consensus. This is weaker Byzantine fault tolerance.

---

## 3. Leaderboard Consistency

### Finding: Leaderboard is not actively synced

**Observation**: `ChainState` has a `leaderboard: HashMap<ChallengeId, Vec<LeaderboardEntry>>` field, and `update_leaderboard()` exists to update it. However:

- The validator node (`main.rs`) only **logs** `LeaderboardRequest` and `LeaderboardResponse` messages at debug level — it does not process them or update state
- There is no code in the validator that calls `update_leaderboard()` 
- Leaderboard data in `ChainState` is part of the state that gets synced via `StateResponse`, but leaderboard updates themselves are never actively populated

### Severity: **MEDIUM**
**Impact**: Leaderboard data is effectively empty or stale across all validators. If any validator does populate it locally, there's no mechanism to propagate those updates consistently.

---

## 4. State Replication & Sync

### 4.1 State Sync via StateRequest/StateResponse

The `ChainState` (in `p2p-consensus/state.rs`) is the canonical shared state. Sync works via:
1. `StateRequest`: A validator requests full state, sending its current hash and sequence
2. `StateResponse`: Another validator responds with full serialized state
3. `StateManager::apply_sync_state()` accepts the new state only if:
   - New sequence > current sequence
   - Hash verification passes (recomputed hash matches claimed hash)

### Severity: **LOW**
**Issue**: The hash function in `ChainState::update_hash()` only hashes a summary (sequence, epoch, counts) — NOT the actual data. Two states with the same sequence/epoch/counts but different data would have the same hash. This weakens state verification.

```rust
struct HashInput {
    sequence: SequenceNumber,
    epoch: u64,
    validator_count: usize,
    challenge_count: usize,
    pending_count: usize,
    netuid: u16,
}
```

The hash doesn't include actual validator stakes, challenge configs, evaluation data, storage proposals, leaderboards, or any other substantive state fields.

---

## 5. Epoch Transition & Storage State

### How epoch transitions work:
1. Triggered by `BlockSyncEvent::EpochTransition` from Bittensor block sync
2. Calls `state.next_epoch()` which:
   - Increments epoch counter
   - Clears current `weight_votes`
   - Prunes historical weights older than 100 epochs
   - Increments sequence number

### Finding: Epoch transitions are locally triggered
Each validator detects epoch boundaries independently from Bittensor block sync. There's no PBFT consensus on when to transition epochs — validators rely on seeing the same Bittensor blocks.

### Severity: **LOW**
If validators have slightly different views of the Bittensor chain (e.g., different RPC endpoints, temporary fork), they could transition epochs at different times, causing temporary state divergence.

---

## 6. Scenarios Where Validators Could Have Divergent State

### 6.1 WASM Storage Writes (HIGH)
- **Scenario**: Two validators both process a WASM route request that calls `host_storage_set()`
- **Result**: Each writes to its own local storage; no P2P propagation occurs
- **Divergence**: Permanent until full state sync overrides one validator's data

### 6.2 Race Conditions in Storage Proposals (MEDIUM)
- **Scenario**: Two validators simultaneously propose writes to the same key with different values
- **Result**: Each proposal gets its own consensus round; both could succeed
- **Divergence**: Last-write-wins at the distributed storage level, but ordering may differ

### 6.3 Network Partition During Consensus (LOW)
- **Scenario**: Network partition during PBFT round
- **Result**: View change triggers new leader; prepared state carries forward
- **Mitigation**: PBFT view change protocol properly handles this; prepared proofs are verified

### 6.4 Stale State Sync (LOW)
- **Scenario**: Validator rejoins after being offline
- **Result**: `apply_sync_state()` accepts newer state but relies on weak hash verification
- **Mitigation**: Sequence number ordering prevents accepting old state

### 6.5 Leaderboard Inconsistency (MEDIUM)
- **Scenario**: Leaderboard is never populated via consensus
- **Result**: All validators have empty/stale leaderboards
- **Impact**: Any leaderboard queries return inconsistent or empty data

---

## 7. Conflict Resolution for Concurrent Writes

### PBFT Path
The PBFT consensus engine serializes state changes through sequence numbers. Only one proposal can be active per round, so concurrent writes are inherently serialized.

### Storage Proposal Path
Multiple storage proposals can be in flight simultaneously since each gets a unique `proposal_id`. The `ChainState` tracks them in `pending_storage_proposals`. If two proposals write the same key, both can be approved and applied — the last one applied wins.

### Direct WASM Write Path
No conflict resolution exists. Each validator writes independently to local storage.

---

## 8. Summary of Findings

| # | Finding | Severity | Component |
|---|---------|----------|-----------|
| 1 | WASM `host_storage_set()` bypasses consensus — writes directly to local storage with `allow_direct_writes=true, require_consensus=false` | **HIGH** | `wasm_executor.rs`, `storage.rs` |
| 2 | Storage proposals auto-approved without WASM validation | **MEDIUM** | `validator-node/main.rs:1948` |
| 3 | Leaderboard data never populated via consensus; request/response messages only logged | **MEDIUM** | `validator-node/main.rs:1666-1681` |
| 4 | `ChainState` hash only covers summary counts, not actual state data | **MEDIUM** | `p2p-consensus/state.rs:update_hash()` |
| 5 | Storage proposals use simple majority `(n/2)+1`, weaker than PBFT's 2f+1 + stake-weighted quorum | **LOW** | `p2p-consensus/state.rs:vote_storage_proposal()` |
| 6 | Epoch transitions triggered locally per validator from Bittensor sync, not via PBFT consensus | **LOW** | `validator-node/main.rs:2123-2126` |
| 7 | `P2PMessage::StorageProposal` path is disconnected from WASM `host_storage_set()` path | **HIGH** | Architecture gap |
| 8 | No mechanism to reconcile divergent local storage across validators | **HIGH** | System-wide |

---

## 9. Positive Security Controls Observed

- PBFT consensus properly implements cryptographic signature verification on all phases
- Replay attack protection with nonce tracking and sliding window rate limiting
- Validator-only enforcement for consensus-critical messages
- View change protocol with prepared proof verification
- ConfigUpdate proposals require sudo authorization
- Weight vote hash verification
- State deserialization size limits (256MB)
- Evaluation scores use verified stakes from validator map (prevents stake inflation)
- Fraud proof mechanism exists in `state_consensus.rs` (though integration unclear)

---

## 10. Recommendations

1. **Critical**: Route WASM `host_storage_set()` through the P2P storage proposal path, or set `require_consensus=true` and `allow_direct_writes=false` in production
2. **High**: Implement actual WASM validation in storage proposal handling instead of auto-approval
3. **Medium**: Include actual state data in `ChainState::update_hash()` (at minimum, hash the serialized state or use a merkle root)
4. **Medium**: Implement leaderboard population and synchronization via consensus
5. **Low**: Consider using the same 2f+1 + stake-weighted quorum for storage proposals as for PBFT consensus
6. **Low**: Consider using a PBFT proposal for epoch transitions to ensure atomic, coordinated transitions
