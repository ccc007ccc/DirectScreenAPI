#!/system/bin/sh
set -eu

state_dir="${DSAPI_MODULE_STATE_DIR:-/data/adb/dsapi/state/modules/dsapi.demo.touch_ui}"
mkdir -p "$state_dir"

demo_pid_file="$state_dir/touch_demo.pid"
presenter_pid_file="$state_dir/presenter.pid"
ctl_path="${DSAPI_MODROOT:-/data/adb/modules/directscreenapi}/bin/dsapi_service_ctl.sh"

pid_running() {
  pid="$1"
  [ -n "$pid" ] || return 1
  kill -0 "$pid" >/dev/null 2>&1
}

pid_cmdline() {
  pid="$1"
  [ -n "$pid" ] || return 1
  [ -r "/proc/$pid/cmdline" ] || return 1
  tr '\000' ' ' </proc/"$pid"/cmdline 2>/dev/null || return 1
}

pid_has_text() {
  pid="$1"
  token="$2"
  [ -n "$token" ] || return 1
  [ -r "/proc/$pid/cmdline" ] || return 0
  cmdline="$(pid_cmdline "$pid" 2>/dev/null || true)"
  [ -n "$cmdline" ] || return 1
  printf '%s' "$cmdline" | grep -F -q "$token"
}

read_pid() {
  file="$1"
  [ -f "$file" ] || { echo ""; return 0; }
  cat "$file" 2>/dev/null || echo ""
}

kill_pid_if_match() {
  pid="$1"
  token="${2:-}"
  [ -n "$pid" ] || return 0
  if [ -n "$token" ] && ! pid_has_text "$pid" "$token"; then
    return 0
  fi
  if pid_running "$pid"; then
    kill "$pid" >/dev/null 2>&1 || true
    sleep 0.2
  fi
  if pid_running "$pid"; then
    kill -KILL "$pid" >/dev/null 2>&1 || true
  fi
}

force_kill_by_name() {
  name="$1"
  [ -n "$name" ] || return 0
  if command -v pidof >/dev/null 2>&1; then
    for pid in $(pidof "$name" 2>/dev/null || true); do
      [ -n "$pid" ] || continue
      [ "$pid" = "$$" ] && continue
      [ "${PPID:-}" = "$pid" ] && continue
      kill "$pid" >/dev/null 2>&1 || true
      sleep 0.02
      kill -KILL "$pid" >/dev/null 2>&1 || true
    done
  fi
}

to_bool() {
  case "${1:-0}" in
    1|true|TRUE|yes|YES|on|ON) echo 1 ;;
    *) echo 0 ;;
  esac
}

demo_pid="$(read_pid "$demo_pid_file")"
presenter_pid="$(read_pid "$presenter_pid_file")"

kill_pid_if_match "$demo_pid" "dsapi_touch_demo"
kill_pid_if_match "$presenter_pid" "touch-ui-loop"

force_kill_by_name "dsapi_touch_demo"
force_kill_by_name "DSAPIPresenterDemo"

rm -f "$demo_pid_file" "$presenter_pid_file" "$state_dir/touch_demo.state.pipe"

if [ "$(to_bool "${TOUCH_DEMO_STOP_DAEMON:-0}")" = "1" ] && [ -f "$ctl_path" ]; then
  if ! /system/bin/sh "$ctl_path" stop >/dev/null 2>&1; then
    echo "state=error pid=- reason=core_stop_failed"
    exit 2
  fi
fi

echo "state=stopped pid=- reason=manual_stop"
