#!/usr/bin/env bash
# Run CI checks locally before pushing
set -euo pipefail

echo "=== fmt ==="
cargo fmt --all -- --check

echo "=== clippy ==="
cargo clippy -p soldebug -p soldebug-core -p soldebug-output -- -D warnings

echo "=== check ==="
cargo check --workspace

echo "=== all passed ==="
