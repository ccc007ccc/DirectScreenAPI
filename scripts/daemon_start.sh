#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"
PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
LOG_FILE="${DSAPI_LOG_FILE:-artifacts/run/dsapid.log}"

mkdir -p "$(dirname "$SOCKET_PATH")"
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

nohup ./target/release/dsapid --socket "$SOCKET_PATH" >>"$LOG_FILE" 2>&1 &
new_pid="$!"
echo "$new_pid" > "$PID_FILE"

sleep 0.2
if kill -0 "$new_pid" >/dev/null 2>&1; then
  echo "daemon_status=started pid=$new_pid socket=$SOCKET_PATH"
  exit 0
fi

echo "daemon_status=failed_start"
exit 2
