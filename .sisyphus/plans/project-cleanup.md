# Project Cleanup - Platform

## TL;DR

> **Quick Summary**: Comprehensive cleanup of Platform Rust codebase - fix sp-core version conflicts, consolidate duplicate types with proper semantic naming, remove dead code, delete unused mock infrastructure, and document unsafe code blocks.
> 
> **Deliverables**:
> - Unified sp-core version (38.1.0) across all crates
> - Consolidated `ChallengeConfig` type in platform-core
> - Renamed types: `ChallengeRecord`, `ChallengeDeploymentConfig`, `CliToolConfig`
> - Removed dead code (imports, variables, functions, structs)
> - Deleted `bins/mock-subtensor/` and docker-test CI job
> - SAFETY documentation for 52 unsafe blocks
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES - 4 waves
> **Critical Path**: sp-core fix → Type renames → Type consolidation → Dead code removal → Mock deletion → SAFETY docs

---

## Context

### Original Request
"Nettoie completement tout le projet et incohérences, enlève toutes les méthodes unsafe et corrige tout le bittensor sdk enlève tout ce qui est inutile"

### Interview Summary

**Key Discussions**:
- **Unsafe code**: User wanted to remove ALL unsafe → discovered 52 occurrences are architecturally necessary for WASM FFI → agreed to document instead
- **Bittensor SDK**: Found sp-core version mismatch (31.0 vs 38.1.0) → safe to upgrade
- **ChallengeConfig**: Discovered 5 semantically different types, not 4 duplicates → agreed to merge core+sdk, rename others
- **mock-subtensor**: Found dependency in CI docker-test job → user confirmed deletion along with CI job

**Research Findings**:
- **Unsafe code**: 52 unsafe blocks/impls across 4 files - all unavoidable for WASM FFI boundary
- **sp-core**: API stable between 31.0 and 38.1.0 for sr25519 usage - safe upgrade
- **ChallengeConfig**: 5 different semantic types - only core+sdk should merge
- **mock-subtensor**: Used by docker-test CI job with 13 integration tests

### Metis Review

**Identified Gaps** (addressed):
- ChallengeConfig types are semantically different - renamed instead of consolidated
- mock-subtensor CI dependency - user confirmed deletion of both
- Field naming differences (timeout_secs vs evaluation_timeout_secs) - core naming as canonical

---

## Work Objectives

### Core Objective
Clean and standardize Platform codebase by fixing version conflicts, consolidating duplicate type definitions, removing dead code, deleting unused mock infrastructure, and documenting unsafe code.

### Concrete Deliverables
- `crates/core/Cargo.toml`, `bins/utils/Cargo.toml`, `crates/rpc-server/Cargo.toml` - sp-core unified to 38.1.0
- `crates/core/src/challenge.rs` - Unified ChallengeConfig type
- `crates/p2p-consensus/src/state.rs` - Renamed ChallengeRecord type  
- `crates/subnet-manager/src/config.rs` - Renamed ChallengeDeploymentConfig type
- `bins/platform-cli/src/main.rs` - Renamed CliToolConfig type
- Deleted `bins/mock-subtensor/` directory
- Updated `.github/workflows/ci.yml` - removed docker-test job
- SAFETY comments in `crates/challenge-sdk-wasm/src/*.rs` and `bins/validator-node/src/wasm_executor.rs`

