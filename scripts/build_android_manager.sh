#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SRC_DIR="android/ksu_manager/src/main/java"
RES_DIR="android/ksu_manager/src/main/res"
MANIFEST="android/ksu_manager/src/main/AndroidManifest.xml"

OUT_DIR="${DSAPI_MANAGER_OUT_DIR:-artifacts/ksu_module/android_manager}"

if { [ -d "$OUT_DIR" ] && [ ! -w "$OUT_DIR" ]; } || { [ -d "$OUT_DIR/build" ] && [ ! -w "$OUT_DIR/build" ]; }; then
  echo "android_manager_error=out_dir_not_writable path=$OUT_DIR"
  exit 3
fi
BUILD_DIR="$OUT_DIR/build"
CLASSES_DIR="$BUILD_DIR/classes"
CLASS_JAR="$BUILD_DIR/dsapi-manager-classes.jar"
DEX_DIR="$BUILD_DIR/dex"
RES_ZIP="$BUILD_DIR/resources.zip"
UNSIGNED_APK="$OUT_DIR/dsapi-manager-unsigned.apk"
ALIGNED_APK="$OUT_DIR/dsapi-manager-aligned.apk"
SIGNED_APK="$OUT_DIR/dsapi-manager.apk"
KEYSTORE="$OUT_DIR/debug.keystore"

ANDROID_JAR_DEFAULT="third_party/android-sdk/platforms/android-35/android.jar"
ANDROID_JAR="${DSAPI_ANDROID_JAR:-$ANDROID_JAR_DEFAULT}"
VERSION_NAME="${DSAPI_MANAGER_VERSION_NAME:-0.1.0}"
VERSION_CODE="${DSAPI_MANAGER_VERSION_CODE:-1}"
BUILD_TOOLS_DIR_DEFAULT="third_party/android-sdk/build-tools/35.0.1"
BUILD_TOOLS_DIR="${DSAPI_ANDROID_BUILD_TOOLS_DIR:-$BUILD_TOOLS_DIR_DEFAULT}"

if [ -n "${DSAPI_AAPT2_ANDROID_JAR:-}" ]; then
  AAPT2_ANDROID_JAR="${DSAPI_AAPT2_ANDROID_JAR}"
elif [ -f "third_party/android-sdk/platforms/android-23-framework.jar" ]; then
  AAPT2_ANDROID_JAR="third_party/android-sdk/platforms/android-23-framework.jar"
elif [ -f "/usr/lib/android-sdk/platforms/android-23/android.jar" ]; then
  AAPT2_ANDROID_JAR="/usr/lib/android-sdk/platforms/android-23/android.jar"
elif [ -f "/system/framework/framework-res.apk" ]; then
  AAPT2_ANDROID_JAR="/system/framework/framework-res.apk"
else
  AAPT2_ANDROID_JAR="$ANDROID_JAR"
fi

AAPT2_BIN="${DSAPI_AAPT2_BIN:-aapt2}"
ZIPALIGN_BIN="${DSAPI_ZIPALIGN_BIN:-zipalign}"
APKSIGNER_BIN="${DSAPI_APKSIGNER_BIN:-apksigner}"
SKIP_SIGN="${DSAPI_MANAGER_SKIP_SIGN:-0}"

DEX_MODE=""
DEX_BIN="${DSAPI_DEX_TOOL_BIN:-}"
if [ -n "$DEX_BIN" ]; then
  DEX_MODE="${DSAPI_DEX_MODE:-d8}"
  if [ "$DEX_MODE" != "d8" ]; then
    echo "android_manager_error=unsupported_dex_mode mode=$DEX_MODE required=d8"
    exit 2
  fi
elif [ -x "$BUILD_TOOLS_DIR/d8" ]; then
  DEX_MODE="d8"
  DEX_BIN="$BUILD_TOOLS_DIR/d8"
elif command -v d8 >/dev/null 2>&1; then
  DEX_MODE="d8"
  DEX_BIN="d8"
else
  echo "android_manager_error=d8_tool_not_found"
  exit 2
fi

JAVA_RELEASE="${DSAPI_MANAGER_JAVA_RELEASE:-21}"

