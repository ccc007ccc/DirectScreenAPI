#!/system/bin/sh

# system.ime v2:
# 旧方案（cmd/ime 系统命令切换输入法）已移除，统一走 Manager 的透明输入代理 Activity。

ime_to_bool() {
  case "${1:-0}" in
    1|true|TRUE|yes|YES|on|ON) echo 1 ;;
    *) echo 0 ;;
  esac
}

ime_require_backend() {
  if command -v am >/dev/null 2>&1; then
    return 0
  fi
  echo "state=error pid=- reason=am_missing"
  return 1
}

ime_sanitize_token() {
  raw="${1:-}"
  if [ -z "$raw" ]; then
    echo "-"
    return 0
  fi
  printf '%s' "$raw" | tr '\r\n\t ' '_'
}

ime_proxy_package() {
  pkg="${IME_PROXY_PACKAGE:-org.directscreenapi.manager}"
  if [ -n "$pkg" ]; then
    echo "$pkg"
  else
    echo "org.directscreenapi.manager"
  fi
}

ime_proxy_activity() {
  act="${IME_PROXY_ACTIVITY:-.ImeProxyActivity}"
  if [ -n "$act" ]; then
    echo "$act"
  else
    echo ".ImeProxyActivity"
  fi
}

ime_proxy_component() {
  pkg="$(ime_proxy_package)"
  act="$(ime_proxy_activity)"
  case "$act" in
    */*) echo "$act" ;;
    .*) echo "$pkg/$act" ;;
    *) echo "$pkg/$act" ;;
  esac
}

ime_proxy_mode_normalize() {
  case "${1:-text}" in
    number|password|number_password|text) echo "$1" ;;
    *) echo "text" ;;
  esac
}

ime_proxy_mode() {
  ime_proxy_mode_normalize "${IME_PROXY_MODE:-text}"
}

ime_bridge_service() {
  svc="${IME_PROXY_BRIDGE_SERVICE:-${DSAPI_MANAGER_BRIDGE_SERVICE:-dsapi.core}}"
  if [ -n "$svc" ]; then
    echo "$svc"
  else
    echo "dsapi.core"
  fi
}

ime_ctl_path() {
  if [ -n "${IME_PROXY_CTL_PATH:-}" ]; then
    echo "$IME_PROXY_CTL_PATH"
    return 0
  fi
  if [ -n "${DSAPI_MODROOT:-}" ]; then
    echo "$DSAPI_MODROOT/bin/dsapi_service_ctl.sh"
    return 0
  fi
  echo "/data/adb/modules/directscreenapi/bin/dsapi_service_ctl.sh"
}

IME_LAST_BACKEND=""
IME_LAST_OUTPUT=""

ime_proxy_start() {
  mode="$(ime_proxy_mode_normalize "${1:-text}")"
  comp="$(ime_proxy_component)"
  ctl_path="$(ime_ctl_path)"
  bridge_service="$(ime_bridge_service)"
  IME_LAST_OUTPUT="$(
    am start \
      -n "$comp" \
      --activity-single-top \
      --activity-clear-top \
      --es ime_proxy_action open \
      --es ime_proxy_mode "$mode" \
      --es ctl_path "$ctl_path" \
      --es bridge_service "$bridge_service" 2>&1 || true
  )"
  case "$IME_LAST_OUTPUT" in
    *"Error:"*|*"Exception"*) return 1 ;;
  esac
  IME_LAST_BACKEND="am_activity"
  export IME_LAST_BACKEND IME_LAST_OUTPUT
  return 0
}

ime_proxy_stop() {
  comp="$(ime_proxy_component)"
  mode="$(ime_proxy_mode)"
  ctl_path="$(ime_ctl_path)"
  bridge_service="$(ime_bridge_service)"
  IME_LAST_OUTPUT="$(
    am start \
      -n "$comp" \
      --activity-single-top \
      --activity-clear-top \
      --es ime_proxy_action close \
      --es ime_proxy_mode "$mode" \
      --es ctl_path "$ctl_path" \
      --es bridge_service "$bridge_service" 2>&1 || true
  )"
  case "$IME_LAST_OUTPUT" in
    *"Error:"*|*"Exception"*) return 1 ;;
  esac
  IME_LAST_BACKEND="am_activity"
  export IME_LAST_BACKEND IME_LAST_OUTPUT
  return 0
}

ime_proxy_is_running() {
  comp="$(ime_proxy_component)"
  dumpsys activity activities 2>/dev/null \
    | grep -E "topResumedActivity=|Resumed:|mResumedActivity" \
    | grep -F "$comp" >/dev/null 2>&1
}

ime_proxy_force_stop() {
  pkg="$(ime_proxy_package)"
  am force-stop "$pkg" >/dev/null 2>&1 || true
}
