#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SRC="ksu/zygisk_loader/dsapi_zygisk_loader.cpp"
OUT_DIR="${DSAPI_ZYGISK_OUT_DIR:-artifacts/ksu_module/zygisk_loader}"
OUT_SO="$OUT_DIR/libdsapi_zygisk_loader.so"

if [ ! -f "$SRC" ]; then
  echo "zygisk_loader_error=source_missing path=$SRC"
  exit 2
fi

CXX="${CXX:-}"
if [ -z "$CXX" ]; then
  if command -v clang++ >/dev/null 2>&1; then
    CXX="clang++"
  elif command -v g++ >/dev/null 2>&1; then
    CXX="g++"
  elif command -v c++ >/dev/null 2>&1; then
    CXX="c++"
  else
    echo "zygisk_loader_error=cxx_not_found"
    exit 2
  fi
fi

mkdir -p "$OUT_DIR"
"$CXX" \
  -std=c++17 \
  -fPIC \
  -shared \
  -O2 \
  -Wall \
  -Wextra \
  -Werror \
  "$SRC" \
  -o "$OUT_SO"

chmod 0644 "$OUT_SO" 2>/dev/null || true

echo "zygisk_loader_so=$OUT_SO"
echo "zygisk_loader_cxx=$CXX"
