# P2P Consensus Module

PBFT-style consensus with libp2p gossipsub + Kademlia DHT.

## Overview

Fully decentralized consensus for validator coordination. No central server - all communication via P2P mesh.

## Where to Look

| Task | File | Notes |
|------|------|-------|
| Consensus engine | `consensus.rs` | PBFT phases: Idle → PrePrepare → Prepared → Committed |
| Message types | `messages.rs` | All P2P message structs |
| Network layer | `network.rs` | gossipsub + DHT, peer scoring |
| Validator set | `validator.rs` | Stake-weighted voting, leader election |
| State management | `state.rs` | ChainState, pending mutations |

## Key Types

```
ConsensusEngine
├── current_view: ViewNumber
├── current_round: Option<ConsensusRound>
├── validator_set: Arc<ValidatorSet>
└── state_manager: Arc<StateManager>
```

## Consensus Flow

```
Leader creates Proposal → Broadcast PrePrepare → 
Validators send Prepare (2f+1 + 2/3 stake) → 
Commit → Decision finalized
```

## Security Patterns

**CRITICAL - sudo Authorization:**
- `StateChangeType::ConfigUpdate` requires `is_sudo()` check (lines 221, 363)
- Both `create_proposal()` and `handle_proposal()` verify sudo key

**IMPORTANT - sudo_key Protection:**
- `merge_from()` preserves local sudo_key (in core crate)
- Cannot be changed via peer sync - explicit SudoAction required

## Extension Points

- `StateChangeType` enum - add new mutation types
- `SudoAction` enum - add privileged operations (via core crate)
- Message handlers in main validator-node

## Anti-Patterns

- DO NOT bypass `is_sudo()` check for ConfigUpdate
- DO NOT copy sudo_key from peer state
- ALWAYS verify signature before processing P2P messages
