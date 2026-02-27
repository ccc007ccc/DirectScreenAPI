#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SOCKET_PATH="${DSAPI_SOCKET_PATH:-artifacts/run/dsapi.sock}"

if [ "$#" -lt 1 ]; then
  echo "usage: daemon_cmd.sh <COMMAND ...>"
  exit 1
fi

cargo run -p directscreen-core --bin dsapictl -- --socket "$SOCKET_PATH" "$@"