### Definition of Done
- [ ] `cargo build --workspace --all-features` succeeds
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo clippy --workspace --all-features` has 0 warnings
- [ ] No sp-core version conflicts in Cargo.lock

### Must Have
- sp-core unified to 38.1.0 across ALL crates
- ChallengeConfig consolidated (core + sdk)
- Semantically different types renamed
- Dead code removed
- mock-subtensor deleted
- SAFETY documentation for all unsafe blocks

### Must NOT Have (Guardrails)
- DO NOT merge semantically different ChallengeConfig types (p2p-consensus, subnet-manager, platform-cli)
- DO NOT remove unsafe code - only document it
- DO NOT change field semantics (weight_smoothing ≠ emission_weight)
- DO NOT remove public type exports without deprecation aliases

---

## Verification Strategy (MANDATORY)

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: TDD (Tests Driven Development)
- **Framework**: cargo test
- **Approach**: Run baseline tests before changes, verify after each atomic change

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: Fix sp-core version mismatch [quick]
├── Task 2: Run baseline test verification [quick]
├── Task 3: Rename p2p-consensus ChallengeConfig → ChallengeRecord [quick]
└── Task 4: Rename subnet-manager ChallengeConfig → ChallengeDeploymentConfig [quick]

Wave 2 (After Wave 1 - more renames):
├── Task 5: Rename platform-cli ChallengeConfig → CliToolConfig [quick]
├── Task 6: Add deprecation aliases for renamed types [quick]
├── Task 7: Merge core + sdk ChallengeConfig types [unspecified-high]
└── Task 8: Update all ChallengeConfig imports [unspecified-high]

Wave 3 (After Wave 2 - cleanup):
├── Task 9: Remove unused imports (AtomicU64, etc.) [quick]
├── Task 10: Remove unused variables (policy_ranges, challenge_uuid) [quick]
├── Task 11: Remove dead structs (SdkResponse*, etc.) [quick]
├── Task 12: Remove dead methods (clear_pending_writes*, parse_openai_response) [quick]
├── Task 13: Remove deprecated Configuration error variant [quick]
└── Task 14: Remove #[allow(dead_code)] annotations where appropriate [quick]

Wave 4 (After Wave 3 - infrastructure):
├── Task 15: Delete bins/mock-subtensor/ directory [quick]
├── Task 16: Update Cargo.toml workspace members [quick]
├── Task 17: Remove docker-test CI job from ci.yml [quick]
├── Task 18: Delete mock.rs from bittensor-integration [quick]
└── Task 19: Add SAFETY comments to unsafe blocks (52 total) [unspecified-high]

Wave FINAL (After ALL tasks - verification):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Full test suite verification (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay
```

### Dependency Matrix
- **1**: — — 2, 3, 4, 5
- **2**: 1 — —
- **3-8**: 2 — 9
- **9-14**: 8 — 15, 16
- **15-18**: 14 — 19
- **19**: 18 — F1-F4

### Agent Dispatch Summary
- **Wave 1**: 4 tasks → all `quick`
- **Wave 2**: 4 tasks → T7-T8 `unspecified-high`, others `quick`
- **Wave 3**: 6 tasks → all `quick`
- **Wave 4**: 5 tasks → T19 `unspecified-high`, others `quick`
- **FINAL**: 4 tasks → F1 `oracle`, F2-F3 `unspecified-high`, F4 `deep`

---

## TODOs

- [x] 1. Fix sp-core version mismatch in Cargo.toml files

  **What to do**:
  - Update `crates/core/Cargo.toml`: change `sp-core = { version = "31.0", ... }` to `sp-core = { workspace = true }`
  - Update `bins/utils/Cargo.toml`: change `sp-core = { version = "31.0", ... }` to `sp-core = { workspace = true }`
  - Update `crates/rpc-server/Cargo.toml`: change `sp-core = { version = "31.0", ... }` to `sp-core = { workspace = true }`
  - Verify workspace Cargo.toml already defines `sp-core = "38.1.0"`

  **Must NOT do**:
  - DO NOT change sp-core version in workspace Cargo.toml (already correct)
  - DO NOT modify sp-core in vendor/bittensor-rs (kept separately)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple Cargo.toml text replacements
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: NO (must complete before other tasks)
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Tasks 2-19
  - **Blocked By**: None

  **References**:
  - `/root/platform/Cargo.toml:61` - workspace sp-core definition (38.1.0)
  - `/root/platform/crates/core/Cargo.toml:22` - needs update from 31.0
  - `/root/platform/bins/utils/Cargo.toml:7` - needs update from 31.0
  - `/root/platform/crates/rpc-server/Cargo.toml:28` - needs update from 31.0

  **Acceptance Criteria**:
  - [ ] All three Cargo.toml files updated to use workspace sp-core
  - [ ] `cargo build --workspace` succeeds without sp-core version conflicts

  **QA Scenarios**:
  ```
  Scenario: Build succeeds with unified sp-core
    Tool: Bash
    Preconditions: Clean workspace
    Steps:
      1. cargo build --workspace --all-features 2>&1
    Expected Result: Build completes with 0 errors
    Failure Indicators: "error: multiple versions for crate `sp-core`"
    Evidence: .sisyphus/evidence/task-01-build.log
  ```

  **Commit**: YES
  - Message: `refactor(cargo): unify sp-core to workspace version 38.1.0`
  - Files: `crates/core/Cargo.toml`, `bins/utils/Cargo.toml`, `crates/rpc-server/Cargo.toml`

