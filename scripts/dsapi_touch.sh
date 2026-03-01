touch_stop_impl() {
  CONTROL_SOCKET_PATH="$(control_socket_path)"
  DATA_SOCKET_PATH="$(data_socket_path)"
  PID_FILE="${DSAPI_TOUCH_BRIDGE_PID_FILE:-artifacts/run/dsapi_touch_bridge.pid}"
  SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"
  DSAPICTL_BIN="$(release_bin dsapictl)"

  if [ "$SUPERVISE_INPUT" = "1" ]; then
    echo "touch_bridge_status=managed_by_daemon supervise=1 action=use_daemon_stop"
    return 0
  fi

  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
      if pid_cmdline_contains "$pid" "dsapi.sh touch run"; then
        kill -- "-$pid" >/dev/null 2>&1 || kill "$pid" >/dev/null 2>&1 || true
        sleep 0.2
        kill -KILL -- "-$pid" >/dev/null 2>&1 || kill -KILL "$pid" >/dev/null 2>&1 || true
      else
        echo "touch_bridge_warn=pid_cmdline_mismatch pid=$pid expected='dsapi.sh touch run'"
      fi
    fi
    rm -f "$PID_FILE"
  fi

  if [ -x "$DSAPICTL_BIN" ] && [ -S "$CONTROL_SOCKET_PATH" ]; then
    "$DSAPICTL_BIN" --socket "$CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
  fi

  echo "touch_bridge_status=stopped"
}

