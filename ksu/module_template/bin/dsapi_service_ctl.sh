#!/system/bin/sh
set -eu

MODDIR="${0%/*}"
MODROOT="${MODDIR%/*}"
DSAPI_LIB_FILE="$MODDIR/dsapi_ksu_lib.sh"

if [ ! -f "$DSAPI_LIB_FILE" ]; then
  echo "ksu_dsapi_error=lib_missing path=$DSAPI_LIB_FILE"
  exit 2
fi

# shellcheck source=/dev/null
. "$DSAPI_LIB_FILE"

dsapi_init_layout
dsapi_runtime_seed_sync

RESULT_V2=0
if [ "${1:-}" = "--v2" ]; then
  RESULT_V2=1
  shift
fi

is_uint() {
  case "$1" in
    ''|*[!0-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

dsapi_result_v2_finalize() {
  dsapi_exit_code="${1:-1}"
  trap - EXIT

  exec 1>&3
  exec 3>&-

  dsapi_result_type="ctl.${cmd:-unknown}"
  if [ "$dsapi_exit_code" -eq 0 ]; then
    dsapi_result_message="ok"
  else
    dsapi_result_message="error"
  fi

  echo "result_version=2"
  echo "result_type=$dsapi_result_type"
  echo "result_code=$dsapi_exit_code"
  echo "result_message=$dsapi_result_message"
  if [ -f "$DSAPI_RESULT_BODY_FILE" ]; then
    cat "$DSAPI_RESULT_BODY_FILE"
    rm -f "$DSAPI_RESULT_BODY_FILE"
  fi
  exit "$dsapi_exit_code"
}

dsapi_result_v2_init() {
  DSAPI_RESULT_BODY_FILE="$RUN_DIR/ctl.result.$$"
  rm -f "$DSAPI_RESULT_BODY_FILE"
  exec 3>&1
  exec >"$DSAPI_RESULT_BODY_FILE"
  trap 'dsapi_result_v2_finalize $?' EXIT
}

print_daemon_status_line() {
  dsapi_status="$(dsapi_daemon_status_kv)"
  dsapi_state="$(dsapi_kv_get state "$dsapi_status")"
  dsapi_pid="$(dsapi_kv_get pid "$dsapi_status")"
  dsapi_reason="$(dsapi_kv_get reason "$dsapi_status")"

  [ -n "$dsapi_state" ] || dsapi_state="unknown"
  [ -n "$dsapi_pid" ] || dsapi_pid="-"
  [ -n "$dsapi_reason" ] || dsapi_reason="-"

  echo "ksu_dsapi_status=$dsapi_state pid=$dsapi_pid reason=$dsapi_reason"
}

ui_status_kv() {
  ui_pkg="$(dsapi_manager_package_name)"
  ui_comp="$(dsapi_manager_main_component)"
  host_status="$(dsapi_manager_host_status_kv)"
  host_state="$(dsapi_kv_get state "$host_status")"
  if [ "$host_state" = "running" ] || [ "$host_state" = "starting" ]; then
    echo "$host_status package=$ui_pkg component=$ui_comp"
    return 0
  fi
  echo "state=stopped pid=- mode=parasitic_host package=$ui_pkg component=$ui_comp"
}

ui_install() {
  ui_pkg="$(dsapi_manager_package_name)"
  ui_apk="$MANAGER_APK_FILE"
  if dsapi_manager_pkg_installed "$ui_pkg"; then
    echo "ksu_dsapi_ui_install=already_installed package=$ui_pkg"
    return 0
  fi
  if ! dsapi_manager_ensure_installed "$ui_pkg" "$ui_apk"; then
    echo "ksu_dsapi_error=manager_install_failed_manual package=$ui_pkg path=$ui_apk"
    return 2
  fi
  echo "ksu_dsapi_ui_install=ok package=$ui_pkg path=$ui_apk"
}

ui_start() {
  refresh_ms="${1:-1200}"
  if ! is_uint "$refresh_ms"; then
    echo "ksu_dsapi_error=invalid_refresh_ms value=$refresh_ms"
    return 2
  fi

  ui_status="$(ui_status_kv)"
  ui_state="$(dsapi_kv_get state "$ui_status")"
  ui_visible="$(dsapi_kv_get visible "$ui_status")"
  [ -n "$ui_visible" ] || ui_visible="0"
  if [ "$ui_state" = "running" ] && [ "$ui_visible" = "1" ]; then
    echo "ksu_dsapi_ui=already_running $ui_status refresh_ms=$refresh_ms"
    return 0
  fi
  if [ "$ui_state" = "running" ] && [ "$ui_visible" != "1" ]; then
    dsapi_manager_force_foreground "$(dsapi_manager_main_component)" "$bridge_service" "$refresh_ms" zygote >/dev/null 2>&1 || true
    ui_status="$(ui_status_kv)"
    ui_visible="$(dsapi_kv_get visible "$ui_status")"
    [ -n "$ui_visible" ] || ui_visible="0"
    if [ "$ui_visible" = "1" ]; then
      echo "ksu_dsapi_ui=started $ui_status refresh_ms=$refresh_ms"
      return 0
    fi
  fi

  ui_pkg="$(dsapi_manager_package_name)"
  ui_comp="$(dsapi_manager_main_component)"
  if ! dsapi_manager_ensure_installed "$ui_pkg" "$MANAGER_APK_FILE"; then
    echo "ksu_dsapi_error=manager_auto_install_failed package=$ui_pkg path=$MANAGER_APK_FILE"
    return 2
  fi

  bridge_status="$(dsapi_manager_bridge_status_kv)"
  bridge_state="$(dsapi_kv_get state "$bridge_status")"
  bridge_service="$(dsapi_kv_get service "$bridge_status")"
  [ -n "$bridge_service" ] || bridge_service="$MANAGER_BRIDGE_DEFAULT_SERVICE"
  if [ "$bridge_state" != "running" ]; then
    if ! dsapi_manager_bridge_start "$bridge_service" >/dev/null 2>&1; then
      echo "ksu_dsapi_error=bridge_start_failed service=$bridge_service log=$MANAGER_BRIDGE_LOG_FILE"
      return 2
    fi
  fi

  if ! dsapi_manager_host_start "$refresh_ms" "$bridge_service"; then
    echo "ksu_dsapi_error=ui_host_start_failed package=$ui_pkg component=$ui_comp bridge_service=$bridge_service log=$MANAGER_HOST_LOG_FILE"
    return 2
  fi

  if ! dsapi_manager_is_foreground "$ui_pkg"; then
    dsapi_manager_force_foreground "$ui_comp" "$bridge_service" "$refresh_ms" zygote >/dev/null 2>&1 || true
  fi

  ui_retry=0
  while [ "$ui_retry" -lt 20 ]; do
    sleep 0.1
    ui_status="$(ui_status_kv)"
    ui_state="$(dsapi_kv_get state "$ui_status")"
    ui_visible="$(dsapi_kv_get visible "$ui_status")"
    [ -n "$ui_visible" ] || ui_visible="0"
    if [ "$ui_state" = "running" ] && [ "$ui_visible" = "1" ]; then
      echo "ksu_dsapi_ui=started $ui_status refresh_ms=$refresh_ms"
      return 0
    fi
    ui_retry=$((ui_retry + 1))
  done

  if [ "$ui_state" = "running" ]; then
    echo "ksu_dsapi_ui=started $ui_status refresh_ms=$refresh_ms focus=0"
    return 0
  fi

  echo "ksu_dsapi_error=ui_host_not_running package=$ui_pkg component=$ui_comp bridge_service=$bridge_service log=$MANAGER_HOST_LOG_FILE"
  return 2
}

ui_stop() {
  ui_pkg="$(dsapi_manager_package_name)"
  dsapi_manager_host_stop >/dev/null 2>&1 || true
  rm -f "$CAP_UI_PID_FILE"
  echo "ksu_dsapi_ui=stopped mode=parasitic_host package=$ui_pkg"
}

capability_cmd() {
  sub="${1:-}"
  case "$sub" in
    list)
      dsapi_cap_list_lines | while IFS='|' read -r \
        cap_id cap_name cap_kind cap_source cap_state cap_pid cap_reason cap_file
      do
        printf 'capability_row=%s|%s|%s|%s|%s|%s|%s\n' \
          "$cap_id" "$cap_name" "$cap_kind" "$cap_source" "$cap_state" "$cap_pid" "$cap_reason"
      done
      ;;
    start)
      cap_id="${2:-}"
      [ -n "$cap_id" ] || { echo "ksu_dsapi_error=capability_id_missing"; return 2; }
      dsapi_cap_start_by_id "$cap_id"
      echo "capability_action=started id=$cap_id"
      ;;
    stop)
      cap_id="${2:-}"
      [ -n "$cap_id" ] || { echo "ksu_dsapi_error=capability_id_missing"; return 2; }
      dsapi_cap_stop_by_id "$cap_id"
      echo "capability_action=stopped id=$cap_id"
      ;;
    remove)
      cap_id="${2:-}"
      [ -n "$cap_id" ] || { echo "ksu_dsapi_error=capability_id_missing"; return 2; }
      dsapi_cap_remove_by_id "$cap_id"
      echo "capability_action=removed id=$cap_id"
      ;;
    enable)
      cap_id="${2:-}"
      [ -n "$cap_id" ] || { echo "ksu_dsapi_error=capability_id_missing"; return 2; }
      dsapi_cap_enable_by_id "$cap_id"
      echo "capability_action=enabled id=$cap_id"
      ;;
    status)
      cap_id="${2:-}"
      [ -n "$cap_id" ] || { echo "ksu_dsapi_error=capability_id_missing"; return 2; }
      cap_status="$(dsapi_cap_status_by_id "$cap_id")"
      echo "capability_status id=$cap_id $cap_status"
      ;;
    detail)
      cap_id="${2:-}"
      [ -n "$cap_id" ] || { echo "ksu_dsapi_error=capability_id_missing"; return 2; }
      dsapi_cap_detail_by_id "$cap_id"
      ;;
    add)
      cap_file="${2:-}"
      [ -n "$cap_file" ] || { echo "ksu_dsapi_error=capability_file_missing"; return 2; }
      cap_id="$(dsapi_cap_add_custom "$cap_file")"
      echo "capability_action=added id=$cap_id"
      ;;
    *)
      echo "usage: dsapi_service_ctl.sh capability list|start <id>|stop <id>|remove <id>|enable <id>|status <id>|detail <id>|add <file>"
      return 1
      ;;
  esac
}

