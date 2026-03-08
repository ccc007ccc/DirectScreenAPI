presenter_kill_pid_if_running() {
  target_pid="$1"
  if [ -z "$target_pid" ] || ! pid_is_running "$target_pid"; then
    return 0
  fi
  kill -- "-$target_pid" >/dev/null 2>&1 || kill "$target_pid" >/dev/null 2>&1 || true
  i=0
  while [ "$i" -lt 30 ]
  do
    if ! pid_is_running "$target_pid"; then
      return 0
    fi
    i=$((i + 1))
    sleep 0.1
  done
  kill -KILL -- "-$target_pid" >/dev/null 2>&1 || kill -KILL "$target_pid" >/dev/null 2>&1 || true
  i=0
  while [ "$i" -lt 10 ]
  do
    if ! pid_is_running "$target_pid"; then
      return 0
    fi
    i=$((i + 1))
    sleep 0.1
  done
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
      i=0
      while [ "$i" -lt 30 ]
      do
        if ! su_exec kill -0 "$pid" >/dev/null 2>&1; then
          break
        fi
        i=$((i + 1))
        sleep 0.1
      done
      su_exec kill -KILL -- "-$pid" >/dev/null 2>&1 || su_exec kill -KILL "$pid" >/dev/null 2>&1 || true
      i=0
      while [ "$i" -lt 10 ]
      do
        if ! su_exec kill -0 "$pid" >/dev/null 2>&1; then
          break
        fi
        i=$((i + 1))
        sleep 0.1
      done
    done
  fi
}

presenter_start_impl() {
  PRESENTER_PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  PRESENTER_LOG_FILE="${DSAPI_PRESENTER_LOG_FILE:-artifacts/run/dsapi_presenter.log}"
  PRESENTER_SUPERVISE="${DSAPI_SUPERVISE_PRESENTER:-0}"
  PRESENTER_USE_SETSID="${DSAPI_PRESENTER_USE_SETSID:-1}"
  PRESENTER_DAEMON_READY_RETRIES="${DSAPI_PRESENTER_DAEMON_READY_RETRIES:-40}"
  DAEMON_CONTROL_SOCKET_PATH="$(control_socket_path)"

  mkdir -p "$(dirname "$PRESENTER_PID_FILE")"
  mkdir -p "$(dirname "$PRESENTER_LOG_FILE")"
  if ! ( : >>"$PRESENTER_LOG_FILE" ) 2>/dev/null; then
    echo "presenter_error=log_file_not_writable path=$PRESENTER_LOG_FILE"
    return 2
  fi
  LOG_START_LINE=0
  if [ -f "$PRESENTER_LOG_FILE" ]; then
    LOG_START_LINE="$(wc -l < "$PRESENTER_LOG_FILE" 2>/dev/null || echo 0)"
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
  daemon_ctl_bin="$(release_bin dsapictl)"
  if [ -x "$daemon_ctl_bin" ]; then
    if ! daemon_wait_ready_impl "$daemon_ctl_bin" "$DAEMON_CONTROL_SOCKET_PATH" "$PRESENTER_DAEMON_READY_RETRIES" 0.1; then
      echo "presenter_error=daemon_not_ready control_socket=$DAEMON_CONTROL_SOCKET_PATH"
      return 2
    fi
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
    START_MARKER_OK=0
    START_MARKER_FAIL=0
    i=0
    while [ "$i" -lt 40 ]
    do
      NEW_LOGS="$(tail -n +"$((LOG_START_LINE + 1))" "$PRESENTER_LOG_FILE" 2>/dev/null || true)"
      if printf '%s\n' "$NEW_LOGS" | grep -q "presenter_status=started"; then
        START_MARKER_OK=1
        break
      fi
      if printf '%s\n' "$NEW_LOGS" | grep -q "presenter_status=failed\\|presenter_error="; then
        START_MARKER_FAIL=1
        break
      fi
      i=$((i + 1))
      sleep 0.1
    done

    if [ "$START_MARKER_OK" = "1" ]; then
      echo "presenter_status=started pid=$presenter_pid log=$PRESENTER_LOG_FILE"
      return 0
    fi
    if [ "$START_MARKER_FAIL" = "1" ]; then
      presenter_kill_pid_if_running "$presenter_pid"
      rm -f "$PRESENTER_PID_FILE"
      echo "presenter_status=failed_start marker=failed_in_log"
      if [ -f "$PRESENTER_LOG_FILE" ]; then
        echo "presenter_last_log_start"
        tail -n 30 "$PRESENTER_LOG_FILE"
        echo "presenter_last_log_end"
      fi
      return 2
    fi

    echo "presenter_warn=start_marker_timeout pid=$presenter_pid log=$PRESENTER_LOG_FILE"
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
  OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/ksu_module/android_adapter}"
  DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
  MAIN_CLASS="org.directscreenapi.adapter.AndroidAdapterMain"
  APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-}"
  RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
  WAIT_TIMEOUT_MS="${DSAPI_PRESENTER_WAIT_TIMEOUT_MS:-0}"
  LAYER_Z="${DSAPI_PRESENTER_LAYER_Z:-1000000}"
  LAYER_NAME="${DSAPI_PRESENTER_LAYER_NAME:-DirectScreenAPI}"
  BLUR_RADIUS="${DSAPI_PRESENTER_BLUR_RADIUS:-0}"
  BLUR_SIGMA="${DSAPI_PRESENTER_BLUR_SIGMA:-0}"
  FILTER_CHAIN="${DSAPI_PRESENTER_FILTER_CHAIN:-}"
  FRAME_RATE_MODE="${DSAPI_PRESENTER_FRAME_RATE:-auto}"
  AUTO_REBUILD="${DSAPI_PRESENTER_AUTO_REBUILD:-1}"
  PRECHECK="${DSAPI_PRESENTER_PRECHECK:-1}"

  if { [ -d "$OUT_DIR" ] && [ ! -w "$OUT_DIR" ]; } \
    || { [ -d "$OUT_DIR/classes" ] && [ ! -w "$OUT_DIR/classes" ]; }; then
    echo "presenter_error=android_out_dir_not_writable path=$OUT_DIR"
    return 2
  fi

  if [ ! -S "$CONTROL_SOCKET_PATH" ]; then
    echo "presenter_error=daemon_control_socket_missing socket=$CONTROL_SOCKET_PATH"
    return 2
  fi

  case "$CONTROL_SOCKET_PATH" in
    /*) CONTROL_SOCKET_ABS="$CONTROL_SOCKET_PATH" ;;
    *) CONTROL_SOCKET_ABS="$ROOT_DIR/$CONTROL_SOCKET_PATH" ;;
  esac

  if [ "$AUTO_REBUILD" = "1" ] || [ ! -f "$DEX_JAR" ]; then
    if ! DSAPI_ANDROID_BUILD_MODE=presenter ./scripts/build_android_adapter.sh >/dev/null; then
      echo "presenter_error=android_adapter_build_failed path=$DEX_JAR"
      return 2
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
      "$CONTROL_SOCKET_ABS" "$WAIT_TIMEOUT_MS" "$LAYER_Z" "$LAYER_NAME" \
      "$BLUR_RADIUS" "$BLUR_SIGMA" "$FILTER_CHAIN" "$FRAME_RATE_MODE"
    return 0
  fi

  CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" present-loop \
    "$CONTROL_SOCKET_PATH" "$WAIT_TIMEOUT_MS" "$LAYER_Z" "$LAYER_NAME" \
    "$BLUR_RADIUS" "$BLUR_SIGMA" "$FILTER_CHAIN" "$FRAME_RATE_MODE"
}
