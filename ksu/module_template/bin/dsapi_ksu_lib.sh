#!/system/bin/sh

# DSAPI KSU shared library

if [ -z "${MODROOT:-}" ]; then
  if [ -n "${DSAPI_LIB_FILE:-}" ]; then
    MODROOT="${DSAPI_LIB_FILE%/*}"
    MODROOT="${MODROOT%/*}"
  elif [ -n "${0:-}" ]; then
    MODROOT="${0%/*}"
    MODROOT="${MODROOT%/*}"
  else
    MODROOT="/data/adb/modules/directscreenapi"
  fi
fi

BASE_DIR=/data/adb/dsapi
RUN_DIR="$BASE_DIR/run"
LOG_DIR="$BASE_DIR/log"
STATE_DIR="$BASE_DIR/state"
RUNTIME_DIR="$BASE_DIR/runtime"
RELEASES_DIR="$RUNTIME_DIR/releases"
CURRENT_LINK="$RUNTIME_DIR/current"
ACTIVE_RELEASE_FILE="$STATE_DIR/active_release"
ENABLED_FILE="$STATE_DIR/enabled"
CAP_CUSTOM_DIR="$BASE_DIR/capabilities/custom"
CAP_DISABLED_DIR="$STATE_DIR/capabilities_disabled"
MODULES_DIR="$BASE_DIR/modules"
MODULE_DISABLED_DIR="$STATE_DIR/modules_disabled"
MODULE_STATE_ROOT="$STATE_DIR/modules"
MODULE_IMPORT_DIR="$BASE_DIR/imports"
AGENT_RUN_DIR="$RUN_DIR/agents"
DAEMON_PID_FILE="$RUN_DIR/dsapid.pid"
DAEMON_SOCKET="$RUN_DIR/dsapi.sock"
DAEMON_LOG_FILE="$LOG_DIR/dsapid.log"
CAP_UI_PID_FILE="$RUN_DIR/cap_ui.pid"
CAP_UI_LOG_FILE="$LOG_DIR/cap_ui.log"
MANAGER_HOST_PID_FILE="$RUN_DIR/manager_host.pid"
MANAGER_HOST_READY_FILE="$RUN_DIR/manager_host.ready"
MANAGER_HOST_LOG_FILE="$LOG_DIR/manager_host.log"
MANAGER_BRIDGE_PID_FILE="$RUN_DIR/manager_bridge.pid"
MANAGER_BRIDGE_SERVICE_FILE="$RUN_DIR/manager_bridge.service"
MANAGER_BRIDGE_READY_FILE="$RUN_DIR/manager_bridge.ready"
MANAGER_BRIDGE_LOG_FILE="$LOG_DIR/manager_bridge.log"
MANAGER_BRIDGE_DEFAULT_SERVICE="dsapi.core"
ZYGOTE_AGENT_PID_FILE="$RUN_DIR/zygote_agent.pid"
ZYGOTE_AGENT_SERVICE_FILE="$RUN_DIR/zygote_agent.service"
ZYGOTE_AGENT_DAEMON_FILE="$RUN_DIR/zygote_agent.daemon_service"
ZYGOTE_AGENT_READY_FILE="$RUN_DIR/zygote_agent.ready"
ZYGOTE_AGENT_LOG_FILE="$LOG_DIR/zygote_agent.log"
ZYGOTE_AGENT_DEFAULT_SERVICE="dsapi.zygote.injector"
ZYGOTE_SCOPE_FILE="$STATE_DIR/zygote_scope.db"
LAST_ERROR_FILE="$STATE_DIR/last_error.kv"
MANAGER_PACKAGE_DEFAULT="org.directscreenapi.manager"
MANAGER_MAIN_ACTIVITY_DEFAULT=".MainActivity"
MANAGER_APK_FILE="$MODROOT/manager.apk"
MANAGER_APK_STAMP_FILE="$STATE_DIR/manager_apk.stamp"

dsapi_now() {
  date '+%Y-%m-%d %H:%M:%S'
}

dsapi_log() {
  mkdir -p "$LOG_DIR"
  printf '%s %s\n' "$(dsapi_now)" "$*" >>"$DAEMON_LOG_FILE"
}

dsapi_value_tokenize() {
  printf '%s' "${1:-}" | tr '\r\n\t |' '_____'
}

dsapi_file_kv_get() {
  dsapi_file="$1"
  dsapi_key="$2"
  [ -f "$dsapi_file" ] || { echo ""; return 0; }
  dsapi_line="$(grep -E "^${dsapi_key}=" "$dsapi_file" 2>/dev/null | tail -n 1 || true)"
  [ -n "$dsapi_line" ] || { echo ""; return 0; }
  printf '%s' "${dsapi_line#*=}" | tr -d '\r'
}

dsapi_last_error_clear() {
  rm -f "$LAST_ERROR_FILE"
}

dsapi_last_error_set() {
  dsapi_scope="${1:-generic}"
  dsapi_code="${2:-unknown}"
  dsapi_message_raw="${3:-unknown}"
  dsapi_detail_raw="${4:--}"
  dsapi_message="$(dsapi_value_tokenize "$dsapi_message_raw")"
  dsapi_detail="$(dsapi_value_tokenize "$dsapi_detail_raw")"
  dsapi_tmp="$RUN_DIR/last_error.$$"
  {
    printf 'ts=%s\n' "$(dsapi_now)"
    printf 'scope=%s\n' "$dsapi_scope"
    printf 'code=%s\n' "$dsapi_code"
    printf 'message=%s\n' "$dsapi_message"
    printf 'detail=%s\n' "$dsapi_detail"
  } >"$dsapi_tmp"
  mv "$dsapi_tmp" "$LAST_ERROR_FILE"
  dsapi_log "ksu_dsapi_error scope=$dsapi_scope code=$dsapi_code message=$dsapi_message detail=$dsapi_detail"
}

dsapi_last_error_status_kv() {
  if [ ! -f "$LAST_ERROR_FILE" ]; then
    echo "state=none"
    return 0
  fi
  dsapi_scope="$(dsapi_file_kv_get "$LAST_ERROR_FILE" scope)"
  dsapi_code="$(dsapi_file_kv_get "$LAST_ERROR_FILE" code)"
  dsapi_message="$(dsapi_file_kv_get "$LAST_ERROR_FILE" message)"
  dsapi_detail="$(dsapi_file_kv_get "$LAST_ERROR_FILE" detail)"
  dsapi_ts="$(dsapi_file_kv_get "$LAST_ERROR_FILE" ts)"
  [ -n "$dsapi_scope" ] || dsapi_scope="unknown"
  [ -n "$dsapi_code" ] || dsapi_code="unknown"
  [ -n "$dsapi_message" ] || dsapi_message="-"
  [ -n "$dsapi_detail" ] || dsapi_detail="-"
  [ -n "$dsapi_ts" ] || dsapi_ts="-"
  echo "state=present scope=$dsapi_scope code=$dsapi_code message=$dsapi_message detail=$dsapi_detail ts=$dsapi_ts"
}

dsapi_last_error_dump() {
  if [ ! -f "$LAST_ERROR_FILE" ]; then
    echo "last_error_state=none"
    return 0
  fi
  while IFS= read -r dsapi_line || [ -n "$dsapi_line" ]; do
    [ -n "$dsapi_line" ] || continue
    echo "last_error_$dsapi_line"
  done <"$LAST_ERROR_FILE"
}

dsapi_fix_module_perms() {
  dsapi_root="${1:-$MODROOT}"
  [ -d "$dsapi_root" ] || return 2

  for dsapi_file in \
    "$dsapi_root/action.sh" \
    "$dsapi_root/post-fs-data.sh" \
    "$dsapi_root/service.sh" \
    "$dsapi_root/uninstall.sh" \
    "$dsapi_root/bin/dsapi_service_ctl.sh" \
    "$dsapi_root/bin/dsapi_ksu_lib.sh" \
    "$dsapi_root/bin/dsapid" \
    "$dsapi_root/bin/dsapictl" \
    "$dsapi_root/bin/dsapi_touch_demo"
  do
    [ -f "$dsapi_file" ] || continue
    chmod 0755 "$dsapi_file" >/dev/null 2>&1 || return 2
  done

  if [ -f "$dsapi_root/manager.apk" ]; then
    chmod 0444 "$dsapi_root/manager.apk" >/dev/null 2>&1 || return 2
  fi
  return 0
}

dsapi_manager_package_name() {
  dsapi_pkg="${DSAPI_MANAGER_PACKAGE:-$MANAGER_PACKAGE_DEFAULT}"
  case "$dsapi_pkg" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) dsapi_pkg="$MANAGER_PACKAGE_DEFAULT" ;;
  esac
  echo "$dsapi_pkg"
}

dsapi_manager_main_activity() {
  dsapi_act="${DSAPI_MANAGER_MAIN_ACTIVITY:-$MANAGER_MAIN_ACTIVITY_DEFAULT}"
  case "$dsapi_act" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) dsapi_act="$MANAGER_MAIN_ACTIVITY_DEFAULT" ;;
  esac
  echo "$dsapi_act"
}