runtime_cmd() {
  sub="${1:-}"
  case "$sub" in
    list)
      active="$(dsapi_runtime_active_release_id)"
      for rel in $(dsapi_runtime_list_ids)
      do
        if [ "$rel" = "$active" ]; then
          echo "runtime_release=$rel active=1"
        else
          echo "runtime_release=$rel active=0"
        fi
      done
      ;;
    active)
      active="$(dsapi_runtime_active_release_id)"
      if [ -n "$active" ]; then
        echo "runtime_active=$active"
      else
        echo "runtime_active=<none>"
      fi
      ;;
    activate)
      rel="${2:-}"
      [ -n "$rel" ] || { echo "ksu_dsapi_error=runtime_id_missing"; return 2; }
      dsapi_runtime_activate "$rel"
      if [ "$(dsapi_enabled_get)" = "1" ]; then
        dsapi_daemon_stop >/dev/null 2>&1 || true
        dsapi_daemon_start
      fi
      echo "runtime_action=activated release=$rel"
      ;;
    install)
      rel="${2:-}"
      src_dir="${3:-}"
      [ -n "$rel" ] || { echo "ksu_dsapi_error=runtime_id_missing"; return 2; }
      [ -n "$src_dir" ] || { echo "ksu_dsapi_error=runtime_src_missing"; return 2; }
      dsapi_runtime_install_dir "$rel" "$src_dir"
      echo "runtime_action=installed release=$rel"
      ;;
    remove)
      rel="${2:-}"
      [ -n "$rel" ] || { echo "ksu_dsapi_error=runtime_id_missing"; return 2; }
      dsapi_runtime_remove "$rel"
      echo "runtime_action=removed release=$rel"
      ;;
    *)
      echo "usage: dsapi_service_ctl.sh runtime list|active|activate <id>|install <id> <dir>|remove <id>"
      return 1
      ;;
  esac
}

