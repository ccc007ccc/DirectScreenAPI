#!/system/bin/sh
ACTION_NAME="启动GPU实验窗口(AIDL)"
ACTION_DANGER=0
set -eu

ctl_path="${DSAPI_MODROOT:-/data/adb/modules/directscreenapi}/bin/dsapi_service_ctl.sh"
[ -f "$ctl_path" ] || {
  echo "state=error pid=- reason=ctl_missing"
  exit 2
}

bridge_service="${GPU_DEMO_BRIDGE_SERVICE:-dsapi.core}"
width="${GPU_DEMO_WIDTH:-0}"
height="${GPU_DEMO_HEIGHT:-0}"
z_layer="${GPU_DEMO_Z_LAYER:-1000000}"
layer_name="${GPU_DEMO_LAYER_NAME:-DirectScreenAPI-GPU}"
run_seconds="${GPU_DEMO_RUN_SECONDS:-0}"

set +e
out="$(/system/bin/sh "$ctl_path" demo aidl "$bridge_service" start "$width" "$height" "$z_layer" "$layer_name" "$run_seconds" 2>&1)"
rc=$?
set -e
printf '%s\n' "$out"

if [ "$rc" -eq 0 ] && printf '%s\n' "$out" | grep -q '^result_code=0$'; then
  exit 0
fi
exit 2
