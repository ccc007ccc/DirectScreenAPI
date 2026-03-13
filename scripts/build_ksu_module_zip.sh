#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

SRC_ROOT="${DSAPI_KSU_MODULE_SRC_ROOT:-ksu/module_examples}"
OUT_ROOT="${DSAPI_KSU_MODULE_ZIP_OUT_DIR:-artifacts/ksu_module_zips}"

usage() {
  cat <<'USAGE'
usage:
  ./scripts/build_ksu_module_zip.sh <module_id_or_dir>
  ./scripts/build_ksu_module_zip.sh --all
env:
  DSAPI_KSU_MODULE_SRC_ROOT   default: ksu/module_examples
  DSAPI_KSU_MODULE_ZIP_OUT_DIR default: artifacts/ksu_module_zips
USAGE
}

meta_get() {
  meta_file="$1"
  key="$2"
  [ -f "$meta_file" ] || { echo ""; return 0; }
  # shellcheck disable=SC2002
  cat "$meta_file" 2>/dev/null | while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      "$key="*)
        echo "${line#*=}" | tr -d '\r' | sed -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//"
        return 0
        ;;
    esac
  done
  echo ""
}

resolve_module_id() {
  module_dir="$1"
  meta="$module_dir/dsapi.module"
  module_id="$(meta_get "$meta" MODULE_ID)"
  [ -n "$module_id" ] || module_id="$(meta_get "$meta" DSAPI_MODULE_ID)"
  [ -n "$module_id" ] || module_id="$(basename "$module_dir")"
  printf '%s\n' "$module_id"
}

zip_module_dir() {
  module_dir="$1"
  out_zip="$2"
  rm -f "$out_zip"

  if command -v zip >/dev/null 2>&1; then
    (
      cd "$module_dir"
      zip -qr "$ROOT_DIR/$out_zip" .
    )
    return 0
  fi

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$module_dir" "$ROOT_DIR/$out_zip" <<'PY'
import os
import sys
import zipfile

module_dir = sys.argv[1]
zip_path = sys.argv[2]

if not os.path.isdir(module_dir):
    raise SystemExit("source_dir_missing")

with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for root, _, files in os.walk(module_dir):
        for name in files:
            abs_path = os.path.join(root, name)
            rel_path = os.path.relpath(abs_path, module_dir)
            zf.write(abs_path, rel_path)
PY
    return 0
  fi

  echo "ksu_module_zip_error=zip_tool_missing"
  return 2
}

pack_dir() {
  module_dir="$1"
  [ -d "$module_dir" ] || { echo "ksu_module_zip_error=module_dir_missing path=$module_dir"; return 2; }
  if [ ! -f "$module_dir/dsapi.module" ]; then
    echo "ksu_module_zip_error=module_meta_missing path=$module_dir/dsapi.module"
    return 2
  fi

  module_id="$(resolve_module_id "$module_dir")"
  case "$module_id" in
    ''|*' '*|*'/'*|*'..'*)
      echo "ksu_module_zip_error=invalid_module_id id=$module_id"
      return 2
      ;;
  esac

  mkdir -p "$OUT_ROOT"
  out_zip="$OUT_ROOT/$module_id.zip"
  if ! zip_module_dir "$module_dir" "$out_zip"; then
    echo "ksu_module_zip_error=pack_failed id=$module_id dir=$module_dir"
    return 2
  fi
  chmod 0644 "$out_zip" 2>/dev/null || true
  echo "ksu_module_zip=ok id=$module_id zip=$out_zip"
}

find_module_dir_by_id() {
  wanted="$1"
  [ -n "$wanted" ] || return 1
  for module_dir in "$SRC_ROOT"/*; do
    [ -d "$module_dir" ] || continue
    [ -f "$module_dir/dsapi.module" ] || continue
    id="$(resolve_module_id "$module_dir")"
    [ "$id" = "$wanted" ] && { echo "$module_dir"; return 0; }
  done
  return 1
}

pack_one() {
  arg="$1"
  case "$arg" in
    ''|*' '*|*'/'*|*'..'*)
      echo "ksu_module_zip_error=invalid_arg value=$arg"
      return 2
      ;;
  esac

  module_dir="$SRC_ROOT/$arg"
  if [ -d "$module_dir" ] && [ -f "$module_dir/dsapi.module" ]; then
    pack_dir "$module_dir"
    return $?
  fi

  module_dir="$(find_module_dir_by_id "$arg" 2>/dev/null || true)"
  if [ -n "$module_dir" ]; then
    pack_dir "$module_dir"
    return $?
  fi

  echo "ksu_module_zip_error=module_not_found id_or_dir=$arg root=$SRC_ROOT"
  return 2
}

if [ ! -d "$SRC_ROOT" ]; then
  echo "ksu_module_zip_error=source_root_missing path=$SRC_ROOT"
  exit 2
fi

cmd="${1:-}"
case "$cmd" in
  --all)
    found=0
    for module_dir in "$SRC_ROOT"/*; do
      [ -d "$module_dir" ] || continue
      pack_dir "$module_dir"
      found=1
    done
    if [ "$found" = "0" ]; then
      echo "ksu_module_zip_warn=no_modules_found path=$SRC_ROOT"
    fi
    ;;
  -h|--help|'')
    usage
    [ -n "$cmd" ] || exit 1
    ;;
  *)
    pack_one "$cmd"
    ;;
esac
