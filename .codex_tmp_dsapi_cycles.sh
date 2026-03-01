cd /data/data/com.termux/files/home/DirectScreenAPI || exit 1

pkill -f 'target/release/dsapid' || true
pkill -f 'AndroidAdapterMain present-loop' || true
pkill -f 'dsapi.sh presenter run' || true
sleep 0.5

DSAPI_RUN_AS_ROOT=1 DSAPI_PRESENTER_PRECHECK=0 DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_APP_PROCESS_BIN=/system/bin/app_process64 ./scripts/dsapi.sh daemon start

# 连续 3 次 start/stop
for i in 1 2 3; do
  echo "=== cycle $i start ==="
  DSAPI_RUN_AS_ROOT=1 DSAPI_PRESENTER_PRECHECK=0 DSAPI_ANDROID_OUT_DIR=artifacts/android_user DSAPI_APP_PROCESS_BIN=/system/bin/app_process64 DSAPI_PRESENTER_LOG_FILE=artifacts/run/presenter_cycle_$i.log ./scripts/dsapi.sh presenter start || true
  sleep 1
  echo "-- processes after start --"
  ps -ef | grep -E 'AndroidAdapterMain|dsapi.sh presenter run|app_process64' | grep -v grep || true

  DSAPI_RUN_AS_ROOT=1 ./scripts/dsapi.sh presenter stop || true
  sleep 1
  echo "-- processes after stop --"
  ps -ef | grep -E 'AndroidAdapterMain|dsapi.sh presenter run|app_process64' | grep -v grep || true
  echo "=== cycle $i end ==="
done

# 看 daemon 连接告警
rg -n "too_many_connections|dispatch_queue_busy" artifacts/run/dsapid.log | tail -n 120 || true

./scripts/dsapi.sh daemon stop || true