module_cmd() {
  sub="${1:-list}"
  scope_pkg="${DSAPI_SCOPE_PACKAGE:-*}"
  scope_user="${DSAPI_SCOPE_USER:--1}"
  case "$sub" in
    list)
      dsapi_daemon_cmd MODULE_LIST
      ;;
    install-zip|install)
      zip_path="${2:-}"
      [ -n "$zip_path" ] || { echo "ksu_dsapi_error=module_zip_missing"; return 2; }
      mod_id="$(dsapi_module_install_zip "$zip_path")" || {
        dsapi_last_error_set "module.install" "install_failed" "module_install_failed" "path=$zip_path"
        echo "ksu_dsapi_error=module_install_failed path=$zip_path"
        return 2
      }
      dsapi_daemon_cmd MODULE_SYNC >/dev/null 2>&1 || {
        dsapi_last_error_set "module.install" "daemon_sync_failed" "module_sync_failed" "id=$mod_id"
        echo "ksu_dsapi_error=module_sync_failed id=$mod_id"
        return 2
      }
      if dsapi_module_auto_start_enabled "$mod_id"; then
        dsapi_daemon_cmd MODULE_START "$mod_id" "$scope_pkg" "$scope_user" >/dev/null 2>&1 || true
      fi
      dsapi_last_error_clear
      echo "module_action=installed id=$mod_id path=$zip_path"
      ;;
    start)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_START "$mod_id" "$scope_pkg" "$scope_user"
      ;;
    stop)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_STOP "$mod_id"
      ;;
    reload)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_RELOAD "$mod_id" "$scope_pkg" "$scope_user"
      ;;
    reload-all)
      dsapi_daemon_cmd MODULE_RELOAD_ALL "$scope_pkg" "$scope_user"
      ;;
    disable)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_DISABLE "$mod_id"
      ;;
    enable)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_ENABLE "$mod_id"
      ;;
    remove)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_REMOVE "$mod_id"
      ;;
    status)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_STATUS "$mod_id"
      ;;
    detail)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_DETAIL "$mod_id"
      ;;
    action-list)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_ACTION_LIST "$mod_id"
      ;;
    action-run)
      mod_id="${2:-}"
      action_id="${3:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      [ -n "$action_id" ] || { echo "ksu_dsapi_error=module_action_missing"; return 2; }
      dsapi_daemon_cmd MODULE_ACTION_RUN "$mod_id" "$action_id" "$scope_pkg" "$scope_user"
      ;;
    env-list)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_ENV_LIST "$mod_id"
      ;;
    env-set)
      mod_id="${2:-}"
      env_key="${3:-}"
      env_value="${4:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      [ -n "$env_key" ] || { echo "ksu_dsapi_error=module_env_key_missing"; return 2; }
      dsapi_daemon_cmd MODULE_ENV_SET "$mod_id" "$env_key" "$env_value"
      ;;
    env-unset)
      mod_id="${2:-}"
      env_key="${3:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      [ -n "$env_key" ] || { echo "ksu_dsapi_error=module_env_key_missing"; return 2; }
      dsapi_daemon_cmd MODULE_ENV_UNSET "$mod_id" "$env_key"
      ;;
    scope-list)
      mod_id="${2:-}"
      if [ -n "$mod_id" ]; then
        dsapi_daemon_cmd MODULE_SCOPE_LIST "$mod_id"
      else
        dsapi_daemon_cmd MODULE_SCOPE_LIST
      fi
      ;;
    scope-set)
      mod_id="${2:-}"
      scope_pkg="${3:-}"
      scope_uid="${4:-}"
      scope_policy="${5:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      [ -n "$scope_pkg" ] || { echo "ksu_dsapi_error=scope_package_missing"; return 2; }
      [ -n "$scope_uid" ] || { echo "ksu_dsapi_error=scope_user_missing"; return 2; }
      [ -n "$scope_policy" ] || { echo "ksu_dsapi_error=scope_policy_missing"; return 2; }
      dsapi_daemon_cmd MODULE_SCOPE_SET "$mod_id" "$scope_pkg" "$scope_uid" "$scope_policy"
      ;;
    scope-clear)
      mod_id="${2:-}"
      [ -n "$mod_id" ] || { echo "ksu_dsapi_error=module_id_missing"; return 2; }
      dsapi_daemon_cmd MODULE_SCOPE_CLEAR "$mod_id"
      ;;
    *)
      echo "usage: dsapi_service_ctl.sh module list|install-zip <zip>|start <id>|stop <id>|reload <id>|reload-all|disable <id>|enable <id>|remove <id>|status <id>|detail <id>|action-list <id>|action-run <id> <action>|env-list <id>|env-set <id> <key> <value>|env-unset <id> <key>|scope-list [id]|scope-set <id> <package|*> <user|-1> <allow|deny>|scope-clear <id>"
      return 1
      ;;
  esac
}