- [x] 2. Run baseline test verification

  **What to do**:
  - Execute full test suite to establish baseline before changes
  - Capture test output to file for comparison
  - Document test count and any existing failures

  **Must NOT do**:
  - DO NOT proceed if tests fail significantly (need to fix existing issues first)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: None
  - **Blocked By**: Task 1 (should run after sp-core fix)

  **References**:
  - `/root/platform/tests/` - integration tests directory

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace --all-features --no-run` succeeds
  - [ ] Test count documented in evidence file

  **QA Scenarios**:
  ```
  Scenario: Baseline tests compile
    Tool: Bash
    Steps:
      1. cargo test --workspace --all-features --no-run 2>&1 | tee baseline_compile.log
    Expected Result: All test binaries compile
    Evidence: .sisyphus/evidence/task-02-baseline-compile.log
  ```

  **Commit**: NO

- [x] 3. Rename p2p-consensus ChallengeConfig → ChallengeRecord

  **What to do**:
  - Rename `ChallengeConfig` struct in `crates/p2p-consensus/src/state.rs` to `ChallengeRecord`
  - Update all usages of `ChallengeConfig` in p2p-consensus crate
  - Add deprecation alias: `#[deprecated(since = "0.2.0", note = "Use ChallengeRecord instead")] pub type ChallengeConfig = ChallengeRecord;`
  - Update lib.rs re-exports

  **Must NOT do**:
  - DO NOT change field meanings or types
  - DO NOT remove the type - only rename

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Task 8
  - **Blocked By**: Task 2 (baseline tests)

  **References**:
  - `/root/platform/crates/p2p-consensus/src/state.rs:203` - ChallengeConfig definition
  - `/root/platform/crates/p2p-consensus/src/lib.rs` - public exports

  **Acceptance Criteria**:
  - [ ] Struct renamed to ChallengeRecord
  - [ ] All uses in p2p-consensus updated
  - [ ] Deprecation alias added for backward compatibility
  - [ ] `cargo build -p p2p-consensus` succeeds

  **QA Scenarios**:
  ```
  Scenario: p2p-consensus compiles with renamed type
    Tool: Bash
    Steps:
      1. cargo build -p p2p-consensus 2>&1
    Expected Result: 0 compile errors
    Failure Indicators: "cannot find type `ChallengeRecord`" or unused ChallengeConfig
    Evidence: .sisyphus/evidence/task-03-p2p-build.log
  ```

  **Commit**: YES
  - Message: `refactor(p2p-consensus): rename ChallengeConfig to ChallengeRecord`

- [x] 4. Rename subnet-manager ChallengeConfig → ChallengeDeploymentConfig

  **What to do**:
  - Rename `ChallengeConfig` struct in `crates/subnet-manager/src/config.rs` to `ChallengeDeploymentConfig`
  - Update all usages in subnet-manager crate
  - Add deprecation alias for backward compatibility

  **Must NOT do**:
  - DO NOT change fields or their semantics

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Task 8
  - **Blocked By**: Task 2 (baseline tests)

  **References**:
  - `/root/platform/crates/subnet-manager/src/config.rs:151` - ChallengeConfig definition

  **Acceptance Criteria**:
  - [ ] Struct renamed to ChallengeDeploymentConfig
  - [ ] Deprecation alias added
  - [ ] `cargo build -p subnet-manager` succeeds

  **QA Scenarios**:
  ```
  Scenario: subnet-manager compiles with renamed type
    Tool: Bash
    Steps:
      1. cargo build -p subnet-manager 2>&1
    Expected Result: 0 compile errors
    Evidence: .sisyphus/evidence/task-04-subnet-build.log
  ```

  **Commit**: YES
  - Message: `refactor(subnet-manager): rename ChallengeConfig to ChallengeDeploymentConfig`

