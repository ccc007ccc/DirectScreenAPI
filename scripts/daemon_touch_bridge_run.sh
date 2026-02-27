#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"
GETEVENT_BIN="${DSAPI_GETEVENT_BIN:-/system/bin/getevent}"
RUN_AS_ROOT="${DSAPI_TOUCH_RUN_AS_ROOT:-1}"
SYNC_DISPLAY="${DSAPI_TOUCH_SYNC_DISPLAY:-1}"
TOUCH_DEVICE="${DSAPI_TOUCH_DEVICE:-}"
AWK_SCRIPT="scripts/touch_event_to_dsapi.awk"

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

cleanup() {
  if [ -x ./target/release/dsapictl ]; then
    ./target/release/dsapictl --socket "$SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT INT TERM

if [ ! -x ./target/release/dsapistream ] || [ ! -x ./target/release/dsapictl ]; then
  ./scripts/build_core.sh >/dev/null
fi

if [ ! -S "$SOCKET_PATH" ]; then
  echo "touch_bridge_error=daemon_socket_missing socket=$SOCKET_PATH"
  exit 2
fi

if [ "$SYNC_DISPLAY" = "1" ]; then
  ./scripts/daemon_sync_display.sh >/dev/null 2>&1 || true
fi

display_line="$(./target/release/dsapictl --socket "$SOCKET_PATH" DISPLAY_GET)"
set -- $display_line
if [ "$#" -lt 6 ] || [ "$1" != "OK" ]; then
  echo "touch_bridge_error=invalid_display_response line=$display_line"
  exit 3
fi

display_w="$2"
display_h="$3"
display_rot="$6"
if ! is_uint "$display_w" || ! is_uint "$display_h" || ! is_uint "$display_rot"; then
  echo "touch_bridge_error=invalid_display_values line=$display_line"
  exit 3
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

./target/release/dsapictl --socket "$SOCKET_PATH" TOUCH_CLEAR >/dev/null 2>&1 || true

echo "touch_bridge_status=running device=$TOUCH_DEVICE max_x=$max_x max_y=$max_y display_w=$display_w display_h=$display_h rotation=$display_rot"

if [ "$RUN_AS_ROOT" = "1" ]; then
  su -c "$GETEVENT_BIN -lt '$TOUCH_DEVICE'" \
    | gawk -v max_x="$max_x" -v max_y="$max_y" -v width="$display_w" -v height="$display_h" -v rotation="$display_rot" -f "$AWK_SCRIPT" \
    | ./target/release/dsapistream --socket "$SOCKET_PATH" --quiet
else
  "$GETEVENT_BIN" -lt "$TOUCH_DEVICE" \
    | gawk -v max_x="$max_x" -v max_y="$max_y" -v width="$display_w" -v height="$display_h" -v rotation="$display_rot" -f "$AWK_SCRIPT" \
    | ./target/release/dsapistream --socket "$SOCKET_PATH" --quiet
fi
