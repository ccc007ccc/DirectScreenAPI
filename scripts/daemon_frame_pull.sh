#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"
OUT_PATH="${1:-}"
CHUNK_BYTES="${2:-65536}"

if [ -z "$OUT_PATH" ]; then
  echo "usage: daemon_frame_pull.sh <out_rgba_path> [chunk_bytes]"
  exit 1
fi

if [ "$CHUNK_BYTES" -le 0 ] 2>/dev/null; then
  echo "frame_pull_error=invalid_chunk_bytes value=$CHUNK_BYTES"
  exit 2
fi

if ! command -v base64 >/dev/null 2>&1; then
  echo "frame_pull_error=base64_not_found"
  exit 3
fi

frame_line="$(./target/release/dsapictl --socket "$SOCKET_PATH" RENDER_FRAME_GET 2>/dev/null || true)"
if [ -z "$frame_line" ]; then
  echo "frame_pull_error=daemon_unreachable socket=$SOCKET_PATH"
  exit 4
fi

set -- $frame_line
if [ "$#" -ne 7 ] || [ "$1" != "OK" ] || [ "$5" != "RGBA8888" ]; then
  echo "frame_pull_error=invalid_frame_info line='$frame_line'"
  exit 5
fi

FRAME_SEQ="$2"
WIDTH="$3"
HEIGHT="$4"
TOTAL_BYTES="$6"
CHECKSUM="$7"

TMP_PATH="$OUT_PATH.tmp.$$"
mkdir -p "$(dirname "$OUT_PATH")"
: > "$TMP_PATH"

offset=0
while [ "$offset" -lt "$TOTAL_BYTES" ]; do
  chunk_line="$(./target/release/dsapictl --socket "$SOCKET_PATH" RENDER_FRAME_READ_BASE64 "$offset" "$CHUNK_BYTES" 2>/dev/null || true)"
  if [ -z "$chunk_line" ]; then
    rm -f "$TMP_PATH"
    echo "frame_pull_error=chunk_read_failed offset=$offset"
    exit 6
  fi

  set -- $chunk_line
  if [ "$#" -ne 6 ] || [ "$1" != "OK" ]; then
    rm -f "$TMP_PATH"
    echo "frame_pull_error=invalid_chunk_line line='$chunk_line'"
    exit 7
  fi

  READ_FRAME_SEQ="$2"
  READ_TOTAL_BYTES="$3"
  READ_OFFSET="$4"
  READ_CHUNK_BYTES="$5"
  READ_PAYLOAD="$6"

  if [ "$READ_FRAME_SEQ" != "$FRAME_SEQ" ] || [ "$READ_TOTAL_BYTES" != "$TOTAL_BYTES" ] || [ "$READ_OFFSET" != "$offset" ]; then
    rm -f "$TMP_PATH"
    echo "frame_pull_error=chunk_mismatch line='$chunk_line'"
    exit 8
  fi

  if ! printf '%s' "$READ_PAYLOAD" | base64 -d >> "$TMP_PATH"; then
    rm -f "$TMP_PATH"
    echo "frame_pull_error=base64_decode_failed offset=$offset"
    exit 9
  fi

  offset=$((offset + READ_CHUNK_BYTES))
done

actual_bytes="$(wc -c < "$TMP_PATH" | tr -d '[:space:]')"
if [ "$actual_bytes" != "$TOTAL_BYTES" ]; then
  rm -f "$TMP_PATH"
  echo "frame_pull_error=byte_count_mismatch expected=$TOTAL_BYTES actual=$actual_bytes"
  exit 10
fi

mv "$TMP_PATH" "$OUT_PATH"
echo "frame_pull=ok frame_seq=$FRAME_SEQ size=${WIDTH}x${HEIGHT} bytes=$TOTAL_BYTES checksum=$CHECKSUM out=$OUT_PATH"