- [x] 5. Rename platform-cli ChallengeConfig → CliToolConfig

  **What to do**:
  - Rename `ChallengeConfig` struct in `bins/platform-cli/src/main.rs` to `CliToolConfig`
  - Update all usages in platform-cli binary
  - This type is for CLI tool downloads (github_repo, binary_name) - unrelated to other ChallengeConfigs

  **Must NOT do**:
  - DO NOT merge with other ChallengeConfig types

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6, 7, 8)
  - **Blocks**: Task 8
  - **Blocked By**: Task 2 (baseline)

  **References**:
  - `/root/platform/bins/platform-cli/src/main.rs` - locate ChallengeConfig definition

  **Acceptance Criteria**:
  - [ ] Struct renamed to CliToolConfig
  - [ ] `cargo build -p platform-cli` succeeds

  **QA Scenarios**:
  ```
  Scenario: platform-cli compiles with renamed type
    Tool: Bash
    Steps:
      1. cargo build -p platform-cli 2>&1
    Expected Result: 0 compile errors
    Evidence: .sisyphus/evidence/task-05-cli-build.log
  ```

  **Commit**: YES
  - Message: `refactor(platform-cli): rename ChallengeConfig to CliToolConfig`

- [x] 6. Add deprecation aliases for renamed types

  **What to do**:
  - Add `#[deprecated]` type aliases in p2p-consensus, subnet-manager, platform-cli
  - Ensure old type names still work for downstream code
  - Add clear deprecation messages pointing to new names

  **Must NOT do**:
  - DO NOT remove the deprecated aliases (migration period needed)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 7, 8)
  - **Blocks**: None
  - **Blocked By**: Tasks 3, 4, 5 (renames complete)

  **References**:
  - `/root/platform/crates/p2p-consensus/src/lib.rs`
  - `/root/platform/crates/subnet-manager/src/lib.rs`
  - `/root/platform/bins/platform-cli/src/main.rs`

  **Acceptance Criteria**:
  - [ ] Deprecation aliases compile with warnings
  - [ ] Old type names still usable

  **QA Scenarios**:
  ```
  Scenario: Deprecation warnings appear
    Tool: Bash
    Steps:
      1. cargo build --workspace 2>&1 | grep -i deprecated
    Expected Result: Deprecation warnings present for old type names
    Evidence: .sisyphus/evidence/task-06-deprecation.log
  ```

  **Commit**: YES
  - Message: `refactor: add deprecation aliases for renamed types`

- [x] 7. Merge core + sdk ChallengeConfig types

  **What to do**:
  - Create unified `ChallengeConfig` in `crates/core/src/challenge.rs` with all fields from both versions
  - Handle field naming differences:
    - `timeout_secs` (core) vs `evaluation_timeout_secs` (sdk) → use `timeout_secs`
    - `min_validators` (core) vs `min_validators_for_weights` (sdk) → use `min_validators`
  - Keep ALL fields as superset: `weight_smoothing` (sdk only) and `emission_weight` (core only) both included
  - Add `From<ChallengeConfigSdk>` trait for backward compatibility

  **Must NOT do**:
  - DO NOT lose any fields from either version
  - DO NOT change field semantics

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex type consolidation requiring careful field mapping
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (this is the canonical type)
  - **Parallel Group**: Wave 2 (sequential within wave)
  - **Blocks**: Task 8
  - **Blocked By**: Tasks 3-6 (other renames complete)

  **References**:
  - `/root/platform/crates/core/src/challenge.rs:90` - current core ChallengeConfig
  - `/root/platform/crates/challenge-sdk/src/types.rs:76` - sdk ChallengeConfig
  - `/root/platform/crates/core/src/lib.rs` - exports

  **Acceptance Criteria**:
  - [ ] Unified ChallengeConfig in platform-core
  - [ ] All fields from both versions present
  - [ ] From trait implemented for backward compat
  - [ ] `cargo build --workspace` succeeds

  **QA Scenarios**:
  ```
  Scenario: Unified type compiles and used correctly
    Tool: Bash
    Steps:
      1. cargo build --workspace --all-features 2>&1
    Expected Result: 0 compile errors
    Failure Indicators: "field X not found" or "expected struct ChallengeConfig"
    Evidence: .sisyphus/evidence/task-07-consolidated-type.log
  ```

  **Commit**: YES
  - Message: `refactor(core): consolidate ChallengeConfig from core and sdk`

