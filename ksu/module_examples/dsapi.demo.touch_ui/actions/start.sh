#!/system/bin/sh
ACTION_NAME="启动测试窗口"
ACTION_DANGER=0
set -eu

module_dir="${DSAPI_MODULE_DIR:-/data/adb/dsapi/modules/dsapi.demo.touch_ui}"
helper_start="$module_dir/helpers/start_legacy.sh"
if [ ! -f "$helper_start" ]; then
  echo "state=error pid=- reason=helper_missing"
  exit 2
fi
exec /system/bin/sh "$helper_start"
