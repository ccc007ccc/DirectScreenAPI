#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SRC_DIR="android/adapter/src/main/java"
BUILD_MODE="${DSAPI_ANDROID_BUILD_MODE:-display_probe}"

OUT_DIR_EXPLICIT=0
if [ -n "${DSAPI_ANDROID_OUT_DIR:-}" ]; then
  OUT_DIR_EXPLICIT=1
fi
OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/ksu_module/android_adapter}"

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\"'\"'/g")"
}

if [ "$OUT_DIR_EXPLICIT" = "0" ]; then
  if [ -d "$OUT_DIR" ] && [ ! -w "$OUT_DIR" ]; then
    echo "android_adapter_error=out_dir_not_writable path=$OUT_DIR"
    exit 3
  elif [ -d "$OUT_DIR/classes" ] && [ ! -w "$OUT_DIR/classes" ]; then
    echo "android_adapter_error=classes_dir_not_writable path=$OUT_DIR/classes"
    exit 3
  elif [ -d "$OUT_DIR/dex" ] && [ ! -w "$OUT_DIR/dex" ]; then
    echo "android_adapter_error=dex_dir_not_writable path=$OUT_DIR/dex"
    exit 3
  fi
fi

CLASSES_DIR="$OUT_DIR/classes"
CLASS_JAR="$OUT_DIR/directscreen-adapter-classes.jar"
DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
DEX_TMP_DIR="$OUT_DIR/dex"
NATIVE_BRIDGE_SO="$OUT_DIR/libdsapi_hwbuffer_bridge.so"
NATIVE_BRIDGE_SRC="android/adapter/native/dsapi_hwbuffer_bridge.c"
NATIVE_BRIDGE_BUILD="${DSAPI_ANDROID_BUILD_NATIVE:-1}"
ANDROID_JAR_DEFAULT="third_party/android-sdk/platforms/android-35/android.jar"
ANDROID_JAR="${DSAPI_ANDROID_JAR:-$ANDROID_JAR_DEFAULT}"
BUILD_TOOLS_DIR_DEFAULT="third_party/android-sdk/build-tools/35.0.1"
BUILD_TOOLS_DIR="${DSAPI_ANDROID_BUILD_TOOLS_DIR:-$BUILD_TOOLS_DIR_DEFAULT}"

to_abs_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$ROOT_DIR" "$1" ;;
  esac
}

CLASS_JAR_ABS="$(to_abs_path "$CLASS_JAR")"
DEX_JAR_ABS="$(to_abs_path "$DEX_JAR")"
NATIVE_BRIDGE_SO_ABS="$(to_abs_path "$NATIVE_BRIDGE_SO")"

if ! command -v javac >/dev/null 2>&1; then
  echo "android_adapter_error=javac_not_found"
  exit 2
fi

if ! command -v jar >/dev/null 2>&1; then
  echo "android_adapter_error=jar_not_found"
  exit 2
fi

DEX_MODE=""
DEX_BIN="${DSAPI_DEX_TOOL_BIN:-}"
if [ -n "$DEX_BIN" ]; then
  DEX_MODE="${DSAPI_DEX_MODE:-d8}"
elif [ -x "$BUILD_TOOLS_DIR/d8" ]; then
  DEX_MODE="d8"
  DEX_BIN="$BUILD_TOOLS_DIR/d8"
elif command -v d8 >/dev/null 2>&1; then
  DEX_MODE="d8"
  DEX_BIN="d8"
elif command -v dalvik-exchange >/dev/null 2>&1; then
  DEX_MODE="dx"
  DEX_BIN="dalvik-exchange"
else
  echo "android_adapter_error=dex_tool_not_found"
  exit 2
fi

JAVA_RELEASE="${DSAPI_ANDROID_JAVA_RELEASE:-21}"

if [ "$DEX_MODE" = "dx" ] && [ "$JAVA_RELEASE" -gt 8 ]; then
  echo "android_adapter_error=dx_requires_java8 java_release=$JAVA_RELEASE"
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
org/directscreenapi/adapter/HardwareBufferBridge.java
org/directscreenapi/adapter/SurfaceLayerSession.java
org/directscreenapi/adapter/RgbaFramePresenter.java
org/directscreenapi/adapter/ScreenCaptureStreamer.java
org/directscreenapi/adapter/CapabilityManagerUi.java
org/directscreenapi/adapter/BridgeContract.java
org/directscreenapi/adapter/BridgeControlServer.java
org/directscreenapi/adapter/ParasiticManagerHost.java
org/directscreenapi/adapter/ZygoteAgentContract.java
org/directscreenapi/adapter/ZygoteAgentServer.java
"
    ;;
  presenter)
    required_sources="
org/directscreenapi/adapter/DisplayAdapter.java
org/directscreenapi/adapter/AndroidDisplayAdapter.java
org/directscreenapi/adapter/AndroidAdapterMain.java
org/directscreenapi/adapter/ReflectBridge.java
org/directscreenapi/adapter/DaemonSession.java
org/directscreenapi/adapter/HardwareBufferBridge.java
org/directscreenapi/adapter/SurfaceLayerSession.java
org/directscreenapi/adapter/RgbaFramePresenter.java
org/directscreenapi/adapter/ScreenCaptureStreamer.java
org/directscreenapi/adapter/CapabilityManagerUi.java
org/directscreenapi/adapter/BridgeContract.java
org/directscreenapi/adapter/BridgeControlServer.java
org/directscreenapi/adapter/ParasiticManagerHost.java
org/directscreenapi/adapter/ZygoteAgentContract.java
org/directscreenapi/adapter/ZygoteAgentServer.java
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

