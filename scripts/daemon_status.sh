#!/bin/sh
set -eu

CONTROL_SOCKET_PATH="${DSAPI_CONTROL_SOCKET_PATH:-${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}}"
DATA_SOCKET_PATH="${DSAPI_DATA_SOCKET_PATH:-}"
PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"

if [ -z "$DATA_SOCKET_PATH" ]; then
  case "$CONTROL_SOCKET_PATH" in
    *.sock) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH%.sock}.data.sock" ;;
    *) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH}.data" ;;
  esac
fi

pid=""
if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
fi

if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
  if [ -S "$CONTROL_SOCKET_PATH" ] && [ -S "$DATA_SOCKET_PATH" ]; then
    echo "daemon_status=running pid=$pid control_socket=$CONTROL_SOCKET_PATH data_socket=$DATA_SOCKET_PATH"
    exit 0
  fi
  echo "daemon_status=degraded pid=$pid control_socket_exists=$([ -S "$CONTROL_SOCKET_PATH" ] && echo 1 || echo 0) data_socket_exists=$([ -S "$DATA_SOCKET_PATH" ] && echo 1 || echo 0)"
  exit 1
fi

echo "daemon_status=stopped"
exit 2
