#!/system/bin/sh
ACTION_NAME="启动Hello Provider"
ACTION_DANGER=0
set -eu

state_dir="${DSAPI_MODULE_STATE_DIR:-/data/adb/dsapi/state/modules/dsapi.demo.hello_provider}"
mkdir -p "$state_dir"

pid_file="$state_dir/provider.pid"
log_file="$state_dir/provider.log"

core_service="${HELLO_CORE_SERVICE:-dsapi.core}"
service_id="${HELLO_SERVICE_ID:-dsapi.demo.hello}"
version="${HELLO_SERVICE_VERSION:-1}"
meta="${HELLO_META:-}"
prefix="${HELLO_RESPONSE_PREFIX:-echo:}"

read_pid() {
  [ -f "$1" ] || { echo ""; return 0; }
  cat "$1" 2>/dev/null || echo ""
}

pid_running() {
  pid="$1"
  [ -n "$pid" ] || return 1
  kill -0 "$pid" >/dev/null 2>&1
}

old_pid="$(read_pid "$pid_file")"
if [ -n "$old_pid" ] && pid_running "$old_pid"; then
  echo "state=running pid=$old_pid reason=already_running"
  exit 0
fi
rm -f "$pid_file"

dex_jar="${DSAPI_ACTIVE_ADAPTER_DEX:-${DSAPI_MODROOT:-/data/adb/modules/directscreenapi}/android/directscreen-adapter-dex.jar}"
if [ ! -f "$dex_jar" ]; then
  echo "state=error pid=- reason=adapter_dex_missing"
  exit 2
fi

app_process_bin="${DSAPI_APP_PROCESS_BIN:-}"
if [ -z "$app_process_bin" ]; then
  for b in /system/bin/app_process64 /system/bin/app_process /system/bin/app_process32; do
    [ -x "$b" ] && app_process_bin="$b" && break
  done
fi
if [ -z "$app_process_bin" ] || [ ! -x "$app_process_bin" ]; then
  echo "state=error pid=- reason=app_process_missing"
  exit 2
fi

main="org.directscreenapi.adapter.AndroidAdapterMain"

CLASSPATH="$dex_jar" nohup "$app_process_bin" /system/bin --nice-name=dsapi.demo.hello_provider "$main" \
  hello-provider "$core_service" "$service_id" "$version" "$meta" "$prefix" \
  >>"$log_file" 2>&1 &
pid="$!"
echo "$pid" >"$pid_file"

sleep 0.12
if ! pid_running "$pid"; then
  rm -f "$pid_file"
  echo "state=error pid=- reason=start_failed"
  exit 2
fi

echo "state=running pid=$pid reason=-"