mkdir -p "$OUT_DIR"
if [ -d "$CLASSES_DIR" ] && [ ! -w "$CLASSES_DIR" ]; then
  echo "android_adapter_error=classes_dir_not_writable path=$CLASSES_DIR"
  exit 3
fi
if [ -d "$DEX_TMP_DIR" ] && [ ! -w "$DEX_TMP_DIR" ]; then
  if command -v su >/dev/null 2>&1; then
    su -c "rm -rf $(shell_quote "$DEX_TMP_DIR")" >/dev/null 2>&1 || true
  fi
fi
if [ -d "$DEX_TMP_DIR" ] && [ ! -w "$DEX_TMP_DIR" ]; then
  echo "android_adapter_error=dex_dir_not_writable path=$DEX_TMP_DIR"
  exit 3
fi
rm -rf "$CLASSES_DIR"
mkdir -p "$CLASSES_DIR"
chmod u+w "$CLASS_JAR_ABS" "$DEX_JAR_ABS" 2>/dev/null || true
rm -f "$CLASS_JAR_ABS" "$DEX_JAR_ABS"
rm -rf "$DEX_TMP_DIR"

JAVAC_EXTRA_ARGS=""
if [ -f "$ANDROID_JAR" ]; then
  JAVAC_EXTRA_ARGS="-classpath $ANDROID_JAR"
fi

# shellcheck disable=SC2086
javac -g:none --release "$JAVA_RELEASE" -encoding UTF-8 $JAVAC_EXTRA_ARGS -d "$CLASSES_DIR" $JAVA_FILES

(
  cd "$CLASSES_DIR"
  jar --create --file "$CLASS_JAR_ABS" .
)

if [ "$DEX_MODE" = "d8" ]; then
  "$DEX_BIN" --release --min-api 26 --output "$DEX_JAR_ABS" "$CLASS_JAR_ABS"
else
  mkdir -p "$DEX_TMP_DIR"
  "$DEX_BIN" --dex --min-sdk-version=26 --output="$DEX_TMP_DIR/classes.dex" "$CLASS_JAR_ABS"
  if [ ! -f "$DEX_TMP_DIR/classes.dex" ]; then
    echo "android_adapter_error=classes_dex_missing path=$DEX_TMP_DIR/classes.dex"
    exit 2
  fi
  if command -v zip >/dev/null 2>&1; then
    (
      cd "$DEX_TMP_DIR"
      zip -q "$DEX_JAR_ABS" classes.dex
    )
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "$DEX_TMP_DIR/classes.dex" "$DEX_JAR_ABS" <<'PY'
import sys
import zipfile

dex_path = sys.argv[1]
jar_path = sys.argv[2]

with zipfile.ZipFile(jar_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    zf.write(dex_path, "classes.dex")
PY
  elif command -v python >/dev/null 2>&1; then
    python - "$DEX_TMP_DIR/classes.dex" "$DEX_JAR_ABS" <<'PY'
import sys
import zipfile

dex_path = sys.argv[1]
jar_path = sys.argv[2]

with zipfile.ZipFile(jar_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    zf.write(dex_path, "classes.dex")
PY
  else
    echo "android_adapter_error=zip_and_python_missing_for_dx"
    exit 2
  fi
fi

# ART 会拒绝执行当前进程可写的 dex/apk，这里统一改为只读。
chmod 444 "$CLASS_JAR_ABS" "$DEX_JAR_ABS" 2>/dev/null || true

if [ "$NATIVE_BRIDGE_BUILD" = "1" ] && [ -f "$NATIVE_BRIDGE_SRC" ]; then
  rm -f "$NATIVE_BRIDGE_SO_ABS"
  if command -v clang >/dev/null 2>&1; then
    native_target=""
    case "$(uname -m)" in
      aarch64) native_target="aarch64-linux-android26" ;;
      armv7l|arm) native_target="armv7a-linux-androideabi26" ;;
      x86_64) native_target="x86_64-linux-android26" ;;
      i686|x86) native_target="i686-linux-android26" ;;
      *) native_target="" ;;
    esac
    if [ -n "$native_target" ] && clang \
      -target "$native_target" \
      -shared -fPIC -O2 \
      -I/data/data/com.termux/files/usr/include \
      "$NATIVE_BRIDGE_SRC" \
      -landroid \
      -o "$NATIVE_BRIDGE_SO_ABS" >/dev/null 2>&1; then
      echo "android_adapter_native_bridge=$NATIVE_BRIDGE_SO"
    else
      echo "android_adapter_warn=native_bridge_build_failed source=$NATIVE_BRIDGE_SRC"
    fi
  else
    echo "android_adapter_warn=native_bridge_clang_not_found"
  fi
fi

echo "android_adapter_classes=$CLASS_JAR"
echo "android_adapter_dex=$DEX_JAR"
echo "android_adapter_main=org.directscreenapi.adapter.AndroidAdapterMain"
echo "android_adapter_build_mode=$BUILD_MODE"
echo "android_adapter_dex_mode=$DEX_MODE"
echo "android_adapter_java_release=$JAVA_RELEASE"
echo "android_adapter_android_jar=$ANDROID_JAR"
echo "android_adapter_build_tools_dir=$BUILD_TOOLS_DIR"
