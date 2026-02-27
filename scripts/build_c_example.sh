#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

./scripts/build_core.sh

OUT_DIR="artifacts/bin"
OUT_BIN="$OUT_DIR/dsapi_example"
mkdir -p "$OUT_DIR"

cc -Ibridge/c/include \
  bridge/c/examples/simple_route.c \
  target/release/libdirectscreen_core.a \
  -ldl -lpthread -lm \
  -o "$OUT_BIN"

echo "c_example=$OUT_BIN"