- [x] 8. Update all ChallengeConfig imports across crates

  **What to do**:
  - Update all crates to import unified ChallengeConfig from platform-core
  - Remove local ChallengeConfig definitions
  - Update Cargo.toml dependencies if needed
  - Fix all compile errors from changed imports

  **Must NOT do**:
  - DO NOT change business logic - only imports

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 7)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 9-14 (dead code removal)
  - **Blocked By**: Task 7 (consolidated type)

  **References**:
  - Search: `grep -r "ChallengeConfig" crates/ bins/`
  - `/root/platform/crates/challenge-sdk/src/types.rs:76` - remove this definition
  - `/root/platform/crates/challenge-sdk/src/lib.rs` - update re-exports

  **Acceptance Criteria**:
  - [ ] All crates use unified ChallengeConfig from platform-core
  - [ ] Local definitions removed
  - [ ] `cargo build --workspace` succeeds
  - [ ] `cargo test --workspace` passes

  **QA Scenarios**:
  ```
  Scenario: All imports resolve correctly
    Tool: Bash
    Steps:
      1. cargo build --workspace --all-features 2>&1
      2. cargo test --workspace --lib 2>&1 | tail -5
    Expected Result: Build succeeds, tests pass
    Evidence: .sisyphus/evidence/task-08-imports-build.log, task-08-imports-test.log
  ```

  **Commit**: YES
  - Message: `refactor: update all ChallengeConfig imports to use platform-core`

- [x] 9. Remove unused imports

  **What to do**:
  - Remove `AtomicU64` unused import in `bins/validator-node/src/wasm_executor.rs:6`
  - Search for and remove other unused imports flagged by `cargo clippy`
  - Run `cargo clippy --fix` to auto-fix easy cases

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 10-14)
  - **Blocks**: None
  - **Blocked By**: Task 8 (imports stable)

  **References**:
  - `/root/platform/bins/validator-node/src/wasm_executor.rs:6:37` - AtomicU64 unused

  **Acceptance Criteria**:
  - [ ] `cargo clippy --workspace` shows 0 unused_import warnings

  **QA Scenarios**:
  ```
  Scenario: No unused import warnings
    Tool: Bash
    Steps:
      1. cargo clippy --workspace --all-features 2>&1 | grep -i "unused" | grep -i "import"
    Expected Result: No output (no unused imports)
    Evidence: .sisyphus/evidence/task-09-unused-imports.log
  ```

  **Commit**: YES
  - Message: `fix: remove unused imports`

