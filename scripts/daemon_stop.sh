#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"
PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"

if [ -S "$SOCKET_PATH" ]; then
  ./target/debug/dsapictl --socket "$SOCKET_PATH" SHUTDOWN >/dev/null 2>&1 || true
fi

if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    kill "$pid" >/dev/null 2>&1 || true
    sleep 0.2
    kill -KILL "$pid" >/dev/null 2>&1 || true
  fi
  rm -f "$PID_FILE"
fi

rm -f "$SOCKET_PATH"
echo "daemon_status=stopped"
