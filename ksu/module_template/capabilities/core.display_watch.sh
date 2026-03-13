#!/system/bin/sh

CAP_ID="core.display_watch"
CAP_NAME="Display State Watcher"
CAP_KIND="core"
CAP_DESC="监听显示尺寸/旋转变化并同步 DISPLAY_SET 到 dsapid，保证触摸坐标与窗口布局在横竖屏切换时自动更新。"

DISPLAY_WATCH_PID_FILE="$DSAPI_RUN_DIR/display_watch.pid"
DISPLAY_WATCH_LOG_FILE="$DSAPI_LOG_DIR/display_watch.log"

cap_start() {
  dsapi_adapter_dex="${DSAPI_ACTIVE_ADAPTER_DEX:-}"
  dsapi_app_process="${DSAPI_APP_PROCESS_BIN:-}"
  dsapi_socket="${DSAPI_DAEMON_SOCKET:-}"
  dsapi_main_class="org.directscreenapi.adapter.AndroidAdapterMain"

  if [ -z "$dsapi_socket" ]; then
    echo "state=error pid=- reason=daemon_socket_missing"
    return 1
  fi
  if [ -z "$dsapi_adapter_dex" ] || [ ! -f "$dsapi_adapter_dex" ]; then
    echo "state=error pid=- reason=adapter_dex_missing"
    return 1
  fi
  if [ -z "$dsapi_app_process" ] || [ ! -x "$dsapi_app_process" ]; then
    echo "state=error pid=- reason=app_process_missing"
    return 1
  fi

  if [ -f "$DISPLAY_WATCH_PID_FILE" ]; then
    dsapi_pid="$(cat "$DISPLAY_WATCH_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      echo "state=running pid=$dsapi_pid reason=-"
      return 0
    fi
    rm -f "$DISPLAY_WATCH_PID_FILE"
  fi

  mkdir -p "$DSAPI_RUN_DIR" "$DSAPI_LOG_DIR"
  rm -f "$DISPLAY_WATCH_LOG_FILE"

  CLASSPATH="$dsapi_adapter_dex" nohup "$dsapi_app_process" /system/bin --nice-name=DSAPIDisplayWatch "$dsapi_main_class" \
    display-watch "$dsapi_socket" >>"$DISPLAY_WATCH_LOG_FILE" 2>&1 &
  dsapi_pid="$!"
  echo "$dsapi_pid" >"$DISPLAY_WATCH_PID_FILE"
  sleep 0.08
  if ! dsapi_is_running "$dsapi_pid"; then
    rm -f "$DISPLAY_WATCH_PID_FILE"
    echo "state=error pid=- reason=start_failed"
    return 1
  fi
  echo "state=running pid=$dsapi_pid reason=-"
}

cap_stop() {
  dsapi_stop_pidfile_process "$DISPLAY_WATCH_PID_FILE" "DSAPIDisplayWatch"
  echo "state=stopped pid=- reason=manual_stop"
}

cap_status() {
  if [ -f "$DISPLAY_WATCH_PID_FILE" ]; then
    dsapi_pid="$(cat "$DISPLAY_WATCH_PID_FILE" 2>/dev/null || true)"
    if dsapi_is_running "$dsapi_pid"; then
      echo "state=running pid=$dsapi_pid reason=-"
      return 0
    fi
    rm -f "$DISPLAY_WATCH_PID_FILE"
    echo "state=stopped pid=- reason=stale_pid_cleaned"
    return 0
  fi
  echo "state=stopped pid=- reason=not_running"
}

cap_detail() {
  echo "pid_file=$DISPLAY_WATCH_PID_FILE"
  echo "log_file=$DISPLAY_WATCH_LOG_FILE"
  echo "socket=$DSAPI_DAEMON_SOCKET"
  echo "adapter_dex=$DSAPI_ACTIVE_ADAPTER_DEX"
}

