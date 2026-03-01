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

def recv_line_with_optional_fd(sock: socket.socket, require_fd: bool):
    line = bytearray()
    received_fd = None
    fd_size = array.array("i").itemsize
    ancbuf_size = socket.CMSG_SPACE(fd_size)

    while True:
        data, ancdata, _, _ = sock.recvmsg(1024, ancbuf_size)
        if not data and not ancdata:
            fail("frame_pull_error=header_eof", 4)

        for level, ctype, cdata in ancdata:
            if level == socket.SOL_SOCKET and ctype == socket.SCM_RIGHTS and received_fd is None:
                fds = array.array("i")
                fds.frombytes(cdata[:fd_size])
                if len(fds) > 0:
                    received_fd = int(fds[0])

        for b in data:
            if b == 10:  # '\n'
                text = line.decode("utf-8", "replace").strip()
                if require_fd and received_fd is None:
                    fail("frame_pull_error=bind_missing_fd", 5)
                return text, received_fd
            if b != 13:  # '\r'
                line.append(b)
            if len(line) > 4096:
                fail("frame_pull_error=header_too_long", 4)

try:
    s.connect(sock_path)
    s.sendall(b"RENDER_FRAME_BIND_SHM\\n")
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

    s.sendall(b"RENDER_FRAME_WAIT_SHM_PRESENT 0 64\\n")
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

  mv "$TMP_PATH" "$OUT_PATH"
  echo "frame_pull_out=$OUT_PATH"
}
