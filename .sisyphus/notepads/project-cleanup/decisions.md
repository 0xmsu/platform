# Project Cleanup Decisions

## 2025-03-26: ChallengeConfig → ChallengeRecord Rename

**Decision**: Renamed `ChallengeConfig` to `ChallengeRecord` in p2p-consensus crate.

**Rationale**: The struct represents chain state (id, name, weight, is_active, creator, created_at, wasm_hash), not configuration. "Record" is the correct semantic naming for persisted state entries.

**Implementation**:
- Renamed struct definition in `state.rs`
- Updated all usages within p2p-consensus crate
- Added deprecated type alias for backward compatibility: `#[deprecated(since = "0.2.0", note = "Use ChallengeRecord instead")] pub type ChallengeConfig = ChallengeRecord;`
- Updated lib.rs re-exports

**Verification**: `cargo build -p platform-p2p-consensus` succeeds
