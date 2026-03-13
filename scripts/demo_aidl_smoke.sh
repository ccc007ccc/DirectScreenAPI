#!/bin/sh
set -eu

MOD_ROOT="${DSAPI_MODULE_ROOT:-/data/adb/modules/directscreenapi}"
CTL_SH="${DSAPI_CTL_SH:-$MOD_ROOT/bin/dsapi_service_ctl.sh}"
SERVICE_NAME="${1:-${DSAPI_BRIDGE_SERVICE:-dsapi.core}}"

if [ ! -x "$CTL_SH" ]; then
  echo "demo_smoke_error=ctl_missing path=$CTL_SH"
  exit 2
fi

run_ctl() {
  if command -v su >/dev/null 2>&1; then
    su -c "$CTL_SH $*"
  else
    "$CTL_SH" "$@"
  fi
}

must_contain() {
  hay="$1"
  needle="$2"
  if ! printf '%s\n' "$hay" | grep -q "$needle"; then
    echo "demo_smoke_assert_failed needle=$needle"
    echo "demo_smoke_output_begin"
    printf '%s\n' "$hay"
    echo "demo_smoke_output_end"
    exit 2
  fi
}

echo "demo_smoke_step=bridge_start service=$SERVICE_NAME"
bridge_out="$(run_ctl bridge start "$SERVICE_NAME" || true)"
must_contain "$bridge_out" "ksu_dsapi_bridge"

echo "demo_smoke_step=demo_start_aidl"
start_out="$(run_ctl demo aidl "$SERVICE_NAME" start 0 0 1000000 DirectScreenAPI-GPU 0 || true)"
must_contain "$start_out" "result_code=0"
must_contain "$start_out" "ksu_dsapi_demo"

echo "demo_smoke_step=demo_status_aidl"
status_out="$(run_ctl demo aidl "$SERVICE_NAME" status || true)"
must_contain "$status_out" "result_code=0"
must_contain "$status_out" "ksu_dsapi_demo state=running"

echo "demo_smoke_step=demo_set_pos"
set_pos_out="$(run_ctl demo aidl "$SERVICE_NAME" set-pos 120 240 || true)"
must_contain "$set_pos_out" "result_code=0"

echo "demo_smoke_step=demo_set_view"
set_view_out="$(run_ctl demo aidl "$SERVICE_NAME" set-view 900 600 || true)"
must_contain "$set_view_out" "result_code=0"

echo "demo_smoke_step=demo_set_color"
set_color_out="$(run_ctl demo aidl "$SERVICE_NAME" set-color 64 180 255 255 || true)"
must_contain "$set_color_out" "result_code=0"

echo "demo_smoke_step=demo_set_speed"
set_speed_out="$(run_ctl demo aidl "$SERVICE_NAME" set-speed 1.25 || true)"
must_contain "$set_speed_out" "result_code=0"

echo "demo_smoke_step=demo_stop_aidl"
stop_out="$(run_ctl demo aidl "$SERVICE_NAME" stop || true)"
must_contain "$stop_out" "result_code=0"

echo "demo_smoke_step=demo_status_after_stop"
status2_out="$(run_ctl demo aidl "$SERVICE_NAME" status || true)"
must_contain "$status2_out" "result_code=0"
must_contain "$status2_out" "ksu_dsapi_demo state=stopped"

echo "demo_smoke_result=ok service=$SERVICE_NAME"
