# Subtensor Connection Retry Refactoring

## TL;DR

> **Quick Summary**: Fix unstable Subtensor connections by implementing automatic retry/reconnect for all RPC operations. The `with_reconnect()` method exists but is never used - we need to make retry the default behavior across weight submission, metagraph sync, and validator operations.
>
> **Deliverables**:
> - All RPC operations wrapped with retry logic
> - Connection health checking before operations
> - Unified error handling for transport failures
> - Tests for retry scenarios
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 2 waves
> **Critical Path**: SubtensorClient refactor → weights.rs retry wrappers

---

## Context

### Original Request
"J'aimerai que tu refactors et que tu optimises tout, la connexion à subtensor des fois bug il y a des souscis dans le validateur es fois il arrete des et les weights, peut etre du a un noeud subtensor qui retire la connexion. La connexion doit etre reessayé lors de chaque requete, et pas garder la connexion"

### Root Cause Analysis

**The Problem**: The `SubtensorClient::with_reconnect()` method exists (lines 53-95 in client.rs) but is NEVER CALLED. All methods use `self.client()` which returns `&BittensorClient` directly without any retry:

```rust
// Current code (NO RETRY):
let tx_hash = set_weights(
    self.client.client()?,  // ← FAILS IMMEDIATELY ON DEAD CONNECTION
    ...
).await?;
```

**Evidence**:
- `weights.rs`: 15+ calls to `self.client.client()?` without retry
- `validator_sync.rs`: Calls `client.sync_metagraph()` without retry
- `block_sync.rs`: Has its own reconnection logic (only module with retry)

**Why BlockSync Works**: It monitors broadcast channel closes and recreates the client entirely. This is the correct pattern but not applied elsewhere.

### Interview Summary

**Key Findings**:
1. `block_sync.rs` already has auto-reconnect logic (recreates client on disconnect)
2. `weights.rs` and `MechanismWeightManager` have NO retry - they fail immediately
3. `validator_sync.rs` has NO retry for metagraph syncs
4. `with_reconnect()` in client.rs is dead code (never called)

**Architecture**:
```
SubtensorClient (client.rs)
├── HAS: with_reconnect<F>() method (UNUSED)
├── HAS: reconnect() method
├── USED BY: weights.rs, validator_sync.rs (NO RETRY)
└── USED BY: block_sync.rs (HAS OWN RECONNECT)
```

---

## Work Objectives

### Core Objective
Implement automatic connection retry for ALL Subtensor RPC operations to handle websocket disconnections gracefully.

### Concrete Deliverables
- `crates/bittensor-integration/src/client.rs` - Enhanced retry methods
- `crates/bittensor-integration/src/weights.rs` - All operations wrapped with retry
- `crates/bittensor-integration/src/validator_sync.rs` - Metagraph sync with retry
- Tests for retry scenarios

### Must Have
- Automatic retry on transport errors (websocket, connection reset, timeout)
- Configurable retry count (default: 3)
- Exponential backoff between retries

### Must NOT Have
- Breaking changes to public API signatures
- Retry on non-transport errors (business logic errors should fail fast)
- Infinite retry loops (must have upper bound)

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES (cargo test)
- **Automated tests**: Tests-after
- **Framework**: cargo test
- **Approach**: Run existing tests after each change, add new retry tests

### QA Policy
Every task MUST include agent-executed QA scenarios.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - client enhancement):
├── Task 1: Add retry config to SubtensorClient [quick]
└── Task 2: Add with_retry() method for generic operations [quick]

Wave 2 (After Wave 1 - usage migration):
├── Task 3: Wrap WeightSubmitter methods with retry [unspecified-high]
├── Task 4: Wrap MechanismWeightManager methods with retry [unspecified-high]
└── Task 5: Wrap ValidatorSync with retry [quick]

