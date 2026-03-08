#!/system/bin/sh

CAP_ID="inject.window"
CAP_NAME="Window Inject Bridge"
CAP_KIND="inject"
CAP_DESC="窗口注入能力占位模块；后续接入 Zygisk/系统进程注入。"

state_file() {
  echo "$DSAPI_STATE_DIR/inject.window.enabled"
}

cap_start() {
  echo 1 >"$(state_file)"
  echo "state=missing pid=- reason=not_implemented"
}

cap_stop() {
  rm -f "$(state_file)"
  echo "state=stopped pid=- reason=not_running"
}

cap_status() {
  if [ -f "$(state_file)" ]; then
    echo "state=missing pid=- reason=not_implemented"
  else
    echo "state=stopped pid=- reason=disabled"
  fi
}

cap_detail() {
  echo "impl=placeholder"
  echo "target=zygisk_or_lsposed_injection"
  echo "notes=reserved_for_window_layer_and_filter_host"
}
