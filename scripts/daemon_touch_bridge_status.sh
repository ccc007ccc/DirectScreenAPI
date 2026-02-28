#!/bin/sh
set -eu

PID_FILE="${DSAPI_TOUCH_BRIDGE_PID_FILE:-artifacts/run/dsapi_touch_bridge.pid}"
SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"

if [ "$SUPERVISE_INPUT" = "1" ]; then
  if ./scripts/daemon_status.sh >/dev/null 2>&1; then
    echo "touch_bridge_status=managed_by_daemon supervise=1"
    exit 0
  fi
  echo "touch_bridge_status=stopped supervise=1 daemon_stopped=1"
  exit 2
fi

if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    echo "touch_bridge_status=running pid=$pid"
    exit 0
  fi
fi

echo "touch_bridge_status=stopped"
exit 2
