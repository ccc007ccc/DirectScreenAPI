#!/system/bin/sh

CAP_ID="test.touch_demo"
CAP_NAME="Touch Demo Test"
CAP_KIND="test"
CAP_DESC="触摸窗口测试 capability，委托模块 action 进行启停。"

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
