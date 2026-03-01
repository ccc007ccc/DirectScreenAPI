frame_pull_once_python() {
  DATA_SOCKET_PATH="$1"
  TMP_PATH="$2"

  python3 - "$DATA_SOCKET_PATH" "$TMP_PATH" <<'PY'
import array
import mmap
import os
import socket
import sys

sock_path = sys.argv[1]
tmp_path = sys.argv[2]

def fail(msg: str, code: int) -> None:
    print(msg)
    sys.exit(code)

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
timeout_ms_raw = os.environ.get("DSAPI_FRAME_PULL_TIMEOUT_MS", "30000")
try:
    timeout_ms = int(timeout_ms_raw)
except ValueError:
    timeout_ms = 30000
if timeout_ms <= 0:
    timeout_ms = 30000
s.settimeout(timeout_ms / 1000.0)

wait_timeout_ms_raw = os.environ.get("DSAPI_FRAME_WAIT_TIMEOUT_MS", "250")
try:
    wait_timeout_ms = int(wait_timeout_ms_raw)
except ValueError:
    wait_timeout_ms = 250
if wait_timeout_ms <= 0:
    wait_timeout_ms = 250

def recv_line_with_optional_fd(sock: socket.socket, require_fd: bool):
    line = bytearray()
    received_fd = None
    line_done = False
    line_text = ""
    fd_size = array.array("i").itemsize
    ancbuf_size = socket.CMSG_SPACE(fd_size)

    while True:
        try:
            data, ancdata, _, _ = sock.recvmsg(1024, ancbuf_size)
        except TimeoutError:
            fail("frame_pull_error=socket_timeout", 4)
        if not data and not ancdata:
            if line_done:
                fail(f"frame_pull_error=bind_missing_fd line='{line_text}'", 5)
            fail("frame_pull_error=header_eof", 4)

        for level, ctype, cdata in ancdata:
            if level == socket.SOL_SOCKET and ctype == socket.SCM_RIGHTS and received_fd is None:
                fds = array.array("i")
                fds.frombytes(cdata[:fd_size])
                if len(fds) > 0:
                    received_fd = int(fds[0])

        for b in data:
            if line_done:
                continue
            if b == 10:  # '\n'
                line_text = line.decode("utf-8", "replace").strip()
                line_done = True
                break
            if b != 13:  # '\r'
                line.append(b)
            if len(line) > 4096:
                fail("frame_pull_error=header_too_long", 4)

        if line_done and (not require_fd or received_fd is not None):
            return line_text, received_fd

try:
    s.connect(sock_path)
    s.sendall(b"RENDER_FRAME_BIND_SHM\n")
    bind_line, fd = recv_line_with_optional_fd(s, require_fd=True)
    bind_parts = bind_line.split()
    if len(bind_parts) != 4 or bind_parts[0] != "OK" or bind_parts[1] != "SHM_BOUND":
        fail(f"frame_pull_error=bad_bind_header line='{bind_line}'", 5)

    capacity = int(bind_parts[2])
    bind_offset = int(bind_parts[3])
    if capacity <= 0 or bind_offset < 0:
        fail("frame_pull_error=invalid_bind_layout", 6)

    map_len = capacity + bind_offset
    mm = mmap.mmap(fd, map_len, prot=mmap.PROT_READ, flags=mmap.MAP_SHARED)
    os.close(fd)

    s.sendall(f"RENDER_FRAME_WAIT_SHM_PRESENT 0 {wait_timeout_ms}\n".encode("ascii"))
    text, _ = recv_line_with_optional_fd(s, require_fd=False)
    parts = text.split()
    if len(parts) == 2 and parts[0] == "OK" and parts[1] == "TIMEOUT":
        fail("frame_pull_error=frame_wait_timeout", 7)
    if len(parts) != 8 or parts[0] != "OK":
        fail(f"frame_pull_error=bad_wait_header line='{text}'", 5)

    frame_seq = parts[1]
    width = parts[2]
    height = parts[3]
    pixel_format = parts[4]
    byte_len = int(parts[5])
    offset = int(parts[7])
    if pixel_format != "RGBA8888":
        fail(f"frame_pull_error=bad_pixel_format:{pixel_format}", 6)
    if byte_len <= 0:
        fail("frame_pull_error=invalid_byte_len", 6)
    if offset < 0 or offset + byte_len > map_len:
        fail("frame_pull_error=invalid_offset_or_length", 6)

    rgba = mm[offset : offset + byte_len]
    with open(tmp_path, "wb") as f:
        f.write(rgba)
    mm.close()

    actual = os.path.getsize(tmp_path)
    if actual != byte_len:
        fail(f"frame_pull_error=byte_count_mismatch expected={byte_len} actual={actual}", 8)
    print(f"frame_pull=ok frame_seq={frame_seq} size={width}x{height} bytes={byte_len}")
finally:
    s.close()
PY
}

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

  FRAME_PULL_ENGINE="${DSAPI_FRAME_PULL_ENGINE:-auto}"
  FRAME_PULL_RUST_BIN="${DSAPI_FRAME_PULL_BIN:-$(release_bin dsapiframepull)}"

  case "$FRAME_PULL_ENGINE" in
    auto)
      if [ -x "$FRAME_PULL_RUST_BIN" ]; then
        FRAME_PULL_BACKEND="rust"
      else
        FRAME_PULL_BACKEND="python"
      fi
      ;;
    rust|python)
      FRAME_PULL_BACKEND="$FRAME_PULL_ENGINE"
      ;;
    *)
      echo "frame_pull_error=engine_invalid engine=$FRAME_PULL_ENGINE expected=auto|rust|python"
      return 2
      ;;
  esac

  if [ "$FRAME_PULL_BACKEND" = "rust" ] && [ ! -x "$FRAME_PULL_RUST_BIN" ]; then
    echo "frame_pull_error=rust_helper_not_found path=$FRAME_PULL_RUST_BIN"
    return 3
  fi

  if [ "$FRAME_PULL_BACKEND" = "python" ] && ! command -v python3 >/dev/null 2>&1; then
    echo "frame_pull_error=python3_not_found"
    return 3
  fi

  mkdir -p "$(dirname "$OUT_PATH")"
  TMP_PATH="$OUT_PATH.tmp.$$"
  FRAME_PULL_RETRIES_RAW="${DSAPI_FRAME_PULL_RETRIES:-4}"
  FRAME_PULL_RETRY_DELAY_MS_RAW="${DSAPI_FRAME_PULL_RETRY_DELAY_MS:-80}"

  case "$FRAME_PULL_RETRIES_RAW" in
    ''|*[!0-9]*) FRAME_PULL_RETRIES=4 ;;
    *) FRAME_PULL_RETRIES="$FRAME_PULL_RETRIES_RAW" ;;
  esac
  case "$FRAME_PULL_RETRY_DELAY_MS_RAW" in
    ''|*[!0-9]*) FRAME_PULL_RETRY_DELAY_MS=80 ;;
    *) FRAME_PULL_RETRY_DELAY_MS="$FRAME_PULL_RETRY_DELAY_MS_RAW" ;;
  esac

  rm -f "$TMP_PATH" >/dev/null 2>&1 || true
  FRAME_PULL_ATTEMPT=0
  while :
  do
    FRAME_PULL_ATTEMPT=$((FRAME_PULL_ATTEMPT + 1))

    set +e
    if [ "$FRAME_PULL_BACKEND" = "rust" ]; then
      FRAME_PULL_OUTPUT="$($FRAME_PULL_RUST_BIN "$DATA_SOCKET_PATH" "$TMP_PATH" 2>&1)"
      frame_pull_rc=$?
    else
      FRAME_PULL_OUTPUT="$(frame_pull_once_python "$DATA_SOCKET_PATH" "$TMP_PATH")"
      frame_pull_rc=$?
    fi
    set -e

    printf '%s\n' "$FRAME_PULL_OUTPUT"
    if [ "$frame_pull_rc" -eq 0 ]; then
      mv "$TMP_PATH" "$OUT_PATH"
      echo "frame_pull_out=$OUT_PATH"
      return 0
    fi

    frame_pull_retryable=0
    case "$FRAME_PULL_OUTPUT" in
      *frame_pull_error=socket_timeout*|\
      *frame_pull_error=bind_missing_fd*|\
      *frame_pull_error=frame_wait_timeout*|\
      *frame_pull_error=bad_bind_header*ERR\ BUSY*|\
      *frame_pull_error=bad_wait_header*ERR\ BUSY*|\
      *ERR\ INTERNAL_ERROR\ read_timeout*)
        frame_pull_retryable=1
        ;;
    esac

    if [ "$frame_pull_retryable" != "1" ] || [ "$FRAME_PULL_ATTEMPT" -gt "$FRAME_PULL_RETRIES" ]; then
      rm -f "$TMP_PATH" >/dev/null 2>&1 || true
      return "$frame_pull_rc"
    fi

    if [ "$FRAME_PULL_RETRY_DELAY_MS" -gt 0 ]; then
      sleep "$(awk -v ms="$FRAME_PULL_RETRY_DELAY_MS" 'BEGIN{printf "%.3f", ms / 1000.0}')"
    fi
  done
}
