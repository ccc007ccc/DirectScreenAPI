#!/system/bin/sh

CAP_ID="dsapi.demo.hello_consumer"
CAP_NAME="Hello Consumer Demo"
CAP_KIND="demo"
CAP_DESC="模块即接口：Consumer 通过 ServiceRegistry 获取 Binder 并调用。"

cap_start() {
  /system/bin/sh "$DSAPI_MODULE_DIR/actions/run.sh"
}

cap_stop() {
  echo "state=stopped pid=- reason=instant_action"
}

cap_status() {
  echo "state=stopped pid=- reason=instant_action"
}

cap_detail() {
  echo "module_id=$DSAPI_MODULE_ID"
  echo "module_dir=$DSAPI_MODULE_DIR"
  echo "state_dir=$DSAPI_MODULE_STATE_DIR"
}
