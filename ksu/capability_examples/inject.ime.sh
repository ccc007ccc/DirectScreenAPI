#!/system/bin/sh

CAP_ID="inject.ime"
CAP_NAME="IME Inject Bridge"
CAP_KIND="inject"
CAP_DESC="输入法注入能力占位模块；后续实现 InputConnection 桥接。"

state_file() {
  echo "$DSAPI_STATE_DIR/inject.ime.enabled"
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
  echo "target=inputconnection_bridge"
  echo "notes=reserved_for_system_ime_focus_and_text_commit"
}