errors_cmd() {
  sub="${1:-last}"
  case "$sub" in
    last)
      dsapi_last_error_dump
      ;;
    clear)
      dsapi_last_error_clear
      echo "last_error_state=cleared"
      ;;
    *)
      echo "usage: dsapi_service_ctl.sh errors last|clear"
      return 1
      ;;
  esac
}

demo_cmd() {
  sub="${1:-status}"
  aidl_service=""

  # V3：GPU demo 统一走 Binder/AIDL 控制面（bridge exec_v2），不再支持旧的独立进程模式。
  if [ "$sub" = "aidl" ]; then
    aidl_service="${2:-$(dsapi_manager_bridge_service_get)}"
    if [ "$#" -ge 3 ]; then
      shift 2
    else
      set -- status
    fi
  else
    aidl_service="$(dsapi_manager_bridge_service_get)"
    if [ "$#" -lt 1 ]; then
      set -- status
    fi
  fi

  aidl_bridge_status="$(dsapi_manager_bridge_status_kv)"
  aidl_bridge_state="$(dsapi_kv_get state "$aidl_bridge_status")"
  if [ "$aidl_bridge_state" != "running" ] && [ "$aidl_bridge_state" != "starting" ]; then
    if ! dsapi_manager_bridge_start "$aidl_service" >/dev/null 2>&1; then
      echo "ksu_dsapi_error=demo_aidl_bridge_start_failed service=$aidl_service log=$MANAGER_BRIDGE_LOG_FILE"
      return 2
    fi
  fi

  if ! dsapi_bridge_exec_ctl "$aidl_service" demo "$@"; then
    sleep 0.12
    if ! dsapi_bridge_exec_ctl "$aidl_service" demo "$@"; then
      echo "ksu_dsapi_error=demo_aidl_exec_failed service=$aidl_service"
      return 2
    fi
  fi
}

