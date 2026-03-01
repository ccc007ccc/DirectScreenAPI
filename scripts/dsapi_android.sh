android_probe_impl() {
  probe_cmd="${1:-display-kv}"
  OUT_DIR="${DSAPI_ANDROID_OUT_DIR:-artifacts/android}"
  DEX_JAR="$OUT_DIR/directscreen-adapter-dex.jar"
  MAIN_CLASS="org.directscreenapi.adapter.AndroidAdapterMain"
  RUN_AS_ROOT="${DSAPI_RUN_AS_ROOT:-1}"

  if [ ! -f "$DEX_JAR" ]; then
    ./scripts/build_android_adapter.sh >/dev/null
  fi

  case "$DEX_JAR" in
    /*) DEX_JAR_ABS="$DEX_JAR" ;;
    *) DEX_JAR_ABS="$ROOT_DIR/$DEX_JAR" ;;
  esac

  APP_PROCESS_BIN="${DSAPI_APP_PROCESS_BIN:-app_process}"
  if ! command -v "$APP_PROCESS_BIN" >/dev/null 2>&1; then
    if [ -x /system/bin/app_process ]; then
      APP_PROCESS_BIN="/system/bin/app_process"
    else
      echo "android_probe_error=app_process_not_found"
      return 2
    fi
  fi

  if [ "$RUN_AS_ROOT" = "1" ]; then
    if ! command -v su >/dev/null 2>&1; then
      echo "android_probe_error=su_not_found"
      return 2
    fi
    su_exec_with_env CLASSPATH "$DEX_JAR_ABS" \
      "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" "$probe_cmd"
    return 0
  fi

  CLASSPATH="$DEX_JAR_ABS" "$APP_PROCESS_BIN" /system/bin "$MAIN_CLASS" "$probe_cmd"
}

android_sync_display_impl() {
  probe_output="$(android_probe_impl display-kv)"

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
    return 3
  fi

  if ! is_float "$refresh_hz"; then
    echo "display_sync_error=invalid_refresh_hz"
    printf '%s\n' "$probe_output"
    return 3
  fi

  daemon_cmd_impl DISPLAY_SET "$width" "$height" "$refresh_hz" "$density_dpi" "$rotation" >/dev/null
  daemon_display="$(daemon_cmd_impl DISPLAY_GET)"

  echo "display_sync=ok width=$width height=$height refresh_hz=$refresh_hz density_dpi=$density_dpi rotation=$rotation"
  echo "daemon_display=$daemon_display"
}
