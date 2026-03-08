# User Testing Surface

## Testing Tools

### 1. swe-forge CLI

**Build and test:**
```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
```

**Mine datasets:**
```bash
export OPENROUTER_API_KEY="..."
export GITHUB_TOKEN="..."

cargo run --release -- swe mine \
  --output ./output \
  --max-tasks 5 \
  --difficulty medium \
  --once
```

**Load and inspect:**
```bash
cargo run --release -- swe load --input ./output/owner-repo-1234
```

**Run harness:**
```bash
cargo run --release -- swe harness \
  --input ./output \
  --agent-dir ./agent \
  --agent-cmd "python -m agent" \
  --parallel 2
```

### 2. Docker Validation

**Fresh-container replay:**
```bash
# Validates a task in fresh Docker container
docker run --rm -v $(pwd)/output:/tasks python:3.12-slim \
  bash -c "cd /tasks/owner-repo-1234 && cat workspace.yaml"
```

### 3. Dataset Validation

**Schema check:**
```bash
# Check workspace.yaml format
find output -name "workspace.yaml" -exec yq eval {} \;

# Check for empty test arrays
grep -r "fail_to_pass: \[\]" output/ || echo "No empty fail_to_pass found"
grep -r "pass_to_pass: \[\]" output/ || echo "No empty pass_to_pass found"
```

**HuggingFace upload test:**
```bash
export HF_TOKEN="..."
python auto_publish.py --dry-run
```

## Known Quirks

1. **Docker timeouts**: Large repos (elastic/kibana) may timeout during clone
2. **Rate limits**: GitHub API 5000/h limit affects enrichment speed
3. **EnvironmentBroken**: Some tasks fail due to missing dependencies in base image
4. **Agentic loop**: Up to 200 turns may take significant time per task

## Test Accounts/Data

- Use CortexLM/swe-forge HF dataset for existing validated tasks
- GH Archive requires no auth (public data)
- GitHub API requires token with public_repo access