Wave FINAL (After ALL tasks - verification):
├── Task F1: Add retry scenario tests [quick]
├── Task F2: Run full test suite [quick]
└── Task F3: Integration verification [unspecified-high]
```

---

## TODOs

- [ ] 1. Add retry configuration to SubtensorClient

  **What to do**:
  - Add `RetryConfig` struct to client.rs with `max_retries` and `retry_delay_ms`
  - Add `retry_config` field to `SubtensorClient`
  - Add `with_retry_config()` builder method
  - Default: 3 retries, 1000ms delay

  **Must NOT do**:
  - DO NOT change existing constructor signatures
  - DO NOT break backward compatibility

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 2)
  - **Blocks**: Tasks 3, 4, 5
  - **Blocked By**: None

  **References**:
  - `crates/bittensor-integration/src/client.rs:11-17` - SubtensorClient struct
  - `crates/bittensor-integration/src/config.rs` - BittensorConfig pattern

  **Acceptance Criteria**:
  - [ ] RetryConfig struct defined
  - [ ] SubtensorClient has retry_config field
  - [ ] Default retry config applied
  - [ ] `cargo check -p platform-bittensor` passes

  **QA Scenarios**:
  ```
  Scenario: Retry config compiles
    Tool: Bash
    Steps:
      1. cargo check -p platform-bittensor 2>&1
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-01-check.log
  ```

- [ ] 2. Add with_retry() method for generic operations

  **What to do**:
  - Add async `with_retry<F, T>(&mut self, operation: F) -> Result<T>` method
  - Implement exponential backoff
  - Detect transport errors (websocket, connection, timeout)
  - Log retry attempts at INFO level

  **Pattern**:
  ```rust
  pub async fn with_retry<F, Fut, T>(&mut self, operation: F) -> Result<T>
  where
      F: FnMut(&mut Self) -> Fut,
      Fut: Future<Output = Result<T>>,
  {
      let mut attempts = 0;
      let max = self.retry_config.max_retries;
      
      loop {
          match operation(self).await {
              Ok(result) => return Ok(result),
              Err(e) if is_transport_error(&e) && attempts < max => {
                  attempts += 1;
                  let delay = self.retry_config.retry_delay_ms * (1 << attempts);
                  tokio::time::sleep(Duration::from_millis(delay)).await;
                  self.reconnect().await?;
              }
              Err(e) => return Err(e),
          }
      }
  }
  ```

  **Must NOT do**:
  - DO NOT retry on non-transport errors
  - DO NOT add infinite retry

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 1)
  - **Blocks**: Tasks 3, 4, 5
  - **Blocked By**: None

  **References**:
  - `crates/bittensor-integration/src/client.rs:53-95` - existing with_reconnect pattern
  - `bins/validator-node/src/main.rs:4885-4892` - is_transport_error pattern

  **Acceptance Criteria**:
  - [ ] with_retry method implemented
  - [ ] Transport error detection working
  - [ ] Exponential backoff implemented
  - [ ] `cargo check -p platform-bittensor` passes

  **QA Scenarios**:
  ```
  Scenario: with_retry compiles
    Tool: Bash
    Steps:
      1. cargo check -p platform-bittensor 2>&1
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-02-check.log
  ```

- [ ] 3. Wrap WeightSubmitter methods with retry

  **What to do**:
  - Refactor `submit_direct()` to use retry
  - Refactor `submit_with_commit_reveal()` to use retry
  - Refactor `reveal_pending_v2()` to use retry
  - Refactor `reveal_pending_mechanism_commits()` to use retry
  - Refactor `submit_mechanism_weights_batch_*` to use retry

  **Pattern change**:
  ```rust
  // BEFORE (fails immediately):
  let tx_hash = set_weights(self.client.client()?, ...).await?;
  
  // AFTER (retries on transport error):
  let tx_hash = self.client.with_retry(|c| async {
      set_weights(c.client()?, ...).await
  }).await?;
  ```

  **Must NOT do**:
  - DO NOT change method signatures
  - DO NOT retry on business logic errors

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multiple methods across large file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 2)
  - **Parallel Group**: Wave 2 (with Tasks 4, 5)
  - **Blocks**: None
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/bittensor-integration/src/weights.rs:295-320` - submit_direct
  - `crates/bittensor-integration/src/weights.rs:323-378` - submit_with_commit_reveal
  - `crates/bittensor-integration/src/weights.rs:380-407` - reveal_pending_v2
  - `crates/bittensor-integration/src/weights.rs:465-572` - submit_mechanism_weights_batch_crv4
  - `crates/bittensor-integration/src/weights.rs:574-600` - submit_mechanism_weights_batch_direct
  - `crates/bittensor-integration/src/weights.rs:699-754` - reveal_pending_mechanism_commits

  **Acceptance Criteria**:
  - [ ] All weight submission methods use retry
  - [ ] `cargo test -p platform-bittensor --lib` passes
  - [ ] No functional changes to behavior

  **QA Scenarios**:
  ```
  Scenario: Weight submission methods compile
    Tool: Bash
    Steps:
      1. cargo build -p platform-bittensor 2>&1
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-03-build.log
  ```

- [ ] 4. Wrap MechanismWeightManager methods with retry

  **What to do**:
  - Refactor `submit_mechanism_direct()` to use retry
  - Refactor `submit_mechanism_with_commit_reveal()` to use retry
  - Refactor `reveal_mechanism_pending_v2()` to use retry
  - Refactor `reveal_all_pending()` to use retry

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3, 5)
  - **Blocks**: None
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/bittensor-integration/src/weights.rs:1236-1269` - submit_mechanism_direct
  - `crates/bittensor-integration/src/weights.rs:1272-1333` - submit_mechanism_with_commit_reveal
  - `crates/bittensor-integration/src/weights.rs:1336-1367` - reveal_mechanism_pending_v2
  - `crates/bittensor-integration/src/weights.rs:1413-1426` - reveal_all_pending

  **Acceptance Criteria**:
  - [ ] All mechanism weight methods use retry
  - [ ] `cargo test -p platform-bittensor --lib` passes

- [ ] 5. Wrap ValidatorSync with retry

  **What to do**:
  - Modify `sync()` method to use retry for `sync_metagraph()` call
  - Add connection health check before sync

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3, 4)
  - **Blocks**: None
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/bittensor-integration/src/validator_sync.rs:77-133` - sync method

  **Acceptance Criteria**:
  - [ ] sync_metagraph() call wrapped with retry
  - [ ] `cargo test -p platform-bittensor --lib` passes

---

## Final Verification Wave

- [ ] F1. Add retry scenario tests
  Add unit tests that simulate connection failures and verify retry behavior.

- [ ] F2. Run full test suite
  Verify no regressions: `cargo test --workspace --all-features`

- [ ] F3. Integration verification
  Manual verification that weight submission recovers from simulated disconnects.

---

## Commit Strategy

- **After Wave 1**: `refactor(bittensor): add retry configuration to SubtensorClient`
- **After Wave 2**: `refactor(bittensor): wrap all RPC operations with automatic retry`

---

## Success Criteria

### Verification Commands
```bash
cargo build --workspace --all-features  # Expected: 0 errors
cargo test --workspace --all-features   # Expected: all tests pass
```

### Final Checklist
- [ ] RetryConfig added to SubtensorClient
- [ ] with_retry() method implemented with exponential backoff
- [ ] WeightSubmitter methods wrapped with retry
- [ ] MechanismWeightManager methods wrapped with retry
- [ ] ValidatorSync wrapped with retry
- [ ] Tests for retry scenarios added
