#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"
PID_FILE="${DSAPI_TOUCH_BRIDGE_PID_FILE:-artifacts/run/dsapi_touch_bridge.pid}"
SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"

if [ "$SUPERVISE_INPUT" = "1" ]; then
  echo "touch_bridge_status=managed_by_daemon supervise=1 action=use_daemon_stop"
  exit 0
fi

if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    kill -- "-$pid" >/dev/null 2>&1 || kill "$pid" >/dev/null 2>&1 || true
    sleep 0.2
    kill -KILL -- "-$pid" >/dev/null 2>&1 || kill -KILL "$pid" >/dev/null 2>&1 || true
  fi
  rm -f "$PID_FILE"
fi

pkill -f "target/release/dsapiinput --socket $SOCKET_PATH" >/dev/null 2>&1 || true
pkill -f "target/release/dsapistream --socket $SOCKET_PATH --quiet" >/dev/null 2>&1 || true

if [ -x ./target/release/dsapictl ] && [ -S "$SOCKET_PATH" ]; then
  ./target/release/dsapictl --socket "$SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
fi

echo "touch_bridge_status=stopped"
