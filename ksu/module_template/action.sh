#!/system/bin/sh
set -eu

MODROOT="$(CDPATH= cd -- "${0%/*}" && pwd)"
CTL="$MODROOT/bin/dsapi_service_ctl.sh"
REFRESH_MS="${DSAPI_MANAGER_UI_REFRESH_MS:-1000}"
SH_BIN="/system/bin/sh"

if [ ! -x "$SH_BIN" ]; then
  echo "dsapi_action_error=sh_missing path=$SH_BIN"
  exit 2
fi

if [ ! -f "$CTL" ]; then
  echo "dsapi_action_error=ctl_missing path=$CTL"
  exit 2
fi

ui_line="$($SH_BIN "$CTL" ui start "$REFRESH_MS" 2>&1 || true)"
case "$ui_line" in
  *"ksu_dsapi_ui=started"*|*"ksu_dsapi_ui=already_running"*)
    echo "dsapi_action_status=ok mode=ui_only refresh_ms=$REFRESH_MS"
    printf '%s\n' "$ui_line"
    exit 0
    ;;
  *)
    echo "dsapi_action_error=ui_start_failed"
    printf '%s\n' "$ui_line"
    echo "fix_hint=查看日志 /data/adb/dsapi/log/manager_host.log"
    exit 2
    ;;
esac
