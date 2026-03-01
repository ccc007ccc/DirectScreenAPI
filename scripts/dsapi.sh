#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'USAGE'
usage:
  ./scripts/dsapi.sh daemon start|stop|status|cmd <COMMAND ...>
  ./scripts/dsapi.sh presenter start|stop|status|run
  ./scripts/dsapi.sh screen start|stop|status|run|bench [samples]
  ./scripts/dsapi.sh touch start|stop|status|run
  ./scripts/dsapi.sh android probe [display-kv|display-line]
  ./scripts/dsapi.sh android sync-display
  ./scripts/dsapi.sh frame pull <out_rgba_path>
  ./scripts/dsapi.sh build core|android|c-example|framepull
  ./scripts/dsapi.sh check|fix
USAGE
}

target_dir() {
  printf '%s\n' "${CARGO_TARGET_DIR:-${DSAPI_TARGET_DIR:-target}}"
}

release_bin() {
  printf '%s\n' "$(target_dir)/release/$1"
}

control_socket_path() {
  printf '%s\n' "${DSAPI_CONTROL_SOCKET_PATH:-${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}}"
}

derive_data_socket_path() {
  case "$1" in
    *.sock) printf '%s\n' "${1%.sock}.data.sock" ;;
    *) printf '%s\n' "${1}.data" ;;
  esac
}

data_socket_path() {
  if [ -n "${DSAPI_DATA_SOCKET_PATH:-}" ]; then
    printf '%s\n' "${DSAPI_DATA_SOCKET_PATH}"
  else
    derive_data_socket_path "$(control_socket_path)"
  fi
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\"'\"'/g")"
}

build_quoted_cmd() {
  out=""
  for arg in "$@"
  do
    q="$(shell_quote "$arg")"
    if [ -z "$out" ]; then
      out="$q"
    else
      out="$out $q"
    fi
  done
  printf '%s\n' "$out"
}

su_exec() {
  cmd="$(build_quoted_cmd "$@")"
  su -c "$cmd"
}

su_exec_with_env() {
  env_name="$1"
  env_value="$2"
  shift 2
  cmd="${env_name}=$(shell_quote "$env_value")"
  quoted="$(build_quoted_cmd "$@")"
  if [ -n "$quoted" ]; then
    cmd="$cmd $quoted"
  fi
  su -c "$cmd"
}

pid_cmdline_contains() {
  pid="$1"
  needle="$2"
  if [ -z "$pid" ] || [ ! -r "/proc/$pid/cmdline" ]; then
    return 1
  fi
  tr '\0' ' ' < "/proc/$pid/cmdline" | grep -F -- "$needle" >/dev/null 2>&1
}

pid_is_running() {
  pid="$1"
  if [ -z "$pid" ]; then
    return 1
  fi
  if kill -0 "$pid" >/dev/null 2>&1; then
    return 0
  fi
  [ -d "/proc/$pid" ]
}

ensure_release_ctl() {
  if [ ! -x "$(release_bin dsapictl)" ]; then
    ./scripts/build_core.sh >/dev/null
  fi
}


# 子命令实现拆分，降低主脚本复杂度。
. ./scripts/dsapi_daemon.sh
. ./scripts/dsapi_presenter.sh
. ./scripts/dsapi_screen.sh
. ./scripts/dsapi_touch.sh
. ./scripts/dsapi_android.sh
. ./scripts/dsapi_frame.sh

build_impl() {
  sub="${1:-}"
  case "$sub" in
    core)
      shift
      ./scripts/build_core.sh "$@"
      ;;
    android)
      shift
      ./scripts/build_android_adapter.sh "$@"
      ;;
    c-example)
      shift
      ./scripts/build_c_example.sh "$@"
      ;;
    framepull)
      shift
      ./scripts/build_framepull.sh "$@"
      ;;
    *)
      echo "dsapi_error=build_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

run_daemon() {
  sub="${1:-}"
  case "$sub" in
    start) shift; daemon_start_impl "$@" ;;
    stop) shift; daemon_stop_impl "$@" ;;
    status) shift; daemon_status_impl "$@" ;;
    cmd) shift; daemon_cmd_impl "$@" ;;
    *)
      echo "dsapi_error=daemon_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

run_presenter() {
  sub="${1:-}"
  case "$sub" in
    start) shift; presenter_start_impl "$@" ;;
    stop) shift; presenter_stop_impl "$@" ;;
    status) shift; presenter_status_impl "$@" ;;
    run) shift; presenter_run_impl "$@" ;;
    *)
      echo "dsapi_error=presenter_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

run_screen() {
  sub="${1:-}"
  case "$sub" in
    start) shift; screen_start_impl "$@" ;;
    stop) shift; screen_stop_impl "$@" ;;
    status) shift; screen_status_impl "$@" ;;
    run) shift; screen_run_impl "$@" ;;
    bench) shift; screen_bench_impl "$@" ;;
    *)
      echo "dsapi_error=screen_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

run_touch() {
  sub="${1:-}"
  case "$sub" in
    start) shift; touch_start_impl "$@" ;;
    stop) shift; touch_stop_impl "$@" ;;
    status) shift; touch_status_impl "$@" ;;
    run) shift; touch_run_impl "$@" ;;
    *)
      echo "dsapi_error=touch_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

run_android() {
  sub="${1:-}"
  case "$sub" in
    probe) shift; android_probe_impl "$@" ;;
    sync-display) shift; android_sync_display_impl "$@" ;;
    *)
      echo "dsapi_error=android_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

run_frame() {
  sub="${1:-}"
  case "$sub" in
    pull) shift; frame_pull_impl "$@" ;;
    *)
      echo "dsapi_error=frame_subcommand_invalid subcommand=${sub:-<empty>}"
      usage
      return 1
      ;;
  esac
}

if [ "$#" -lt 1 ]; then
  usage
  exit 1
fi

cmd="$1"
shift

case "$cmd" in
  daemon) run_daemon "$@" ;;
  presenter) run_presenter "$@" ;;
  screen) run_screen "$@" ;;
  touch) run_touch "$@" ;;
  android) run_android "$@" ;;
  frame) run_frame "$@" ;;
  build) build_impl "$@" ;;
  check) ./scripts/check.sh "$@" ;;
  fix) ./scripts/fix.sh "$@" ;;
  help|-h|--help) usage ;;
  *)
    echo "dsapi_error=command_invalid command=$cmd"
    usage
    exit 1
    ;;
esac
