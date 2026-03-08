# Environment

## Required Environment Variables

### OPENROUTER_API_KEY
- **Purpose**: LLM API access for difficulty classification and test generation
- **Format**: `sk-or-v1-...`
- **Source**: https://openrouter.ai/
- **Rate Limits**: Varies by model

### GITHUB_TOKEN
- **Purpose**: GitHub API access for PR enrichment
- **Format**: `ghp_...` (classic) or `github_pat_...` (fine-grained)
- **Rate Limit**: 5000 requests/hour
- **Permissions**: public_repo read access

### HF_TOKEN
- **Purpose**: HuggingFace dataset upload
- **Format**: `hf_...`
- **Permissions**: Write access to CortexLM/swe-forge

## Optional Environment Variables

### RUST_LOG
- **Purpose**: Logging level control
- **Values**: `error`, `warn`, `info`, `debug`, `trace`
- **Default**: `info`

### DOCKER_AGENT_DIR
- **Purpose**: Override agent directory path in nested Docker
- **Format**: Absolute path string

## Docker Setup

Docker is required for:
- Test generation (agentic loop in containers)
- Harness evaluation
- Fresh-container replay validation

Ensure Docker daemon is running:
```bash
sudo systemctl start docker
docker info
```
