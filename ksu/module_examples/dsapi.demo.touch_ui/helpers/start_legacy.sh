#!/system/bin/sh
set -eu

state_dir="${DSAPI_MODULE_STATE_DIR:-/data/adb/dsapi/state/modules/dsapi.demo.touch_ui}"
mkdir -p "$state_dir"

demo_pid_file="$state_dir/touch_demo.pid"
presenter_pid_file="$state_dir/presenter.pid"
demo_log_file="$state_dir/touch_demo.log"
presenter_log_file="$state_dir/presenter.log"
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

cleanup_stale_pid_file() {
  file="$1"
  token="${2:-}"
  pid="$(read_pid "$file")"
  if [ -z "$pid" ]; then
    rm -f "$file"
    return 0
  fi
  if ! pid_running "$pid"; then
    rm -f "$file"
    return 0
  fi
  if [ -n "$token" ] && ! pid_has_text "$pid" "$token"; then
    rm -f "$file"
    return 0
  fi
}

to_bool() {
  case "${1:-0}" in
    1|true|TRUE|yes|YES|on|ON) echo 1 ;;
    *) echo 0 ;;
  esac
}

is_uint() {
  case "${1:-}" in
    ''|*[!0-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

wait_core_ready() {
  max_tries="${1:-40}"
  i=0
  while [ "$i" -lt "$max_tries" ]; do
    ready_line="$(daemon_cmd READY 2>/dev/null || true)"
    case "$ready_line" in
      OK\ state=ready\ *|*state=ready*)
        return 0
        ;;
    esac
    i=$((i + 1))
    sleep 0.1
  done
  return 1
}

wait_display_ready() {
  max_tries="${1:-40}"
  i=0
  while [ "$i" -lt "$max_tries" ]; do
    display_line="$(daemon_cmd DISPLAY_GET 2>/dev/null || true)"
    set -- $display_line
    if [ "${1:-}" = "OK" ] && is_uint "${2:-}" && is_uint "${3:-}" \
      && [ "${2:-0}" -gt 0 ] && [ "${3:-0}" -gt 0 ]; then
      return 0
    fi
    i=$((i + 1))
    sleep 0.1
  done
  return 1
}

daemon_cmd() {
  dsapi_ctl="${DSAPI_ACTIVE_DSAPICTL:-}"
  dsapi_sock="${DSAPI_DAEMON_SOCKET:-/data/adb/dsapi/run/dsapi.sock}"
  dsapi_timeout_ms="${DSAPI_CLIENT_TIMEOUT_MS:-30000}"
  case "$dsapi_timeout_ms" in
    ''|*[!0-9]*) dsapi_timeout_ms=30000 ;;
  esac
  if [ -n "$dsapi_ctl" ] && [ -x "$dsapi_ctl" ] && [ -S "$dsapi_sock" ]; then
    DSAPI_CLIENT_TIMEOUT_MS="$dsapi_timeout_ms" "$dsapi_ctl" --socket "$dsapi_sock" "$@"
    return $?
  fi
  /system/bin/sh "$ctl_path" cmd "$@"
}

wait_process_stable() {
  pid="$1"
  loops="${2:-8}"
  i=0
  while [ "$i" -lt "$loops" ]; do
    if ! pid_running "$pid"; then
      return 1
    fi
    i=$((i + 1))
    sleep 0.1
  done
  return 0
}

wait_presenter_started() {
  pid="$1"
  log_file="$2"
  max_tries="${3:-40}"
  i=0
  while [ "$i" -lt "$max_tries" ]; do
    if ! pid_running "$pid"; then
      return 1
    fi
    if [ -f "$log_file" ] && grep -F -q "touch_ui_status=started" "$log_file"; then
      return 0
    fi
    i=$((i + 1))
    sleep 0.1
  done
  return 1
}

normalize_layer_z() {
  raw="${1:-}"
  case "$raw" in
    ''|*[!0-9]*)
      echo "2100000000"
      return 0
      ;;
  esac
  if [ "$raw" -lt 2000000000 ] || [ "$raw" -gt 2147483647 ]; then
    echo "2100000000"
    return 0
  fi
  echo "$raw"
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

find_app_process() {
  for b in /system/bin/app_process64 /system/bin/app_process /system/bin/app_process32
  do
    [ -x "$b" ] && { echo "$b"; return 0; }
  done
  if command -v app_process >/dev/null 2>&1; then
    command -v app_process
    return 0
  fi
  return 1
}

cleanup_stale_pid_file "$demo_pid_file" "dsapi_touch_demo"
cleanup_stale_pid_file "$presenter_pid_file" "DSAPIPresenterDemo"

old_demo_pid="$(read_pid "$demo_pid_file")"
if [ -n "$old_demo_pid" ] && pid_running "$old_demo_pid"; then
  echo "state=running pid=$old_demo_pid reason=already_running"
  exit 0
fi

# 启动前清理漂移残留，避免出现重复 presenter/demo 导致状态错乱。
force_kill_by_name "dsapi_touch_demo"
force_kill_by_name "DSAPIPresenterDemo"

[ -f "$ctl_path" ] || {
  echo "state=error pid=- reason=ctl_missing"
  exit 2
}

# When running inside daemon MODULE_ACTION_RUN context, never call `ctl start`,
# otherwise it re-enters MODULE_SYNC while the module lock is held.
if [ -n "${DSAPI_MODULE_ACTION_ID:-}" ]; then
  :
else
  if ! /system/bin/sh "$ctl_path" start >/dev/null 2>&1; then
    echo "state=error pid=- reason=core_start_failed"
    exit 2
  fi
  if ! wait_core_ready 40; then
    echo "state=error pid=- reason=core_not_ready"
    exit 2
  fi
