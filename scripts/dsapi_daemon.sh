daemon_start_impl() {
  DAEMON_CONTROL_SOCKET_PATH="$(control_socket_path)"
  DAEMON_DATA_SOCKET_PATH="$(data_socket_path)"
  DAEMON_PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
  DAEMON_LOG_FILE="${DSAPI_LOG_FILE:-artifacts/run/dsapid.log}"
  DAEMON_SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"
  DAEMON_SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"
  DAEMON_SUPERVISE_PRESENT_CMD="${DSAPI_SUPERVISE_PRESENTER_CMD:-./scripts/dsapi.sh presenter run}"
  DAEMON_SUPERVISE_INPUT_CMD="${DSAPI_SUPERVISE_INPUT_CMD:-./scripts/dsapi.sh touch run}"
  DAEMON_SUPERVISE_RESTART_MS="${DSAPI_SUPERVISE_RESTART_MS:-500}"
  DAEMON_RENDER_OUTPUT_DIR="${DSAPI_RENDER_OUTPUT_DIR:-artifacts/render}"

  mkdir -p "$(dirname "$DAEMON_CONTROL_SOCKET_PATH")"
  mkdir -p "$(dirname "$DAEMON_DATA_SOCKET_PATH")"
  mkdir -p "$(dirname "$DAEMON_PID_FILE")"
  mkdir -p "$(dirname "$DAEMON_LOG_FILE")"

  if [ -f "$DAEMON_PID_FILE" ]; then
    old_pid="$(cat "$DAEMON_PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && pid_is_running "$old_pid"; then
      echo "daemon_status=already_running pid=$old_pid"
      return 0
    fi
    rm -f "$DAEMON_PID_FILE"
  fi

  ./scripts/build_core.sh >/dev/null

  daemon_bin="$(release_bin dsapid)"
  set -- "$daemon_bin" \
    --control-socket "$DAEMON_CONTROL_SOCKET_PATH" \
    --data-socket "$DAEMON_DATA_SOCKET_PATH" \
    --render-output-dir "$DAEMON_RENDER_OUTPUT_DIR" \
    --supervise-restart-ms "$DAEMON_SUPERVISE_RESTART_MS"
  if [ "$DAEMON_SUPERVISE_PRESENTER" = "1" ]; then
    set -- "$@" --supervise-presenter "$DAEMON_SUPERVISE_PRESENT_CMD"
  fi
  if [ "$DAEMON_SUPERVISE_INPUT" = "1" ]; then
    set -- "$@" --supervise-input "$DAEMON_SUPERVISE_INPUT_CMD"
  fi

  nohup "$@" >>"$DAEMON_LOG_FILE" 2>&1 &
  new_pid="$!"
  echo "$new_pid" > "$DAEMON_PID_FILE"

  sleep 0.2
  if pid_is_running "$new_pid"; then
    daemon_ready=0
    daemon_ctl_bin="$(release_bin dsapictl)"
    if [ -x "$daemon_ctl_bin" ]; then
      i=0
      while [ "$i" -lt 20 ]; do
        if [ -S "$DAEMON_CONTROL_SOCKET_PATH" ] \
          && "$daemon_ctl_bin" --socket "$DAEMON_CONTROL_SOCKET_PATH" PING >/dev/null 2>&1; then
          daemon_ready=1
          break
        fi
        i=$((i + 1))
        sleep 0.1
      done
    else
      daemon_ready=1
    fi

    if [ "$daemon_ready" = "0" ]; then
      echo "daemon_error=startup_probe_timeout pid=$new_pid control_socket=$DAEMON_CONTROL_SOCKET_PATH"
      kill "$new_pid" >/dev/null 2>&1 || true
      sleep 0.1
      kill -KILL "$new_pid" >/dev/null 2>&1 || true
      rm -f "$DAEMON_PID_FILE" "$DAEMON_CONTROL_SOCKET_PATH" "$DAEMON_DATA_SOCKET_PATH"
      return 2
    fi
    echo "daemon_status=started pid=$new_pid control_socket=$DAEMON_CONTROL_SOCKET_PATH data_socket=$DAEMON_DATA_SOCKET_PATH"
    return 0
  fi

  echo "daemon_status=failed_start"
  return 2
}

