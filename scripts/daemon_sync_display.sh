#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

probe_output="$(./scripts/android_display_probe.sh display-kv)"

width="$(printf '%s\n' "$probe_output" | awk -F= '/^width=/{print $2; exit}')"
height="$(printf '%s\n' "$probe_output" | awk -F= '/^height=/{print $2; exit}')"
refresh_hz="$(printf '%s\n' "$probe_output" | awk -F= '/^refresh_hz=/{print $2; exit}')"
density_dpi="$(printf '%s\n' "$probe_output" | awk -F= '/^density_dpi=/{print $2; exit}')"
rotation="$(printf '%s\n' "$probe_output" | awk -F= '/^rotation=/{print $2; exit}')"

is_uint() {
  case "$1" in
    ''|*[!0-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

is_float() {
  case "$1" in
    ''|*[!0-9.]*|*.*.*) return 1 ;;
    *) return 0 ;;
  esac
}

if ! is_uint "$width" || ! is_uint "$height" || ! is_uint "$density_dpi" || ! is_uint "$rotation"; then
  echo "display_sync_error=invalid_probe_output"
  printf '%s\n' "$probe_output"
  exit 3
fi

if ! is_float "$refresh_hz"; then
  echo "display_sync_error=invalid_refresh_hz"
  printf '%s\n' "$probe_output"
  exit 3
fi

./scripts/daemon_cmd.sh DISPLAY_SET "$width" "$height" "$refresh_hz" "$density_dpi" "$rotation" >/dev/null
daemon_display="$(./scripts/daemon_cmd.sh DISPLAY_GET)"

echo "display_sync=ok width=$width height=$height refresh_hz=$refresh_hz density_dpi=$density_dpi rotation=$rotation"
echo "daemon_display=$daemon_display"
