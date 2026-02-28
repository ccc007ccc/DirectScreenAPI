#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SRC_DIR="android/adapter/src/main/java"
OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/android}"
CLASSES_DIR="$OUT_DIR/classes"
CLASS_JAR="$OUT_DIR/directscreen-adapter-classes.jar"
DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
BUILD_MODE="${DSAPI_ANDROID_BUILD_MODE:-display_probe}"

if ! command -v javac >/dev/null 2>&1; then
  echo "android_adapter_error=javac_not_found"
  exit 2
fi

if ! command -v jar >/dev/null 2>&1; then
  echo "android_adapter_error=jar_not_found"
  exit 2
fi

if ! command -v d8 >/dev/null 2>&1; then
  echo "android_adapter_error=d8_not_found"
  exit 2
fi

if [ ! -d "$SRC_DIR" ]; then
  echo "android_adapter_error=source_dir_missing path=$SRC_DIR"
  exit 3
fi

JAVA_FILES=""
required_sources=""

case "$BUILD_MODE" in
  display_probe)
    required_sources="
org/directscreenapi/adapter/DisplayAdapter.java
org/directscreenapi/adapter/AndroidDisplayAdapter.java
org/directscreenapi/adapter/AndroidAdapterMain.java
org/directscreenapi/adapter/ReflectBridge.java
org/directscreenapi/adapter/DaemonSession.java
org/directscreenapi/adapter/SurfaceLayerSession.java
org/directscreenapi/adapter/RgbaFramePresenter.java
"
    ;;
  presenter)
    required_sources="
org/directscreenapi/adapter/DisplayAdapter.java
org/directscreenapi/adapter/AndroidDisplayAdapter.java
org/directscreenapi/adapter/AndroidAdapterMain.java
org/directscreenapi/adapter/ReflectBridge.java
org/directscreenapi/adapter/DaemonSession.java
org/directscreenapi/adapter/SurfaceLayerSession.java
org/directscreenapi/adapter/RgbaFramePresenter.java
"
    ;;
  all)
    JAVA_FILES="$(find "$SRC_DIR" -type f -name '*.java' | sort)"
    ;;
  *)
    echo "android_adapter_error=invalid_build_mode mode=$BUILD_MODE"
    exit 3
    ;;
esac

if [ -n "$required_sources" ]; then
  for rel in $required_sources
  do
    file="$SRC_DIR/$rel"
    if [ ! -f "$file" ]; then
      echo "android_adapter_error=required_source_missing path=$file"
      exit 3
    fi
    if [ -z "$JAVA_FILES" ]; then
      JAVA_FILES="$file"
    else
      JAVA_FILES="$JAVA_FILES $file"
    fi
  done
fi

if [ -z "$JAVA_FILES" ]; then
  echo "android_adapter_error=no_java_sources"
  exit 3
fi

rm -rf "$CLASSES_DIR"
mkdir -p "$CLASSES_DIR"
mkdir -p "$OUT_DIR"
rm -f "$CLASS_JAR" "$DEX_JAR"

# shellcheck disable=SC2086
javac --release 11 -encoding UTF-8 -d "$CLASSES_DIR" $JAVA_FILES

(
  cd "$CLASSES_DIR"
  jar --create --file "$ROOT_DIR/$CLASS_JAR" .
)

d8 --release --min-api 26 --output "$DEX_JAR" "$CLASS_JAR"

echo "android_adapter_classes=$CLASS_JAR"
echo "android_adapter_dex=$DEX_JAR"
echo "android_adapter_main=org.directscreenapi.adapter.AndroidAdapterMain"
echo "android_adapter_build_mode=$BUILD_MODE"