- [x] 10. Remove unused variables

  **What to do**:
  - Fix `policy_ranges` in `crates/wasm-runtime-interface/src/network.rs:317` - use or prefix with `_`
  - Fix `challenge_uuid` in `bins/validator-node/src/challenge_storage.rs:125` - use or prefix with `_`
  - Search for other unused variables: `cargo clippy 2>&1 | grep "unused variable"`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 11-14)
  - **Blocks**: None
  - **Blocked By**: Task 8

  **References**:
  - `/root/platform/crates/wasm-runtime-interface/src/network.rs:317` - policy_ranges
  - `/root/platform/bins/validator-node/src/challenge_storage.rs:125` - challenge_uuid

  **Acceptance Criteria**:
  - [ ] `cargo clippy --workspace` shows 0 unused_variables warnings

  **QA Scenarios**:
  ```
  Scenario: No unused variable warnings
    Tool: Bash
    Steps:
      1. cargo clippy --workspace --all-features 2>&1 | grep -i "unused" | grep -i "variable"
    Expected Result: No output
    Evidence: .sisyphus/evidence/task-10-unused-vars.log
  ```

  **Commit**: YES
  - Message: `fix: prefix or use unused variables`

- [x] 11. Remove dead structs (SdkResponse*, etc.)

  **What to do**:
  - Remove unused struct definitions in `bins/validator-node/src/wasm_executor.rs`:
    - `SdkResponse`
    - `SdkResponseToolCall`
    - `SdkResponseFunctionCall`
    - `SdkUsage`
  - Verify they're truly unused with `lsp_find_references`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 8

  **References**:
  - `/root/platform/bins/validator-node/src/wasm_executor.rs` - locate dead structs

  **Acceptance Criteria**:
  - [ ] Dead structs removed
  - [ ] `cargo build -p validator-node` succeeds

  **QA Scenarios**:
  ```
  Scenario: Dead structs removed, build succeeds
    Tool: Bash
    Steps:
      1. cargo build -p validator-node 2>&1
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-11-dead-structs.log
  ```

  **Commit**: YES
  - Message: `refactor: remove unused SdkResponse structs`

- [x] 12. Remove dead methods

  **What to do**:
  - Remove `clear_pending_writes` method (never used)
  - Remove `clear_pending_writes_for_challenge` method (never used)
  - Remove `parse_openai_response` function (never used)
  - Remove `resolve_and_validate_ip` method (never used)
  - Remove `validate_ip_against_policy` method (never used)
  - Verify each is truly unused with `lsp_find_references`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 8

  **References**:
  - `/root/platform/bins/validator-node/src/challenge_storage.rs` - clear_pending_writes*
  - `/root/platform/crates/wasm-runtime-interface/` - parse_openai_response, resolve_and_validate_ip

  **Acceptance Criteria**:
  - [ ] Dead methods removed
  - [ ] `cargo build --workspace` succeeds

  **QA Scenarios**:
  ```
  Scenario: Dead methods removed, build succeeds
    Tool: Bash
    Steps:
      1. cargo build --workspace 2>&1
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-12-dead-methods.log
  ```

  **Commit**: YES
  - Message: `refactor: remove unused methods`

- [x] 13. Remove deprecated Configuration error variant

  **What to do**:
  - Remove `Configuration` variant from `ChallengeError` enum in `crates/challenge-sdk/src/error.rs`
  - `Config` variant already exists and serves same purpose
  - Update any code using `ChallengeError::Configuration` to use `ChallengeError::Config`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 8

  **References**:
  - `/root/platform/crates/challenge-sdk/src/error.rs:45` - deprecated Configuration variant

  **Acceptance Criteria**:
  - [ ] `Configuration` variant removed
  - [ ] All usages updated to `Config`
  - [ ] `cargo build -p platform-challenge-sdk` succeeds

  **QA Scenarios**:
  ```
  Scenario: Deprecated variant removed
    Tool: Bash
    Steps:
      1. cargo build -p platform-challenge-sdk 2>&1
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-13-deprecated-error.log
  ```

  **Commit**: YES
  - Message: `refactor(error): remove deprecated Configuration variant`