dsapi_manager_main_component() {
  dsapi_pkg="$(dsapi_manager_package_name)"
  dsapi_act="$(dsapi_manager_main_activity)"
  case "$dsapi_act" in
    */*) echo "$dsapi_act" ;;
    .*) echo "$dsapi_pkg/$dsapi_act" ;;
    *) echo "$dsapi_pkg/$dsapi_act" ;;
  esac
}

dsapi_manager_pkg_installed() {
  dsapi_pkg="${1:-$(dsapi_manager_package_name)}"
  if command -v pm >/dev/null 2>&1; then
    if pm path "$dsapi_pkg" >/dev/null 2>&1; then
      return 0
    fi
  fi
  if [ -x /system/bin/cmd ]; then
    if /system/bin/cmd package path "$dsapi_pkg" >/dev/null 2>&1; then
      return 0
    fi
  fi
  return 1
}

dsapi_file_fingerprint() {
  dsapi_file="$1"
  [ -f "$dsapi_file" ] || return 1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$dsapi_file" 2>/dev/null | awk '{print $1}' | tr -d '\r'
    return 0
  fi
  if command -v cksum >/dev/null 2>&1; then
    cksum "$dsapi_file" 2>/dev/null | awk '{print $1 ":" $2}' | tr -d '\r'
    return 0
  fi
  ls -ln "$dsapi_file" 2>/dev/null | awk '{print $5 ":" $6 ":" $7 ":" $8}' | tr -d '\r'
}

dsapi_manager_ensure_installed() {
  dsapi_pkg="${1:-$(dsapi_manager_package_name)}"
  dsapi_apk="${2:-$MANAGER_APK_FILE}"
  dsapi_installed=0
  if dsapi_manager_pkg_installed "$dsapi_pkg"; then
    dsapi_installed=1
  fi

  if [ ! -f "$dsapi_apk" ]; then
    if [ "$dsapi_installed" = "1" ]; then
      return 0
    fi
    dsapi_log "ksu_dsapi_error=manager_apk_missing_for_auto_install package=$dsapi_pkg path=$dsapi_apk"
    return 2
  fi

  dsapi_apk_fp="$(dsapi_file_fingerprint "$dsapi_apk" 2>/dev/null || true)"
  dsapi_prev_fp="$(cat "$MANAGER_APK_STAMP_FILE" 2>/dev/null | tr -d '\r' || true)"
  if [ "$dsapi_installed" = "1" ] && [ -n "$dsapi_apk_fp" ] && [ "$dsapi_apk_fp" = "$dsapi_prev_fp" ]; then
    return 0
  fi

  if command -v pm >/dev/null 2>&1; then
    if pm install -r --user 0 "$dsapi_apk" >/dev/null 2>&1; then
      :
    elif pm install -r "$dsapi_apk" >/dev/null 2>&1; then
      :
    else
      pm install-existing --user 0 "$dsapi_pkg" >/dev/null 2>&1 || true
    fi
  elif [ -x /system/bin/cmd ]; then
    /system/bin/cmd package install-existing --user 0 "$dsapi_pkg" >/dev/null 2>&1 || true
  else
    dsapi_log "ksu_dsapi_error=manager_install_tool_missing package=$dsapi_pkg"
    return 2
  fi

  if dsapi_manager_pkg_installed "$dsapi_pkg"; then
    if [ -n "$dsapi_apk_fp" ]; then
      echo "$dsapi_apk_fp" >"$MANAGER_APK_STAMP_FILE" 2>/dev/null || true
    fi
    return 0
  fi
  dsapi_log "ksu_dsapi_error=manager_auto_install_failed package=$dsapi_pkg path=$dsapi_apk"
  return 2
}

dsapi_is_running() {
  dsapi_pid="$1"
  [ -n "$dsapi_pid" ] || return 1
  kill -0 "$dsapi_pid" >/dev/null 2>&1
}

dsapi_stop_pid() {
  dsapi_pid="$1"
  if dsapi_is_running "$dsapi_pid"; then
    kill "$dsapi_pid" >/dev/null 2>&1 || true
    sleep 0.1
    kill -KILL "$dsapi_pid" >/dev/null 2>&1 || true
  fi
}

dsapi_stop_pidfile_process() {
  dsapi_pid_file="$1"
  dsapi_proc_name="$2"

  if [ -f "$dsapi_pid_file" ]; then
    dsapi_pid="$(cat "$dsapi_pid_file" 2>/dev/null || true)"
    dsapi_stop_pid "$dsapi_pid"
    rm -f "$dsapi_pid_file"
  fi

  if [ -n "$dsapi_proc_name" ] && command -v pidof >/dev/null 2>&1; then
    dsapi_pid_list="$(pidof "$dsapi_proc_name" 2>/dev/null || true)"
    for dsapi_pid in $dsapi_pid_list; do
      dsapi_stop_pid "$dsapi_pid"
    done
  fi
}

dsapi_init_layout() {
  mkdir -p \
    "$BASE_DIR" \
    "$RUN_DIR" \
    "$LOG_DIR" \
    "$STATE_DIR" \
    "$RUNTIME_DIR" \
    "$RELEASES_DIR" \
    "$CAP_CUSTOM_DIR" \
    "$CAP_DISABLED_DIR" \
    "$MODULES_DIR" \
    "$MODULE_DISABLED_DIR" \
    "$MODULE_STATE_ROOT" \
    "$MODULE_IMPORT_DIR" \
    "$AGENT_RUN_DIR" \
    "$BASE_DIR/render"
  chmod 0770 "$BASE_DIR" "$RUN_DIR" "$LOG_DIR" "$STATE_DIR" "$RUNTIME_DIR" "$RELEASES_DIR" 2>/dev/null || true
  chmod 0770 "$CAP_CUSTOM_DIR" "$CAP_DISABLED_DIR" "$MODULES_DIR" "$MODULE_DISABLED_DIR" "$MODULE_STATE_ROOT" "$MODULE_IMPORT_DIR" "$AGENT_RUN_DIR" 2>/dev/null || true
  if ! dsapi_fix_module_perms; then
    dsapi_log "ksu_dsapi_warn=module_perm_fix_failed root=$MODROOT"
  fi
  if [ ! -f "$ENABLED_FILE" ]; then
    echo 1 >"$ENABLED_FILE"
  fi
  chmod 0644 "$ENABLED_FILE" 2>/dev/null || true
  dsapi_migrate_legacy_layout
}

dsapi_migrate_legacy_layout() {
  legacy_modules_dir="$MODROOT/modules"
  if [ -d "$legacy_modules_dir" ]; then
    for legacy_mod in "$legacy_modules_dir"/*; do
      [ -d "$legacy_mod" ] || continue
      legacy_id="$(basename "$legacy_mod")"
      [ -n "$legacy_id" ] || continue
      if [ ! -d "$MODULES_DIR/$legacy_id" ]; then
        mv "$legacy_mod" "$MODULES_DIR/$legacy_id" >/dev/null 2>&1 || cp -a "$legacy_mod" "$MODULES_DIR/$legacy_id" >/dev/null 2>&1 || true
      fi
    done
    rmdir "$legacy_modules_dir" >/dev/null 2>&1 || true
  fi

  legacy_disabled_dir="$MODROOT/state/modules_disabled"
  if [ -d "$legacy_disabled_dir" ]; then
    for legacy_flag in "$legacy_disabled_dir"/*.disabled; do
      [ -f "$legacy_flag" ] || continue
      cp -f "$legacy_flag" "$MODULE_DISABLED_DIR/" >/dev/null 2>&1 || true
    done
  fi

  legacy_state_dir="$MODROOT/state/modules"
  if [ -d "$legacy_state_dir" ]; then
    for legacy_state in "$legacy_state_dir"/*; do
      [ -d "$legacy_state" ] || continue
      legacy_id="$(basename "$legacy_state")"
      [ -n "$legacy_id" ] || continue
      if [ ! -d "$MODULE_STATE_ROOT/$legacy_id" ]; then
        mv "$legacy_state" "$MODULE_STATE_ROOT/$legacy_id" >/dev/null 2>&1 || cp -a "$legacy_state" "$MODULE_STATE_ROOT/$legacy_id" >/dev/null 2>&1 || true
      fi
    done
  fi
}

dsapi_enabled_get() {
  if [ ! -f "$ENABLED_FILE" ]; then
    echo 1
    return 0
  fi
  cat "$ENABLED_FILE" 2>/dev/null || echo 1
}

dsapi_enabled_set() {
  echo "$1" >"$ENABLED_FILE"
}

dsapi_bool_is_true() {
  case "${1:-0}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

dsapi_runtime_validate_id() {
  echo "$1" | grep -Eq '^[A-Za-z0-9._-]+$'
}

dsapi_runtime_activate() {
  dsapi_rel="$1"
  if [ ! -d "$RELEASES_DIR/$dsapi_rel" ]; then
    return 2
  fi
  ln -sfn "$RELEASES_DIR/$dsapi_rel" "$CURRENT_LINK"
  echo "$dsapi_rel" >"$ACTIVE_RELEASE_FILE"
  return 0
}

dsapi_runtime_list_ids() {
  for dsapi_dir in "$RELEASES_DIR"/*; do
    [ -d "$dsapi_dir" ] || continue
    basename "$dsapi_dir"
  done | sort
}

dsapi_runtime_active_release_id() {
  if [ -L "$CURRENT_LINK" ] && command -v readlink >/dev/null 2>&1; then
    dsapi_link="$(readlink "$CURRENT_LINK" 2>/dev/null || true)"
    dsapi_id="${dsapi_link##*/}"
    if [ -n "$dsapi_id" ] && [ -d "$RELEASES_DIR/$dsapi_id" ]; then
      echo "$dsapi_id"
      return 0
    fi
  fi

  if [ -f "$ACTIVE_RELEASE_FILE" ]; then
    dsapi_id="$(cat "$ACTIVE_RELEASE_FILE" 2>/dev/null || true)"
    if [ -n "$dsapi_id" ] && [ -d "$RELEASES_DIR/$dsapi_id" ]; then
      echo "$dsapi_id"
      return 0
    fi
  fi

  dsapi_runtime_list_ids | head -n 1
}

dsapi_runtime_active_dir() {
  if [ -d "$CURRENT_LINK" ]; then
    echo "$CURRENT_LINK"
    return 0
  fi

  dsapi_id="$(dsapi_runtime_active_release_id)"
  if [ -n "$dsapi_id" ] && [ -d "$RELEASES_DIR/$dsapi_id" ]; then
    ln -sfn "$RELEASES_DIR/$dsapi_id" "$CURRENT_LINK"
    echo "$CURRENT_LINK"
    return 0
  fi

  echo ""
}

