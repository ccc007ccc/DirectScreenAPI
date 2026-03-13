#!/system/bin/sh
ACTION_NAME="调整速度(AIDL)"
ACTION_DANGER=0
set -eu

ctl_path="${DSAPI_MODROOT:-/data/adb/modules/directscreenapi}/bin/dsapi_service_ctl.sh"
[ -f "$ctl_path" ] || {
  echo "state=error pid=- reason=ctl_missing"
  exit 2
}

bridge_service="${GPU_DEMO_BRIDGE_SERVICE:-dsapi.core}"
speed="${GPU_DEMO_SPEED:-1.25}"

set +e
out="$(/system/bin/sh "$ctl_path" demo aidl "$bridge_service" set-speed "$speed" 2>&1)"
rc=$?
set -e
printf '%s\n' "$out"

if [ "$rc" -eq 0 ] && printf '%s\n' "$out" | grep -q '^result_code=0$'; then
  exit 0
fi
exit 2
