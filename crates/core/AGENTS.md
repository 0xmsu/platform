# Core Types Module

Shared types, crypto (sr25519), state management for Platform.

## Overview

Foundation crate - all other crates depend on this. Contains ChainState, crypto primitives, and network messages.

## Where to Look

| Task | File | Notes |
|------|------|-------|
| ChainState | `state.rs` | Global state struct, apply_sudo_action() |
| Crypto | `crypto.rs` | Keypair, sr25519 signing, Hotkey |
| Messages | `message.rs` | NetworkMessage, SudoAction enum |
| Constants | `constants.rs` | SUDO_KEY_BYTES, production keys |
| Checkpoint | `checkpoint.rs` | State snapshots, restoration |

## Key Types

```
ChainState
├── sudo_key: Hotkey          # PROTECTED - see merge_from()
├── validators: HashMap<Hotkey, ValidatorInfo>
├── challenges: HashMap<ChallengeId, Challenge>
├── wasm_challenge_configs
├── mutation_sequence: u64    # For sync ordering
└── state_hash: [u8; 32]

SudoAction (enum)
├── UpdateConfig, SetEmission, SetMechanism
├── AddValidator, RemoveValidator
├── EmergencyPause, Resume
├── ForceStateUpdate         # PRESERVES sudo_key
└── RenameChallenge, RemoveChallenge
```

## Security Patterns

**CRITICAL - sudo_key Protection:**
```rust
// In merge_from() - line 738+
// SECURITY: Preserve the local sudo_key
let local_sudo_key = self.sudo_key.clone();
// ... merge other fields ...
self.sudo_key = local_sudo_key;  // RESTORE
```

**IMPORTANT - ForceStateUpdate:**
- Preserves local sudo_key even when replacing entire state
- Attacker cannot change sudo via ForceStateUpdate

**ALWAYS - serde(default):**
- New fields MUST have `#[serde(default)]` for backward compatibility

## Anti-Patterns

- NEVER copy sudo_key from peer_state in merge_from
- NEVER change sudo_key via ForceStateUpdate (it's preserved)
- ALWAYS add #[serde(default)] to new ChainState fields

## Extension Points

- Add new SudoAction variants (requires consensus update)
- Extend ChainState with new fields (with serde(default))
- New message types in message.rs

## Testing

```bash
cargo test -p platform-core
# Includes security_sudo_tests for sudo_key protection
```
