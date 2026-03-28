# Validator Node

Main binary - P2P validator with WASM runtime.

## Overview

Orchestrates all Platform components: P2P networking, WASM execution, epoch management, weight submission.

## Where to Look

| Task | File | Notes |
|------|------|-------|
| Entry point | `main.rs` | #[tokio::main], CLI args |
| WASM executor | `wasm_executor.rs` | Challenge execution, policies |
| Challenge storage | `challenge_storage.rs` | Hot-reload, versioning |

## Key Functions

```
main()                    # Async runtime setup
handle_network_event()    # P2P message dispatch
handle_block_event()      # Bittensor block sync
process_wasm_evaluations() # Run challenges
update_validator_set_from_metagraph() # Sync stakes
```

## CLI Arguments

| Arg | Default | Notes |
|-----|---------|-------|
| --subtensor-endpoint | ws://127.0.0.1:9944 | Bittensor node |
| --netuid | 263 | Subnet ID |
| --p2p-port | 0 | Auto-assigned if 0 |
| --wasm-max-memory | 64MB | Per-challenge limit |
| --wasm-enable-fuel | true | Determinism enforcement |

## Start Sequence

1. Load keypair from `--secret-key` or mnemonic
2. Initialize sled storage
3. Sync from metagraph or bootstrap peers
4. Start P2P gossipsub + DHT
5. Start RPC server
6. Enter block event loop

## Security Patterns

- All state mutations use `mutate_and_persist()`
- Sudo actions verified before applying
- Signatures checked on all P2P messages

## Testing

```bash
# Integration tests require Docker
cargo test -p platform-e2e-tests --test integration_test
```

## Anti-Patterns

- DO NOT modify state without persist()
- ALWAYS verify signatures on P2P messages
- DO NOT bypass sudo authorization checks