fi

control_socket="${TOUCH_DEMO_CONTROL_SOCKET:-${DSAPI_DAEMON_SOCKET:-/data/adb/dsapi/run/dsapi.sock}}"
data_socket="${TOUCH_DEMO_DATA_SOCKET:-$control_socket}"
state_pipe="${TOUCH_DEMO_STATE_PIPE:-$state_dir/touch_demo.state.pipe}"
rm -f "$state_pipe"
if ! mkfifo "$state_pipe"; then
  echo "state=error pid=- reason=state_pipe_create_failed"
  exit 2
fi

demo_bin="${TOUCH_DEMO_BIN:-${DSAPI_ACTIVE_RELEASE:-}/bin/dsapi_touch_demo}"
if [ ! -x "$demo_bin" ]; then
  demo_bin="${DSAPI_MODROOT:-/data/adb/modules/directscreenapi}/bin/dsapi_touch_demo"
fi
if [ ! -x "$demo_bin" ]; then
  echo "state=error pid=- reason=touch_demo_bin_missing"
  exit 2
fi

dex_jar="${DSAPI_ACTIVE_ADAPTER_DEX:-${DSAPI_MODROOT:-/data/adb/modules/directscreenapi}/android/directscreen-adapter-dex.jar}"
if [ ! -f "$dex_jar" ]; then
  echo "state=error pid=- reason=adapter_dex_missing"
  exit 2
fi

app_process_bin="${TOUCH_DEMO_APP_PROCESS_BIN:-${DSAPI_APP_PROCESS_BIN:-}}"
if [ -z "$app_process_bin" ]; then
  app_process_bin="$(find_app_process 2>/dev/null || true)"
fi
if [ -z "$app_process_bin" ] || [ ! -x "$app_process_bin" ]; then
  echo "state=error pid=- reason=app_process_missing"
  exit 2
fi

blur_radius="${TOUCH_DEMO_BLUR_RADIUS:-0}"
frame_rate="${TOUCH_DEMO_PRESENTER_FRAME_RATE:-current}"
layer_z="$(normalize_layer_z "${TOUCH_DEMO_LAYER_Z:-2100000000}")"
layer_name="${TOUCH_DEMO_LAYER_NAME:-DirectScreenAPI}"

presenter_pid="$(read_pid "$presenter_pid_file")"
if [ -n "$presenter_pid" ] && ! pid_running "$presenter_pid"; then
  rm -f "$presenter_pid_file"
  presenter_pid=""
fi

if [ -z "$presenter_pid" ]; then
  TOUCH_DEMO_STATE_PIPE="$state_pipe" CLASSPATH="$dex_jar" nohup "$app_process_bin" /system/bin --nice-name=DSAPIPresenterDemo \
    org.directscreenapi.adapter.AndroidAdapterMain touch-ui-loop \
    "$state_pipe" "$layer_z" "$layer_name" "$blur_radius" "$frame_rate" \
    >>"$presenter_log_file" 2>&1 &
  presenter_pid="$!"
  echo "$presenter_pid" >"$presenter_pid_file"
  if ! wait_process_stable "$presenter_pid" 6; then
    rm -f "$presenter_pid_file"
    rm -f "$state_pipe"
    echo "state=error pid=- reason=presenter_start_failed"
    exit 2
  fi
  if ! wait_presenter_started "$presenter_pid" "$presenter_log_file" 40; then
    rm -f "$presenter_pid_file"
    rm -f "$state_pipe"
    echo "state=error pid=- reason=presenter_not_ready"
    exit 2
  fi
fi
if ! wait_display_ready 40; then
  echo "state=error pid=- reason=display_not_ready"
  exit 2
fi

route_name="${TOUCH_DEMO_ROUTE_NAME:-demo-window}"
demo_fps="${TOUCH_DEMO_FPS:-60}"
run_seconds="${TOUCH_DEMO_RUN_SECONDS:-0}"
window_w="${TOUCH_DEMO_WINDOW_W:-620}"
window_h="${TOUCH_DEMO_WINDOW_H:-440}"
window_x="${TOUCH_DEMO_WINDOW_X:-}"
window_y="${TOUCH_DEMO_WINDOW_Y:-}"
device_path="${TOUCH_DEMO_DEVICE:-}"
no_touch_router="$(to_bool "${TOUCH_DEMO_NO_TOUCH_ROUTER:-0}")"

set -- "$demo_bin" \
  --control-socket "$control_socket" \
  --data-socket "$data_socket" \
  --state-pipe "$state_pipe" \
  --presenter-pid "$presenter_pid" \
  --route-name "$route_name" \
  --fps "$demo_fps" \
  --window-w "$window_w" \
  --window-h "$window_h"

if [ -n "$window_x" ]; then
  set -- "$@" --window-x "$window_x"
fi
if [ -n "$window_y" ]; then
  set -- "$@" --window-y "$window_y"
fi
if [ -n "$device_path" ]; then
  set -- "$@" --device "$device_path"
fi
if [ "$no_touch_router" = "1" ]; then
  set -- "$@" --no-touch-router
fi
if [ "$run_seconds" != "0" ] && [ -n "$run_seconds" ]; then
  set -- "$@" --run-seconds "$run_seconds"
fi

nohup "$@" >>"$demo_log_file" 2>&1 &
demo_pid="$!"
echo "$demo_pid" >"$demo_pid_file"

if ! wait_process_stable "$demo_pid" 8; then
  rm -f "$demo_pid_file"
  rm -f "$state_pipe"
  echo "state=error pid=- reason=demo_start_failed"
  exit 2
fi

echo "state=running pid=$demo_pid reason=- presenter_pid=$presenter_pid"
