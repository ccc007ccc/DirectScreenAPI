frame_pull_impl() {
  if [ "$#" -ne 1 ]; then
    echo "dsapi_error=frame_pull_requires_path"
    return 2
  fi

  DATA_SOCKET_PATH="$(data_socket_path)"
  OUT_PATH="$1"

  if [ ! -S "$DATA_SOCKET_PATH" ]; then
    echo "frame_pull_error=data_socket_missing socket=$DATA_SOCKET_PATH"
    return 2
  fi

  if ! command -v python3 >/dev/null 2>&1; then
    echo "frame_pull_error=python3_not_found"
    return 3
  fi

  mkdir -p "$(dirname "$OUT_PATH")"
  TMP_PATH="$OUT_PATH.tmp.$$"

  python3 - "$DATA_SOCKET_PATH" "$TMP_PATH" <<'PY'
import os
import socket
import sys

sock_path = sys.argv[1]
tmp_path = sys.argv[2]

def fail(msg: str, code: int) -> None:
    print(msg)
    sys.exit(code)

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
try:
    s.connect(sock_path)
    s.sendall(b"RENDER_FRAME_GET_RAW\\n")
    line = bytearray()
    while True:
        b = s.recv(1)
        if not b:
            fail("frame_pull_error=header_eof", 4)
        if b == b"\\n":
            break
        if b != b"\\r":
            line.extend(b)
        if len(line) > 4096:
            fail("frame_pull_error=header_too_long", 4)

    text = line.decode("utf-8", "replace").strip()
    parts = text.split()
    if len(parts) != 5 or parts[0] != "OK":
        fail(f"frame_pull_error=bad_header line='{text}'", 5)

    frame_seq = parts[1]
    width = parts[2]
    height = parts[3]
    byte_len = int(parts[4])
    if byte_len <= 0:
        fail("frame_pull_error=invalid_byte_len", 6)

    remain = byte_len
    with open(tmp_path, "wb") as f:
        while remain > 0:
            chunk = s.recv(min(1024 * 1024, remain))
            if not chunk:
                fail("frame_pull_error=body_eof", 7)
            f.write(chunk)
            remain -= len(chunk)

    actual = os.path.getsize(tmp_path)
    if actual != byte_len:
        fail(f"frame_pull_error=byte_count_mismatch expected={byte_len} actual={actual}", 8)
    print(f"frame_pull=ok frame_seq={frame_seq} size={width}x{height} bytes={byte_len}")
finally:
    s.close()
PY

  mv "$TMP_PATH" "$OUT_PATH"
  echo "frame_pull_out=$OUT_PATH"
}
