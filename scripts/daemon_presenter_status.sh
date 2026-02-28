#!/bin/sh
set -eu

PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"

if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    echo "presenter_status=running pid=$pid"
    exit 0
  fi
fi

echo "presenter_status=stopped"
exit 2