daemon_status_impl() {
  DAEMON_CONTROL_SOCKET_PATH="$(control_socket_path)"
  DAEMON_DATA_SOCKET_PATH="$(data_socket_path)"
  DAEMON_PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
  daemon_bin="$(release_bin dsapid)"

  pid=""
  if [ -f "$DAEMON_PID_FILE" ]; then
    pid="$(cat "$DAEMON_PID_FILE" 2>/dev/null || true)"
  fi

  if [ -n "$pid" ] && pid_is_running "$pid"; then
    if ! pid_cmdline_contains "$pid" "$daemon_bin"; then
      echo "daemon_status=degraded pid=$pid pid_mismatch=1 expected=$daemon_bin"
      return 1
    fi
    if [ -S "$DAEMON_CONTROL_SOCKET_PATH" ] && [ -S "$DAEMON_DATA_SOCKET_PATH" ]; then
      if [ -x "$(release_bin dsapictl)" ] \
        && ! "$(release_bin dsapictl)" --socket "$DAEMON_CONTROL_SOCKET_PATH" PING >/dev/null 2>&1; then
        echo "daemon_status=degraded pid=$pid control_probe_failed=1 control_socket=$DAEMON_CONTROL_SOCKET_PATH data_socket=$DAEMON_DATA_SOCKET_PATH"
        return 1
      fi
      echo "daemon_status=running pid=$pid control_socket=$DAEMON_CONTROL_SOCKET_PATH data_socket=$DAEMON_DATA_SOCKET_PATH"
      return 0
    fi
    echo "daemon_status=degraded pid=$pid control_socket_exists=$([ -S "$DAEMON_CONTROL_SOCKET_PATH" ] && echo 1 || echo 0) data_socket_exists=$([ -S "$DAEMON_DATA_SOCKET_PATH" ] && echo 1 || echo 0)"
    return 1
  fi

  echo "daemon_status=stopped"
  return 2
}

daemon_stop_impl() {
  DAEMON_CONTROL_SOCKET_PATH="$(control_socket_path)"
  DAEMON_DATA_SOCKET_PATH="$(data_socket_path)"
  DAEMON_PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
  DAEMON_CTL_BIN="$(release_bin dsapictl)"
  DAEMON_BIN="$(release_bin dsapid)"

  presenter_stop_impl >/dev/null 2>&1 || true
  touch_stop_impl >/dev/null 2>&1 || true

  if [ -S "$DAEMON_CONTROL_SOCKET_PATH" ] && [ -x "$DAEMON_CTL_BIN" ]; then
    "$DAEMON_CTL_BIN" --socket "$DAEMON_CONTROL_SOCKET_PATH" SHUTDOWN >/dev/null 2>&1 || true
  fi

  if [ -f "$DAEMON_PID_FILE" ]; then
    pid="$(cat "$DAEMON_PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && pid_is_running "$pid"; then
      if pid_cmdline_contains "$pid" "$DAEMON_BIN"; then
        kill "$pid" >/dev/null 2>&1 || true
        sleep 0.2
        kill -KILL "$pid" >/dev/null 2>&1 || true
      else
        echo "daemon_warn=pid_cmdline_mismatch pid=$pid expected='$DAEMON_BIN'"
      fi
    fi
    rm -f "$DAEMON_PID_FILE"
  fi

  rm -f "$DAEMON_CONTROL_SOCKET_PATH" "$DAEMON_DATA_SOCKET_PATH"
  echo "daemon_status=stopped"
}

daemon_cmd_impl() {
  if [ "$#" -lt 1 ]; then
    echo "dsapi_error=daemon_cmd_missing"
    return 2
  fi
  ensure_release_ctl
  "$(release_bin dsapictl)" --socket "$(control_socket_path)" "$@"
}
