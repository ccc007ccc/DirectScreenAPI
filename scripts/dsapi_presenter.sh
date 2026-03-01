presenter_kill_pid_if_running() {
  target_pid="$1"
  if [ -z "$target_pid" ] || ! pid_is_running "$target_pid"; then
    return 0
  fi
  kill -- "-$target_pid" >/dev/null 2>&1 || kill "$target_pid" >/dev/null 2>&1 || true
  sleep 0.1
  kill -KILL -- "-$target_pid" >/dev/null 2>&1 || kill -KILL "$target_pid" >/dev/null 2>&1 || true
}

presenter_collect_nonroot_pids() {
  pattern="$1"
  ps -ef | awk -v p="$pattern" '$0 ~ p && $2 ~ /^[0-9]+$/ { print $2 }'
}

presenter_collect_root_pids() {
  pattern="$1"
  su -c "ps -ef | awk -v p='${pattern}' '\$0 ~ p && \$2 ~ /^[0-9]+$/ { print \$2 }'" 2>/dev/null || true
}

presenter_cleanup_orphans_impl() {
  RUN_AS_ROOT="${1:-1}"

  for pid in $(presenter_collect_nonroot_pids "dsapi.sh presenter run")
  do
    presenter_kill_pid_if_running "$pid"
  done

  for pid in $(presenter_collect_nonroot_pids "AndroidAdapterMain.*present-loop")
  do
    presenter_kill_pid_if_running "$pid"
  done

  if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
    for pid in $(presenter_collect_root_pids "AndroidAdapterMain.*present-loop")
    do
      su_exec kill -- "-$pid" >/dev/null 2>&1 || su_exec kill "$pid" >/dev/null 2>&1 || true
      sleep 0.1
      su_exec kill -KILL -- "-$pid" >/dev/null 2>&1 || su_exec kill -KILL "$pid" >/dev/null 2>&1 || true
    done
  fi
}

presenter_start_impl() {
  PRESENTER_PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  PRESENTER_LOG_FILE="${DSAPI_PRESENTER_LOG_FILE:-artifacts/run/dsapi_presenter.log}"
  PRESENTER_SUPERVISE="${DSAPI_SUPERVISE_PRESENTER:-0}"
  PRESENTER_USE_SETSID="${DSAPI_PRESENTER_USE_SETSID:-1}"

  mkdir -p "$(dirname "$PRESENTER_PID_FILE")"
  mkdir -p "$(dirname "$PRESENTER_LOG_FILE")"

  if ! ( : >>"$PRESENTER_LOG_FILE" ) 2>/dev/null; then
    if [ -n "${DSAPI_PRESENTER_LOG_FILE:-}" ]; then
      echo "presenter_error=log_file_not_writable path=$PRESENTER_LOG_FILE"
      return 2
    fi
    fallback_log="artifacts/run/dsapi_presenter_user.log"
    mkdir -p "$(dirname "$fallback_log")"
    if ! ( : >>"$fallback_log" ) 2>/dev/null; then
      echo "presenter_error=log_file_not_writable path=$PRESENTER_LOG_FILE fallback=$fallback_log"
      return 2
    fi
    echo "presenter_warn=log_file_not_writable path=$PRESENTER_LOG_FILE fallback=$fallback_log"
    PRESENTER_LOG_FILE="$fallback_log"
  fi

  if [ "$PRESENTER_SUPERVISE" = "1" ]; then
    daemon_start_impl >/dev/null
    echo "presenter_status=managed_by_daemon supervise=1"
    return 0
  fi

  presenter_cleanup_orphans_impl "${DSAPI_RUN_AS_ROOT:-1}"

  if [ -f "$PRESENTER_PID_FILE" ]; then
    old_pid="$(cat "$PRESENTER_PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && pid_is_running "$old_pid"; then
      echo "presenter_status=already_running pid=$old_pid"
      return 0
    fi
    rm -f "$PRESENTER_PID_FILE"
  fi

  if ! daemon_status_impl >/dev/null 2>&1; then
    daemon_start_impl >/dev/null
  fi

  android_sync_display_impl >/dev/null 2>&1 || true

  if [ "$PRESENTER_USE_SETSID" = "1" ] && command -v setsid >/dev/null 2>&1; then
    nohup setsid ./scripts/dsapi.sh presenter run >>"$PRESENTER_LOG_FILE" 2>&1 < /dev/null &
  else
    nohup ./scripts/dsapi.sh presenter run >>"$PRESENTER_LOG_FILE" 2>&1 < /dev/null &
  fi
  presenter_pid="$!"
  echo "$presenter_pid" > "$PRESENTER_PID_FILE"

  sleep 1
  if pid_is_running "$presenter_pid"; then
    echo "presenter_status=started pid=$presenter_pid log=$PRESENTER_LOG_FILE"
    return 0
  fi

  echo "presenter_status=failed_start"
  if [ -f "$PRESENTER_LOG_FILE" ]; then
    echo "presenter_last_log_start"
    tail -n 20 "$PRESENTER_LOG_FILE"
    echo "presenter_last_log_end"
  fi
  return 2
}

