#!/system/bin/sh
ACTION_NAME="停止测试窗口"
ACTION_DANGER=1
set -eu

module_dir="${DSAPI_MODULE_DIR:-/data/adb/dsapi/modules/dsapi.demo.touch_ui}"
helper_stop="$module_dir/helpers/stop_legacy.sh"
if [ ! -f "$helper_stop" ]; then
  echo "state=error pid=- reason=helper_missing"
  exit 2
fi
exec /system/bin/sh "$helper_stop"
