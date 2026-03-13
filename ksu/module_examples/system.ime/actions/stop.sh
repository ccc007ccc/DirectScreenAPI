#!/system/bin/sh
ACTION_NAME="停止输入代理"
ACTION_DANGER=1
set -eu

fail() {
  reason="$1"
  shift || true
  if [ "$#" -gt 0 ]; then
    echo "state=error pid=- reason=$reason $*"
  else
    echo "state=error pid=- reason=$reason"
  fi
  exit 2
}

mod_dir="$(
  if [ -n "${DSAPI_MODULE_DIR:-}" ] && [ -d "${DSAPI_MODULE_DIR:-}" ]; then
    echo "$DSAPI_MODULE_DIR"
  else
    action_dir="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
    echo "${action_dir%/actions}"
  fi
)"
lib_file="$mod_dir/lib/ime_common.sh"
[ -f "$lib_file" ] || fail ime_lib_missing "path=$lib_file"
# shellcheck source=/dev/null
. "$lib_file"

ime_require_backend || exit 2

ime_proxy_stop || fail proxy_stop_failed "output=$(ime_sanitize_token "${IME_LAST_OUTPUT:-}")"

retry=0
while [ "$retry" -lt 12 ]
do
  if ! ime_proxy_is_running; then
    break
  fi
  retry=$((retry + 1))
  sleep 0.1
done

if ime_proxy_is_running; then
  ime_proxy_force_stop
  sleep 0.2
fi

if ime_proxy_is_running; then
  fail proxy_still_running "component=$(ime_proxy_component)"
fi

echo "state=stopped pid=- reason=manual_stop component=$(ime_proxy_component) backend=${IME_LAST_BACKEND:-unknown}"
