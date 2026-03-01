daemon_start_impl() {
  CONTROL_SOCKET_PATH="$(control_socket_path)"
  DATA_SOCKET_PATH="$(data_socket_path)"
  PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
  LOG_FILE="${DSAPI_LOG_FILE:-artifacts/run/dsapid.log}"
  SUPERVISE_PRESENTER="${DSAPI_SUPERVISE_PRESENTER:-0}"
  SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"
  SUPERVISE_PRESENT_CMD="${DSAPI_SUPERVISE_PRESENTER_CMD:-./scripts/dsapi.sh presenter run}"
  SUPERVISE_INPUT_CMD="${DSAPI_SUPERVISE_INPUT_CMD:-./scripts/dsapi.sh touch run}"
  SUPERVISE_RESTART_MS="${DSAPI_SUPERVISE_RESTART_MS:-500}"
  RENDER_OUTPUT_DIR="${DSAPI_RENDER_OUTPUT_DIR:-artifacts/render}"

  mkdir -p "$(dirname "$CONTROL_SOCKET_PATH")"
  mkdir -p "$(dirname "$DATA_SOCKET_PATH")"
  mkdir -p "$(dirname "$PID_FILE")"
  mkdir -p "$(dirname "$LOG_FILE")"

  if [ -f "$PID_FILE" ]; then
    old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && kill -0 "$old_pid" >/dev/null 2>&1; then
      echo "daemon_status=already_running pid=$old_pid"
      return 0
    fi
    rm -f "$PID_FILE"
  fi

  ./scripts/build_core.sh >/dev/null

  DSAPID_BIN="$(release_bin dsapid)"
  set -- "$DSAPID_BIN" \
    --control-socket "$CONTROL_SOCKET_PATH" \
    --data-socket "$DATA_SOCKET_PATH" \
    --render-output-dir "$RENDER_OUTPUT_DIR" \
    --supervise-restart-ms "$SUPERVISE_RESTART_MS"
  if [ "$SUPERVISE_PRESENTER" = "1" ]; then
    set -- "$@" --supervise-presenter "$SUPERVISE_PRESENT_CMD"
  fi
  if [ "$SUPERVISE_INPUT" = "1" ]; then
    set -- "$@" --supervise-input "$SUPERVISE_INPUT_CMD"
  fi

  nohup "$@" >>"$LOG_FILE" 2>&1 &
  new_pid="$!"
  echo "$new_pid" > "$PID_FILE"

  sleep 0.2
  if kill -0 "$new_pid" >/dev/null 2>&1; then
    echo "daemon_status=started pid=$new_pid control_socket=$CONTROL_SOCKET_PATH data_socket=$DATA_SOCKET_PATH"
    return 0
  fi

  echo "daemon_status=failed_start"
  return 2
}

daemon_status_impl() {
  CONTROL_SOCKET_PATH="$(control_socket_path)"
  DATA_SOCKET_PATH="$(data_socket_path)"
  PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"

  pid=""
  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  fi

  if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
    if [ -S "$CONTROL_SOCKET_PATH" ] && [ -S "$DATA_SOCKET_PATH" ]; then
      echo "daemon_status=running pid=$pid control_socket=$CONTROL_SOCKET_PATH data_socket=$DATA_SOCKET_PATH"
      return 0
    fi
    echo "daemon_status=degraded pid=$pid control_socket_exists=$([ -S "$CONTROL_SOCKET_PATH" ] && echo 1 || echo 0) data_socket_exists=$([ -S "$DATA_SOCKET_PATH" ] && echo 1 || echo 0)"
    return 1
  fi

  echo "daemon_status=stopped"
  return 2
}

daemon_stop_impl() {
  CONTROL_SOCKET_PATH="$(control_socket_path)"
  DATA_SOCKET_PATH="$(data_socket_path)"
  PID_FILE="${DSAPI_PID_FILE:-artifacts/run/dsapid.pid}"
  DSAPICTL_BIN="$(release_bin dsapictl)"
  DSAPID_BIN="$(release_bin dsapid)"

  touch_stop_impl >/dev/null 2>&1 || true

  if [ -S "$CONTROL_SOCKET_PATH" ] && [ -x "$DSAPICTL_BIN" ]; then
    "$DSAPICTL_BIN" --socket "$CONTROL_SOCKET_PATH" SHUTDOWN >/dev/null 2>&1 || true
  fi

  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
      if pid_cmdline_contains "$pid" "$DSAPID_BIN"; then
        kill "$pid" >/dev/null 2>&1 || true
        sleep 0.2
        kill -KILL "$pid" >/dev/null 2>&1 || true
      else
        echo "daemon_warn=pid_cmdline_mismatch pid=$pid expected='$DSAPID_BIN'"
      fi
    fi
    rm -f "$PID_FILE"
  fi

  rm -f "$CONTROL_SOCKET_PATH" "$DATA_SOCKET_PATH"
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