bridge_called_from_bridge() {
  [ -f "$MANAGER_BRIDGE_PID_FILE" ] || return 1
  bridge_pid="$(cat "$MANAGER_BRIDGE_PID_FILE" 2>/dev/null || true)"
  [ -n "$bridge_pid" ] || return 1
  [ "$PPID" = "$bridge_pid" ] && return 0
  [ "$$" = "$bridge_pid" ] && return 0
  return 1
}

bridge_async_reexec() {
  async_sub="$1"
  async_service="$2"
  nohup env DSAPI_BRIDGE_ASYNC=1 /system/bin/sh "$MODDIR/dsapi_service_ctl.sh" bridge "$async_sub" "$async_service" \
    >>"$MANAGER_BRIDGE_LOG_FILE" 2>&1 &
  async_pid="$!"
  echo "ksu_dsapi_bridge=${async_sub}_queued pid=$async_pid service=$async_service"
}

bridge_cmd() {
  sub="${1:-status}"

  case "$sub" in
    start)
      bridge_service="${2:-$MANAGER_BRIDGE_DEFAULT_SERVICE}"
      if ! dsapi_manager_bridge_start "$bridge_service"; then
        echo "ksu_dsapi_error=bridge_start_failed service=$bridge_service log=$MANAGER_BRIDGE_LOG_FILE"
        return 2
      fi
      bridge_status="$(dsapi_manager_bridge_status_kv)"
      echo "ksu_dsapi_bridge=started $bridge_status"
      ;;
    stop)
      if bridge_called_from_bridge && [ "${DSAPI_BRIDGE_ASYNC:-0}" != "1" ]; then
        bridge_service="$(dsapi_manager_bridge_service_get)"
        bridge_async_reexec stop "$bridge_service"
        return 0
      fi
      dsapi_manager_bridge_stop
      echo "ksu_dsapi_bridge=stopped $(dsapi_manager_bridge_status_kv)"
      ;;
    restart)
      bridge_service="${2:-$MANAGER_BRIDGE_DEFAULT_SERVICE}"
      if bridge_called_from_bridge && [ "${DSAPI_BRIDGE_ASYNC:-0}" != "1" ]; then
        bridge_async_reexec restart "$bridge_service"
        return 0
      fi
      dsapi_manager_bridge_stop
      if ! dsapi_manager_bridge_start "$bridge_service"; then
        echo "ksu_dsapi_error=bridge_restart_failed service=$bridge_service log=$MANAGER_BRIDGE_LOG_FILE"
        return 2
      fi
      echo "ksu_dsapi_bridge=restarted $(dsapi_manager_bridge_status_kv)"
      ;;
    status)
      echo "ksu_dsapi_bridge $(dsapi_manager_bridge_status_kv)"
      ;;
    *)
      echo "usage: dsapi_service_ctl.sh bridge start [service]|stop|restart [service]|status"
      return 1
      ;;
  esac
}

