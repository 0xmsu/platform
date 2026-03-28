# Integration Tests

End-to-end and integration tests for Platform.

## Overview

Separate workspace member (`platform-e2e-tests`). Tests run via cargo-nextest with explicit `[[test]]` targets.

## Files

| File | Purpose |
|------|---------|
| e2e_tests.rs | End-to-end storage/challenge flow |
| wasm_challenge_tests.rs | WASM + PBFT consensus integration |
| epoch_tests.rs | Epoch phase, commit-reveal |
| storage_tests.rs | Persistence with tempfile |
| security_sudo_tests.rs | Sudo key protection tests |
| error_cases.rs | Edge cases, corruption handling |
| rpc_server_tests.rs | RPC endpoint testing |

## Test Patterns

**Fixture Helpers:**
```rust
fn create_chain_state() -> Arc<RwLock<ChainState>>
fn create_five_validators() -> (Keypair, Vec<Keypair>)
fn create_populated_chain_state() -> Arc<RwLock<ChainState>>
```

**Mock Challenge:**
```rust
struct MockChallenge { should_fail: bool }
impl ServerChallenge for MockChallenge { ... }
```

**Async Testing:**
```rust
#[tokio::test]
async fn test_async_flow() {
    let (tx, rx) = mpsc::channel(100);
    tokio::time::timeout(Duration::from_secs(5), async { ... }).await;
}
```

## Running Tests

```bash
# All tests (excludes live/integration/e2e by default)
cargo test -p platform-e2e-tests

# Integration tests (requires Docker)
cargo test -p platform-e2e-tests --test integration_test

# WASM tests (feature-gated)
cargo test -p platform-e2e-tests --features wasm-tests
```

## Anti-Patterns

- DO NOT use `unwrap()` in tests - use `.expect("message")`
- DO NOT share state between tests - use fresh fixtures
- ALWAYS use `tempfile::tempdir()` for storage isolation
