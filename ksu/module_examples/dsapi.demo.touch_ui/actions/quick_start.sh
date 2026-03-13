#!/system/bin/sh
ACTION_NAME="快速重启"
ACTION_DANGER=0
set -eu

mod_dir="${DSAPI_MODULE_DIR:-/data/adb/dsapi/modules/dsapi.demo.touch_ui}"
/system/bin/sh "$mod_dir/actions/stop.sh" >/dev/null 2>&1 || true
/system/bin/sh "$mod_dir/actions/start.sh"