zygote_cmd() {
  sub="${1:-status}"
  case "$sub" in
    policy-list)
      dsapi_zygote_scope_list_lines
      ;;
    policy-set)
      scope_pkg="${2:-}"
      scope_user="${3:-}"
      scope_policy="${4:-}"
      [ -n "$scope_pkg" ] || { echo "ksu_dsapi_error=scope_package_missing"; return 2; }
      [ -n "$scope_user" ] || { echo "ksu_dsapi_error=scope_user_missing"; return 2; }
      [ -n "$scope_policy" ] || { echo "ksu_dsapi_error=scope_policy_missing"; return 2; }
      dsapi_zygote_scope_set "$scope_pkg" "$scope_user" "$scope_policy" || {
        echo "ksu_dsapi_error=zygote_scope_set_failed package=$scope_pkg user=$scope_user policy=$scope_policy"
        return 2
      }
      echo "zygote_scope_action=set package=$scope_pkg user=$scope_user policy=$scope_policy"
      ;;
    policy-clear)
      scope_pkg="${2:-}"
      scope_user="${3:-}"
      if [ -n "$scope_pkg" ] || [ -n "$scope_user" ]; then
        [ -n "$scope_pkg" ] || { echo "ksu_dsapi_error=scope_package_missing"; return 2; }
        [ -n "$scope_user" ] || { echo "ksu_dsapi_error=scope_user_missing"; return 2; }
        dsapi_zygote_scope_clear "$scope_pkg" "$scope_user" || {
          echo "ksu_dsapi_error=zygote_scope_clear_failed package=$scope_pkg user=$scope_user"
          return 2
        }
        echo "zygote_scope_action=cleared package=$scope_pkg user=$scope_user"
      else
        dsapi_zygote_scope_clear
        echo "zygote_scope_action=cleared_all"
      fi
      ;;
    start)
      zygote_service="${2:-$(dsapi_zygote_agent_service_get)}"
      daemon_service="${3:-$(dsapi_zygote_agent_daemon_get)}"
      if ! dsapi_zygote_agent_start "$zygote_service" "$daemon_service"; then
        echo "ksu_dsapi_error=zygote_agent_start_failed service=$zygote_service daemon_service=$daemon_service log=$ZYGOTE_AGENT_LOG_FILE"
        return 2
      fi
      echo "ksu_dsapi_zygote $(dsapi_zygote_agent_status_kv)"
      ;;
    stop)
      dsapi_zygote_agent_stop
      echo "ksu_dsapi_zygote $(dsapi_zygote_agent_status_kv)"
      ;;
    restart)
      zygote_service="${2:-$(dsapi_zygote_agent_service_get)}"
      daemon_service="${3:-$(dsapi_zygote_agent_daemon_get)}"
      dsapi_zygote_agent_stop
      if ! dsapi_zygote_agent_start "$zygote_service" "$daemon_service"; then
        echo "ksu_dsapi_error=zygote_agent_restart_failed service=$zygote_service daemon_service=$daemon_service log=$ZYGOTE_AGENT_LOG_FILE"
        return 2
      fi
      echo "ksu_dsapi_zygote $(dsapi_zygote_agent_status_kv)"
      ;;
    status)
      echo "ksu_dsapi_zygote $(dsapi_zygote_agent_status_kv)"
      ;;
    *)
      echo "usage: dsapi_service_ctl.sh zygote policy-list|policy-set <package|*> <user|-1> <allow|deny>|policy-clear [package user]|start [service] [daemon_service]|stop|restart [service] [daemon_service]|status"
      return 1
      ;;
  esac
}