- [x] 14. Remove unnecessary #[allow(dead_code)] annotations

  **What to do**:
  - Review all `#[allow(dead_code)]` annotations
  - Remove annotations for code that will now be used
  - For truly dead code that's intentionally kept, move annotation to specific item with TODO comment explaining why
  - Files with most annotations:
    - `bins/validator-node/src/wasm_executor.rs` (13 annotations)
    - `bins/mock-subtensor/src/jsonrpc.rs` (6 annotations)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 8

  **References**:
  - Search: `grep -rn "#\[allow(dead_code)\]" crates/ bins/`

  **Acceptance Criteria**:
  - [ ] Unnecessary annotations removed
  - [ ] Intentional dead code has TODO comment
  - [ ] `cargo clippy --workspace` shows no new warnings

  **QA Scenarios**:
  ```
  Scenario: Dead code annotations reviewed
    Tool: Bash
    Steps:
      1. cargo clippy --workspace --all-features 2>&1 | grep -i dead_code | head -10
    Expected Result: Minimal or no dead_code warnings
    Evidence: .sisyphus/evidence/task-14-dead-code-annotations.log
  ```

  **Commit**: YES
  - Message: `refactor: clean up dead_code annotations`

- [x] 15. Delete bins/mock-subtensor/ directory

  **What to do**:
  - Remove entire `bins/mock-subtensor/` directory
  - This mock Bittensor RPC node is no longer needed (docker-test job being deleted)

  **Must NOT do**:
  - DO NOT delete until CI job is updated (Task 17)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 16-19)
  - **Blocks**: None
  - **Blocked By**: Task 14

  **References**:
  - `/root/platform/bins/mock-subtensor/` - entire directory to delete

  **Acceptance Criteria**:
  - [ ] Directory deleted
  - [ ] No compile errors

  **QA Scenarios**:
  ```
  Scenario: mock-subtensor deleted, workspace still builds
    Tool: Bash
    Steps:
      1. rm -rf bins/mock-subtensor
      2. cargo build --workspace 2>&1 | head -20
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-15-delete-mock.log
  ```

  **Commit**: YES
  - Message: `refactor: remove mock-subtensor binary`

- [x] 16. Update Cargo.toml workspace members

  **What to do**:
  - Remove `bins/mock-subtensor` from workspace members in root `Cargo.toml`
  - Verify no other references to mock-subtensor in project

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 15

  **References**:
  - `/root/platform/Cargo.toml` - workspace members list

  **Acceptance Criteria**:
  - [ ] `bins/mock-subtensor` removed from members
  - [ ] `cargo check --workspace` succeeds

  **QA Scenarios**:
  ```
  Scenario: Workspace updated correctly
    Tool: Bash
    Steps:
      1. cargo check --workspace 2>&1 | head -10
    Expected Result: "mock-subtensor" not found in output
    Evidence: .sisyphus/evidence/task-16-cargo-members.log
  ```

  **Commit**: YES (combine with Task 15)

- [x] 17. Remove docker-test CI job from ci.yml

  **What to do**:
  - Remove the `docker-test` job from `.github/workflows/ci.yml` (lines 122-164)
  - Remove `tests/docker/docker-compose.multi-validator.yml` if no longer needed
  - Remove any related test files in `tests/docker/` if unused

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 15

  **References**:
  - `/root/platform/.github/workflows/ci.yml:122-164` - docker-test job
  - `/root/platform/tests/docker/` - docker compose files

  **Acceptance Criteria**:
  - [ ] docker-test job removed from ci.yml
  - [ ] No references to mock-subtensor in CI

  **QA Scenarios**:
  ```
  Scenario: CI workflow valid
    Tool: Bash
    Steps:
      1. cat .github/workflows/ci.yml | grep -c "docker-test"
    Expected Result: Output is "0"
    Evidence: .sisyphus/evidence/task-17-ci-update.log
  ```

  **Commit**: YES
  - Message: `ci: remove docker-test job (mock-subtensor deleted)`

- [x] 18. Delete mock.rs from bittensor-integration

  **What to do**:
  - Delete `crates/bittensor-integration/src/mock.rs`
  - Remove `test-utils` feature if no longer needed
  - Update any imports of mock utilities

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 14

  **References**:
  - `/root/platform/crates/bittensor-integration/src/mock.rs`
  - `/root/platform/crates/bittensor-integration/Cargo.toml` - test-utils feature
  - `/root/platform/crates/bittensor-integration/src/lib.rs` - exports

  **Acceptance Criteria**:
  - [ ] mock.rs deleted
  - [ ] test-utils feature removed if unused
  - [ ] `cargo build --workspace` succeeds

  **QA Scenarios**:
  ```
  Scenario: mock.rs removed, still builds
    Tool: Bash
    Steps:
      1. cargo build --workspace 2>&1 | head -10
    Expected Result: 0 errors
    Evidence: .sisyphus/evidence/task-18-delete-mock-rs.log
  ```

  **Commit**: YES
  - Message: `refactor(bittensor): remove mock.rs test utilities`

