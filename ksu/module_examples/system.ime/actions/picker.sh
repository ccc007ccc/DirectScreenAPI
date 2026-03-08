#!/system/bin/sh
ACTION_NAME=拉起输入代理
ACTION_DANGER=0
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

mode="$(ime_proxy_mode)"
ime_proxy_start "$mode" || fail proxy_start_failed "mode=$mode output=$(ime_sanitize_token "${IME_LAST_OUTPUT:-}")"
if ime_proxy_is_running; then
  proxy_state="running"
  reason="-"
else
  proxy_state="launched"
  reason="proxy_not_resumed"
fi

echo "state=ready pid=- reason=$reason mode=$mode component=$(ime_proxy_component) backend=${IME_LAST_BACKEND:-unknown}"
echo "proxy_state=$proxy_state"
