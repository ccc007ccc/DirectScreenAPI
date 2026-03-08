#!/system/bin/sh

CAP_ID="core.daemon"
CAP_NAME="DSAPI Core Daemon"
CAP_KIND="core"
CAP_DESC="DSAPI 核心守护进程，负责统一控制协议与帧交换。"

cap_start() {
  dsapi_enabled_set 1
  if dsapi_daemon_start; then
    dsapi_daemon_status_kv
    return 0
  fi
  echo "state=error pid=- reason=start_failed"
  return 1
}

cap_stop() {
  dsapi_enabled_set 0
  dsapi_daemon_stop
  echo "state=stopped pid=- reason=manual_stop"
}

cap_status() {
  dsapi_daemon_status_kv
}

cap_detail() {
  echo "pid_file=$DSAPI_DAEMON_PID_FILE"
  echo "active_release=$(dsapi_runtime_active_release_id)"
  echo "active_dsapid=$DSAPI_ACTIVE_DSAPID"
  echo "active_dsapictl=$DSAPI_ACTIVE_DSAPICTL"
}
