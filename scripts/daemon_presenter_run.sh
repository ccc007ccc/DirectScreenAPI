#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

CONTROL_SOCKET_PATH="${DSAPI_CONTROL_SOCKET_PATH:-${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}}"
DATA_SOCKET_PATH="${DSAPI_DATA_SOCKET_PATH:-}"
OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/android}"
DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
MAIN_CLASS="org.directscreenapi.adapter.AndroidAdapterMain"
APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-app_process}"
RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
POLL_MS="${DSAPI_PRESENTER_POLL_MS:-2}"
LAYER_Z="${DSAPI_PRESENTER_LAYER_Z:-1000000}"
LAYER_NAME="${DSAPI_PRESENTER_LAYER_NAME:-DirectScreenAPI}"
AUTO_REBUILD="${DSAPI_PRESENTER_AUTO_REBUILD:-1}"

if [ -z "$DATA_SOCKET_PATH" ]; then
  case "$CONTROL_SOCKET_PATH" in
    *.sock) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH%.sock}.data.sock" ;;
    *) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH}.data" ;;
  esac
fi

if [ ! -S "$CONTROL_SOCKET_PATH" ]; then
  echo "presenter_error=daemon_control_socket_missing socket=$CONTROL_SOCKET_PATH"
  exit 2
fi
if [ ! -S "$DATA_SOCKET_PATH" ]; then
  echo "presenter_error=daemon_data_socket_missing socket=$DATA_SOCKET_PATH"
  exit 2
fi

if [ "$AUTO_REBUILD" = "1" ] || [ ! -f "$DEX_JAR" ]; then
  DSAPI_ANDROID_BUILD_MODE=presenter ./scripts/build_android_adapter.sh >/dev/null
fi

case "$DEX_JAR" in
  /*) DEX_JAR_ABS="$DEX_JAR" ;;
  *) DEX_JAR_ABS="$ROOT_DIR/$DEX_JAR" ;;
esac

if ! command -v "$APP_PROCESS_BIN" >/dev/null 2>&1; then
  if [ -x /system/bin/app_process ]; then
    APP_PROCESS_BIN="/system/bin/app_process"
  else
    echo "presenter_error=app_process_not_found"
    exit 2
  fi
fi

if [ "$RUN_AS_ROOT" = "1" ]; then
  if ! command -v su >/dev/null 2>&1; then
    echo "presenter_error=su_not_found"
    exit 2
  fi
  su -c "CLASSPATH='$DEX_JAR_ABS' '$APP_PROCESS_BIN' /system/bin '$MAIN_CLASS' present-loop '$CONTROL_SOCKET_PATH' '$DATA_SOCKET_PATH' '$POLL_MS' '$LAYER_Z' '$LAYER_NAME'"
  exit 0
fi

CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" present-loop "$CONTROL_SOCKET_PATH" "$DATA_SOCKET_PATH" "$POLL_MS" "$LAYER_Z" "$LAYER_NAME"
