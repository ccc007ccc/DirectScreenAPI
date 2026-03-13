#!/system/bin/sh
set -eu

MODROOT="${0%/*}"
DSAPI_LIB_FILE="$MODROOT/bin/dsapi_ksu_lib.sh"

if [ ! -f "$DSAPI_LIB_FILE" ]; then
  exit 0
fi

# shellcheck source=/dev/null
. "$DSAPI_LIB_FILE"

dsapi_init_layout
dsapi_runtime_seed_sync

# 避免开机早期阶段 race。
sleep 8

dsapi_manager_pkg="$(dsapi_manager_package_name)"
if ! dsapi_manager_ensure_installed "$dsapi_manager_pkg" "$MANAGER_APK_FILE" >/dev/null 2>&1; then
  dsapi_log "ksu_dsapi_error=service_manager_auto_install_failed package=$dsapi_manager_pkg path=$MANAGER_APK_FILE"
fi

if [ "$(dsapi_enabled_get)" = "1" ]; then
  if ! dsapi_cap_start_by_id core.daemon >/dev/null 2>&1; then
    dsapi_log "ksu_dsapi_error=service_daemon_start_failed capability=core.daemon"
  fi
  # 同步 display 状态到 daemon，保证触摸/坐标在旋转时自动更新。
  dsapi_cap_start_by_id core.display_watch >/dev/null 2>&1 || true
else
  dsapi_cap_stop_by_id core.display_watch >/dev/null 2>&1 || true
  if ! dsapi_cap_stop_by_id core.daemon >/dev/null 2>&1; then
    dsapi_log "ksu_dsapi_error=service_daemon_stop_failed capability=core.daemon"
  fi
fi

if ! dsapi_manager_bridge_start >/dev/null 2>&1; then
  dsapi_log "ksu_dsapi_error=service_bridge_start_failed service=$MANAGER_BRIDGE_DEFAULT_SERVICE"
fi

if ! dsapi_zygote_agent_start >/dev/null 2>&1; then
  dsapi_log "ksu_dsapi_error=service_zygote_agent_start_failed service=$(dsapi_zygote_agent_service_get)"
fi
