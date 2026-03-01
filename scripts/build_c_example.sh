#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

./scripts/build_core.sh

OUT_DIR="artifacts/bin"
OUT_BIN="$OUT_DIR/dsapi_example"
TARGET_DIR="${CARGO_TARGET_DIR:-${DSAPI_TARGET_DIR:-target}}"
LIB_PATH="$TARGET_DIR/release/libdirectscreen_core.a"
mkdir -p "$OUT_DIR"

if [ ! -f "$LIB_PATH" ]; then
  echo "c_example_error=core_static_lib_missing path=$LIB_PATH"
  exit 3
fi

cc -Ibridge/c/include \
  bridge/c/examples/simple_route.c \
  "$LIB_PATH" \
  -ldl -lpthread -lm \
  -o "$OUT_BIN"

echo "c_example=$OUT_BIN"