usage() {
  cat <<'USAGE'
usage:
  dsapi_service_ctl.sh [--v2] start|stop|status|cmd <COMMAND ...>
  dsapi_service_ctl.sh [--v2] capability list|start <id>|stop <id>|remove <id>|enable <id>|status <id>|detail <id>|add <file>
  dsapi_service_ctl.sh [--v2] runtime list|active|activate <id>|install <id> <dir>|remove <id>
  dsapi_service_ctl.sh [--v2] module list|install-zip <zip>|start <id>|stop <id>|reload <id>|reload-all|disable <id>|enable <id>|remove <id>|status <id>|detail <id>|action-list <id>|action-run <id> <action>|env-list <id>|env-set <id> <key> <value>|env-unset <id> <key>|scope-list [id]|scope-set <id> <package|*> <user|-1> <allow|deny>|scope-clear <id>
  dsapi_service_ctl.sh [--v2] errors last|clear
  dsapi_service_ctl.sh [--v2] bridge start [service]|stop|restart [service]|status
  dsapi_service_ctl.sh [--v2] zygote policy-list|policy-set <package|*> <user|-1> <allow|deny>|policy-clear [package user]|start [service] [daemon_service]|stop|restart [service] [daemon_service]|status
  dsapi_service_ctl.sh [--v2] ui install|start [refresh_ms]|stop|status|run [refresh_ms]
  dsapi_service_ctl.sh [--v2] demo aidl [service] [status|start [w h z layer run_seconds]|stop|cmd <...>|set-pos x y|set-view w h|set-color r g b [a]|set-speed v]
  dsapi_service_ctl.sh [--v2] demo start [width] [height] [z_layer] [layer_name] [run_seconds]|stop|status|run [width] [height] [z_layer] [layer_name] [run_seconds]
USAGE
}

cmd="${1:-status}"
if [ "$RESULT_V2" = "1" ]; then
  dsapi_result_v2_init
