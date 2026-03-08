screen_kill_pid_if_running() {
  target_pid="$1"
  if [ -z "$target_pid" ] || ! pid_is_running "$target_pid"; then
    return 0
  fi
  kill "$target_pid" >/dev/null 2>&1 || true
  sleep 0.1
  kill -KILL "$target_pid" >/dev/null 2>&1 || true
}

screen_collect_nonroot_pids() {
  pattern="$1"
  ps -ef | awk -v p="$pattern" '$0 ~ p && $2 ~ /^[0-9]+$/ { print $2 }'
}

screen_collect_root_pids() {
  pattern="$1"
  su -c "ps -ef | awk -v p='${pattern}' '\$0 ~ p && \$2 ~ /^[0-9]+$/ { print \$2 }'" 2>/dev/null || true
}

screen_cleanup_orphans_impl() {
  RUN_AS_ROOT="${1:-1}"

  for pid in $(screen_collect_nonroot_pids "dsapi.sh screen run")
  do
    screen_kill_pid_if_running "$pid"
  done

  for pid in $(screen_collect_nonroot_pids "AndroidAdapterMain.*screen-stream")
  do
    screen_kill_pid_if_running "$pid"
  done

  if [ "$RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
    for pid in $(screen_collect_root_pids "AndroidAdapterMain.*screen-stream")
    do
      su_exec kill "$pid" >/dev/null 2>&1 || true
      sleep 0.1
      su_exec kill -KILL "$pid" >/dev/null 2>&1 || true
    done
  fi
}

screen_start_impl() {
  SCREEN_PID_FILE="${DSAPI_SCREEN_PID_FILE:-artifacts/run/dsapi_screen.pid}"
  SCREEN_LOG_FILE="${DSAPI_SCREEN_LOG_FILE:-artifacts/run/dsapi_screen.log}"
  SCREEN_USE_SETSID="${DSAPI_SCREEN_USE_SETSID:-1}"
  SCREEN_RUN_AS_ROOT="${DSAPI_SCREEN_RUN_AS_ROOT:-${DSAPI_RUN_AS_ROOT:-1}}"

  mkdir -p "$(dirname "$SCREEN_PID_FILE")"
  mkdir -p "$(dirname "$SCREEN_LOG_FILE")"

  if ! ( : >>"$SCREEN_LOG_FILE" ) 2>/dev/null; then
    if [ -n "${DSAPI_SCREEN_LOG_FILE:-}" ]; then
      echo "screen_stream_error=log_file_not_writable path=$SCREEN_LOG_FILE"
      return 2
    fi
    fallback_log="artifacts/run/dsapi_screen_user.log"
    mkdir -p "$(dirname "$fallback_log")"
    if ! ( : >>"$fallback_log" ) 2>/dev/null; then
      echo "screen_stream_error=log_file_not_writable path=$SCREEN_LOG_FILE fallback=$fallback_log"
      return 2
    fi
    echo "screen_stream_warn=log_file_not_writable path=$SCREEN_LOG_FILE fallback=$fallback_log"
    SCREEN_LOG_FILE="$fallback_log"
  fi
  LOG_START_LINE=0
  if [ -f "$SCREEN_LOG_FILE" ]; then
    LOG_START_LINE="$(wc -l < "$SCREEN_LOG_FILE" 2>/dev/null || echo 0)"
  fi

  screen_cleanup_orphans_impl "$SCREEN_RUN_AS_ROOT"

  if [ -f "$SCREEN_PID_FILE" ]; then
    old_pid="$(cat "$SCREEN_PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && pid_is_running "$old_pid"; then
      echo "screen_stream_status=already_running pid=$old_pid"
      return 0
    fi
    rm -f "$SCREEN_PID_FILE"
  fi

  if ! daemon_status_impl >/dev/null 2>&1; then
    daemon_start_impl >/dev/null
  fi
  android_sync_display_impl >/dev/null 2>&1 || true

  if [ "$SCREEN_USE_SETSID" = "1" ] && command -v setsid >/dev/null 2>&1; then
    nohup setsid ./scripts/dsapi.sh screen run >>"$SCREEN_LOG_FILE" 2>&1 < /dev/null &
  else
    nohup ./scripts/dsapi.sh screen run >>"$SCREEN_LOG_FILE" 2>&1 < /dev/null &
  fi
  screen_pid="$!"
  echo "$screen_pid" > "$SCREEN_PID_FILE"

  sleep 1
  if pid_is_running "$screen_pid"; then
    START_MARKER_OK=0
    START_MARKER_FAIL=0
    i=0
    while [ "$i" -lt 40 ]
    do
      NEW_LOGS="$(tail -n +"$((LOG_START_LINE + 1))" "$SCREEN_LOG_FILE" 2>/dev/null || true)"
      if printf '%s\n' "$NEW_LOGS" | grep -q "screen_stream_status=started target_fps="; then
        START_MARKER_OK=1
        break
      fi
      if printf '%s\n' "$NEW_LOGS" | grep -q "screen_stream_status=failed\\|screen_stream_error="; then
        START_MARKER_FAIL=1
        break
      fi
      i=$((i + 1))
      sleep 0.1
    done

    if [ "$START_MARKER_OK" = "1" ]; then
      echo "screen_stream_status=started pid=$screen_pid log=$SCREEN_LOG_FILE"
      return 0
    fi
    if [ "$START_MARKER_FAIL" = "1" ]; then
      screen_kill_pid_if_running "$screen_pid"
      rm -f "$SCREEN_PID_FILE"
      echo "screen_stream_status=failed_start marker=failed_in_log"
      if [ -f "$SCREEN_LOG_FILE" ]; then
        echo "screen_stream_last_log_start"
        tail -n 40 "$SCREEN_LOG_FILE"
        echo "screen_stream_last_log_end"
      fi
      return 2
    fi

    echo "screen_stream_warn=start_marker_timeout pid=$screen_pid log=$SCREEN_LOG_FILE"
    echo "screen_stream_status=started pid=$screen_pid log=$SCREEN_LOG_FILE"
    return 0
  fi

  echo "screen_stream_status=failed_start"
  if [ -f "$SCREEN_LOG_FILE" ]; then
    echo "screen_stream_last_log_start"
    tail -n 30 "$SCREEN_LOG_FILE"
    echo "screen_stream_last_log_end"
  fi
  return 2
}

screen_stop_impl() {
  SCREEN_PID_FILE="${DSAPI_SCREEN_PID_FILE:-artifacts/run/dsapi_screen.pid}"
  SCREEN_RUN_AS_ROOT="${DSAPI_SCREEN_RUN_AS_ROOT:-${DSAPI_RUN_AS_ROOT:-1}}"

  if [ -f "$SCREEN_PID_FILE" ]; then
    screen_pid="$(cat "$SCREEN_PID_FILE" 2>/dev/null || true)"
    if [ -n "$screen_pid" ] && pid_is_running "$screen_pid"; then
      if pid_cmdline_contains "$screen_pid" "dsapi.sh screen run"; then
        screen_kill_pid_if_running "$screen_pid"
        if [ "$SCREEN_RUN_AS_ROOT" = "1" ] && command -v su >/dev/null 2>&1; then
          su_exec kill -KILL "$screen_pid" >/dev/null 2>&1 || true
        fi
      else
        echo "screen_stream_warn=pid_cmdline_mismatch pid=$screen_pid expected='dsapi.sh screen run'"
      fi
    fi
    rm -f "$SCREEN_PID_FILE"
  fi

  screen_cleanup_orphans_impl "$SCREEN_RUN_AS_ROOT"
  echo "screen_stream_status=stopped"
}

screen_status_impl() {
  SCREEN_PID_FILE="${DSAPI_SCREEN_PID_FILE:-artifacts/run/dsapi_screen.pid}"
  if [ -f "$SCREEN_PID_FILE" ]; then
    pid="$(cat "$SCREEN_PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ] && pid_is_running "$pid"; then
      echo "screen_stream_status=running pid=$pid"
      return 0
    fi
  fi
  echo "screen_stream_status=stopped"
  return 2
}

