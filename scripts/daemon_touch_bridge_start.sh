#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

PID_FILE="${DSAPI_TOUCH_BRIDGE_PID_FILE:-artifacts/run/dsapi_touch_bridge.pid}"
LOG_FILE="${DSAPI_TOUCH_BRIDGE_LOG_FILE:-artifacts/run/dsapi_touch_bridge.log}"
SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"

mkdir -p "$(dirname "$PID_FILE")"
mkdir -p "$(dirname "$LOG_FILE")"

if [ "$SUPERVISE_INPUT" = "1" ]; then
  ./scripts/daemon_start.sh >/dev/null
  echo "touch_bridge_status=managed_by_daemon supervise=1"
  exit 0
fi

if [ -f "$PID_FILE" ]; then
  old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$old_pid" ] && kill -0 "$old_pid" >/dev/null 2>&1; then
    echo "touch_bridge_status=already_running pid=$old_pid"
    exit 0
  fi
  rm -f "$PID_FILE"
fi

if ! ./scripts/daemon_status.sh >/dev/null 2>&1; then
  ./scripts/daemon_start.sh >/dev/null
fi

./scripts/build_core.sh >/dev/null

nohup setsid ./scripts/daemon_touch_bridge_run.sh >>"$LOG_FILE" 2>&1 < /dev/null &
pid="$!"
echo "$pid" > "$PID_FILE"

sleep 0.3
if kill -0 "$pid" >/dev/null 2>&1; then
  echo "touch_bridge_status=started pid=$pid log=$LOG_FILE"
  exit 0
fi

echo "touch_bridge_status=failed_start"
exit 2