dsapi_runtime_seed_fingerprint() {
  dsapi_seed_root="$1"
  [ -d "$dsapi_seed_root" ] || {
    echo "none"
    return 0
  }

  dsapi_fp_tmp="$RUN_DIR/runtime_seed_fingerprint.$$"
  : >"$dsapi_fp_tmp"
  for dsapi_src in "$dsapi_seed_root"/*; do
    [ -d "$dsapi_src" ] || continue
    find "$dsapi_src" -type f 2>/dev/null | sort | while IFS= read -r dsapi_file
    do
      [ -f "$dsapi_file" ] || continue
      dsapi_rel="${dsapi_file#$dsapi_seed_root/}"
      if command -v cksum >/dev/null 2>&1; then
        dsapi_ck="$(cksum "$dsapi_file" 2>/dev/null | awk 'NR==1{print $1 ":" $2}')"
      else
        dsapi_size="$(wc -c <"$dsapi_file" 2>/dev/null | tr -d '[:space:]')"
        [ -n "$dsapi_size" ] || dsapi_size=0
        dsapi_ck="size:$dsapi_size"
      fi
      [ -n "$dsapi_ck" ] || dsapi_ck="0:0"
      printf '%s=%s\n' "$dsapi_rel" "$dsapi_ck"
    done >>"$dsapi_fp_tmp"
  done

  if command -v cksum >/dev/null 2>&1; then
    dsapi_fp="$(cksum "$dsapi_fp_tmp" 2>/dev/null | awk 'NR==1{print $1 ":" $2}')"
  else
    dsapi_fp="$(wc -c <"$dsapi_fp_tmp" 2>/dev/null | tr -d '[:space:]')"
    [ -n "$dsapi_fp" ] || dsapi_fp=0
    dsapi_fp="size:$dsapi_fp"
  fi
  rm -f "$dsapi_fp_tmp"
  [ -n "$dsapi_fp" ] || dsapi_fp="none"
  echo "$dsapi_fp"
}

dsapi_runtime_seed_sync() {
  dsapi_seed_root="$MODROOT/runtime_seed/releases"
  [ -d "$dsapi_seed_root" ] || return 0

  dsapi_lock_dir="$RUN_DIR/runtime_seed_sync.lock"
  dsapi_lock_try=0
  while ! mkdir "$dsapi_lock_dir" >/dev/null 2>&1; do
    dsapi_lock_try=$((dsapi_lock_try + 1))
    if [ "$dsapi_lock_try" -ge 80 ]; then
      dsapi_log "ksu_dsapi_error=runtime_seed_sync_lock_timeout lock=$dsapi_lock_dir"
      return 2
    fi
    sleep 0.05
  done
  trap 'rmdir "$dsapi_lock_dir" >/dev/null 2>&1 || true' EXIT INT TERM

  dsapi_latest=""
  dsapi_seed_count=0
  for dsapi_src in "$dsapi_seed_root"/*; do
    [ -d "$dsapi_src" ] || continue
    dsapi_id="$(basename "$dsapi_src")"
    dsapi_seed_count=$((dsapi_seed_count + 1))
    if [ -z "$dsapi_latest" ] || [ "$dsapi_id" \> "$dsapi_latest" ]; then
      dsapi_latest="$dsapi_id"
    fi
  done

  dsapi_marker_file="$STATE_DIR/runtime_seed_sync.marker"
  # 仅基于 release 目录元信息做增量判断，避免每次 action 都遍历大文件做 cksum。
  dsapi_mod_vc="$(dsapi_file_kv_get "$MODROOT/module.prop" versionCode)"
  [ -n "$dsapi_mod_vc" ] || dsapi_mod_vc="-"
  dsapi_marker_want="$MODROOT|$dsapi_latest|$dsapi_seed_count|$dsapi_mod_vc"
  dsapi_marker_cur="$(cat "$dsapi_marker_file" 2>/dev/null || true)"
  dsapi_active_now="$(dsapi_runtime_active_release_id)"
  if [ -n "$dsapi_latest" ] \
    && [ "$dsapi_marker_cur" = "$dsapi_marker_want" ] \
    && [ -d "$RELEASES_DIR/$dsapi_latest" ] \
    && [ "$dsapi_active_now" = "$dsapi_latest" ]; then
    trap - EXIT INT TERM
    rmdir "$dsapi_lock_dir" >/dev/null 2>&1 || true
    return 0
  fi

  for dsapi_src in "$dsapi_seed_root"/*; do
    [ -d "$dsapi_src" ] || continue
    dsapi_id="$(basename "$dsapi_src")"
    dsapi_dst="$RELEASES_DIR/$dsapi_id"
    dsapi_tmp="$RELEASES_DIR/.seed_sync_${dsapi_id}_$$"
    dsapi_old="$RELEASES_DIR/.seed_prev_${dsapi_id}_$$"
    rm -rf "$dsapi_tmp" "$dsapi_old"
    mkdir -p "$dsapi_tmp" || return 2
    if ! cp -a "$dsapi_src/." "$dsapi_tmp/"; then
      rm -rf "$dsapi_tmp"
      return 2
    fi
    chmod 0755 "$dsapi_tmp/bin/dsapid" "$dsapi_tmp/bin/dsapictl" 2>/dev/null || true
    chmod 0755 "$dsapi_tmp/bin/dsapi_touch_demo" 2>/dev/null || true
    chmod 0755 "$dsapi_tmp/capabilities/"*.sh 2>/dev/null || true

    if [ -d "$dsapi_dst" ]; then
      if ! mv "$dsapi_dst" "$dsapi_old"; then
        rm -rf "$dsapi_tmp" "$dsapi_old"
        trap - EXIT INT TERM
        rmdir "$dsapi_lock_dir" >/dev/null 2>&1 || true
        return 2
      fi
    fi

    if ! mv "$dsapi_tmp" "$dsapi_dst"; then
      rm -rf "$dsapi_tmp"
      if [ -d "$dsapi_old" ]; then
        mv "$dsapi_old" "$dsapi_dst" >/dev/null 2>&1 || true
      fi
      trap - EXIT INT TERM
      rmdir "$dsapi_lock_dir" >/dev/null 2>&1 || true
      return 2
    fi
    rm -rf "$dsapi_old"
  done

  dsapi_active="$(dsapi_runtime_active_release_id)"
  if [ -n "$dsapi_latest" ] && [ "$dsapi_active" != "$dsapi_latest" ]; then
    dsapi_runtime_activate "$dsapi_latest" >/dev/null 2>&1 || true
  fi
  if [ -n "$dsapi_latest" ]; then
    printf '%s\n' "$dsapi_marker_want" >"$dsapi_marker_file"
  fi

  trap - EXIT INT TERM
  rmdir "$dsapi_lock_dir" >/dev/null 2>&1 || true
}

dsapi_active_bin() {
  dsapi_name="$1"
  dsapi_dir="$(dsapi_runtime_active_dir)"
  if [ -n "$dsapi_dir" ] && [ -x "$dsapi_dir/bin/$dsapi_name" ]; then
    echo "$dsapi_dir/bin/$dsapi_name"
    return 0
  fi
  if [ -x "$MODROOT/bin/$dsapi_name" ]; then
    echo "$MODROOT/bin/$dsapi_name"
    return 0
  fi
  echo ""
}

dsapi_active_android_dex() {
  dsapi_dir="$(dsapi_runtime_active_dir)"
  if [ -n "$dsapi_dir" ] && [ -f "$dsapi_dir/android/directscreen-adapter-dex.jar" ]; then
    echo "$dsapi_dir/android/directscreen-adapter-dex.jar"
    return 0
  fi
  if [ -f "$MODROOT/android/directscreen-adapter-dex.jar" ]; then
    echo "$MODROOT/android/directscreen-adapter-dex.jar"
    return 0
  fi
  echo ""
}

dsapi_find_app_process() {
  for dsapi_bin in /system/bin/app_process64 /system/bin/app_process /system/bin/app_process32
  do
    [ -x "$dsapi_bin" ] && echo "$dsapi_bin" && return 0
  done
  if command -v app_process >/dev/null 2>&1; then
    command -v app_process
    return 0
  fi
  return 1
}

dsapi_bridge_exec_ctl() {
  dsapi_service="${1:-$(dsapi_manager_bridge_service_get)}"
  shift || true
  case "$dsapi_service" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) dsapi_service="$(dsapi_manager_bridge_service_get)" ;;
  esac

  dsapi_adapter_dex="$(dsapi_active_android_dex)"
  dsapi_app_process="$(dsapi_find_app_process 2>/dev/null || true)"
  dsapi_main_class="org.directscreenapi.adapter.AndroidAdapterMain"
  if [ -z "$dsapi_adapter_dex" ] || [ ! -f "$dsapi_adapter_dex" ]; then
    dsapi_log "ksu_dsapi_error=bridge_exec_adapter_dex_missing path=$dsapi_adapter_dex"
    return 2
  fi
  if [ -z "$dsapi_app_process" ] || [ ! -x "$dsapi_app_process" ]; then
    dsapi_log "ksu_dsapi_error=bridge_exec_app_process_missing"
    return 2
  fi

  CLASSPATH="$dsapi_adapter_dex" "$dsapi_app_process" /system/bin "$dsapi_main_class" bridge-exec "$dsapi_service" "$@"
}

dsapi_demo_status_via_bridge_kv() {
  dsapi_bridge_status="$(dsapi_manager_bridge_status_kv)"
  dsapi_bridge_state="$(dsapi_kv_get state "$dsapi_bridge_status")"
  dsapi_bridge_service="$(dsapi_kv_get service "$dsapi_bridge_status")"
  [ -n "$dsapi_bridge_service" ] || dsapi_bridge_service="$MANAGER_BRIDGE_DEFAULT_SERVICE"
  if [ "$dsapi_bridge_state" != "running" ] && [ "$dsapi_bridge_state" != "starting" ]; then
    return 1
  fi

  dsapi_reply="$(dsapi_bridge_exec_ctl "$dsapi_bridge_service" demo status 2>/dev/null || true)"
  dsapi_line="$(printf '%s\n' "$dsapi_reply" | grep '^ksu_dsapi_demo ' | tail -n 1 || true)"
  [ -n "$dsapi_line" ] || return 1
  echo "${dsapi_line#ksu_dsapi_demo }"
}

dsapi_demo_status_kv() {
  dsapi_bridge_demo="$(dsapi_demo_status_via_bridge_kv 2>/dev/null || true)"
  if [ -n "$dsapi_bridge_demo" ]; then
    echo "$dsapi_bridge_demo"
    return 0
  fi
  echo "state=stopped pid=- mode=gpu_demo"
}

dsapi_daemon_start() {
  dsapi_runtime_bin=""
  dsapi_active_dir="$(dsapi_runtime_active_dir)"
  if [ -n "$dsapi_active_dir" ] && [ -x "$dsapi_active_dir/bin/dsapid" ]; then
    dsapi_runtime_bin="$dsapi_active_dir/bin/dsapid"
  fi

  dsapi_module_bin=""
  if [ -x "$MODROOT/bin/dsapid" ]; then
    dsapi_module_bin="$MODROOT/bin/dsapid"
  fi

  dsapi_candidates=""
  if [ -n "$dsapi_runtime_bin" ]; then
    dsapi_candidates="$dsapi_runtime_bin"
  fi
  if [ -n "$dsapi_module_bin" ] && [ "$dsapi_module_bin" != "$dsapi_runtime_bin" ]; then
    dsapi_candidates="$dsapi_candidates $dsapi_module_bin"
  fi
  if [ -z "$dsapi_candidates" ]; then
    dsapi_log "ksu_dsapi_error=dsapid_missing"
    return 2
  fi

  if [ -f "$DAEMON_PID_FILE" ]; then
    dsapi_old_pid="$(cat "$DAEMON_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_old_pid"; then
      return 0
    fi
  fi

  rm -f "$DAEMON_SOCKET" "$RUN_DIR/dsapi.data.sock"

  dsapi_last_err=""
  for dsapi_bin in $dsapi_candidates; do
    nohup "$dsapi_bin" \
      --control-socket "$DAEMON_SOCKET" \
      --data-socket "$DAEMON_SOCKET" \
      --unified-socket 1 \
      --render-output-dir "$BASE_DIR/render" \
      --module-root-dir "$MODULES_DIR" \
      --module-state-root-dir "$MODULE_STATE_ROOT" \
      --module-disabled-dir "$MODULE_DISABLED_DIR" \
      --module-registry-file "$STATE_DIR/module_registry.db" \
      --module-scope-file "$STATE_DIR/module_scope.db" \
      --module-action-timeout-sec "${DSAPI_MODULE_ACTION_TIMEOUT_SEC:-60}" \
      >>"$DAEMON_LOG_FILE" 2>&1 &
    dsapi_pid="$!"
    sleep 0.12
    if dsapi_is_running "$dsapi_pid"; then
      echo "$dsapi_pid" >"$DAEMON_PID_FILE"
      return 0
    fi
    # 启动失败，避免残留脏 pidfile。
    dsapi_stop_pid "$dsapi_pid" >/dev/null 2>&1 || true
    dsapi_last_err="bin_failed=$dsapi_bin"
  done

  rm -f "$DAEMON_PID_FILE"
  dsapi_log "ksu_dsapi_error=daemon_start_failed attempted=$(dsapi_value_tokenize "$dsapi_candidates") detail=${dsapi_last_err:-unknown}"
  return 2
}

dsapi_daemon_stop() {
  if [ -f "$DAEMON_PID_FILE" ]; then
    dsapi_pid="$(cat "$DAEMON_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      kill "$dsapi_pid" >/dev/null 2>&1 || true
      sleep 0.2
      kill -KILL "$dsapi_pid" >/dev/null 2>&1 || true
    fi
    rm -f "$DAEMON_PID_FILE"
  fi
  rm -f "$DAEMON_SOCKET" "$RUN_DIR/dsapi.data.sock"
}

dsapi_manager_bridge_stop() {
  rm -f "$MANAGER_BRIDGE_READY_FILE"
  dsapi_stop_pidfile_process "$MANAGER_BRIDGE_PID_FILE" "DSAPIBridge"
  rm -f "$MANAGER_BRIDGE_SERVICE_FILE"
}

dsapi_manager_bridge_service_get() {
  # 仅允许 dsapi.* 命名空间，避免误用系统 service（例如 assetatlas）导致 UI/模块互相不一致。
  if [ -f "$MANAGER_BRIDGE_SERVICE_FILE" ]; then
    dsapi_service="$(head -n 1 "$MANAGER_BRIDGE_SERVICE_FILE" 2>/dev/null | tr -d '\r' || true)"
    case "$dsapi_service" in
      dsapi.*)
        echo "$dsapi_service"
        return 0
        ;;
    esac
  fi
  echo "$MANAGER_BRIDGE_DEFAULT_SERVICE"
}

dsapi_manager_bridge_ready_kv() {
  [ -f "$MANAGER_BRIDGE_READY_FILE" ] || return 1
  dsapi_line="$(head -n 1 "$MANAGER_BRIDGE_READY_FILE" 2>/dev/null | tr -d '\r')"
  [ -n "$dsapi_line" ] || return 1
  echo "$dsapi_line"
}

dsapi_manager_bridge_status_kv() {
  dsapi_service="$(dsapi_manager_bridge_service_get)"
  if [ -f "$MANAGER_BRIDGE_PID_FILE" ]; then
    dsapi_pid="$(cat "$MANAGER_BRIDGE_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      dsapi_ready="$(dsapi_manager_bridge_ready_kv 2>/dev/null || true)"
      dsapi_ready_state="$(dsapi_kv_get state "$dsapi_ready")"
      dsapi_ready_pid="$(dsapi_kv_get pid "$dsapi_ready")"
      dsapi_ready_service="$(dsapi_kv_get service "$dsapi_ready")"
      if [ "$dsapi_ready_state" = "ready" ] \
        && [ "$dsapi_ready_pid" = "$dsapi_pid" ] \
        && [ "$dsapi_ready_service" = "$dsapi_service" ]; then
        echo "state=running pid=$dsapi_pid service=$dsapi_service"
        return 0
      fi
      echo "state=starting pid=$dsapi_pid reason=ready_not_confirmed service=$dsapi_service"
      return 0
    fi
    rm -f "$MANAGER_BRIDGE_PID_FILE" "$MANAGER_BRIDGE_READY_FILE"
    echo "state=stopped pid=- reason=stale_pid_cleaned service=$dsapi_service"
    return 0
  fi
  echo "state=stopped pid=- service=$dsapi_service"
}

dsapi_manager_bridge_is_name_usable() {
  dsapi_service="$1"
  dsapi_service_bin="/system/bin/service"
  if [ ! -x "$dsapi_service_bin" ]; then
    return 0
  fi
  dsapi_line=""
  while IFS= read -r dsapi_row; do
    dsapi_name="$(printf '%s\n' "$dsapi_row" | sed -n 's/^[0-9][0-9]*[[:space:]]\+\([^:]*\):.*/\1/p')"
    [ -n "$dsapi_name" ] || continue
    [ "$dsapi_name" = "$dsapi_service" ] || continue
    dsapi_line="$(printf '%s\n' "$dsapi_row" | sed -n 's/^[0-9][0-9]*[[:space:]]\+[^:]*:[[:space:]]*//p')"
    break
  done <<EOF
$("$dsapi_service_bin" list 2>/dev/null)
EOF
  if [ -z "$dsapi_line" ]; then
    return 0
  fi
  case "$dsapi_line" in
    *org.directscreenapi.core.ICoreService*) return 0 ;;
    *) return 1 ;;
  esac
}

dsapi_manager_bridge_start() {
  dsapi_service="${1:-$MANAGER_BRIDGE_DEFAULT_SERVICE}"
  case "$dsapi_service" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) return 2 ;;
  esac
  if ! dsapi_manager_bridge_is_name_usable "$dsapi_service"; then
    dsapi_log "ksu_dsapi_error=manager_bridge_service_conflict service=$dsapi_service expected=org.directscreenapi.core.ICoreService"
    return 2
  fi

  dsapi_ctl="$MODROOT/bin/dsapi_service_ctl.sh"
  dsapi_adapter_dex="$(dsapi_active_android_dex)"
  dsapi_app_process="$(dsapi_find_app_process 2>/dev/null || true)"
  dsapi_main_class="org.directscreenapi.adapter.AndroidAdapterMain"

  if [ ! -f "$dsapi_ctl" ]; then
    dsapi_log "ksu_dsapi_error=manager_bridge_ctl_missing path=$dsapi_ctl"
    return 2
  fi
  if [ -z "$dsapi_adapter_dex" ] || [ ! -f "$dsapi_adapter_dex" ]; then
    dsapi_log "ksu_dsapi_error=manager_bridge_adapter_dex_missing path=$dsapi_adapter_dex"
    return 2
  fi
  if [ -z "$dsapi_app_process" ] || [ ! -x "$dsapi_app_process" ]; then
    dsapi_log "ksu_dsapi_error=manager_bridge_app_process_missing"
    return 2
  fi

  dsapi_status="$(dsapi_manager_bridge_status_kv)"
  dsapi_state="$(dsapi_kv_get state "$dsapi_status")"
  dsapi_cur_service="$(dsapi_kv_get service "$dsapi_status")"
  if [ "$dsapi_state" = "running" ] && [ "$dsapi_cur_service" = "$dsapi_service" ]; then
    return 0
  fi
  dsapi_manager_bridge_stop >/dev/null 2>&1 || true

  echo "$dsapi_service" >"$MANAGER_BRIDGE_SERVICE_FILE"
  rm -f "$MANAGER_BRIDGE_READY_FILE"
  CLASSPATH="$dsapi_adapter_dex" nohup "$dsapi_app_process" /system/bin --nice-name=DSAPIBridge "$dsapi_main_class" \
    bridge-server "$dsapi_ctl" "$dsapi_service" "$MANAGER_BRIDGE_READY_FILE" >>"$MANAGER_BRIDGE_LOG_FILE" 2>&1 &
  dsapi_pid="$!"
  echo "$dsapi_pid" >"$MANAGER_BRIDGE_PID_FILE"
  dsapi_wait=50
  while [ "$dsapi_wait" -gt 0 ]; do
    if ! dsapi_is_running "$dsapi_pid"; then
      rm -f "$MANAGER_BRIDGE_PID_FILE" "$MANAGER_BRIDGE_READY_FILE"
      return 2
    fi
    dsapi_ready="$(dsapi_manager_bridge_ready_kv 2>/dev/null || true)"
    dsapi_ready_state="$(dsapi_kv_get state "$dsapi_ready")"
    dsapi_ready_pid="$(dsapi_kv_get pid "$dsapi_ready")"
    dsapi_ready_service="$(dsapi_kv_get service "$dsapi_ready")"
    if [ "$dsapi_ready_state" = "ready" ] \
      && [ "$dsapi_ready_pid" = "$dsapi_pid" ] \
      && [ "$dsapi_ready_service" = "$dsapi_service" ]; then
      return 0
    fi
    dsapi_wait=$((dsapi_wait - 1))
    sleep 0.05
  done

  kill "$dsapi_pid" >/dev/null 2>&1 || true
  sleep 0.1
  kill -KILL "$dsapi_pid" >/dev/null 2>&1 || true
  rm -f "$MANAGER_BRIDGE_PID_FILE" "$MANAGER_BRIDGE_READY_FILE" "$MANAGER_BRIDGE_SERVICE_FILE"
  dsapi_log "ksu_dsapi_error=manager_bridge_ready_timeout pid=$dsapi_pid service=$dsapi_service"
  return 2
}

