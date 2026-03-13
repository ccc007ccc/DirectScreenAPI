#!/system/bin/sh
ACTION_NAME="运行Hello Consumer"
ACTION_DANGER=0
set -eu

core_service="${HELLO_CORE_SERVICE:-dsapi.core}"
service_id="${HELLO_SERVICE_ID:-dsapi.demo.hello}"
msg="${HELLO_MESSAGE:-ping}"

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
out="$(CLASSPATH="$dex_jar" "$app_process_bin" /system/bin "$main" hello-consumer "$core_service" "$service_id" "$msg" 2>&1)" || true
printf '%s\n' "$out"
echo "state=stopped pid=- reason=completed"