ensure_bin() {
  label="$1"
  bin="$2"
  case "$bin" in
    /*)
      [ -x "$bin" ] && return 0
      ;;
    *)
      command -v "$bin" >/dev/null 2>&1 && return 0
      ;;
  esac
  echo "android_manager_error=${label}_not_found bin=$bin"
  exit 2
}

ensure_bin javac javac
ensure_bin jar jar
ensure_bin keytool keytool
ensure_bin aapt2 "$AAPT2_BIN"
ensure_bin zipalign "$ZIPALIGN_BIN"
if [ "$SKIP_SIGN" != "1" ]; then
  ensure_bin apksigner "$APKSIGNER_BIN"
fi

if [ ! -f "$ANDROID_JAR" ]; then
  echo "android_manager_error=android_jar_missing path=$ANDROID_JAR"
  exit 2
fi
if [ ! -f "$AAPT2_ANDROID_JAR" ]; then
  echo "android_manager_error=aapt2_include_missing path=$AAPT2_ANDROID_JAR"
  exit 2
fi
if [ ! -d "$SRC_DIR" ]; then
  echo "android_manager_error=src_missing path=$SRC_DIR"
  exit 2
fi
if [ ! -f "$MANIFEST" ]; then
  echo "android_manager_error=manifest_missing path=$MANIFEST"
  exit 2
fi
if [ ! -d "$RES_DIR" ]; then
  echo "android_manager_error=res_missing path=$RES_DIR"
  exit 2
fi

JAVA_FILES="$(find "$SRC_DIR" -type f -name '*.java' | sort)"
if [ -z "$JAVA_FILES" ]; then
  echo "android_manager_error=no_java_sources"
  exit 2
fi

rm -rf "$BUILD_DIR"
mkdir -p "$CLASSES_DIR" "$DEX_DIR" "$OUT_DIR"
rm -f "$UNSIGNED_APK" "$ALIGNED_APK" "$SIGNED_APK" "$RES_ZIP" "$CLASS_JAR"

# shellcheck disable=SC2086
javac -g:none --release "$JAVA_RELEASE" -encoding UTF-8 -classpath "$ANDROID_JAR" -d "$CLASSES_DIR" $JAVA_FILES

(
  cd "$CLASSES_DIR"
  # CLASS_JAR 可能是绝对路径（例如 chroot/out_dir 传绝对路径），也可能是相对路径（默认 OUT_DIR）。
  case "$CLASS_JAR" in
    /*) jar_out="$CLASS_JAR" ;;
    *) jar_out="$ROOT_DIR/$CLASS_JAR" ;;
  esac
  jar --create --file "$jar_out" .
)

"$DEX_BIN" --release --min-api 26 --output "$DEX_DIR" "$CLASS_JAR"

"$AAPT2_BIN" compile --dir "$RES_DIR" -o "$RES_ZIP"
"$AAPT2_BIN" link \
  -o "$UNSIGNED_APK" \
  --manifest "$MANIFEST" \
  -I "$AAPT2_ANDROID_JAR" \
  --min-sdk-version 26 \
  --target-sdk-version 35 \
  --version-code "$VERSION_CODE" \
  --version-name "$VERSION_NAME" \
  "$RES_ZIP"

if [ ! -f "$DEX_DIR/classes.dex" ]; then
  echo "android_manager_error=classes_dex_missing path=$DEX_DIR/classes.dex"
  exit 2
fi

if command -v zip >/dev/null 2>&1; then
  zip -qj "$UNSIGNED_APK" "$DEX_DIR/classes.dex"
elif command -v python3 >/dev/null 2>&1; then
  python3 - "$UNSIGNED_APK" "$DEX_DIR/classes.dex" <<'PY'
import sys, zipfile
apk, dex = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk, 'a', compression=zipfile.ZIP_DEFLATED) as z:
    z.write(dex, 'classes.dex')
PY
elif command -v python >/dev/null 2>&1; then
  python - "$UNSIGNED_APK" "$DEX_DIR/classes.dex" <<'PY'
import sys, zipfile
apk, dex = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk, 'a', compression=zipfile.ZIP_DEFLATED) as z:
    z.write(dex, 'classes.dex')
PY
else
  echo "android_manager_error=zip_and_python_missing"
  exit 2
fi

"$ZIPALIGN_BIN" -f 4 "$UNSIGNED_APK" "$ALIGNED_APK"

if [ "$SKIP_SIGN" = "1" ]; then
  chmod 0444 "$ALIGNED_APK" 2>/dev/null || true
  echo "android_manager_apk=$ALIGNED_APK"
  echo "android_manager_sign=skipped"
else
  if [ ! -f "$KEYSTORE" ]; then
    keytool -genkeypair \
      -keystore "$KEYSTORE" \
      -storepass android \
      -alias androiddebugkey \
      -keypass android \
      -dname "CN=Android Debug,O=Android,C=US" \
      -keyalg RSA \
      -keysize 2048 \
      -validity 10000 >/dev/null 2>&1
  fi

  "$APKSIGNER_BIN" sign \
    --ks "$KEYSTORE" \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$SIGNED_APK" \
    "$ALIGNED_APK"

  "$APKSIGNER_BIN" verify "$SIGNED_APK" >/dev/null
  chmod 0444 "$SIGNED_APK" 2>/dev/null || true
  echo "android_manager_apk=$SIGNED_APK"
  echo "android_manager_sign=ok"
fi
echo "android_manager_package=org.directscreenapi.manager"
echo "android_manager_dex_mode=$DEX_MODE"
echo "android_manager_java_release=$JAVA_RELEASE"
echo "android_manager_android_jar=$ANDROID_JAR"
echo "android_manager_aapt2_include=$AAPT2_ANDROID_JAR"
echo "android_manager_build_tools_dir=$BUILD_TOOLS_DIR"
