#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'USAGE'
usage:
  ./scripts/dsapi_touch_demo_test.sh [demo_args...]

examples:
  ./scripts/dsapi_touch_demo_test.sh
  ./scripts/dsapi_touch_demo_test.sh --device /dev/input/event7
  ./scripts/dsapi_touch_demo_test.sh --no-touch-router --fps 30

env:
  DSAPI_DEMO_RUN_AS_ROOT=1|0      run demo as root (default: 1)
  DSAPI_DEMO_KEEP_SERVICES=1|0    keep daemon/presenter after demo exits (default: 0)
  DSAPI_DEMO_RUN_SECONDS=<n>      auto stop seconds if --run-seconds not provided (default: 12)
  DSAPI_DISPATCH_WORKERS=<n>      daemon dispatch workers (default: 8)
  DSAPI_PRESENTER_LOG_FILE=<path> presenter log file (default: artifacts/run/dsapi_presenter_user.log)
  DSAPI_APP_PROCESS_BIN=<path>    app_process binary (default: /system/bin/app_process64)
  DSAPI_ANDROID_OUT_DIR=<dir>     adapter output dir (default: artifacts/android_user)
USAGE
}

derive_data_socket_path() {
  case "$1" in
    *.sock) printf '%s\n' "${1%.sock}.data.sock" ;;
    *) printf '%s\n' "${1}.data" ;;
  esac
}

to_abs_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$ROOT_DIR" "$1" ;;
  esac
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\"'\"'/g")"
}

build_quoted_cmd() {
  out=""
  for arg in "$@"
  do
    q="$(shell_quote "$arg")"
    if [ -z "$out" ]; then
      out="$q"
    else
      out="$out $q"
    fi
  done
  printf '%s\n' "$out"
}

cleanup() {
  if [ "${DSAPI_DEMO_KEEP_SERVICES:-0}" = "1" ]; then
    return 0
  fi
  ./scripts/dsapi.sh presenter stop >/dev/null 2>&1 || true
  ./scripts/dsapi.sh daemon stop >/dev/null 2>&1 || true
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

trap cleanup EXIT INT TERM

DEMO_RUN_AS_ROOT="${DSAPI_DEMO_RUN_AS_ROOT:-1}"
PRESENTER_RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"
CONTROL_SOCKET="${DSAPI_CONTROL_SOCKET_PATH:-${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}}"
DATA_SOCKET="${DSAPI_DATA_SOCKET_PATH:-$(derive_data_socket_path "$CONTROL_SOCKET")}"
CONTROL_SOCKET_ABS="$(to_abs_path "$CONTROL_SOCKET")"
DATA_SOCKET_ABS="$(to_abs_path "$DATA_SOCKET")"

PRESENTER_LOG_FILE="${DSAPI_PRESENTER_LOG_FILE:-artifacts/run/dsapi_presenter_user.log}"
APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-/system/bin/app_process64}"
ANDROID_OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/android_user}"
DEMO_RUN_SECONDS="${DSAPI_DEMO_RUN_SECONDS:-12}"
DAEMON_DISPATCH_WORKERS="${DSAPI_DISPATCH_WORKERS:-8}"

mkdir -p "$(dirname "$PRESENTER_LOG_FILE")"
: >"$PRESENTER_LOG_FILE" 2>/dev/null || true

echo "touch_demo_test_status=boot root_dir=$ROOT_DIR"
echo "touch_demo_test_status=socket control=$CONTROL_SOCKET_ABS data=$DATA_SOCKET_ABS"

./scripts/dsapi.sh presenter stop >/dev/null 2>&1 || true
./scripts/dsapi.sh daemon stop >/dev/null 2>&1 || true

echo "touch_demo_test_status=build_demo"
cargo build --release --manifest-path core/rust/Cargo.toml --bin dsapi_touch_demo >/dev/null

echo "touch_demo_test_status=start_daemon"
DSAPI_DISPATCH_WORKERS="$DAEMON_DISPATCH_WORKERS" ./scripts/dsapi.sh daemon start
./scripts/dsapi.sh daemon status || true
./scripts/dsapi.sh android sync-display

echo "touch_demo_test_status=start_presenter"
DSAPI_RUN_AS_ROOT="$PRESENTER_RUN_AS_ROOT" \
DSAPI_PRESENTER_PRECHECK=0 \
DSAPI_ANDROID_OUT_DIR="$ANDROID_OUT_DIR" \
DSAPI_APP_PROCESS_BIN="$APP_PROCESS_BIN" \
DSAPI_PRESENTER_LOG_FILE="$PRESENTER_LOG_FILE" \
./scripts/dsapi.sh presenter start
./scripts/dsapi.sh presenter status || true

demo_bin="$ROOT_DIR/target/release/dsapi_touch_demo"
if [ ! -x "$demo_bin" ]; then
  echo "touch_demo_test_error=binary_missing path=$demo_bin"
  exit 2
fi

user_has_run_seconds=0
for token in "$@"
do
  if [ "$token" = "--run-seconds" ]; then
    user_has_run_seconds=1
    break
  fi
done
if [ "$user_has_run_seconds" = "0" ] && [ "$DEMO_RUN_SECONDS" != "0" ]; then
  set -- "$@" --run-seconds "$DEMO_RUN_SECONDS"
fi

set -- "$demo_bin" \
  --control-socket "$CONTROL_SOCKET_ABS" \
  --data-socket "$DATA_SOCKET_ABS" \
  "$@"

echo "touch_demo_test_status=run_demo run_as_root=$DEMO_RUN_AS_ROOT presenter_log=$PRESENTER_LOG_FILE"

if [ "$DEMO_RUN_AS_ROOT" = "1" ]; then
  if ! command -v su >/dev/null 2>&1; then
    echo "touch_demo_test_error=su_not_found"
    exit 3
  fi
  cmd="$(build_quoted_cmd "$@")"
  su -c "$cmd"
else
  "$@"
fi

if [ -f "$PRESENTER_LOG_FILE" ] && grep -q "presenter_status=failed" "$PRESENTER_LOG_FILE"; then
  echo "touch_demo_test_error=presenter_failed log=$PRESENTER_LOG_FILE"
  tail -n 40 "$PRESENTER_LOG_FILE"
  exit 4
fi
