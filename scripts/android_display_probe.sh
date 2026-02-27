#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/android}"
DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
MAIN_CLASS="org.directscreenapi.adapter.AndroidAdapterMain"
CMD="${1:-display-kv}"
RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"

if [ ! -f "$DEX_JAR" ]; then
  ./scripts/build_android_adapter.sh >/dev/null
fi

case "$DEX_JAR" in
  /*) DEX_JAR_ABS="$DEX_JAR" ;;
  *) DEX_JAR_ABS="$ROOT_DIR/$DEX_JAR" ;;
esac

APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-app_process}"
if ! command -v "$APP_PROCESS_BIN" >/dev/null 2>&1; then
  if [ -x /system/bin/app_process ]; then
    APP_PROCESS_BIN="/system/bin/app_process"
  else
    echo "android_probe_error=app_process_not_found"
    exit 2
  fi
fi

if [ "$RUN_AS_ROOT" = "1" ]; then
  if ! command -v su >/dev/null 2>&1; then
    echo "android_probe_error=su_not_found"
    exit 2
  fi
  su -c "CLASSPATH='$DEX_JAR_ABS' '$APP_PROCESS_BIN' /system/bin '$MAIN_CLASS' '$CMD'"
  exit 0
fi

CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" "$CMD"