touch_run_is_uint() {
  case "$1" in
    ''|*[!0-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

touch_run_probe_getevent() {
  if [ "$TR_RUN_AS_ROOT" = "1" ]; then
    su_exec "$TR_GETEVENT_BIN" "$@"
  else
    "$TR_GETEVENT_BIN" "$@"
  fi
}

touch_run_detect_touch_device() {
  touch_run_probe_getevent -pl | awk '
    /^add device/ {
      dev = $4;
      has_x = 0;
      has_y = 0;
      has_tid = 0;
    }
    /ABS_MT_POSITION_X/ { has_x = 1; }
    /ABS_MT_POSITION_Y/ { has_y = 1; }
    /ABS_MT_TRACKING_ID/ { has_tid = 1; }
    /^  input props:/ {
      if (has_x && has_y && has_tid) {
        print dev;
        exit 0;
      }
    }
  '
}

touch_run_extract_abs_max() {
  axis="$1"
  caps="$2"
  printf '%s\n' "$caps" | awk -v axis="$axis" '
    $0 ~ axis {
      for (i = 1; i <= NF; i++) {
        if ($i == "max") {
          v = $(i + 1);
          gsub(/[^0-9]/, "", v);
          print v;
          exit 0;
        }
      }
    }
  '
}

touch_run_read_display_line() {
  "$TR_DSAPICTL_BIN" --socket "$TR_CONTROL_SOCKET_PATH" DISPLAY_GET 2>/dev/null || true
}

touch_run_parse_display() {
  line="$1"
  set -- $line
  if [ "$#" -lt 6 ] || [ "$1" != "OK" ]; then
    return 1
  fi
  w="$2"
  h="$3"
  rot="$6"
  if ! touch_run_is_uint "$w" || ! touch_run_is_uint "$h" || ! touch_run_is_uint "$rot"; then
    return 1
  fi

  TR_DISPLAY_W="$w"
  TR_DISPLAY_H="$h"
  TR_DISPLAY_ROT="$rot"
  return 0
}

touch_run_maybe_sync_display() {
  if [ "$TR_AUTO_SYNC_DISPLAY" != "1" ]; then
    return 0
  fi
  now="$(date +%s)"
  if ! touch_run_is_uint "$TR_SYNC_DISPLAY_EVERY_SEC" || [ "$TR_SYNC_DISPLAY_EVERY_SEC" -lt 1 ]; then
    TR_SYNC_DISPLAY_EVERY_SEC=1
  fi
  if [ "$TR_LAST_SYNC_TS" -eq 0 ] || [ $((now - TR_LAST_SYNC_TS)) -ge "$TR_SYNC_DISPLAY_EVERY_SEC" ]; then
    ./scripts/dsapi.sh android sync-display >/dev/null 2>&1 || true
    TR_LAST_SYNC_TS="$now"
  fi
}

touch_run_stop_pipeline() {
  if [ -n "$TR_PIPE_PID" ] && kill -0 "$TR_PIPE_PID" >/dev/null 2>&1; then
    kill "$TR_PIPE_PID" >/dev/null 2>&1 || true
    wait "$TR_PIPE_PID" >/dev/null 2>&1 || true
  fi
  TR_PIPE_PID=""
}

touch_run_start_pipeline() {
  w="$1"
  h="$2"
  rot="$3"
  if [ "$TR_RUN_AS_ROOT" = "1" ]; then
    su_exec "$TR_DSAPIINPUT_BIN" \
      --control-socket "$TR_CONTROL_SOCKET_PATH" \
      --data-socket "$TR_DATA_SOCKET_PATH" \
      --device "$TR_TOUCH_DEVICE" \
      --max-x "$TR_MAX_X" \
      --max-y "$TR_MAX_Y" \
      --width "$w" \
      --height "$h" \
      --rotation "$rot" \
      --quiet &
  else
    "$TR_DSAPIINPUT_BIN" \
      --control-socket "$TR_CONTROL_SOCKET_PATH" \
      --data-socket "$TR_DATA_SOCKET_PATH" \
      --device "$TR_TOUCH_DEVICE" \
      --max-x "$TR_MAX_X" \
      --max-y "$TR_MAX_Y" \
      --width "$w" \
      --height "$h" \
      --rotation "$rot" \
      --quiet &
  fi
  TR_PIPE_PID="$!"
}

touch_run_cleanup() {
  touch_run_stop_pipeline
  if [ -x "$TR_DSAPICTL_BIN" ] && [ -S "$TR_CONTROL_SOCKET_PATH" ]; then
    "$TR_DSAPICTL_BIN" --socket "$TR_CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
  fi
}

touch_run_impl() {
  TR_CONTROL_SOCKET_PATH="$(control_socket_path)"
  TR_DATA_SOCKET_PATH="$(data_socket_path)"
  TR_GETEVENT_BIN="${DSAPI_GETEVENT_BIN:-/system/bin/getevent}"
  TR_RUN_AS_ROOT="${DSAPI_TOUCH_RUN_AS_ROOT:-1}"
  TR_TOUCH_DEVICE="${DSAPI_TOUCH_DEVICE:-}"
  TR_MONITOR_INTERVAL_SEC="${DSAPI_TOUCH_MONITOR_INTERVAL_SEC:-1}"
  TR_AUTO_SYNC_DISPLAY="${DSAPI_TOUCH_AUTO_SYNC_DISPLAY:-1}"
  TR_SYNC_DISPLAY_EVERY_SEC="${DSAPI_TOUCH_SYNC_DISPLAY_EVERY_SEC:-1}"
  TR_DSAPIINPUT_BIN="$(release_bin dsapiinput)"
  TR_DSAPICTL_BIN="$(release_bin dsapictl)"
  TR_PIPE_PID=""
  TR_LAST_SYNC_TS=0

  trap touch_run_cleanup EXIT INT TERM

  if [ ! -x "$TR_DSAPIINPUT_BIN" ] || [ ! -x "$TR_DSAPICTL_BIN" ]; then
    ./scripts/build_core.sh >/dev/null
  fi

  if [ ! -S "$TR_CONTROL_SOCKET_PATH" ]; then
    echo "touch_bridge_error=daemon_control_socket_missing socket=$TR_CONTROL_SOCKET_PATH"
    return 2
  fi
  if [ ! -S "$TR_DATA_SOCKET_PATH" ]; then
    echo "touch_bridge_error=daemon_data_socket_missing socket=$TR_DATA_SOCKET_PATH"
    return 2
  fi

  if [ -z "$TR_TOUCH_DEVICE" ]; then
    TR_TOUCH_DEVICE="$(touch_run_detect_touch_device || true)"
  fi
  if [ -z "$TR_TOUCH_DEVICE" ]; then
    echo "touch_bridge_error=touch_device_not_found"
    return 4
  fi

  caps="$(touch_run_probe_getevent -pl "$TR_TOUCH_DEVICE")"
  TR_MAX_X="$(touch_run_extract_abs_max "ABS_MT_POSITION_X" "$caps")"
  TR_MAX_Y="$(touch_run_extract_abs_max "ABS_MT_POSITION_Y" "$caps")"
  if ! touch_run_is_uint "$TR_MAX_X" || ! touch_run_is_uint "$TR_MAX_Y"; then
    echo "touch_bridge_error=invalid_touch_capabilities device=$TR_TOUCH_DEVICE"
    return 5
  fi

  "$TR_DSAPICTL_BIN" --socket "$TR_CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true

  current_display_line=""
  while true; do
    touch_run_maybe_sync_display
    display_line="$(touch_run_read_display_line)"
    if ! touch_run_parse_display "$display_line"; then
      echo "touch_bridge_warn=display_unavailable line=$display_line"
      sleep "$TR_MONITOR_INTERVAL_SEC"
      continue
    fi

    if [ "$display_line" != "$current_display_line" ]; then
      if [ -n "$current_display_line" ]; then
        echo "touch_bridge_status=display_changed old='$current_display_line' new='$display_line'"
      fi
      current_display_line="$display_line"
      "$TR_DSAPICTL_BIN" --socket "$TR_CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
      touch_run_stop_pipeline
      touch_run_start_pipeline "$TR_DISPLAY_W" "$TR_DISPLAY_H" "$TR_DISPLAY_ROT"
      echo "touch_bridge_status=running device=$TR_TOUCH_DEVICE max_x=$TR_MAX_X max_y=$TR_MAX_Y display_w=$TR_DISPLAY_W display_h=$TR_DISPLAY_H rotation=$TR_DISPLAY_ROT"
    fi

    if [ -n "$TR_PIPE_PID" ] && ! kill -0 "$TR_PIPE_PID" >/dev/null 2>&1; then
      echo "touch_bridge_warn=pipeline_exited restart=1"
      touch_run_stop_pipeline
      touch_run_start_pipeline "$TR_DISPLAY_W" "$TR_DISPLAY_H" "$TR_DISPLAY_ROT"
    fi

    sleep "$TR_MONITOR_INTERVAL_SEC"
  done
}

touch_start_impl() {
  PID_FILE="${DSAPI_TOUCH_BRIDGE_PID_FILE:-artifacts/run/dsapi_touch_bridge.pid}"
  LOG_FILE="${DSAPI_TOUCH_BRIDGE_LOG_FILE:-artifacts/run/dsapi_touch_bridge.log}"
  SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"

  mkdir -p "$(dirname "$PID_FILE")"
  mkdir -p "$(dirname "$LOG_FILE")"

  if [ "$SUPERVISE_INPUT" = "1" ]; then
    daemon_start_impl >/dev/null
    echo "touch_bridge_status=managed_by_daemon supervise=1"
    return 0
  fi

  if [ -f "$PID_FILE" ]; then
    old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && kill -0 "$old_pid" >/dev/null 2>&1; then
      echo "touch_bridge_status=already_running pid=$old_pid"
      return 0
    fi
    rm -f "$PID_FILE"
  fi

  if ! daemon_status_impl >/dev/null 2>&1; then
    daemon_start_impl >/dev/null
  fi

  ./scripts/build_core.sh >/dev/null

  nohup setsid ./scripts/dsapi.sh touch run >>"$LOG_FILE" 2>&1 < /dev/null &
  pid="$!"
  echo "$pid" > "$PID_FILE"

  sleep 0.3
  if kill -0 "$pid" >/dev/null 2>&1; then
    echo "touch_bridge_status=started pid=$pid log=$LOG_FILE"
    return 0
  fi

  echo "touch_bridge_status=failed_start"
  return 2
}

touch_status_impl() {
  PID_FILE="${DSAPI_TOUCH_BRIDGE_PID_FILE:-artifacts/run/dsapi_touch_bridge.pid}"
  SUPERVISE_INPUT="${DSAPI_SUPERVISE_INPUT:-0}"

  if [ "$SUPERVISE_INPUT" = "1" ]; then
    if daemon_status_impl >/dev/null 2>&1; then
      echo "touch_bridge_status=managed_by_daemon supervise=1"
      return 0
    fi
    echo "touch_bridge_status=stopped supervise=1 daemon_stopped=1"
    return 2
  fi

  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && kill -0 "$pid" >/dev/null 2>&1; then
      echo "touch_bridge_status=running pid=$pid"
      return 0
    fi
  fi

  echo "touch_bridge_status=stopped"
  return 2
}
