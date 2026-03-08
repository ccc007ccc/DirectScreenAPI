#!/system/bin/sh
set -eu

MODROOT="${0%/*}"
DSAPI_LIB_FILE="$MODROOT/bin/dsapi_ksu_lib.sh"

if [ ! -f "$DSAPI_LIB_FILE" ]; then
  exit 0
fi

# shellcheck source=/dev/null
. "$DSAPI_LIB_FILE"

dsapi_init_layout
dsapi_runtime_seed_sync
