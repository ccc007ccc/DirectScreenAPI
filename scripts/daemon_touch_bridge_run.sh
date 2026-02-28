#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

CONTROL_SOCKET_PATH="${DSAPI_CONTROL_SOCKET_PATH:-${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}}"
DATA_SOCKET_PATH="${DSAPI_DATA_SOCKET_PATH:-}"
GETEVENT_BIN="${DSAPI_GETEVENT_BIN:-/system/bin/getevent}"
RUN_AS_ROOT="${DSAPI_TOUCH_RUN_AS_ROOT:-1}"
TOUCH_DEVICE="${DSAPI_TOUCH_DEVICE:-}"
MONITOR_INTERVAL_SEC="${DSAPI_TOUCH_MONITOR_INTERVAL_SEC:-1}"
AUTO_SYNC_DISPLAY="${DSAPI_TOUCH_AUTO_SYNC_DISPLAY:-1}"
SYNC_DISPLAY_EVERY_SEC="${DSAPI_TOUCH_SYNC_DISPLAY_EVERY_SEC:-1}"

if [ -z "$DATA_SOCKET_PATH" ]; then
  case "$CONTROL_SOCKET_PATH" in
    *.sock) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH%.sock}.data.sock" ;;
    *) DATA_SOCKET_PATH="${CONTROL_SOCKET_PATH}.data" ;;
  esac
fi

PIPE_PID=""
last_sync_ts=0

is_uint() {
  case "$1" in
    ''|*[!0-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

probe_getevent() {
  if [ "$RUN_AS_ROOT" = "1" ]; then
    su -c "$GETEVENT_BIN $*"
  else
    "$GETEVENT_BIN" "$@"
  fi
}

detect_touch_device() {
  probe_getevent -pl | awk '
    /^add device/ {
      dev = $4;
      has_x = 0;
      has_y = 0;
      has_tid = 0;
    }
    /ABS_MT_POSITION_X/ { has_x = 1; }
    /ABS_MT_POSITION_Y/ { has_y = 1; }
    /ABS_MT_TRACKING_ID/ { has_tid = 1; }
    /^  input props:/ {
      if (has_x && has_y && has_tid) {
        print dev;
        exit 0;
      }
    }
  '
}

extract_abs_max() {
  axis="$1"
  caps="$2"
  printf '%s\n' "$caps" | awk -v axis="$axis" '
    $0 ~ axis {
      for (i = 1; i <= NF; i++) {
        if ($i == "max") {
          v = $(i + 1);
          gsub(/[^0-9]/, "", v);
          print v;
          exit 0;
        }
      }
    }
  '
}

read_display_line() {
  ./target/release/dsapictl --socket "$CONTROL_SOCKET_PATH" DISPLAY_GET 2>/dev/null || true
}

parse_display() {
  line="$1"
  set -- $line
  if [ "$#" -lt 6 ] || [ "$1" != "OK" ]; then
    return 1
  fi
  w="$2"
  h="$3"
  rot="$6"
  if ! is_uint "$w" || ! is_uint "$h" || ! is_uint "$rot"; then
    return 1
  fi

  display_w="$w"
  display_h="$h"
  display_rot="$rot"
  return 0
}

maybe_sync_display() {
  if [ "$AUTO_SYNC_DISPLAY" != "1" ]; then
    return 0
  fi
  now="$(date +%s)"
  if ! is_uint "$SYNC_DISPLAY_EVERY_SEC" || [ "$SYNC_DISPLAY_EVERY_SEC" -lt 1 ]; then
    SYNC_DISPLAY_EVERY_SEC=1
  fi
  if [ "$last_sync_ts" -eq 0 ] || [ $((now - last_sync_ts)) -ge "$SYNC_DISPLAY_EVERY_SEC" ]; then
    ./scripts/daemon_sync_display.sh >/dev/null 2>&1 || true
    last_sync_ts="$now"
  fi
}

stop_pipeline() {
  if [ -n "$PIPE_PID" ] && kill -0 "$PIPE_PID" >/dev/null 2>&1; then
    kill "$PIPE_PID" >/dev/null 2>&1 || true
    wait "$PIPE_PID" >/dev/null 2>&1 || true
  fi
  PIPE_PID=""
}

start_pipeline() {
  w="$1"
  h="$2"
  rot="$3"

  cmd="./target/release/dsapiinput --control-socket '$CONTROL_SOCKET_PATH' --data-socket '$DATA_SOCKET_PATH' --device '$TOUCH_DEVICE' --max-x '$max_x' --max-y '$max_y' --width '$w' --height '$h' --rotation '$rot' --quiet"

  if [ "$RUN_AS_ROOT" = "1" ]; then
    su -c "$cmd" &
  else
    sh -c "$cmd" &
  fi
  PIPE_PID="$!"
}

cleanup() {
  stop_pipeline
  if [ -x ./target/release/dsapictl ] && [ -S "$CONTROL_SOCKET_PATH" ]; then
    ./target/release/dsapictl --socket "$CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT INT TERM

if [ ! -x ./target/release/dsapiinput ] || [ ! -x ./target/release/dsapictl ]; then
  ./scripts/build_core.sh >/dev/null
fi

if [ ! -S "$CONTROL_SOCKET_PATH" ]; then
  echo "touch_bridge_error=daemon_control_socket_missing socket=$CONTROL_SOCKET_PATH"
  exit 2
fi
if [ ! -S "$DATA_SOCKET_PATH" ]; then
  echo "touch_bridge_error=daemon_data_socket_missing socket=$DATA_SOCKET_PATH"
  exit 2
fi

if [ -z "$TOUCH_DEVICE" ]; then
  TOUCH_DEVICE="$(detect_touch_device || true)"
fi
if [ -z "$TOUCH_DEVICE" ]; then
  echo "touch_bridge_error=touch_device_not_found"
  exit 4
fi

caps="$(probe_getevent -pl "$TOUCH_DEVICE")"
max_x="$(extract_abs_max "ABS_MT_POSITION_X" "$caps")"
max_y="$(extract_abs_max "ABS_MT_POSITION_Y" "$caps")"
if ! is_uint "$max_x" || ! is_uint "$max_y"; then
  echo "touch_bridge_error=invalid_touch_capabilities device=$TOUCH_DEVICE"
  exit 5
fi

./target/release/dsapictl --socket "$CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true

current_display_line=""

while true; do
  maybe_sync_display
  display_line="$(read_display_line)"
  if ! parse_display "$display_line"; then
    echo "touch_bridge_warn=display_unavailable line=$display_line"
    sleep "$MONITOR_INTERVAL_SEC"
    continue
  fi

  if [ "$display_line" != "$current_display_line" ]; then
    if [ -n "$current_display_line" ]; then
      echo "touch_bridge_status=display_changed old='$current_display_line' new='$display_line'"
    fi
    current_display_line="$display_line"
    ./target/release/dsapictl --socket "$CONTROL_SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
    stop_pipeline
    start_pipeline "$display_w" "$display_h" "$display_rot"
    echo "touch_bridge_status=running device=$TOUCH_DEVICE max_x=$max_x max_y=$max_y display_w=$display_w display_h=$display_h rotation=$display_rot"
  fi

  if [ -n "$PIPE_PID" ] && ! kill -0 "$PIPE_PID" >/dev/null 2>&1; then
    echo "touch_bridge_warn=pipeline_exited restart=1"
    stop_pipeline
    start_pipeline "$display_w" "$display_h" "$display_rot"
  fi

  sleep "$MONITOR_INTERVAL_SEC"
done
