#!/system/bin/sh

CAP_ID="dsapi.demo.hello_provider"
CAP_NAME="Hello Provider Demo"
CAP_KIND="demo"
CAP_DESC="模块即接口：Provider 注册 Binder 服务到 Core ServiceRegistry。"

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