dsapi_manager_focus_line() {
  dumpsys window 2>/dev/null | grep -m 1 'mCurrentFocus=' | tr -d '\r' || true
}

dsapi_manager_is_foreground() {
  dsapi_pkg="${1:-$(dsapi_manager_package_name)}"
  case "$dsapi_pkg" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) return 1 ;;
  esac
  dsapi_focus="$(dsapi_manager_focus_line)"
  case "$dsapi_focus" in
    *"$dsapi_pkg/"*) return 0 ;;
    *) return 1 ;;
  esac
}

dsapi_manager_force_foreground() {
  dsapi_comp="${1:-$(dsapi_manager_main_component)}"
  case "$dsapi_comp" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) return 2 ;;
  esac

  if [ -x /system/bin/cmd ]; then
    /system/bin/cmd activity start-activity \
      --user 0 \
      --windowingMode 1 \
      -W \
      -a org.directscreenapi.manager.OPEN \
      -n "$dsapi_comp" \
      -f 0x14000000 >/dev/null 2>&1 && return 0
  fi
  if command -v am >/dev/null 2>&1; then
    am start \
      --user 0 \
      --windowingMode 1 \
      -W \
      -a org.directscreenapi.manager.OPEN \
      -n "$dsapi_comp" \
      -f 0x14000000 >/dev/null 2>&1 && return 0
  fi
  return 2
}

dsapi_manager_host_stop() {
  rm -f "$MANAGER_HOST_READY_FILE"
  dsapi_stop_pidfile_process "$MANAGER_HOST_PID_FILE" "DSAPIManagerHost"
  rm -f "$CAP_UI_PID_FILE"
}

dsapi_manager_host_status_kv() {
  dsapi_bridge_service="$(dsapi_manager_bridge_service_get)"
  dsapi_pkg="$(dsapi_manager_package_name)"
  if dsapi_manager_is_foreground "$dsapi_pkg"; then
    dsapi_visible="1"
  else
    dsapi_visible="0"
  fi
  if [ -f "$MANAGER_HOST_PID_FILE" ]; then
    dsapi_pid="$(cat "$MANAGER_HOST_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      dsapi_ready="$(head -n 1 "$MANAGER_HOST_READY_FILE" 2>/dev/null | tr -d '\r' || true)"
      dsapi_ready_state="$(dsapi_kv_get state "$dsapi_ready")"
      dsapi_ready_pid="$(dsapi_kv_get pid "$dsapi_ready")"
      dsapi_ready_bridge="$(dsapi_kv_get bridge_service "$dsapi_ready")"
      [ -n "$dsapi_ready_bridge" ] || dsapi_ready_bridge="$dsapi_bridge_service"
      if [ "$dsapi_ready_state" = "ready" ] && [ "$dsapi_ready_pid" = "$dsapi_pid" ]; then
        echo "state=running pid=$dsapi_pid mode=parasitic_host bridge_service=$dsapi_ready_bridge visible=$dsapi_visible"
        return 0
      fi
      echo "state=starting pid=$dsapi_pid mode=parasitic_host bridge_service=$dsapi_ready_bridge visible=$dsapi_visible"
      return 0
    fi
    rm -f "$MANAGER_HOST_PID_FILE" "$MANAGER_HOST_READY_FILE"
    echo "state=stopped pid=- mode=parasitic_host reason=stale_pid_cleaned bridge_service=$dsapi_bridge_service visible=$dsapi_visible"
    return 0
  fi
  echo "state=stopped pid=- mode=parasitic_host bridge_service=$dsapi_bridge_service visible=$dsapi_visible"
}

dsapi_manager_host_start() {
  dsapi_refresh_ms="${1:-1200}"
  dsapi_bridge_service="${2:-$(dsapi_manager_bridge_service_get)}"
  case "$dsapi_refresh_ms" in
    ''|*[!0-9]*) return 2 ;;
  esac
  case "$dsapi_bridge_service" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) return 2 ;;
  esac

  dsapi_ctl="$MODROOT/bin/dsapi_service_ctl.sh"
  dsapi_adapter_dex="$(dsapi_active_android_dex)"
  dsapi_app_process="$(dsapi_find_app_process 2>/dev/null || true)"
  dsapi_main_class="org.directscreenapi.adapter.AndroidAdapterMain"
  dsapi_pkg="$(dsapi_manager_package_name)"
  dsapi_comp="$(dsapi_manager_main_component)"

  if [ ! -f "$dsapi_ctl" ]; then
    dsapi_log "ksu_dsapi_error=manager_host_ctl_missing path=$dsapi_ctl"
    return 2
  fi
  if [ -z "$dsapi_adapter_dex" ] || [ ! -f "$dsapi_adapter_dex" ]; then
    dsapi_log "ksu_dsapi_error=manager_host_adapter_dex_missing path=$dsapi_adapter_dex"
    return 2
  fi
  if [ -z "$dsapi_app_process" ] || [ ! -x "$dsapi_app_process" ]; then
    dsapi_log "ksu_dsapi_error=manager_host_app_process_missing"
    return 2
  fi
  if ! dsapi_manager_ensure_installed "$dsapi_pkg" "$MANAGER_APK_FILE"; then
    dsapi_log "ksu_dsapi_error=manager_host_auto_install_failed package=$dsapi_pkg path=$MANAGER_APK_FILE"
    return 2
  fi
  if ! dsapi_manager_bridge_start "$dsapi_bridge_service" >/dev/null 2>&1; then
    dsapi_log "ksu_dsapi_error=manager_host_bridge_start_failed service=$dsapi_bridge_service"
    return 2
  fi

  dsapi_status="$(dsapi_manager_host_status_kv)"
  dsapi_state="$(dsapi_kv_get state "$dsapi_status")"
  dsapi_cur_bridge="$(dsapi_kv_get bridge_service "$dsapi_status")"
  [ -n "$dsapi_cur_bridge" ] || dsapi_cur_bridge="$dsapi_bridge_service"
  if [ "$dsapi_state" = "running" ] && [ "$dsapi_cur_bridge" = "$dsapi_bridge_service" ]; then
    if ! dsapi_manager_is_foreground "$dsapi_pkg"; then
      dsapi_manager_force_foreground "$dsapi_comp" >/dev/null 2>&1 || true
    fi
    if [ -f "$MANAGER_HOST_PID_FILE" ]; then
      dsapi_pid="$(cat "$MANAGER_HOST_PID_FILE" 2>/dev/null || true)"
      [ -n "$dsapi_pid" ] && echo "$dsapi_pid" >"$CAP_UI_PID_FILE"
    fi
    return 0
  fi

  if [ "$dsapi_state" = "running" ] || [ "$dsapi_state" = "starting" ]; then
    dsapi_manager_host_stop >/dev/null 2>&1 || true
  fi
  rm -f "$MANAGER_HOST_READY_FILE"
  CLASSPATH="$dsapi_adapter_dex" nohup "$dsapi_app_process" /system/bin --nice-name=DSAPIManagerHost "$dsapi_main_class" \
    manager-host "$dsapi_ctl" "$dsapi_comp" "$dsapi_pkg" "$dsapi_bridge_service" "$dsapi_refresh_ms" "$MANAGER_HOST_READY_FILE" \
    >>"$MANAGER_HOST_LOG_FILE" 2>&1 &
  dsapi_pid="$!"
  echo "$dsapi_pid" >"$MANAGER_HOST_PID_FILE"
  echo "$dsapi_pid" >"$CAP_UI_PID_FILE"

  dsapi_wait=60
  while [ "$dsapi_wait" -gt 0 ]; do
    if ! dsapi_is_running "$dsapi_pid"; then
      rm -f "$MANAGER_HOST_PID_FILE" "$MANAGER_HOST_READY_FILE" "$CAP_UI_PID_FILE"
      return 2
    fi
    dsapi_ready="$(head -n 1 "$MANAGER_HOST_READY_FILE" 2>/dev/null | tr -d '\r' || true)"
    dsapi_ready_state="$(dsapi_kv_get state "$dsapi_ready")"
    dsapi_ready_pid="$(dsapi_kv_get pid "$dsapi_ready")"
    if [ "$dsapi_ready_state" = "ready" ] && [ "$dsapi_ready_pid" = "$dsapi_pid" ]; then
      return 0
    fi
    dsapi_wait=$((dsapi_wait - 1))
    sleep 0.05
  done

  kill "$dsapi_pid" >/dev/null 2>&1 || true
  sleep 0.1
  kill -KILL "$dsapi_pid" >/dev/null 2>&1 || true
  rm -f "$MANAGER_HOST_PID_FILE" "$MANAGER_HOST_READY_FILE" "$CAP_UI_PID_FILE"
  dsapi_log "ksu_dsapi_error=manager_host_ready_timeout pid=$dsapi_pid bridge_service=$dsapi_bridge_service"
  return 2
}

dsapi_zygote_agent_service_get() {
  if [ -f "$ZYGOTE_AGENT_SERVICE_FILE" ]; then
    dsapi_service="$(cat "$ZYGOTE_AGENT_SERVICE_FILE" 2>/dev/null || true)"
    [ -n "$dsapi_service" ] && {
      echo "$dsapi_service"
      return 0
    }
  fi
  echo "$ZYGOTE_AGENT_DEFAULT_SERVICE"
}

dsapi_zygote_agent_daemon_get() {
  if [ -f "$ZYGOTE_AGENT_DAEMON_FILE" ]; then
    dsapi_service="$(head -n 1 "$ZYGOTE_AGENT_DAEMON_FILE" 2>/dev/null | tr -d '\r' || true)"
    case "$dsapi_service" in
      dsapi.*)
        echo "$dsapi_service"
        return 0
        ;;
    esac
  fi
  dsapi_manager_bridge_service_get
}

dsapi_zygote_agent_stop() {
  rm -f "$ZYGOTE_AGENT_READY_FILE"
  dsapi_stop_pidfile_process "$ZYGOTE_AGENT_PID_FILE" "DSAPIZygoteAgent"
  rm -f "$ZYGOTE_AGENT_SERVICE_FILE" "$ZYGOTE_AGENT_DAEMON_FILE"
}

dsapi_zygote_agent_status_kv() {
  dsapi_service="$(dsapi_zygote_agent_service_get)"
  dsapi_daemon_service="$(dsapi_zygote_agent_daemon_get)"
  if [ -f "$ZYGOTE_AGENT_PID_FILE" ]; then
    dsapi_pid="$(cat "$ZYGOTE_AGENT_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      dsapi_ready="$(head -n 1 "$ZYGOTE_AGENT_READY_FILE" 2>/dev/null | tr -d '\r' || true)"
      dsapi_ready_state="$(dsapi_kv_get state "$dsapi_ready")"
      dsapi_ready_pid="$(dsapi_kv_get pid "$dsapi_ready")"
      if [ "$dsapi_ready_state" = "ready" ] && [ "$dsapi_ready_pid" = "$dsapi_pid" ]; then
        echo "state=running pid=$dsapi_pid service=$dsapi_service daemon_service=$dsapi_daemon_service"
        return 0
      fi
      echo "state=starting pid=$dsapi_pid service=$dsapi_service daemon_service=$dsapi_daemon_service"
      return 0
    fi
    rm -f "$ZYGOTE_AGENT_PID_FILE" "$ZYGOTE_AGENT_READY_FILE"
    echo "state=stopped pid=- reason=stale_pid_cleaned service=$dsapi_service daemon_service=$dsapi_daemon_service"
    return 0
  fi
  echo "state=stopped pid=- service=$dsapi_service daemon_service=$dsapi_daemon_service"
}

