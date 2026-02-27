#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
