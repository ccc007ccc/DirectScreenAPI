#!/bin/sh
set -eu

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"
PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"

pid=""
if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
fi

if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
  if [ -S "$SOCKET_PATH" ]; then
    echo "daemon_status=running pid=$pid socket=$SOCKET_PATH"
    exit 0
  fi
  echo "daemon_status=degraded pid=$pid socket_missing=1"
  exit 1
fi

echo "daemon_status=stopped"
exit 2