dsapi_zygote_agent_start() {
  dsapi_service="${1:-$(dsapi_zygote_agent_service_get)}"
  dsapi_daemon_service="${2:-$(dsapi_zygote_agent_daemon_get)}"
  case "$dsapi_service" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) return 2 ;;
  esac
  case "$dsapi_daemon_service" in
    ''|*' '*|*$'\t'*|*$'\r'*|*$'\n'*) return 2 ;;
  esac

  dsapi_adapter_dex="$(dsapi_active_android_dex)"
  dsapi_app_process="$(dsapi_find_app_process 2>/dev/null || true)"
  dsapi_main_class="org.directscreenapi.adapter.AndroidAdapterMain"
  if [ -z "$dsapi_adapter_dex" ] || [ ! -f "$dsapi_adapter_dex" ]; then
    dsapi_log "ksu_dsapi_error=zygote_agent_adapter_dex_missing path=$dsapi_adapter_dex"
    return 2
  fi
  if [ -z "$dsapi_app_process" ] || [ ! -x "$dsapi_app_process" ]; then
    dsapi_log "ksu_dsapi_error=zygote_agent_app_process_missing"
    return 2
  fi

  dsapi_status="$(dsapi_zygote_agent_status_kv)"
  dsapi_state="$(dsapi_kv_get state "$dsapi_status")"
  dsapi_cur_service="$(dsapi_kv_get service "$dsapi_status")"
  dsapi_cur_daemon="$(dsapi_kv_get daemon_service "$dsapi_status")"
  if [ "$dsapi_state" = "running" ] \
    && [ "$dsapi_cur_service" = "$dsapi_service" ] \
    && [ "$dsapi_cur_daemon" = "$dsapi_daemon_service" ]; then
    return 0
  fi

  dsapi_zygote_agent_stop >/dev/null 2>&1 || true
  rm -f "$ZYGOTE_AGENT_READY_FILE"
  echo "$dsapi_service" >"$ZYGOTE_AGENT_SERVICE_FILE"
  echo "$dsapi_daemon_service" >"$ZYGOTE_AGENT_DAEMON_FILE"
  CLASSPATH="$dsapi_adapter_dex" nohup "$dsapi_app_process" /system/bin --nice-name=DSAPIZygoteAgent "$dsapi_main_class" \
    zygote-agent "$dsapi_service" "$dsapi_daemon_service" "$ZYGOTE_AGENT_READY_FILE" \
    >>"$ZYGOTE_AGENT_LOG_FILE" 2>&1 &
  dsapi_pid="$!"
  echo "$dsapi_pid" >"$ZYGOTE_AGENT_PID_FILE"

  dsapi_wait=60
  while [ "$dsapi_wait" -gt 0 ]; do
    if ! dsapi_is_running "$dsapi_pid"; then
      rm -f "$ZYGOTE_AGENT_PID_FILE" "$ZYGOTE_AGENT_READY_FILE"
      return 2
    fi
    dsapi_ready="$(head -n 1 "$ZYGOTE_AGENT_READY_FILE" 2>/dev/null | tr -d '\r' || true)"
    dsapi_ready_state="$(dsapi_kv_get state "$dsapi_ready")"
    dsapi_ready_pid="$(dsapi_kv_get pid "$dsapi_ready")"
    dsapi_ready_service="$(dsapi_kv_get zygote_service "$dsapi_ready")"
    dsapi_ready_daemon="$(dsapi_kv_get daemon_service "$dsapi_ready")"
    if [ "$dsapi_ready_state" = "ready" ] \
      && [ "$dsapi_ready_pid" = "$dsapi_pid" ] \
      && [ "$dsapi_ready_service" = "$dsapi_service" ] \
      && [ "$dsapi_ready_daemon" = "$dsapi_daemon_service" ]; then
      return 0
    fi
    dsapi_wait=$((dsapi_wait - 1))
    sleep 0.05
  done

  kill "$dsapi_pid" >/dev/null 2>&1 || true
  sleep 0.1
  kill -KILL "$dsapi_pid" >/dev/null 2>&1 || true
  rm -f "$ZYGOTE_AGENT_PID_FILE" "$ZYGOTE_AGENT_READY_FILE"
  dsapi_log "ksu_dsapi_error=zygote_agent_ready_timeout pid=$dsapi_pid service=$dsapi_service daemon_service=$dsapi_daemon_service"
  return 2
}

dsapi_zygote_scope_valid_package() {
  dsapi_pkg="${1:-}"
  [ -n "$dsapi_pkg" ] || return 1
  [ "$dsapi_pkg" = "*" ] && return 0
  echo "$dsapi_pkg" | grep -Eq '^[A-Za-z0-9._-]+$'
}

dsapi_zygote_scope_valid_user() {
  dsapi_uid="${1:-}"
  echo "$dsapi_uid" | grep -Eq '^-?[0-9]+$'
}

dsapi_zygote_scope_list_lines() {
  [ -f "$ZYGOTE_SCOPE_FILE" ] || return 0
  while IFS= read -r dsapi_line || [ -n "$dsapi_line" ]; do
    [ -n "$dsapi_line" ] || continue
    case "$dsapi_line" in
      \#*) continue ;;
    esac
    dsapi_kind="$(printf '%s' "$dsapi_line" | cut -d'|' -f1)"
    dsapi_pkg="$(printf '%s' "$dsapi_line" | cut -d'|' -f2)"
    dsapi_user="$(printf '%s' "$dsapi_line" | cut -d'|' -f3)"
    dsapi_policy="$(printf '%s' "$dsapi_line" | cut -d'|' -f4)"
    [ "$dsapi_kind" = "scope" ] || continue
    [ -n "$dsapi_pkg" ] || continue
    [ -n "$dsapi_user" ] || continue
    [ -n "$dsapi_policy" ] || continue
    printf 'zygote_scope_row=%s|%s|%s\n' "$dsapi_pkg" "$dsapi_user" "$dsapi_policy"
  done <"$ZYGOTE_SCOPE_FILE"
}

dsapi_zygote_scope_set() {
  dsapi_pkg="$1"
  dsapi_user="$2"
  dsapi_policy="$3"
  dsapi_zygote_scope_valid_package "$dsapi_pkg" || return 2
  dsapi_zygote_scope_valid_user "$dsapi_user" || return 2
  case "$dsapi_policy" in
    allow|deny) ;;
    *) return 2 ;;
  esac

  dsapi_tmp="$RUN_DIR/zygote_scope.$$"
  mkdir -p "$STATE_DIR"
  : >"$dsapi_tmp"
  dsapi_updated=0
  if [ -f "$ZYGOTE_SCOPE_FILE" ]; then
    while IFS= read -r dsapi_line || [ -n "$dsapi_line" ]; do
      [ -n "$dsapi_line" ] || continue
      dsapi_kind="$(printf '%s' "$dsapi_line" | cut -d'|' -f1)"
      dsapi_cur_pkg="$(printf '%s' "$dsapi_line" | cut -d'|' -f2)"
      dsapi_cur_user="$(printf '%s' "$dsapi_line" | cut -d'|' -f3)"
      dsapi_cur_policy="$(printf '%s' "$dsapi_line" | cut -d'|' -f4)"
      [ "$dsapi_kind" = "scope" ] || continue
      if [ "$dsapi_cur_pkg" = "$dsapi_pkg" ] && [ "$dsapi_cur_user" = "$dsapi_user" ]; then
        printf 'scope|%s|%s|%s\n' "$dsapi_pkg" "$dsapi_user" "$dsapi_policy" >>"$dsapi_tmp"
        dsapi_updated=1
      else
        printf 'scope|%s|%s|%s\n' "$dsapi_cur_pkg" "$dsapi_cur_user" "$dsapi_cur_policy" >>"$dsapi_tmp"
      fi
    done <"$ZYGOTE_SCOPE_FILE"
  fi
  if [ "$dsapi_updated" != "1" ]; then
    printf 'scope|%s|%s|%s\n' "$dsapi_pkg" "$dsapi_user" "$dsapi_policy" >>"$dsapi_tmp"
  fi
  sort -u "$dsapi_tmp" >"$dsapi_tmp.sorted"
  mv "$dsapi_tmp.sorted" "$ZYGOTE_SCOPE_FILE"
  rm -f "$dsapi_tmp"
  chmod 0644 "$ZYGOTE_SCOPE_FILE" >/dev/null 2>&1 || true
}

dsapi_zygote_scope_clear() {
  dsapi_pkg="${1:-}"
  dsapi_user="${2:-}"
  if [ -z "$dsapi_pkg" ] || [ -z "$dsapi_user" ]; then
    rm -f "$ZYGOTE_SCOPE_FILE"
    return 0
  fi
  dsapi_zygote_scope_valid_package "$dsapi_pkg" || return 2
  dsapi_zygote_scope_valid_user "$dsapi_user" || return 2
  [ -f "$ZYGOTE_SCOPE_FILE" ] || return 0

  dsapi_tmp="$RUN_DIR/zygote_scope.$$"
  : >"$dsapi_tmp"
  while IFS= read -r dsapi_line || [ -n "$dsapi_line" ]; do
    [ -n "$dsapi_line" ] || continue
    dsapi_kind="$(printf '%s' "$dsapi_line" | cut -d'|' -f1)"
    dsapi_cur_pkg="$(printf '%s' "$dsapi_line" | cut -d'|' -f2)"
    dsapi_cur_user="$(printf '%s' "$dsapi_line" | cut -d'|' -f3)"
    dsapi_cur_policy="$(printf '%s' "$dsapi_line" | cut -d'|' -f4)"
    [ "$dsapi_kind" = "scope" ] || continue
    if [ "$dsapi_cur_pkg" = "$dsapi_pkg" ] && [ "$dsapi_cur_user" = "$dsapi_user" ]; then
      continue
    fi
    printf 'scope|%s|%s|%s\n' "$dsapi_cur_pkg" "$dsapi_cur_user" "$dsapi_cur_policy" >>"$dsapi_tmp"
  done <"$ZYGOTE_SCOPE_FILE"

  if [ ! -s "$dsapi_tmp" ]; then
    rm -f "$dsapi_tmp" "$ZYGOTE_SCOPE_FILE"
    return 0
  fi
  sort -u "$dsapi_tmp" >"$dsapi_tmp.sorted"
  mv "$dsapi_tmp.sorted" "$ZYGOTE_SCOPE_FILE"
  rm -f "$dsapi_tmp"
  chmod 0644 "$ZYGOTE_SCOPE_FILE" >/dev/null 2>&1 || true
}

dsapi_meta_get() {
  dsapi_file="$1"
  dsapi_key="$2"
  [ -f "$dsapi_file" ] || { echo ""; return 0; }
  dsapi_line="$(grep -E "^${dsapi_key}=" "$dsapi_file" 2>/dev/null | head -n 1 || true)"
  [ -n "$dsapi_line" ] || { echo ""; return 0; }
  dsapi_val="${dsapi_line#*=}"
  printf '%s' "$(printf '%s' "$dsapi_val" | tr -d '\r')"
}

dsapi_module_validate_id() {
  dsapi_runtime_validate_id "$1"
}

dsapi_module_dir() {
  echo "$MODULES_DIR/$1"
}

dsapi_module_state_dir() {
  echo "$MODULE_STATE_ROOT/$1"
}

dsapi_module_meta_file() {
  echo "$(dsapi_module_dir "$1")/dsapi.module"
}

dsapi_module_env_spec_file() {
  echo "$(dsapi_module_dir "$1")/env.spec"
}

dsapi_module_env_values_file() {
  echo "$(dsapi_module_dir "$1")/env.values"
}

dsapi_module_actions_dir() {
  echo "$(dsapi_module_dir "$1")/actions"
}

dsapi_module_capabilities_dir() {
  echo "$(dsapi_module_dir "$1")/capabilities"
}

dsapi_module_disabled() {
  [ -f "$MODULE_DISABLED_DIR/$1.disabled" ]
}

dsapi_module_exists() {
  [ -d "$(dsapi_module_dir "$1")" ]
}