- [x] 19. Add SAFETY comments to all unsafe blocks

  **What to do**:
  - Add `// SAFETY:` comments to all 52 unsafe blocks explaining why each is safe:
  - Files to update:
    - `crates/challenge-sdk-wasm/src/lib.rs` (18 blocks)
    - `crates/challenge-sdk-wasm/src/host_functions.rs` (30 blocks)
    - `crates/challenge-sdk-wasm/src/alloc_impl.rs` (3 blocks)
    - `bins/validator-node/src/wasm_executor.rs` (1 impl)
  - Follow standard pattern from Rust std library
  - Example: `// SAFETY: This is FFI to host function. The host guarantees valid pointers.`

  **Must NOT do**:
  - DO NOT change any unsafe code behavior - only add comments

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Careful documentation requiring understanding of each unsafe block
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (documentation needs to be thorough)
  - **Parallel Group**: Wave 4 (final task)
  - **Blocks**: F1-F4
  - **Blocked By**: Task 18

  **References**:
  - `/root/platform/crates/challenge-sdk-wasm/src/lib.rs:156-380` - FFI boundary unsafe blocks
  - `/root/platform/crates/challenge-sdk-wasm/src/host_functions.rs:42-413` - host function calls
  - `/root/platform/crates/challenge-sdk-wasm/src/alloc_impl.rs:17-44` - allocator
  - `/root/platform/bins/validator-node/src/wasm_executor.rs:142` - Send impl

  **Acceptance Criteria**:
  - [ ] All unsafe blocks have SAFETY comments
  - [ ] Comments explain why each unsafe operation is safe
  - [ ] `cargo build --workspace` succeeds

  **QA Scenarios**:
  ```
  Scenario: SAFETY comments present
    Tool: Bash
    Steps:
      1. grep -c "SAFETY:" crates/challenge-sdk-wasm/src/*.rs
      2. grep -c "SAFETY:" bins/validator-node/src/wasm_executor.rs
    Expected Result: All files have SAFETY comment counts matching unsafe block counts
    Evidence: .sisyphus/evidence/task-19-safety-comments.log
  ```

  **Commit**: YES
  - Message: `docs: add SAFETY comments to all unsafe blocks`

---

## Final Verification Wave (MANDATORY)

- [x] F1. **Plan Compliance Audit** — `oracle`
  Verify all Must Have items implemented, all Must NOT Have avoided.

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy --workspace --all-features`, verify 0 warnings.

- [x] F3. **Full Test Suite Verification** — `unspecified-high`
  Run `cargo test --workspace --all-features`, verify all tests pass.

- [x] F4. **Scope Fidelity Check** — `deep`
  Verify no scope creep, all changes match plan.

---

## Commit Strategy

- **After each wave**: Commit with message `refactor: wave N - description`
- **Final commit**: `refactor: complete project cleanup - sp-core, types, dead code, mocks`

---

## Success Criteria

### Verification Commands
```bash
cargo build --workspace --all-features  # Expected: 0 errors
cargo test --workspace --all-features   # Expected: all tests pass
cargo clippy --workspace --all-features # Expected: 0 warnings
```

### Final Checklist
- [x] All sp-core versions unified to 38.1.0
- [x] ChallengeConfig consolidated in platform-core
- [x] ChallengeRecord, ChallengeDeploymentConfig, CliToolConfig renamed
- [x] Dead code removed
- [x] mock-subtensor deleted
- [x] CI docker-test job removed
- [x] SAFETY comments added to all unsafe blocks
