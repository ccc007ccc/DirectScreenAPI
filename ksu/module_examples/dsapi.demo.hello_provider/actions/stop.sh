#!/system/bin/sh
ACTION_NAME="停止Hello Provider"
ACTION_DANGER=0
set -eu

state_dir="${DSAPI_MODULE_STATE_DIR:-/data/adb/dsapi/state/modules/dsapi.demo.hello_provider}"
pid_file="$state_dir/provider.pid"

read_pid() {
  [ -f "$1" ] || { echo ""; return 0; }
  cat "$1" 2>/dev/null || echo ""
}

pid_running() {
  pid="$1"
  [ -n "$pid" ] || return 1
  kill -0 "$pid" >/dev/null 2>&1
}

pid="$(read_pid "$pid_file")"
if [ -n "$pid" ] && pid_running "$pid"; then
  kill "$pid" >/dev/null 2>&1 || true
  sleep 0.12
  kill -KILL "$pid" >/dev/null 2>&1 || true
fi
rm -f "$pid_file"

echo "state=stopped pid=- reason=manual_stop"
