#!/bin/bash
# Init script for swe-forge mission

set -e

echo "=== Initializing swe-forge environment ==="

# Check Rust toolchain
if ! command -v cargo &> /dev/null; then
    echo "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Check Docker
if ! command -v docker &> /dev/null; then
    echo "WARNING: Docker not found. Some features will not work."
fi

# Install dependencies
echo "Installing dependencies..."
cargo fetch

# Run initial build
echo "Running initial build..."
cargo build --release 2>&1 | head -20 || true

echo "=== Environment ready ==="
echo "Required environment variables:"
echo "  export OPENROUTER_API_KEY='...'"
echo "  export GITHUB_TOKEN='ghp_...'"
echo "  export HF_TOKEN='hf_...'"
