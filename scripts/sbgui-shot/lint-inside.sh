#!/usr/bin/env bash
set -euo pipefail
cd /src
cargo clippy --workspace --all-targets --all-features -- -D warnings
echo "LINUX CLIPPY OK (workspace, all features, -D warnings)"
