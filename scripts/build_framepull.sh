#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

TARGET_DIR="${CARGO_TARGET_DIR:-${DSAPI_TARGET_DIR:-target}}"
export CARGO_TARGET_DIR="$TARGET_DIR"

cargo build --release --locked -p dsapi-framepull
