#!/system/bin/sh
MODROOT="${0%/*}"
DSAPI_LIB_FILE="$MODROOT/bin/dsapi_ksu_lib.sh"

if [ -f "$DSAPI_LIB_FILE" ]; then
  # shellcheck source=/dev/null
  . "$DSAPI_LIB_FILE"
  dsapi_manager_bridge_stop >/dev/null 2>&1 || true
  dsapi_daemon_stop >/dev/null 2>&1 || true
  if [ -f "$CAP_UI_PID_FILE" ]; then
    ui_pid="$(cat "$CAP_UI_PID_FILE" 2>/dev/null || true)"
    if [ -n "$ui_pid" ]; then
      kill "$ui_pid" >/dev/null 2>&1 || true
      sleep 0.2
      kill -KILL "$ui_pid" >/dev/null 2>&1 || true
    fi
    rm -f "$CAP_UI_PID_FILE"
  fi
fi

rm -f /data/adb/dsapi/run/dsapi.sock /data/adb/dsapi/run/dsapi.data.sock 2>/dev/null || true
rm -f /data/adb/dsapi/run/manager_bridge.pid /data/adb/dsapi/run/manager_bridge.service /data/adb/dsapi/run/manager_bridge.ready /data/adb/dsapi/run/manager_bridge.port 2>/dev/null || true
# 保留 /data/adb/dsapi/log 以便排障。
