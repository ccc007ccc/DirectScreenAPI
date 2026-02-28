#!/bin/sh
set -eu

PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"

if [ "$SUPERVISE_PRESENTER" = "1" ]; then
  if ./scripts/daemon_status.sh >/dev/null 2>&1; then
    echo "presenter_status=managed_by_daemon supervise=1"
    exit 0
  fi
  echo "presenter_status=stopped supervise=1 daemon_stopped=1"
  exit 2
fi

if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    echo "presenter_status=running pid=$pid"
    exit 0
  fi
fi

echo "presenter_status=stopped"
exit 2
