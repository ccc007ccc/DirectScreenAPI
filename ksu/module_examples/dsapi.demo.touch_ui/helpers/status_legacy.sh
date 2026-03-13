#!/system/bin/sh
set -eu

state_dir="${DSAPI_MODULE_STATE_DIR:-/data/adb/dsapi/state/modules/dsapi.demo.touch_ui}"
demo_pid_file="$state_dir/touch_demo.pid"
presenter_pid_file="$state_dir/presenter.pid"

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

find_first_pid_by_name() {
  name="$1"
  [ -n "$name" ] || { echo ""; return 0; }
  if command -v pidof >/dev/null 2>&1; then
    pidof "$name" 2>/dev/null | awk '{print $1}' || true
    return 0
  fi
  echo ""
}

demo_pid="$(read_pid "$demo_pid_file")"
presenter_pid="$(read_pid "$presenter_pid_file")"

demo_running=0
presenter_running=0
if [ -n "$demo_pid" ] && pid_running "$demo_pid" && pid_has_text "$demo_pid" "dsapi_touch_demo"; then
  demo_running=1
else
  rm -f "$demo_pid_file"
  demo_pid="-"
fi

if [ -n "$presenter_pid" ] && pid_running "$presenter_pid" \
  && { pid_has_text "$presenter_pid" "touch-ui-loop" || pid_has_text "$presenter_pid" "DSAPIPresenterDemo"; }; then
  presenter_running=1
else
  rm -f "$presenter_pid_file"
  presenter_pid="-"
fi

if [ "$demo_running" != "1" ]; then
  stray_demo_pid="$(find_first_pid_by_name "dsapi_touch_demo")"
  if [ -n "$stray_demo_pid" ] && pid_running "$stray_demo_pid"; then
    demo_running=1
    demo_pid="$stray_demo_pid"
    echo "$stray_demo_pid" >"$demo_pid_file" 2>/dev/null || true
  fi
fi

if [ "$presenter_running" != "1" ]; then
  stray_presenter_pid="$(find_first_pid_by_name "DSAPIPresenterDemo")"
  if [ -n "$stray_presenter_pid" ] && pid_running "$stray_presenter_pid"; then
    presenter_running=1
    presenter_pid="$stray_presenter_pid"
    echo "$stray_presenter_pid" >"$presenter_pid_file" 2>/dev/null || true
  fi
fi

if [ "$demo_running" = "1" ]; then
  echo "state=running pid=$demo_pid reason=- presenter_pid=$presenter_pid presenter_running=$presenter_running"
  exit 0
fi

if [ "$presenter_running" = "1" ]; then
  echo "state=degraded pid=- reason=demo_not_running presenter_pid=$presenter_pid"
  exit 0
fi

echo "state=stopped pid=- reason=not_running presenter_pid=-"
