#!/system/bin/sh
ACTION_NAME="运行状态"
ACTION_DANGER=0
set -eu

module_dir="${DSAPI_MODULE_DIR:-/data/adb/dsapi/modules/dsapi.demo.touch_ui}"
helper_status="$module_dir/helpers/status_legacy.sh"
if [ ! -f "$helper_status" ]; then
  echo "state=error pid=- reason=helper_missing"
  exit 2
fi
exec /system/bin/sh "$helper_status"
