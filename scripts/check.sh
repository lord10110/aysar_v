#!/usr/bin/env bash
# The same four commands CI runs. Use before every PR.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo test
echo "all green"