presenter_stop_impl() {
  PRESENTER_PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  PRESENTER_RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
  PRESENTER_SUPERVISE="${DSAPI_SUPERVISE_PRESENTER:-0}"

  if [ "$PRESENTER_SUPERVISE" = "1" ]; then
    echo "presenter_status=managed_by_daemon supervise=1 action=use_daemon_stop"
    return 0
  fi

  if [ -f "$PRESENTER_PID_FILE" ]; then
    presenter_pid="$(cat "$PRESENTER_PID_FILE" 2>/dev/null || true)"
    if [ -n "$presenter_pid" ] && pid_is_running "$presenter_pid"; then
      if pid_cmdline_contains "$presenter_pid" "dsapi.sh presenter run"; then
        presenter_kill_pid_if_running "$presenter_pid"
        if [ "$PRESENTER_RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
          su_exec kill -KILL -- "-$presenter_pid" >/dev/null 2>&1 || su_exec kill -KILL "$presenter_pid" >/dev/null 2>&1 || true
        fi
      else
        echo "presenter_warn=pid_cmdline_mismatch pid=$presenter_pid expected='dsapi.sh presenter run'"
      fi
    fi
    rm -f "$PRESENTER_PID_FILE"
  fi

  presenter_cleanup_orphans_impl "$PRESENTER_RUN_AS_ROOT"

  echo "presenter_status=stopped"
}

presenter_status_impl() {
  PRESENTER_PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  PRESENTER_SUPERVISE="${DSAPI_SUPERVISE_PRESENTER:-0}"

  if [ "$PRESENTER_SUPERVISE" = "1" ]; then
    if daemon_status_impl >/dev/null 2>&1; then
      echo "presenter_status=managed_by_daemon supervise=1"
      return 0
    fi
    echo "presenter_status=stopped supervise=1 daemon_stopped=1"
    return 2
  fi

  if [ -f "$PRESENTER_PID_FILE" ]; then
    pid="$(cat "$PRESENTER_PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && pid_is_running "$pid"; then
      echo "presenter_status=running pid=$pid"
      return 0
    fi
  fi

  echo "presenter_status=stopped"
  return 2
}

presenter_run_impl() {
  CONTROL_SOCKET_PATH="$(control_socket_path)"
  DATA_SOCKET_PATH="$(data_socket_path)"
  OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/android}"
  DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
  MAIN_CLASS="org.directscreenapi.adapter.AndroidAdapterMain"
  APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-}"
  RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
  POLL_MS="${DSAPI_PRESENTER_POLL_MS:-2}"
  LAYER_Z="${DSAPI_PRESENTER_LAYER_Z:-1000000}"
  LAYER_NAME="${DSAPI_PRESENTER_LAYER_NAME:-DirectScreenAPI}"
  AUTO_REBUILD="${DSAPI_PRESENTER_AUTO_REBUILD:-1}"
  PRECHECK="${DSAPI_PRESENTER_PRECHECK:-1}"

  if [ -z "${DSAPI_ANDROID_OUT_DIR:-}" ]; then
    if { [ -d "$OUT_DIR" ] && [ ! -w "$OUT_DIR" ]; } \
      || { [ -d "$OUT_DIR/classes" ] && [ ! -w "$OUT_DIR/classes" ]; }; then
      OUT_DIR="artifacts/android_user"
      DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
      echo "presenter_warn=android_out_dir_not_writable fallback_out_dir=$OUT_DIR"
    fi
  fi

  if [ ! -S "$CONTROL_SOCKET_PATH" ]; then
    echo "presenter_error=daemon_control_socket_missing socket=$CONTROL_SOCKET_PATH"
    return 2
  fi
  if [ ! -S "$DATA_SOCKET_PATH" ]; then
    echo "presenter_error=daemon_data_socket_missing socket=$DATA_SOCKET_PATH"
    return 2
  fi

  case "$CONTROL_SOCKET_PATH" in
    /*) CONTROL_SOCKET_ABS="$CONTROL_SOCKET_PATH" ;;
    *) CONTROL_SOCKET_ABS="$ROOT_DIR/$CONTROL_SOCKET_PATH" ;;
  esac
  case "$DATA_SOCKET_PATH" in
    /*) DATA_SOCKET_ABS="$DATA_SOCKET_PATH" ;;
    *) DATA_SOCKET_ABS="$ROOT_DIR/$DATA_SOCKET_PATH" ;;
  esac

  if [ "$AUTO_REBUILD" = "1" ] || [ ! -f "$DEX_JAR" ]; then
    if ! DSAPI_ANDROID_BUILD_MODE=presenter ./scripts/build_android_adapter.sh >/dev/null; then
      if [ ! -f "$DEX_JAR" ]; then
        echo "presenter_error=android_adapter_build_failed_no_cached_dex path=$DEX_JAR"
        return 2
      fi
      echo "presenter_warn=android_adapter_build_failed_using_cached_dex path=$DEX_JAR"
    fi
  fi

  case "$DEX_JAR" in
    /*) DEX_JAR_ABS="$DEX_JAR" ;;
    *) DEX_JAR_ABS="$ROOT_DIR/$DEX_JAR" ;;
  esac

  if [ -n "$APP_PROCESS_BIN" ] && ! command -v "$APP_PROCESS_BIN" >/dev/null 2>&1 && [ ! -x "$APP_PROCESS_BIN" ]; then
    echo "presenter_error=app_process_not_found path=$APP_PROCESS_BIN"
    return 2
  fi

  if [ -z "$APP_PROCESS_BIN" ]; then
    candidates=""
    for c in /system/bin/app_process64 /system/bin/app_process /system/bin/app_process32
    do
      if [ -x "$c" ]; then
        candidates="$candidates $c"
      fi
    done
    if command -v app_process >/dev/null 2>&1; then
      c="$(command -v app_process)"
      candidates="$candidates $c"
    fi

    for c in $candidates
    do
      if [ "$PRECHECK" = "1" ]; then
        if CLASSPATH="$DEX_JAR_ABS" "$c" /system/bin "$MAIN_CLASS" display-line >/dev/null 2>&1; then
          APP_PROCESS_BIN="$c"
          break
        fi
        echo "presenter_warn=app_process_candidate_rejected path=$c"
      else
        APP_PROCESS_BIN="$c"
        break
      fi
    done

    if [ -z "$APP_PROCESS_BIN" ]; then
      echo "presenter_error=app_process_not_found_or_unusable"
      return 2
    fi
  fi

  echo "presenter_exec app_process=$APP_PROCESS_BIN dex=$DEX_JAR_ABS"

  if [ "$PRECHECK" = "1" ]; then
    if ! CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" display-line >/dev/null 2>&1; then
      echo "presenter_error=app_process_precheck_failed app_process=$APP_PROCESS_BIN dex=$DEX_JAR_ABS"
      return 2
    fi
  fi

  if [ "$RUN_AS_ROOT" = "1" ]; then
    if ! command -v su >/dev/null 2>&1; then
      echo "presenter_error=su_not_found"
      return 2
    fi
    su_exec_with_env CLASSPATH "$DEX_JAR_ABS" \
      "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" present-loop \
      "$CONTROL_SOCKET_ABS" "$DATA_SOCKET_ABS" "$POLL_MS" "$LAYER_Z" "$LAYER_NAME"
    return 0
  fi

  CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" present-loop "$CONTROL_SOCKET_PATH" "$DATA_SOCKET_PATH" "$POLL_MS" "$LAYER_Z" "$LAYER_NAME"
}
