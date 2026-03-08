#!/system/bin/sh

CAP_ID="inject.ime"
CAP_NAME="DSAPI IME Proxy Control"
CAP_KIND="inject"
CAP_DESC="通过 Manager 透明输入代理 Activity 向 DSAPI 注入键盘事件。"

cap_start() {
  /system/bin/sh "$DSAPI_MODULE_DIR/actions/start.sh"
}

cap_stop() {
  /system/bin/sh "$DSAPI_MODULE_DIR/actions/stop.sh"
}

cap_status() {
  /system/bin/sh "$DSAPI_MODULE_DIR/actions/status.sh"
}

cap_detail() {
  echo "module_id=$DSAPI_MODULE_ID"
  echo "module_dir=$DSAPI_MODULE_DIR"
  /system/bin/sh "$DSAPI_MODULE_DIR/actions/status.sh"
}
