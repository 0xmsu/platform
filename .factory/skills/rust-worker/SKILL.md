---
name: rust-worker
description: Rust developer for swe-forge dataset generation and validation
---

# Rust Worker

Worker for CortexLM/swe-forge mission implementing dataset validation, generation, and code improvements.

## When to Use This Skill

- Dataset validation (workspace.yaml format, test commands)
- Fresh-container replay implementation
- OpenRouter integration
- Dataset generation pipeline
- HuggingFace uploads
- Rust code improvements (compiling, testing, clippy)
- LLM diagnosis improvements
- Documentation updates

## Work Procedure

### 1. Understand Context
- Read existing code in src/swe/harness.rs, pipeline.rs
- Check current dataset structure in hf-tasks/tasks/
- Review AGENTS.md for conventions

### 2. Write Tests First (TDD)
- Write failing tests for new functionality
- Tests must compile but fail initially (red)
- Use `cargo test -- --test-threads=1` if tests are complex

### 3. Implement Feature
- Write code to make tests pass (green)
- Follow Rust idioms and conventions
- Use proper error handling with Result<T, E>
- Add logging with appropriate levels

### 4. Validation
- Run `cargo build --release` - must have 0 warnings
- Run `cargo test` - all tests must pass
- Run `cargo clippy -- -D warnings` - no warnings
- If feature involves external APIs, test with small sample

### 5. Documentation
- Update AGENTS.md if conventions change
- Add rustdoc comments for public functions
- Update README.md if CLI changes

## Example Handoff

```json
{
  "salientSummary": "Implemented fresh-container replay validation for swe-forge. Added test for dual-commit validation, then implemented the validation logic in src/swe/harness.rs. All 1251 tests pass, clippy clean.",
  "whatWasImplemented": "FreshContainerValidator struct with methods for cloning repos, applying patches, and running test commands in Docker. Added validation for base commit (fail_to_pass should fail, pass_to_pass should pass) and patched commit (all should pass). Uses TestRunResult enum for structured results.",
  "whatWasLeftUndone": "",
  "verification": {
    "commandsRun": [
      {
        "command": "cargo build --release",
        "exitCode": 0,
        "observation": "0 warnings, binary built successfully"
      },
      {
        "command": "cargo test fresh_container",
        "exitCode": 0,
        "observation": "12 tests passed, 0 failures"
      },
      {
        "command": "cargo clippy -- -D warnings",
        "exitCode": 0,
        "observation": "0 warnings, 0 errors"
      },
      {
        "command": "cargo run -- swe harness --input ./test-tasks --agent-dir ./mock-agent --parallel 1",
        "exitCode": 0,
        "observation": "Validation passed on 3 test tasks"
      }
    ],
    "interactiveChecks": []
  },
  "tests": {
    "added": [
      {
        "file": "src/swe/harness_tests.rs",
        "cases": [
          {"name": "test_fresh_container_validation_base", "verifies": "Base commit runs fail_to_pass (fails) and pass_to_pass (passes)"},
          {"name": "test_fresh_container_validation_patched", "verifies": "Patched commit passes all tests"},
          {"name": "test_environment_broken_detection", "verifies": "Missing commands detected as EnvironmentBroken"}
        ]
      }
    ]
  },
  "discoveredIssues": []
}
```

## When to Return to Orchestrator

- External API (OpenRouter, GitHub) is unavailable or rate-limited
- Docker daemon is not running or inaccessible
- Schema changes needed for SWE-bench compatibility
- Test failures cannot be resolved within scope
- Performance issues requiring architectural changes