screen_run_impl() {
  CONTROL_SOCKET_PATH="$(control_socket_path)"
  OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/ksu_module/android_adapter}"
  DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
  MAIN_CLASS="org.directscreenapi.adapter.AndroidAdapterMain"
  APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-}"
  RUN_AS_ROOT="${DSAPI_SCREEN_RUN_AS_ROOT:-${DSAPI_RUN_AS_ROOT:-1}}"
  TARGET_FPS="${DSAPI_SCREEN_TARGET_FPS:-60}"
  SUBMIT_MODE="${DSAPI_SCREEN_SUBMIT_MODE:-dmabuf}"
  AUTO_REBUILD="${DSAPI_SCREEN_AUTO_REBUILD:-1}"
  PRECHECK="${DSAPI_SCREEN_PRECHECK:-}"
  if [ -z "$PRECHECK" ]; then
    if [ "$RUN_AS_ROOT" = "1" ]; then
      PRECHECK="0"
    else
      PRECHECK="1"
    fi
  fi

  if [ -z "${DSAPI_ANDROID_OUT_DIR:-}" ]; then
    if { [ -d "$OUT_DIR" ] && [ ! -w "$OUT_DIR" ]; } \
      || { [ -d "$OUT_DIR/classes" ] && [ ! -w "$OUT_DIR/classes" ]; }; then
      OUT_DIR="artifacts/ksu_module/android_adapter_user"
      DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
      echo "screen_stream_warn=android_out_dir_not_writable fallback_out_dir=$OUT_DIR"
    fi
  fi

  if [ ! -S "$CONTROL_SOCKET_PATH" ]; then
    echo "screen_stream_error=daemon_control_socket_missing socket=$CONTROL_SOCKET_PATH"
    return 2
  fi

  case "$CONTROL_SOCKET_PATH" in
    /*) CONTROL_SOCKET_ABS="$CONTROL_SOCKET_PATH" ;;
    *) CONTROL_SOCKET_ABS="$ROOT_DIR/$CONTROL_SOCKET_PATH" ;;
  esac

  if [ "$AUTO_REBUILD" = "1" ] || [ ! -f "$DEX_JAR" ]; then
    if ! DSAPI_ANDROID_BUILD_MODE=presenter ./scripts/build_android_adapter.sh >/dev/null; then
      if [ ! -f "$DEX_JAR" ]; then
        echo "screen_stream_error=android_adapter_build_failed_no_cached_dex path=$DEX_JAR"
        return 2
      fi
      echo "screen_stream_warn=android_adapter_build_failed_using_cached_dex path=$DEX_JAR"
    fi
  fi

  case "$DEX_JAR" in
    /*) DEX_JAR_ABS="$DEX_JAR" ;;
    *) DEX_JAR_ABS="$ROOT_DIR/$DEX_JAR" ;;
  esac
  HWBUFFER_BRIDGE_LIB="${DSAPI_HWBUFFER_BRIDGE_LIB:-$OUT_DIR/libdsapi_hwbuffer_bridge.so}"
  case "$HWBUFFER_BRIDGE_LIB" in
    /*) HWBUFFER_BRIDGE_LIB_ABS="$HWBUFFER_BRIDGE_LIB" ;;
    *) HWBUFFER_BRIDGE_LIB_ABS="$ROOT_DIR/$HWBUFFER_BRIDGE_LIB" ;;
  esac
  if [ "$SUBMIT_MODE" = "dmabuf" ] && [ ! -f "$HWBUFFER_BRIDGE_LIB_ABS" ]; then
    echo "screen_stream_error=hwbuffer_bridge_missing path=$HWBUFFER_BRIDGE_LIB_ABS"
    return 2
  fi

  if [ -n "$APP_PROCESS_BIN" ] && ! command -v "$APP_PROCESS_BIN" >/dev/null 2>&1 && [ ! -x "$APP_PROCESS_BIN" ]; then
    echo "screen_stream_error=app_process_not_found path=$APP_PROCESS_BIN"
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
        echo "screen_stream_warn=app_process_candidate_rejected path=$c"
      else
        APP_PROCESS_BIN="$c"
        break
      fi
    done

    if [ -z "$APP_PROCESS_BIN" ]; then
      echo "screen_stream_error=app_process_not_found_or_unusable"
      return 2
    fi
  fi

  echo "screen_stream_exec app_process=$APP_PROCESS_BIN dex=$DEX_JAR_ABS target_fps=$TARGET_FPS submit_mode=$SUBMIT_MODE"

  if [ "$PRECHECK" = "1" ]; then
    if ! CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" display-line >/dev/null 2>&1; then
      echo "screen_stream_error=app_process_precheck_failed app_process=$APP_PROCESS_BIN dex=$DEX_JAR_ABS"
      return 2
    fi
  fi

  if [ "$RUN_AS_ROOT" = "1" ]; then
    if ! command -v su >/dev/null 2>&1; then
      echo "screen_stream_error=su_not_found"
      return 2
    fi
    su_exec env \
      "CLASSPATH=$DEX_JAR_ABS" \
      "DSAPI_SCREEN_SUBMIT_MODE=$SUBMIT_MODE" \
      "DSAPI_HWBUFFER_BRIDGE_LIB=$HWBUFFER_BRIDGE_LIB_ABS" \
      "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" screen-stream \
      "$CONTROL_SOCKET_ABS" "$TARGET_FPS"
    return 0
  fi

  DSAPI_SCREEN_SUBMIT_MODE="$SUBMIT_MODE" \
  DSAPI_HWBUFFER_BRIDGE_LIB="$HWBUFFER_BRIDGE_LIB_ABS" \
  CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" screen-stream \
    "$CONTROL_SOCKET_PATH" "$TARGET_FPS"
}

screen_bench_impl() {
  SAMPLES="${1:-10}"
  case "$SAMPLES" in
    ''|*[!0-9]*)
      echo "screen_stream_bench_error=samples_invalid value=$SAMPLES"
      return 2
      ;;
  esac
  if [ "$SAMPLES" -le 0 ]; then
    echo "screen_stream_bench_error=samples_invalid value=$SAMPLES"
    return 2
  fi

  TMP_DIR="artifacts/bench/screen"
  mkdir -p "$TMP_DIR"

  KEEP_RUNNING=1
  if ! screen_status_impl >/dev/null 2>&1; then
    KEEP_RUNNING=0
    screen_start_impl >/dev/null
    sleep 1
  fi

  FRAME_PATH="$TMP_DIR/frame_latest.rgba"
  ./scripts/dsapi.sh frame pull "$FRAME_PATH" >/dev/null 2>&1 || {
    echo "screen_stream_bench_error=frame_pull_warmup_failed"
    [ "$KEEP_RUNNING" -eq 1 ] || screen_stop_impl >/dev/null 2>&1 || true
    return 2
  }

  dsapi_total_ns=0
  i=0
  while [ "$i" -lt "$SAMPLES" ]
  do
    t0="$(date +%s%N)"
    ./scripts/dsapi.sh frame pull "$FRAME_PATH" >/dev/null 2>&1 || {
      echo "screen_stream_bench_error=frame_pull_failed sample=$i"
      [ "$KEEP_RUNNING" -eq 1 ] || screen_stop_impl >/dev/null 2>&1 || true
      return 2
    }
    t1="$(date +%s%N)"
    dsapi_total_ns=$((dsapi_total_ns + (t1 - t0)))
    i=$((i + 1))
  done

  native_total_ns=0
  i=0
  while [ "$i" -lt "$SAMPLES" ]
  do
    t0="$(date +%s%N)"
    if ! su_exec screencap >/dev/null 2>&1; then
      echo "screen_stream_bench_error=screencap_failed sample=$i"
      [ "$KEEP_RUNNING" -eq 1 ] || screen_stop_impl >/dev/null 2>&1 || true
      return 2
    fi
    t1="$(date +%s%N)"
    native_total_ns=$((native_total_ns + (t1 - t0)))
    i=$((i + 1))
  done

  dsapi_avg_ms="$(awk -v total="$dsapi_total_ns" -v n="$SAMPLES" 'BEGIN{printf "%.2f", (total / n) / 1000000.0}')"
  native_avg_ms="$(awk -v total="$native_total_ns" -v n="$SAMPLES" 'BEGIN{printf "%.2f", (total / n) / 1000000.0}')"
  speedup="$(awk -v d="$dsapi_avg_ms" -v n="$native_avg_ms" 'BEGIN{ if (d <= 0.0) { printf "0.00"; } else { printf "%.2f", n / d; } }')"

  echo "screen_stream_bench=ok samples=$SAMPLES dsapi_avg_ms=$dsapi_avg_ms native_screencap_avg_ms=$native_avg_ms native_over_dsapi_ratio=$speedup"

  [ "$KEEP_RUNNING" -eq 1 ] || screen_stop_impl >/dev/null 2>&1 || true
}