fi
case "$cmd" in
  start)
    dsapi_enabled_set 1
    if ! dsapi_cap_start_by_id core.daemon >/dev/null 2>&1; then
      dsapi_last_error_set "daemon.lifecycle" "daemon_start_failed" "daemon_start_failed" "capability=core.daemon"
      echo "ksu_dsapi_error=daemon_start_failed capability=core.daemon"
      exit 2
    fi
    # 同步显示状态（尺寸/旋转）到 dsapid，确保触摸/路由在横竖屏切换时自动更新。
    # 这里是强依赖：失败会记录 last_error，但不阻断核心启动（避免因 ROM 差异导致完全不可用）。
    if ! dsapi_cap_start_by_id core.display_watch >/dev/null 2>&1; then
      dsapi_last_error_set "daemon.lifecycle" "display_watch_start_failed" "display_watch_start_failed" "capability=core.display_watch"
      dsapi_log "ksu_dsapi_error=display_watch_start_failed capability=core.display_watch"
    fi
    if ! dsapi_manager_bridge_start >/dev/null 2>&1; then
      dsapi_last_error_set "daemon.lifecycle" "bridge_start_failed" "bridge_start_failed" "service=$MANAGER_BRIDGE_DEFAULT_SERVICE"
      echo "ksu_dsapi_error=bridge_start_failed service=$MANAGER_BRIDGE_DEFAULT_SERVICE log=$MANAGER_BRIDGE_LOG_FILE"
      exit 2
    fi
    dsapi_zygote_agent_start >/dev/null 2>&1 || true
    if ! dsapi_daemon_cmd MODULE_SYNC >/dev/null 2>&1; then
      dsapi_last_error_set "daemon.lifecycle" "module_sync_failed" "module_sync_failed" "op=start"
      echo "ksu_dsapi_error=module_sync_failed"
      exit 2
    fi
    dsapi_last_error_clear
    print_daemon_status_line
    ;;
  stop)
    dsapi_enabled_set 0
    dsapi_cap_stop_by_id core.display_watch >/dev/null 2>&1 || true
    if ! dsapi_cap_stop_by_id core.daemon >/dev/null 2>&1; then
      dsapi_last_error_set "daemon.lifecycle" "daemon_stop_failed" "daemon_stop_failed" "capability=core.daemon"
      echo "ksu_dsapi_error=daemon_stop_failed capability=core.daemon"
      exit 2
    fi
    # Binder bridge 是 Manager 控制平面，stop 核心时不能自杀，否则 UI 会立即掉线（DeadObject）。
    dsapi_last_error_clear
    print_daemon_status_line
    ;;
  status)
    print_daemon_status_line
    ui_status="$(ui_status_kv)"
    echo "ksu_dsapi_ui $ui_status"
    echo "ksu_dsapi_demo $(dsapi_demo_status_kv)"
    echo "ksu_dsapi_zygote $(dsapi_zygote_agent_status_kv)"
    zygote_scope_count="$(dsapi_zygote_scope_list_lines | wc -l | tr -d '[:space:]')"
    [ -n "$zygote_scope_count" ] || zygote_scope_count=0
    echo "zygote_scope_count=$zygote_scope_count"
    echo "ksu_dsapi_bridge $(dsapi_manager_bridge_status_kv)"
    daemon_ready="$(dsapi_daemon_cmd READY 2>/dev/null || true)"
    if [ -n "$daemon_ready" ]; then
      daemon_ready_state="$(dsapi_kv_get state "$daemon_ready")"
      daemon_ready_mod_count="$(dsapi_kv_get module_count "$daemon_ready")"
      daemon_ready_mod_seq="$(dsapi_kv_get module_event_seq "$daemon_ready")"
      daemon_ready_mod_err="$(dsapi_kv_get module_error "$daemon_ready")"
      [ -n "$daemon_ready_state" ] || daemon_ready_state="-"
      [ -n "$daemon_ready_mod_count" ] || daemon_ready_mod_count=0
      [ -n "$daemon_ready_mod_seq" ] || daemon_ready_mod_seq=0
      [ -n "$daemon_ready_mod_err" ] || daemon_ready_mod_err=0
      echo "daemon_ready_state=$daemon_ready_state"
      echo "daemon_module_registry_count=$daemon_ready_mod_count"
      echo "daemon_module_registry_event_seq=$daemon_ready_mod_seq"
      echo "daemon_module_registry_error=$daemon_ready_mod_err"
    fi
    echo "ksu_dsapi_last_error $(dsapi_last_error_status_kv)"
    echo "runtime_active=$(dsapi_runtime_active_release_id)"
    if [ -n "${daemon_ready_mod_count:-}" ]; then
      echo "module_count=$daemon_ready_mod_count"
    else
      echo "module_count=0"
    fi
    echo "enabled=$(dsapi_enabled_get)"
    ;;
  cmd)
    shift
    if [ "$#" -lt 1 ]; then
      echo "ksu_dsapi_error=missing_command"
      exit 2
    fi
    dsapi_daemon_cmd "$@"
    ;;
  capability)
    shift
    capability_cmd "$@"
    ;;
  runtime)
    shift
    runtime_cmd "$@"
    ;;
  module)
    shift
    module_cmd "$@"
    ;;
  errors)
    shift
    errors_cmd "$@"
    ;;
  bridge)
    shift
    bridge_cmd "$@"
    ;;
  zygote)
    shift
    zygote_cmd "$@"
    ;;
  ui)
    sub="${2:-status}"
    case "$sub" in
      start)
        ui_start "${3:-1200}"
        ;;
      install)
        ui_install
        ;;
      stop)
        ui_stop
        ;;
      status)
        echo "ksu_dsapi_ui $(ui_status_kv)"
        ;;
      run)
        ui_start "${3:-1200}"
        while true
        do
          ui_status="$(ui_status_kv)"
          ui_state="$(dsapi_kv_get state "$ui_status")"
          [ "$ui_state" = "running" ] || break
          sleep 1
        done
        ;;
      *)
        echo "usage: dsapi_service_ctl.sh ui install|start [refresh_ms]|stop|status|run [refresh_ms]"
        exit 1
        ;;
    esac
    ;;
  demo)
    shift
    demo_cmd "$@"
    ;;
  *)
    usage
    exit 1
    ;;
esac
