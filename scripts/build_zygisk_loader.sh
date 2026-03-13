#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SRC="ksu/zygisk_loader/dsapi_zygisk_loader.cpp"
OUT_DIR="${DSAPI_ZYGISK_OUT_DIR:-artifacts/ksu_module/zygisk_loader}"
OUT_SO="$OUT_DIR/libdsapi_zygisk_loader.so"
JNI_INCLUDE_DIR="${DSAPI_ZYGISK_JNI_INCLUDE_DIR:-}"
JNI_MD_INCLUDE_DIR="${DSAPI_ZYGISK_JNI_MD_INCLUDE_DIR:-}"

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

if [ -z "$JNI_INCLUDE_DIR" ]; then
  if [ -n "${JAVA_HOME:-}" ] && [ -f "$JAVA_HOME/include/jni.h" ]; then
    JNI_INCLUDE_DIR="$JAVA_HOME/include"
  elif [ -f "/usr/lib/jvm/default-java/include/jni.h" ]; then
    JNI_INCLUDE_DIR="/usr/lib/jvm/default-java/include"
  else
    jni_h_path="$(find /usr/lib/jvm -maxdepth 5 -type f -name jni.h 2>/dev/null | head -n 1 || true)"
    if [ -n "$jni_h_path" ]; then
      JNI_INCLUDE_DIR="$(dirname "$jni_h_path")"
    fi
  fi
fi

if [ -z "$JNI_INCLUDE_DIR" ] || [ ! -f "$JNI_INCLUDE_DIR/jni.h" ]; then
  echo "zygisk_loader_error=jni_h_not_found"
  exit 2
fi

if [ -z "$JNI_MD_INCLUDE_DIR" ]; then
  if [ -f "$JNI_INCLUDE_DIR/linux/jni_md.h" ]; then
    JNI_MD_INCLUDE_DIR="$JNI_INCLUDE_DIR/linux"
  else
    jni_md_path="$(find "$JNI_INCLUDE_DIR" -maxdepth 2 -type f -name jni_md.h 2>/dev/null | head -n 1 || true)"
    if [ -n "$jni_md_path" ]; then
      JNI_MD_INCLUDE_DIR="$(dirname "$jni_md_path")"
    fi
  fi
fi

if [ -z "$JNI_MD_INCLUDE_DIR" ] || [ ! -f "$JNI_MD_INCLUDE_DIR/jni_md.h" ]; then
  echo "zygisk_loader_error=jni_md_h_not_found base=$JNI_INCLUDE_DIR"
  exit 2
fi

"$CXX" \
  -std=c++17 \
  -fPIC \
  -shared \
  -O2 \
  -Wall \
  -Wextra \
  -Werror \
  -I"$JNI_INCLUDE_DIR" \
  -I"$JNI_MD_INCLUDE_DIR" \
  "$SRC" \
  -o "$OUT_SO"

chmod 0644 "$OUT_SO" 2>/dev/null || true

echo "zygisk_loader_so=$OUT_SO"
echo "zygisk_loader_cxx=$CXX"
echo "zygisk_loader_jni_include=$JNI_INCLUDE_DIR"
echo "zygisk_loader_jni_md_include=$JNI_MD_INCLUDE_DIR"
