presenter_start_impl() {
  PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  LOG_FILE="${DSAPI_PRESENTER_LOG_FILE:-artifacts/run/dsapi_presenter.log}"
  SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"

  mkdir -p "$(dirname "$PID_FILE")"
  mkdir -p "$(dirname "$LOG_FILE")"

  if [ "$SUPERVISE_PRESENTER" = "1" ]; then
    daemon_start_impl >/dev/null
    echo "presenter_status=managed_by_daemon supervise=1"
    return 0
  fi

  if [ -f "$PID_FILE" ]; then
    old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && kill -0 "$old_pid" >/dev/null 2>&1; then
      echo "presenter_status=already_running pid=$old_pid"
      return 0
    fi
    rm -f "$PID_FILE"
  fi

  if ! daemon_status_impl >/dev/null 2>&1; then
    daemon_start_impl >/dev/null
  fi

  android_sync_display_impl >/dev/null 2>&1 || true

  nohup setsid ./scripts/dsapi.sh presenter run >>"$LOG_FILE" 2>&1 < /dev/null &
  pid="$!"
  echo "$pid" > "$PID_FILE"

  sleep 0.3
  if kill -0 "$pid" >/dev/null 2>&1; then
    echo "presenter_status=started pid=$pid log=$LOG_FILE"
    return 0
  fi

  echo "presenter_status=failed_start"
  return 2
}

presenter_stop_impl() {
  PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
  SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"

  if [ "$SUPERVISE_PRESENTER" = "1" ]; then
    echo "presenter_status=managed_by_daemon supervise=1 action=use_daemon_stop"
    return 0
  fi

  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
      if pid_cmdline_contains "$pid" "dsapi.sh presenter run"; then
        kill -- "-$pid" >/dev/null 2>&1 || kill "$pid" >/dev/null 2>&1 || true
        if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
          su_exec kill -- "-$pid" >/dev/null 2>&1 || su_exec kill "$pid" >/dev/null 2>&1 || true
        fi
        sleep 0.2
        kill -KILL -- "-$pid" >/dev/null 2>&1 || kill -KILL "$pid" >/dev/null 2>&1 || true
        if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
          su_exec kill -KILL -- "-$pid" >/dev/null 2>&1 || su_exec kill -KILL "$pid" >/dev/null 2>&1 || true
        fi
      else
        echo "presenter_warn=pid_cmdline_mismatch pid=$pid expected='dsapi.sh presenter run'"
      fi
    fi
    rm -f "$PID_FILE"
  fi

  echo "presenter_status=stopped"
}

presenter_status_impl() {
  PID_FILE="${DSAPI_PRESENTER_PID_FILE:-artifacts/run/dsapi_presenter.pid}"
  SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"

  if [ "$SUPERVISE_PRESENTER" = "1" ]; then
    if daemon_status_impl >/dev/null 2>&1; then
      echo "presenter_status=managed_by_daemon supervise=1"
      return 0
    fi
    echo "presenter_status=stopped supervise=1 daemon_stopped=1"
    return 2
  fi

  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
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
  APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-app_process}"
  RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
  POLL_MS="${DSAPI_PRESENTER_POLL_MS:-2}"
  LAYER_Z="${DSAPI_PRESENTER_LAYER_Z:-1000000}"
  LAYER_NAME="${DSAPI_PRESENTER_LAYER_NAME:-DirectScreenAPI}"
  AUTO_REBUILD="${DSAPI_PRESENTER_AUTO_REBUILD:-1}"

  if [ ! -S "$CONTROL_SOCKET_PATH" ]; then
    echo "presenter_error=daemon_control_socket_missing socket=$CONTROL_SOCKET_PATH"
    return 2
  fi
  if [ ! -S "$DATA_SOCKET_PATH" ]; then
    echo "presenter_error=daemon_data_socket_missing socket=$DATA_SOCKET_PATH"
    return 2
  fi

  if [ "$AUTO_REBUILD" = "1" ] || [ ! -f "$DEX_JAR" ]; then
    DSAPI_ANDROID_BUILD_MODE=presenter ./scripts/build_android_adapter.sh >/dev/null
  fi

  case "$DEX_JAR" in
    /*) DEX_JAR_ABS="$DEX_JAR" ;;
    *) DEX_JAR_ABS="$ROOT_DIR/$DEX_JAR" ;;
  esac

  if ! command -v "$APP_PROCESS_BIN" >/dev/null 2>&1; then
    if [ -x /system/bin/app_process ]; then
      APP_PROCESS_BIN="/system/bin/app_process"
    else
      echo "presenter_error=app_process_not_found"
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
      "$CONTROL_SOCKET_PATH" "$DATA_SOCKET_PATH" "$POLL_MS" "$LAYER_Z" "$LAYER_NAME"
    return 0
  fi

  CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" present-loop "$CONTROL_SOCKET_PATH" "$DATA_SOCKET_PATH" "$POLL_MS" "$LAYER_Z" "$LAYER_NAME"
}

