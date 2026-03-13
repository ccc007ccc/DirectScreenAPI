#!/system/bin/sh

CAP_ID="dsapi.demo.touch_ui"
CAP_NAME="Touch UI Demo"
CAP_KIND="demo"
CAP_DESC="GPU 触摸窗口演示 capability（触摸 + blur + 横竖屏），委托模块 action 启停。"

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
  echo "state_dir=$DSAPI_MODULE_STATE_DIR"
  /system/bin/sh "$DSAPI_MODULE_DIR/actions/status.sh"
}
