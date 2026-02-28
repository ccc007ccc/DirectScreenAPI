#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

CONTROL_SOCKET_PATH="${DSAPI_CONTROL_SOCKET_PATH:-${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}}"
DATA_SOCKET_PATH="${DSAPI_DATA_SOCKET_PATH:-}"
PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
LOG_FILE="${DSAPI_LOG_FILE:-artifacts/run/dsapid.log}"
SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"
SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"
SUPERVISE_PRESENT_CMD="${DSAPI_SUPERVISE_PRESENTER_CMD:-./scripts/daemon_presenter_run.sh}"
SUPERVISE_INPUT_CMD="${DSAPI_SUPERVISE_INPUT_CMD:-./scripts/daemon_touch_bridge_run.sh}"
SUPERVISE_RESTART_MS="${DSAPI_SUPERVISE_RESTART_MS:-500}"
RENDER_OUTPUT_DIR="${DSAPI_RENDER_OUTPUT_DIR:-artifacts/render}"

if [ -z "$DATA_SOCKET_PATH" ]; then
  case "$CONTROL_SOCKET_PATH" in
    *.sock) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH%.sock}.data.sock" ;;
    *) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH}.data" ;;
  esac
fi

mkdir -p "$(dirname "$CONTROL_SOCKET_PATH")"
mkdir -p "$(dirname "$DATA_SOCKET_PATH")"
mkdir -p "$(dirname "$PID_FILE")"
mkdir -p "$(dirname "$LOG_FILE")"

if [ -f "$PID_FILE" ]; then
  old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$old_pid" ] && kill -0 "$old_pid" >/dev/null 2>&1; then
    echo "daemon_status=already_running pid=$old_pid"
    exit 0
  fi
  rm -f "$PID_FILE"
fi

./scripts/build_core.sh >/dev/null

set -- ./target/release/dsapid --control-socket "$CONTROL_SOCKET_PATH" --data-socket "$DATA_SOCKET_PATH" --render-output-dir "$RENDER_OUTPUT_DIR" --supervise-restart-ms "$SUPERVISE_RESTART_MS"
if [ "$SUPERVISE_PRESENTER" = "1" ]; then
  set -- "$@" --supervise-presenter "$SUPERVISE_PRESENT_CMD"
fi
if [ "$SUPERVISE_INPUT" = "1" ]; then
  set -- "$@" --supervise-input "$SUPERVISE_INPUT_CMD"
fi

nohup "$@" >>"$LOG_FILE" 2>&1 &
new_pid="$!"
echo "$new_pid" > "$PID_FILE"

sleep 0.2
if kill -0 "$new_pid" >/dev/null 2>&1; then
  echo "daemon_status=started pid=$new_pid control_socket=$CONTROL_SOCKET_PATH data_socket=$DATA_SOCKET_PATH"
  exit 0
fi

echo "daemon_status=failed_start"
exit 2
