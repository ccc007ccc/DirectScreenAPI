#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"

if [ -f "$PID_FILE" ]; then
  pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    kill -- "-$pid" >/dev/null 2>&1 || kill "$pid" >/dev/null 2>&1 || true
    if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
      su -c "kill -- -$pid >/dev/null 2>&1 || kill $pid >/dev/null 2>&1 || true" || true
    fi
    sleep 0.2
    kill -KILL -- "-$pid" >/dev/null 2>&1 || kill -KILL "$pid" >/dev/null 2>&1 || true
    if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
      su -c "kill -KILL -- -$pid >/dev/null 2>&1 || kill -KILL $pid >/dev/null 2>&1 || true" || true
    fi
  fi
  rm -f "$PID_FILE"
fi

pkill -f "AndroidAdapterMain present-loop" >/dev/null 2>&1 || true
if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
  su -c "pkill -f 'AndroidAdapterMain present-loop' >/dev/null 2>&1 || true" || true
fi

echo "presenter_status=stopped"
