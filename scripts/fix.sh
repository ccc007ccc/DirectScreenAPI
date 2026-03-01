#!/bin/sh
set -eu

usage() {
  cat <<'USAGE'
usage:
  ./scripts/fix.sh
  ./scripts/fix.sh --help

actions:
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
USAGE
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
  "")
    ;;
  *)
    echo "fix_error=unexpected_args args=$*"
    usage
    exit 2
    ;;
esac

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
