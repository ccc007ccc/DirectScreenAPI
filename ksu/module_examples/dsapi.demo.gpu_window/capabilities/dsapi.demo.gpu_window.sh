#!/system/bin/sh

CAP_ID="dsapi.demo.gpu_window"
CAP_NAME="GPU Window Demo"
CAP_KIND="demo"
CAP_DESC="GPU 窗口演示 capability，通过 Core(Binder) 直连控制 demo。"

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