dsapi_module_entries() {
  for dsapi_dir in "$MODULES_DIR"/*; do
    [ -d "$dsapi_dir" ] || continue
    dsapi_id="$(basename "$dsapi_dir")"
    echo "$dsapi_id|$dsapi_dir"
  done | sort
}

dsapi_module_conf_get() {
  dsapi_id="$1"
  dsapi_key="$2"
  dsapi_meta="$(dsapi_module_meta_file "$dsapi_id")"
  dsapi_val="$(dsapi_meta_get "$dsapi_meta" "$dsapi_key")"
  if [ -n "$dsapi_val" ]; then
    echo "$dsapi_val"
    return 0
  fi
  case "$dsapi_key" in
    MODULE_ID) dsapi_meta_get "$dsapi_meta" DSAPI_MODULE_ID ;;
    MODULE_NAME) dsapi_meta_get "$dsapi_meta" DSAPI_MODULE_NAME ;;
    MODULE_KIND) dsapi_meta_get "$dsapi_meta" DSAPI_MODULE_KIND ;;
    MODULE_VERSION) dsapi_meta_get "$dsapi_meta" DSAPI_MODULE_VERSION ;;
    MODULE_DESC) dsapi_meta_get "$dsapi_meta" DSAPI_MODULE_DESC ;;
    MAIN_CAP_ID) dsapi_meta_get "$dsapi_meta" DSAPI_MAIN_CAP_ID ;;
    *) echo "" ;;
  esac
}

dsapi_module_name_by_id() {
  dsapi_id="$1"
  dsapi_name="$(dsapi_module_conf_get "$dsapi_id" MODULE_NAME)"
  [ -n "$dsapi_name" ] || dsapi_name="$dsapi_id"
  echo "$dsapi_name"
}

dsapi_module_kind_by_id() {
  dsapi_id="$1"
  dsapi_kind="$(dsapi_module_conf_get "$dsapi_id" MODULE_KIND)"
  [ -n "$dsapi_kind" ] || dsapi_kind="module"
  echo "$dsapi_kind"
}

dsapi_module_version_by_id() {
  dsapi_id="$1"
  dsapi_version="$(dsapi_module_conf_get "$dsapi_id" MODULE_VERSION)"
  [ -n "$dsapi_version" ] || dsapi_version="0"
  echo "$dsapi_version"
}

dsapi_module_desc_by_id() {
  dsapi_id="$1"
  dsapi_desc="$(dsapi_module_conf_get "$dsapi_id" MODULE_DESC)"
  [ -n "$dsapi_desc" ] || dsapi_desc="-"
  echo "$dsapi_desc"
}

dsapi_module_main_cap_by_id() {
  dsapi_id="$1"
  dsapi_cap="$(dsapi_module_conf_get "$dsapi_id" MAIN_CAP_ID)"
  if [ -n "$dsapi_cap" ]; then
    echo "$dsapi_cap"
    return 0
  fi
  dsapi_cap_dir="$(dsapi_module_capabilities_dir "$dsapi_id")"
  for dsapi_file in "$dsapi_cap_dir"/*.sh; do
    [ -f "$dsapi_file" ] || continue
    dsapi_cap="$(dsapi_cap_id "$dsapi_file")"
    if [ -n "$dsapi_cap" ]; then
      echo "$dsapi_cap"
      return 0
    fi
  done
  echo ""
}

dsapi_module_auto_start_enabled() {
  dsapi_id="$1"
  dsapi_auto="$(dsapi_module_conf_get "$dsapi_id" MODULE_AUTO_START)"
  if [ -z "$dsapi_auto" ]; then
    dsapi_auto="$(dsapi_module_conf_get "$dsapi_id" DSAPI_MODULE_AUTO_START)"
  fi
  if dsapi_bool_is_true "$dsapi_auto"; then
    return 0
  fi
  return 1
}

dsapi_module_env_key_valid() {
  echo "$1" | grep -Eq '^[A-Z][A-Z0-9_]*$'
}

dsapi_module_env_value_get() {
  dsapi_id="$1"
  dsapi_key="$2"
  dsapi_file="$(dsapi_module_env_values_file "$dsapi_id")"
  [ -f "$dsapi_file" ] || { echo ""; return 0; }
  dsapi_line="$(grep -E "^${dsapi_key}=" "$dsapi_file" 2>/dev/null | tail -n 1 || true)"
  [ -n "$dsapi_line" ] || { echo ""; return 0; }
  dsapi_val="${dsapi_line#*=}"
  printf '%s' "$(printf '%s' "$dsapi_val" | tr -d '\r')"
}

dsapi_module_env_list_lines() {
  dsapi_id="$1"
  dsapi_spec="$(dsapi_module_env_spec_file "$dsapi_id")"
  [ -f "$dsapi_spec" ] || return 0
  while IFS='|' read -r dsapi_key dsapi_default dsapi_type dsapi_label dsapi_desc || [ -n "${dsapi_key:-}" ]; do
    [ -n "${dsapi_key:-}" ] || continue
    case "$dsapi_key" in
      \#*) continue ;;
    esac
    dsapi_module_env_key_valid "$dsapi_key" || continue
    [ -n "$dsapi_type" ] || dsapi_type="text"
    [ -n "$dsapi_label" ] || dsapi_label="$dsapi_key"
    [ -n "$dsapi_desc" ] || dsapi_desc="-"
    dsapi_value="$(dsapi_module_env_value_get "$dsapi_id" "$dsapi_key")"
    [ -n "$dsapi_value" ] || dsapi_value="$dsapi_default"
    printf '%s|%s|%s|%s|%s|%s\n' \
      "$dsapi_key" "$dsapi_value" "$dsapi_default" "$dsapi_type" "$dsapi_label" "$dsapi_desc"
  done <"$dsapi_spec"
}

dsapi_module_env_set() {
  dsapi_id="$1"
  dsapi_key="$2"
  dsapi_value="$3"
  dsapi_module_env_key_valid "$dsapi_key" || return 2
  dsapi_file="$(dsapi_module_env_values_file "$dsapi_id")"
  dsapi_tmp="$RUN_DIR/module.env.set.$$"
  dsapi_found=0
  : >"$dsapi_tmp"
  if [ -f "$dsapi_file" ]; then
    while IFS= read -r dsapi_line || [ -n "$dsapi_line" ]; do
      case "$dsapi_line" in
        "$dsapi_key"=*)
          printf '%s=%s\n' "$dsapi_key" "$dsapi_value" >>"$dsapi_tmp"
          dsapi_found=1
          ;;
        *)
          printf '%s\n' "$dsapi_line" >>"$dsapi_tmp"
          ;;
      esac
    done <"$dsapi_file"
  fi
  if [ "$dsapi_found" = "0" ]; then
    printf '%s=%s\n' "$dsapi_key" "$dsapi_value" >>"$dsapi_tmp"
  fi
  mv "$dsapi_tmp" "$dsapi_file"
}

dsapi_module_env_unset() {
  dsapi_id="$1"
  dsapi_key="$2"
  dsapi_module_env_key_valid "$dsapi_key" || return 2
  dsapi_file="$(dsapi_module_env_values_file "$dsapi_id")"
  [ -f "$dsapi_file" ] || return 0
  dsapi_tmp="$RUN_DIR/module.env.unset.$$"
  : >"$dsapi_tmp"
  while IFS= read -r dsapi_line || [ -n "$dsapi_line" ]; do
    case "$dsapi_line" in
      "$dsapi_key"=*) ;;
      *) printf '%s\n' "$dsapi_line" >>"$dsapi_tmp" ;;
    esac
  done <"$dsapi_file"
  mv "$dsapi_tmp" "$dsapi_file"
}

dsapi_module_env_export() {
  dsapi_id="$1"
  dsapi_lines="$RUN_DIR/module.env.lines.$$"
  dsapi_module_env_list_lines "$dsapi_id" >"$dsapi_lines"
  while IFS='|' read -r dsapi_key dsapi_value _rest || [ -n "${dsapi_key:-}" ]; do
    [ -n "$dsapi_key" ] || continue
    dsapi_module_env_key_valid "$dsapi_key" || continue
    export "$dsapi_key=$dsapi_value"
  done <"$dsapi_lines"
  rm -f "$dsapi_lines"
}

dsapi_module_id_from_cap_file() {
  dsapi_file="$1"
  case "$dsapi_file" in
    "$MODULES_DIR"/*/capabilities/*)
      dsapi_tmp="${dsapi_file#$MODULES_DIR/}"
      echo "${dsapi_tmp%%/*}"
      ;;
    *)
      echo ""
      ;;
  esac
}

dsapi_module_action_entries_by_id() {
  dsapi_id="$1"
  dsapi_actions_dir="$(dsapi_module_actions_dir "$dsapi_id")"
  [ -d "$dsapi_actions_dir" ] || return 0
  for dsapi_file in "$dsapi_actions_dir"/*.sh; do
    [ -f "$dsapi_file" ] || continue
    dsapi_action_id="$(basename "$dsapi_file" .sh)"
    dsapi_action_name="$(dsapi_meta_get "$dsapi_file" ACTION_NAME)"
    [ -n "$dsapi_action_name" ] || dsapi_action_name="$dsapi_action_id"
    dsapi_action_danger="$(dsapi_meta_get "$dsapi_file" ACTION_DANGER)"
    [ -n "$dsapi_action_danger" ] || dsapi_action_danger=0
    printf '%s|%s|%s|%s\n' "$dsapi_action_id" "$dsapi_action_name" "$dsapi_action_danger" "$dsapi_file"
  done
}

dsapi_module_action_file_by_id() {
  dsapi_id="$1"
  dsapi_action="$2"
  dsapi_actions_dir="$(dsapi_module_actions_dir "$dsapi_id")"
  dsapi_file="$dsapi_actions_dir/$dsapi_action.sh"
  [ -f "$dsapi_file" ] || return 2
  echo "$dsapi_file"
}

dsapi_module_action_count_by_id() {
  dsapi_id="$1"
  dsapi_count=0
  dsapi_actions_dir="$(dsapi_module_actions_dir "$dsapi_id")"
  for dsapi_file in "$dsapi_actions_dir"/*.sh; do
    [ -f "$dsapi_file" ] || continue
    dsapi_count=$((dsapi_count + 1))
  done
  echo "$dsapi_count"
}

dsapi_module_action_run_by_id() {
  dsapi_id="$1"
  dsapi_action="$2"
  if ! dsapi_module_exists "$dsapi_id"; then
    dsapi_last_error_set "module.action" "module_missing" \
      "module_not_found" "id=$dsapi_id action=$dsapi_action"
    return 2
  fi
  dsapi_file="$(dsapi_module_action_file_by_id "$dsapi_id" "$dsapi_action")" || {
    dsapi_last_error_set "module.action" "action_missing" \
      "action_not_found" "id=$dsapi_id action=$dsapi_action"
    return 3
  }
  dsapi_state_dir="$(dsapi_module_state_dir "$dsapi_id")"
  dsapi_action_timeout="${DSAPI_MODULE_ACTION_TIMEOUT_SEC:-60}"
  dsapi_has_timeout=0
  case "$dsapi_action_timeout" in
    ''|*[!0-9]*) dsapi_action_timeout=60 ;;
  esac
  if [ "$dsapi_action_timeout" -lt 1 ]; then
    dsapi_action_timeout=1
  fi
  if command -v timeout >/dev/null 2>&1; then
    dsapi_has_timeout=1
  fi
  mkdir -p "$dsapi_state_dir"
  dsapi_output_file="$RUN_DIR/module.action.out.$$"
  if (
    dsapi_cap_set_env
    export DSAPI_MODULE_ID="$dsapi_id"
    export DSAPI_MODULE_DIR="$(dsapi_module_dir "$dsapi_id")"
    export DSAPI_MODULE_STATE_DIR="$dsapi_state_dir"
    export DSAPI_MODULE_ACTION_ID="$dsapi_action"
    export DSAPI_MODULE_ACTION_TIMEOUT_SEC="$dsapi_action_timeout"
    dsapi_module_env_export "$dsapi_id"
    if [ "$dsapi_has_timeout" = "1" ]; then
      timeout "$dsapi_action_timeout" /system/bin/sh "$dsapi_file"
    else
      /system/bin/sh "$dsapi_file"
    fi
  ) >"$dsapi_output_file" 2>&1; then
    dsapi_rc=0
  else
    dsapi_rc=$?
  fi
  if [ -f "$dsapi_output_file" ]; then
    cat "$dsapi_output_file"
    rm -f "$dsapi_output_file"
  fi
  if [ "$dsapi_has_timeout" != "1" ]; then
    dsapi_log "ksu_dsapi_warn=timeout_cmd_missing module=$dsapi_id action=$dsapi_action"
  fi
  if [ "$dsapi_rc" -ne 0 ]; then
    if [ "$dsapi_has_timeout" = "1" ] && [ "$dsapi_rc" -eq 124 ]; then
      dsapi_last_error_set "module.action" "timeout" \
        "module_action_timeout" "id=$dsapi_id action=$dsapi_action timeout_sec=$dsapi_action_timeout"
      echo "ksu_dsapi_error=module_action_timeout id=$dsapi_id action=$dsapi_action timeout_sec=$dsapi_action_timeout"
    else
      dsapi_last_error_set "module.action" "failed" \
        "module_action_failed" "id=$dsapi_id action=$dsapi_action exit=$dsapi_rc"
      echo "ksu_dsapi_error=module_action_failed id=$dsapi_id action=$dsapi_action exit=$dsapi_rc"
    fi
    return "$dsapi_rc"
  fi
  dsapi_last_error_clear
  return 0
}

dsapi_module_start_by_id() {
  dsapi_id="$1"
  if ! dsapi_module_exists "$dsapi_id"; then
    dsapi_last_error_set "module.lifecycle" "module_missing" "module_not_found" "id=$dsapi_id op=start"
    return 2
  fi
  if dsapi_module_disabled "$dsapi_id"; then
    dsapi_last_error_set "module.lifecycle" "module_disabled" "module_disabled" "id=$dsapi_id op=start"
    return 3
  fi
  if dsapi_module_action_file_by_id "$dsapi_id" start >/dev/null 2>&1; then
    dsapi_module_action_run_by_id "$dsapi_id" start
    return $?
  fi
  dsapi_main_cap="$(dsapi_module_main_cap_by_id "$dsapi_id")"
  if [ -z "$dsapi_main_cap" ]; then
    dsapi_last_error_set "module.lifecycle" "main_cap_missing" "main_cap_missing" "id=$dsapi_id op=start"
    return 4
  fi
  if ! dsapi_cap_start_by_id "$dsapi_main_cap"; then
    dsapi_last_error_set "module.lifecycle" "cap_start_failed" "cap_start_failed" \
      "id=$dsapi_id cap=$dsapi_main_cap op=start"
    return 5
  fi
  dsapi_last_error_clear
  return 0
}

dsapi_module_stop_by_id() {
  dsapi_id="$1"
  if ! dsapi_module_exists "$dsapi_id"; then
    dsapi_last_error_set "module.lifecycle" "module_missing" "module_not_found" "id=$dsapi_id op=stop"
    return 2
  fi
  if dsapi_module_action_file_by_id "$dsapi_id" stop >/dev/null 2>&1; then
    dsapi_module_action_run_by_id "$dsapi_id" stop
    return $?
  fi
  dsapi_main_cap="$(dsapi_module_main_cap_by_id "$dsapi_id")"
  if [ -z "$dsapi_main_cap" ]; then
    dsapi_last_error_set "module.lifecycle" "main_cap_missing" "main_cap_missing" "id=$dsapi_id op=stop"
    return 4
  fi
  if ! dsapi_cap_stop_by_id "$dsapi_main_cap"; then
    dsapi_last_error_set "module.lifecycle" "cap_stop_failed" "cap_stop_failed" \
      "id=$dsapi_id cap=$dsapi_main_cap op=stop"
    return 5
  fi
  dsapi_last_error_clear
  return 0
}

dsapi_module_reload_by_id() {
  dsapi_id="$1"
  if ! dsapi_module_exists "$dsapi_id"; then
    dsapi_last_error_set "module.lifecycle" "module_missing" "module_not_found" "id=$dsapi_id op=reload"
    return 2
  fi
  if dsapi_module_disabled "$dsapi_id"; then
    dsapi_last_error_set "module.lifecycle" "module_disabled" "module_disabled" "id=$dsapi_id op=reload"
    return 3
  fi
  dsapi_module_stop_by_id "$dsapi_id" >/dev/null 2>&1 || true
  if ! dsapi_module_start_by_id "$dsapi_id"; then
    dsapi_last_error_set "module.lifecycle" "reload_failed" "module_reload_failed" "id=$dsapi_id op=reload"
    return 5
  fi
  dsapi_last_error_clear
  return 0
}

dsapi_module_disable_by_id() {
  dsapi_id="$1"
  dsapi_module_exists "$dsapi_id" || return 2
  dsapi_module_stop_by_id "$dsapi_id" >/dev/null 2>&1 || true
  touch "$MODULE_DISABLED_DIR/$dsapi_id.disabled"
}

dsapi_module_enable_by_id() {
  dsapi_id="$1"
  dsapi_module_exists "$dsapi_id" || return 2
  rm -f "$MODULE_DISABLED_DIR/$dsapi_id.disabled"
}

dsapi_module_remove_by_id() {
  dsapi_id="$1"
  dsapi_module_exists "$dsapi_id" || return 2
  dsapi_module_stop_by_id "$dsapi_id" >/dev/null 2>&1 || true
  rm -rf "$(dsapi_module_dir "$dsapi_id")" "$(dsapi_module_state_dir "$dsapi_id")"
  rm -f "$MODULE_DISABLED_DIR/$dsapi_id.disabled"
}

dsapi_module_status_by_id() {
  dsapi_id="$1"
  dsapi_module_exists "$dsapi_id" || return 2
  dsapi_enabled=1
  if dsapi_module_disabled "$dsapi_id"; then
    dsapi_enabled=0
  fi
  dsapi_state="installed"
  dsapi_pid="-"
  dsapi_reason="-"
  dsapi_main_cap="$(dsapi_module_main_cap_by_id "$dsapi_id")"

  if [ "$dsapi_enabled" = "0" ]; then
    dsapi_state="disabled"
    dsapi_reason="disabled"
  else
    if dsapi_module_action_file_by_id "$dsapi_id" status >/dev/null 2>&1; then
      dsapi_status="$(dsapi_module_action_run_by_id "$dsapi_id" status 2>/dev/null || true)"
      dsapi_state_val="$(dsapi_kv_get state "$dsapi_status")"
      dsapi_pid_val="$(dsapi_kv_get pid "$dsapi_status")"
      dsapi_reason_val="$(dsapi_kv_get reason "$dsapi_status")"
      [ -n "$dsapi_state_val" ] && dsapi_state="$dsapi_state_val"
      [ -n "$dsapi_pid_val" ] && dsapi_pid="$dsapi_pid_val"
      [ -n "$dsapi_reason_val" ] && dsapi_reason="$dsapi_reason_val"
    elif [ -n "$dsapi_main_cap" ]; then
      dsapi_status="$(dsapi_cap_status_by_id "$dsapi_main_cap" 2>/dev/null || true)"
      dsapi_state_val="$(dsapi_kv_get state "$dsapi_status")"
      dsapi_pid_val="$(dsapi_kv_get pid "$dsapi_status")"
      dsapi_reason_val="$(dsapi_kv_get reason "$dsapi_status")"
      [ -n "$dsapi_state_val" ] && dsapi_state="$dsapi_state_val"
      [ -n "$dsapi_pid_val" ] && dsapi_pid="$dsapi_pid_val"
      [ -n "$dsapi_reason_val" ] && dsapi_reason="$dsapi_reason_val"
    fi
  fi

  echo "state=$dsapi_state pid=$dsapi_pid reason=$dsapi_reason enabled=$dsapi_enabled main_cap=$dsapi_main_cap"
}

dsapi_module_list_lines() {
  dsapi_module_entries | while IFS='|' read -r dsapi_id dsapi_dir
  do
    [ -n "$dsapi_id" ] || continue
    dsapi_name="$(dsapi_module_name_by_id "$dsapi_id")"
    dsapi_kind="$(dsapi_module_kind_by_id "$dsapi_id")"
    dsapi_version="$(dsapi_module_version_by_id "$dsapi_id")"
    dsapi_status="$(dsapi_module_status_by_id "$dsapi_id" 2>/dev/null || true)"
    dsapi_state="$(dsapi_kv_get state "$dsapi_status")"
    dsapi_enabled="$(dsapi_kv_get enabled "$dsapi_status")"
    dsapi_main_cap="$(dsapi_kv_get main_cap "$dsapi_status")"
    dsapi_reason="$(dsapi_kv_get reason "$dsapi_status")"
    dsapi_action_count="$(dsapi_module_action_count_by_id "$dsapi_id")"
    [ -n "$dsapi_state" ] || dsapi_state="installed"
    [ -n "$dsapi_enabled" ] || dsapi_enabled=1
    [ -n "$dsapi_reason" ] || dsapi_reason="-"
    printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\n' \
      "$dsapi_id" "$dsapi_name" "$dsapi_kind" "$dsapi_version" "$dsapi_state" "$dsapi_enabled" "$dsapi_main_cap" "$dsapi_action_count" "$dsapi_reason"
  done
}

dsapi_module_detail_by_id() {
  dsapi_id="$1"
  dsapi_module_exists "$dsapi_id" || return 2
  echo "id=$dsapi_id"
  echo "name=$(dsapi_module_name_by_id "$dsapi_id")"
  echo "kind=$(dsapi_module_kind_by_id "$dsapi_id")"
  echo "version=$(dsapi_module_version_by_id "$dsapi_id")"
  echo "desc=$(dsapi_module_desc_by_id "$dsapi_id")"
  echo "status=$(dsapi_module_status_by_id "$dsapi_id" 2>/dev/null || true)"
  dsapi_module_action_entries_by_id "$dsapi_id" | while IFS='|' read -r dsapi_action dsapi_name dsapi_danger dsapi_file
  do
    printf 'module_action_row=%s|%s|%s\n' "$dsapi_action" "$dsapi_name" "$dsapi_danger"
  done
  dsapi_module_env_list_lines "$dsapi_id" | while IFS='|' read -r dsapi_key dsapi_val dsapi_default dsapi_type dsapi_label dsapi_desc
  do
    printf 'module_env_row=%s|%s|%s|%s|%s|%s\n' \
      "$dsapi_key" "$dsapi_val" "$dsapi_default" "$dsapi_type" "$dsapi_label" "$dsapi_desc"
  done
}

dsapi_module_install_zip() {
  dsapi_zip="$1"
  [ -f "$dsapi_zip" ] || return 2
  if ! command -v unzip >/dev/null 2>&1; then
    return 3
  fi
  dsapi_tmp="$RUN_DIR/module.unpack.$$"
  rm -rf "$dsapi_tmp"
  mkdir -p "$dsapi_tmp"
  if ! unzip -oq "$dsapi_zip" -d "$dsapi_tmp" >/dev/null 2>&1; then
    rm -rf "$dsapi_tmp"
    return 4
  fi
  dsapi_meta="$(find "$dsapi_tmp" -maxdepth 4 -type f -name 'dsapi.module' 2>/dev/null | head -n 1 || true)"
  [ -n "$dsapi_meta" ] || { rm -rf "$dsapi_tmp"; return 5; }
  dsapi_root="${dsapi_meta%/*}"
  dsapi_id="$(dsapi_meta_get "$dsapi_meta" MODULE_ID)"
  [ -n "$dsapi_id" ] || dsapi_id="$(dsapi_meta_get "$dsapi_meta" DSAPI_MODULE_ID)"
  [ -n "$dsapi_id" ] || dsapi_id="$(basename "$dsapi_root")"
  dsapi_module_validate_id "$dsapi_id" || { rm -rf "$dsapi_tmp"; return 6; }

  dsapi_restart_after=0
  dsapi_dst="$(dsapi_module_dir "$dsapi_id")"
  if [ -d "$dsapi_dst" ]; then
    dsapi_prev_disabled=0
    if dsapi_module_disabled "$dsapi_id"; then
      dsapi_prev_disabled=1
    fi
    if [ "$dsapi_prev_disabled" = "0" ]; then
      dsapi_prev_status="$(dsapi_module_status_by_id "$dsapi_id" 2>/dev/null || true)"
      dsapi_prev_state="$(dsapi_kv_get state "$dsapi_prev_status")"
      case "$dsapi_prev_state" in
        running|ready|degraded) dsapi_restart_after=1 ;;
      esac
    fi
    dsapi_module_stop_by_id "$dsapi_id" >/dev/null 2>&1 || true
    rm -rf "$dsapi_dst"
  fi
  mkdir -p "$dsapi_dst"
  cp -a "$dsapi_root/." "$dsapi_dst/"
  if [ ! -f "$dsapi_dst/dsapi.module" ]; then
    cp "$dsapi_meta" "$dsapi_dst/dsapi.module"
  fi
  mkdir -p "$(dsapi_module_state_dir "$dsapi_id")"
  chmod 0755 "$dsapi_dst/capabilities/"*.sh 2>/dev/null || true
  chmod 0755 "$dsapi_dst/actions/"*.sh 2>/dev/null || true
  [ -f "$dsapi_dst/env.values" ] || : >"$dsapi_dst/env.values"
  rm -f "$MODULE_DISABLED_DIR/$dsapi_id.disabled"
  rm -rf "$dsapi_tmp"

  echo "$dsapi_id"
}

dsapi_kv_get() {
  dsapi_key="$1"
  dsapi_line="$2"
  for dsapi_tok in $dsapi_line; do
    case "$dsapi_tok" in
      "$dsapi_key"=*)
        echo "${dsapi_tok#*=}"
        return 0
        ;;
    esac
  done
  echo ""
}

dsapi_daemon_status_kv() {
  if [ -f "$DAEMON_PID_FILE" ]; then
    dsapi_pid="$(cat "$DAEMON_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      echo "state=running pid=$dsapi_pid reason=-"
      return 0
    fi
    rm -f "$DAEMON_PID_FILE"
    rm -f "$DAEMON_SOCKET" "$RUN_DIR/dsapi.data.sock" 2>/dev/null || true
    echo "state=stopped pid=- reason=stale_pid_cleaned"
    return 0
  fi
  if [ -S "$DAEMON_SOCKET" ]; then
    rm -f "$DAEMON_SOCKET" "$RUN_DIR/dsapi.data.sock" 2>/dev/null || true
    echo "state=stopped pid=- reason=stale_socket_cleaned"
    return 0
  fi
  echo "state=stopped pid=- reason=-"
}

dsapi_daemon_cmd() {
  dsapi_ctl="$(dsapi_active_bin dsapictl)"
  if [ -z "$dsapi_ctl" ] || [ ! -x "$dsapi_ctl" ]; then
    echo "ksu_dsapi_error=dsapictl_missing"
    return 2
  fi
  if [ ! -S "$DAEMON_SOCKET" ]; then
    echo "ksu_dsapi_error=daemon_channel_unavailable"
    return 2
  fi
  dsapi_timeout_ms="${DSAPI_DAEMON_CMD_TIMEOUT_MS:-${DSAPI_CLIENT_TIMEOUT_MS:-30000}}"
  case "$dsapi_timeout_ms" in
    ''|*[!0-9]*) dsapi_timeout_ms=30000 ;;
  esac
  DSAPI_CLIENT_TIMEOUT_MS="$dsapi_timeout_ms" "$dsapi_ctl" --socket "$DAEMON_SOCKET" "$@"
}

dsapi_cap_set_env() {
  export DSAPI_MODROOT="$MODROOT"
  export DSAPI_BASE_DIR="$BASE_DIR"
  export DSAPI_RUN_DIR="$RUN_DIR"
  export DSAPI_LOG_DIR="$LOG_DIR"
  export DSAPI_STATE_DIR="$STATE_DIR"
  export DSAPI_RUNTIME_DIR="$RUNTIME_DIR"
  export DSAPI_RELEASES_DIR="$RELEASES_DIR"
  export DSAPI_ACTIVE_RELEASE="$(dsapi_runtime_active_dir)"
  export DSAPI_ACTIVE_DSAPID="$(dsapi_active_bin dsapid)"
  export DSAPI_ACTIVE_DSAPICTL="$(dsapi_active_bin dsapictl)"
  export DSAPI_DAEMON_PID_FILE="$DAEMON_PID_FILE"
  export DSAPI_DAEMON_SOCKET="$DAEMON_SOCKET"
  export DSAPI_AGENT_RUN_DIR="$AGENT_RUN_DIR"
  export DSAPI_ACTIVE_ADAPTER_DEX="$(dsapi_active_android_dex)"
  export DSAPI_APP_PROCESS_BIN="$(dsapi_find_app_process 2>/dev/null || true)"
  export DSAPI_CAP_UI_PID_FILE="$CAP_UI_PID_FILE"
  export DSAPI_CAP_UI_LOG_FILE="$CAP_UI_LOG_FILE"
  export DSAPI_MODULES_DIR="$MODULES_DIR"
  export DSAPI_MODULE_DISABLED_DIR="$MODULE_DISABLED_DIR"
  export DSAPI_MODULE_STATE_ROOT="$MODULE_STATE_ROOT"
}

dsapi_cap_entries() {
  dsapi_active="$(dsapi_runtime_active_dir)"
  if [ -n "$dsapi_active" ] && [ -d "$dsapi_active/capabilities" ]; then
    for dsapi_file in "$dsapi_active/capabilities"/*.sh; do
      [ -f "$dsapi_file" ] || continue
      echo "builtin|$dsapi_file"
    done
  fi

  for dsapi_file in "$CAP_CUSTOM_DIR"/*.sh; do
    [ -f "$dsapi_file" ] || continue
    echo "custom|$dsapi_file"
  done

  dsapi_mod_entries="$RUN_DIR/module.entries.$$"
  dsapi_module_entries >"$dsapi_mod_entries"
  while IFS='|' read -r dsapi_mod_id dsapi_mod_dir; do
    [ -n "$dsapi_mod_id" ] || continue
    dsapi_module_disabled "$dsapi_mod_id" && continue
    for dsapi_file in "$dsapi_mod_dir/capabilities"/*.sh; do
      [ -f "$dsapi_file" ] || continue
      echo "module:$dsapi_mod_id|$dsapi_file"
    done
  done <"$dsapi_mod_entries"
  rm -f "$dsapi_mod_entries"
}

dsapi_cap_var() {
  dsapi_file="$1"
  dsapi_var="$2"
  (
    dsapi_cap_set_env
    . "$dsapi_file" 2>/dev/null || exit 0
    eval "dsapi_out=\${$dsapi_var:-}"
    printf '%s' "$dsapi_out"
  )
}

dsapi_cap_id() {
  dsapi_file="$1"
  dsapi_id="$(dsapi_cap_var "$dsapi_file" CAP_ID)"
  if [ -z "$dsapi_id" ]; then
    dsapi_id="$(basename "$dsapi_file" .sh)"
  fi
  echo "$dsapi_id"
}

dsapi_cap_name() {
  dsapi_file="$1"
  dsapi_name="$(dsapi_cap_var "$dsapi_file" CAP_NAME)"
  if [ -z "$dsapi_name" ]; then
    dsapi_name="$(dsapi_cap_id "$dsapi_file")"
  fi
  echo "$dsapi_name"
}

dsapi_cap_kind() {
  dsapi_file="$1"
  dsapi_kind="$(dsapi_cap_var "$dsapi_file" CAP_KIND)"
  if [ -z "$dsapi_kind" ]; then
    dsapi_kind="custom"
  fi
  echo "$dsapi_kind"
}

dsapi_cap_desc() {
  dsapi_file="$1"
  dsapi_desc="$(dsapi_cap_var "$dsapi_file" CAP_DESC)"
  if [ -z "$dsapi_desc" ]; then
    dsapi_desc="-"
  fi
  echo "$dsapi_desc"
}

dsapi_cap_disabled() {
  dsapi_id="$1"
  [ -f "$CAP_DISABLED_DIR/$dsapi_id.disabled" ]
}

dsapi_cap_call() {
  dsapi_file="$1"
  dsapi_func="$2"
  shift 2
  (
    dsapi_cap_set_env
    dsapi_mod_id="$(dsapi_module_id_from_cap_file "$dsapi_file")"
    if [ -n "$dsapi_mod_id" ]; then
      export DSAPI_MODULE_ID="$dsapi_mod_id"
      export DSAPI_MODULE_DIR="$(dsapi_module_dir "$dsapi_mod_id")"
      export DSAPI_MODULE_STATE_DIR="$(dsapi_module_state_dir "$dsapi_mod_id")"
      dsapi_module_env_export "$dsapi_mod_id"
    fi
    . "$dsapi_file" || exit 126
    command -v "$dsapi_func" >/dev/null 2>&1 || exit 127
    "$dsapi_func" "$@"
  )
}

dsapi_cap_find_entry() {
  dsapi_target="$1"
  dsapi_entries="$RUN_DIR/cap.entries.$$"
  dsapi_cap_entries >"$dsapi_entries"
  while IFS='|' read -r dsapi_source dsapi_file; do
    [ -n "$dsapi_file" ] || continue
    dsapi_id="$(dsapi_cap_id "$dsapi_file")"
    if [ "$dsapi_id" = "$dsapi_target" ]; then
      echo "$dsapi_source|$dsapi_file"
      rm -f "$dsapi_entries"
      return 0
    fi
  done <"$dsapi_entries"
  rm -f "$dsapi_entries"
  return 1
}

dsapi_cap_entry_source() {
  printf '%s' "$1" | cut -d '|' -f 1
}

dsapi_cap_entry_file() {
  printf '%s' "$1" | cut -d '|' -f 2-
}

dsapi_cap_list_lines() {
  dsapi_out="$RUN_DIR/cap.list.$$"
  : >"$dsapi_out"
  dsapi_entries="$RUN_DIR/cap.entries.$$"
  dsapi_cap_entries >"$dsapi_entries"

  while IFS='|' read -r dsapi_source dsapi_file; do
    [ -n "$dsapi_file" ] || continue
    dsapi_id="$(dsapi_cap_id "$dsapi_file")"
    dsapi_name="$(dsapi_cap_name "$dsapi_file")"
    dsapi_kind="$(dsapi_cap_kind "$dsapi_file")"

    dsapi_state=""
    dsapi_pid="-"
    dsapi_reason="-"

    if dsapi_cap_disabled "$dsapi_id"; then
      dsapi_state="removed"
      dsapi_reason="disabled"
    else
      dsapi_status="$(dsapi_cap_call "$dsapi_file" cap_status 2>/dev/null || true)"
      dsapi_state="$(dsapi_kv_get state "$dsapi_status")"
      dsapi_pid="$(dsapi_kv_get pid "$dsapi_status")"
      dsapi_reason="$(dsapi_kv_get reason "$dsapi_status")"
      [ -n "$dsapi_state" ] || dsapi_state="unknown"
      [ -n "$dsapi_pid" ] || dsapi_pid="-"
      [ -n "$dsapi_reason" ] || dsapi_reason="-"
    fi

    printf '%s|%s|%s|%s|%s|%s|%s|%s\n' \
      "$dsapi_id" "$dsapi_name" "$dsapi_kind" "$dsapi_source" \
      "$dsapi_state" "$dsapi_pid" "$dsapi_reason" "$dsapi_file" \
      >>"$dsapi_out"
  done <"$dsapi_entries"

  sort "$dsapi_out"
  rm -f "$dsapi_out" "$dsapi_entries"
}

dsapi_cap_start_by_id() {
  dsapi_id="$1"
  dsapi_entry="$(dsapi_cap_find_entry "$dsapi_id")" || return 2
  dsapi_file="$(dsapi_cap_entry_file "$dsapi_entry")"

  rm -f "$CAP_DISABLED_DIR/$dsapi_id.disabled" 2>/dev/null || true
  dsapi_cap_call "$dsapi_file" cap_start
}

dsapi_cap_stop_by_id() {
  dsapi_id="$1"
  dsapi_entry="$(dsapi_cap_find_entry "$dsapi_id")" || return 2
  dsapi_file="$(dsapi_cap_entry_file "$dsapi_entry")"
  dsapi_cap_call "$dsapi_file" cap_stop
}

dsapi_cap_remove_by_id() {
  dsapi_id="$1"
  dsapi_entry="$(dsapi_cap_find_entry "$dsapi_id")" || return 2
  dsapi_source="$(dsapi_cap_entry_source "$dsapi_entry")"
  dsapi_file="$(dsapi_cap_entry_file "$dsapi_entry")"

  dsapi_cap_call "$dsapi_file" cap_stop >/dev/null 2>&1 || true

  if [ "$dsapi_source" = "custom" ]; then
    rm -f "$dsapi_file"
  else
    touch "$CAP_DISABLED_DIR/$dsapi_id.disabled"
  fi
}

dsapi_cap_enable_by_id() {
  dsapi_id="$1"
  rm -f "$CAP_DISABLED_DIR/$dsapi_id.disabled"
}

dsapi_cap_status_by_id() {
  dsapi_id="$1"
  dsapi_entry="$(dsapi_cap_find_entry "$dsapi_id")" || return 2
  dsapi_file="$(dsapi_cap_entry_file "$dsapi_entry")"
  if dsapi_cap_disabled "$dsapi_id"; then
    echo "state=removed pid=- reason=disabled"
    return 0
  fi
  dsapi_cap_call "$dsapi_file" cap_status
}

dsapi_cap_detail_by_id() {
  dsapi_id="$1"
  dsapi_entry="$(dsapi_cap_find_entry "$dsapi_id")" || return 2
  dsapi_source="$(dsapi_cap_entry_source "$dsapi_entry")"
  dsapi_file="$(dsapi_cap_entry_file "$dsapi_entry")"
  dsapi_name="$(dsapi_cap_name "$dsapi_file")"
  dsapi_kind="$(dsapi_cap_kind "$dsapi_file")"
  dsapi_desc="$(dsapi_cap_desc "$dsapi_file")"
  dsapi_status="$(dsapi_cap_status_by_id "$dsapi_id" 2>/dev/null || true)"

  echo "id=$dsapi_id"
  echo "name=$dsapi_name"
  echo "kind=$dsapi_kind"
  echo "source=$dsapi_source"
  echo "desc=$dsapi_desc"
  echo "status=$dsapi_status"
  if dsapi_cap_call "$dsapi_file" cap_detail >/dev/null 2>&1; then
    dsapi_cap_call "$dsapi_file" cap_detail
  fi
}

dsapi_cap_add_custom() {
  dsapi_src="$1"
  if [ ! -f "$dsapi_src" ]; then
    return 2
  fi

  dsapi_id="$(dsapi_cap_var "$dsapi_src" CAP_ID)"
  if [ -z "$dsapi_id" ]; then
    dsapi_id="$(basename "$dsapi_src" .sh)"
  fi
  dsapi_runtime_validate_id "$dsapi_id" || return 2

  dsapi_dst="$CAP_CUSTOM_DIR/$dsapi_id.sh"
  cp "$dsapi_src" "$dsapi_dst"
  chmod 0755 "$dsapi_dst" 2>/dev/null || true
  rm -f "$CAP_DISABLED_DIR/$dsapi_id.disabled" 2>/dev/null || true
  echo "$dsapi_id"
}

dsapi_runtime_install_dir() {
  dsapi_rel="$1"
  dsapi_src="$2"
  dsapi_runtime_validate_id "$dsapi_rel" || return 2
  [ -d "$dsapi_src" ] || return 2
  [ ! -e "$RELEASES_DIR/$dsapi_rel" ] || return 3
  [ -x "$dsapi_src/bin/dsapid" ] || return 4
  [ -x "$dsapi_src/bin/dsapictl" ] || return 4

  mkdir -p "$RELEASES_DIR/$dsapi_rel"
  cp -a "$dsapi_src/." "$RELEASES_DIR/$dsapi_rel/"
  chmod 0755 "$RELEASES_DIR/$dsapi_rel/bin/dsapid" "$RELEASES_DIR/$dsapi_rel/bin/dsapictl" 2>/dev/null || true
  chmod 0755 "$RELEASES_DIR/$dsapi_rel/capabilities/"*.sh 2>/dev/null || true
}

dsapi_runtime_remove() {
  dsapi_rel="$1"
  dsapi_active="$(dsapi_runtime_active_release_id)"
  if [ "$dsapi_rel" = "$dsapi_active" ]; then
    return 2
  fi
  [ -d "$RELEASES_DIR/$dsapi_rel" ] || return 3
  rm -rf "$RELEASES_DIR/$dsapi_rel"
}
