#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

TEMPLATE_DIR="ksu/module_template"
OUT_ROOT="${DSAPI_KSU_OUT_DIR:-artifacts/ksu_module}"
STAGE_DIR="$OUT_ROOT/stage"
MOD_DIR="$STAGE_DIR/directscreenapi"
ZIP_PATH=""
STABLE_OUT_ROOT="artifacts/ksu_module"
STABLE_ZIP_PATH="$STABLE_OUT_ROOT/directscreenapi-ksu.zip"
KEEP_STAGE="${DSAPI_KSU_KEEP_STAGE:-0}"

BUILD_TOOLS_DIR="${DSAPI_ANDROID_BUILD_TOOLS_DIR:-third_party/android-sdk/build-tools/35.0.1}"
ANDROID_JAR="${DSAPI_ANDROID_JAR:-third_party/android-sdk/platforms/android-35/android.jar}"
ANDROID_JAVA_RELEASE="${DSAPI_ANDROID_JAVA_RELEASE:-21}"
MANAGER_JAVA_RELEASE="${DSAPI_MANAGER_JAVA_RELEASE:-21}"
if [ -f "third_party/android-sdk/platforms/android-23-framework.jar" ]; then
  AAPT2_ANDROID_JAR_DEFAULT="third_party/android-sdk/platforms/android-23-framework.jar"
elif [ -f "/system/framework/framework-res.apk" ]; then
  AAPT2_ANDROID_JAR_DEFAULT="/system/framework/framework-res.apk"
else
  AAPT2_ANDROID_JAR_DEFAULT="$ANDROID_JAR"
fi
AAPT2_ANDROID_JAR="${DSAPI_AAPT2_ANDROID_JAR:-$AAPT2_ANDROID_JAR_DEFAULT}"

if [ ! -d "$TEMPLATE_DIR" ]; then
  echo "ksu_pack_error=template_missing path=$TEMPLATE_DIR"
  exit 2
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "ksu_pack_error=cargo_not_found"
  exit 2
fi

# 仅打包 KSU 核心必需二进制 + 测试窗口二进制。
cargo build --release --manifest-path core/rust/Cargo.toml --bin dsapid --bin dsapictl --bin dsapi_touch_demo >/dev/null

DSAPID_BIN="target/release/dsapid"
DSAPICTL_BIN="target/release/dsapictl"
DSAPI_TOUCH_DEMO_BIN="target/release/dsapi_touch_demo"
if [ ! -x "$DSAPID_BIN" ] || [ ! -x "$DSAPICTL_BIN" ] || [ ! -x "$DSAPI_TOUCH_DEMO_BIN" ]; then
  echo "ksu_pack_error=core_binaries_missing"
  exit 2
fi

ANDROID_OUT_DIR="$OUT_ROOT/android_adapter"
adapter_build_output=""
if ! adapter_build_output="$(
  DSAPI_ANDROID_OUT_DIR="$ANDROID_OUT_DIR" \
  DSAPI_ANDROID_BUILD_MODE=all \
  DSAPI_ANDROID_JAVA_RELEASE="$ANDROID_JAVA_RELEASE" \
  DSAPI_ANDROID_JAR="$ANDROID_JAR" \
  DSAPI_ANDROID_BUILD_TOOLS_DIR="$BUILD_TOOLS_DIR" \
  ./scripts/build_android_adapter.sh 2>&1
)"; then
  [ -n "$adapter_build_output" ] && printf '%s\n' "$adapter_build_output"
  echo "ksu_pack_error=adapter_build_failed_local"
  exit 2
fi
if [ -n "$adapter_build_output" ]; then
  printf '%s\n' "$adapter_build_output" | sed -n '/^android_adapter_/p'
fi
ADAPTER_DEX="$ANDROID_OUT_DIR/directscreen-adapter-dex.jar"
if [ ! -f "$ADAPTER_DEX" ]; then
  echo "ksu_pack_error=adapter_dex_missing path=$ADAPTER_DEX"
  exit 2
fi

KSU_MODULE_ID="${DSAPI_KSU_MODULE_ID:-directscreenapi}"
KSU_MODULE_NAME="${DSAPI_KSU_MODULE_NAME:-DirectScreenAPI Core}"
VERSION="${DSAPI_KSU_VERSION:-$(date +%Y.%m.%d-%H%M%S)}"
VERSION_CODE="$(date +%s)"
RELEASE_ID_DEFAULT="r${VERSION_CODE}"
RELEASE_ID="${DSAPI_KSU_RELEASE_ID:-$RELEASE_ID_DEFAULT}"
ZIP_PATH="$OUT_ROOT/${KSU_MODULE_ID}-ksu.zip"

zip_tree() {
  src_root="$1"
  entry_name="$2"
  out_zip="$3"

  rm -f "$out_zip"
  if command -v zip >/dev/null 2>&1; then
    (
      cd "$src_root"
      zip -qr "$ROOT_DIR/$out_zip" "$entry_name"
    )
    return 0
  fi

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$src_root" "$entry_name" "$ROOT_DIR/$out_zip" <<'PY'
import os
import sys
import zipfile

src_root = sys.argv[1]
entry_name = sys.argv[2]
zip_path = sys.argv[3]
entry_path = os.path.join(src_root, entry_name)

with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for base, _, files in os.walk(entry_path):
        for name in files:
            full = os.path.join(base, name)
            rel = os.path.relpath(full, src_root)
            zf.write(full, rel)
PY
    return 0
  fi

  if command -v python >/dev/null 2>&1; then
    python - "$src_root" "$entry_name" "$ROOT_DIR/$out_zip" <<'PY'
import os
import sys
import zipfile

src_root = sys.argv[1]
entry_name = sys.argv[2]
zip_path = sys.argv[3]
entry_path = os.path.join(src_root, entry_name)

with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for base, _, files in os.walk(entry_path):
        for name in files:
            full = os.path.join(base, name)
            rel = os.path.relpath(full, src_root)
            zf.write(full, rel)
PY
    return 0
  fi

  return 1
}

KSU_WITH_MANAGER_APK="${DSAPI_KSU_WITH_MANAGER_APK:-auto}"
MANAGER_APK=""
MANAGER_OUT_DIR="$OUT_ROOT/android_manager"
if [ "$KSU_WITH_MANAGER_APK" = "auto" ]; then
  KSU_WITH_MANAGER_APK=1
fi

if [ "$KSU_WITH_MANAGER_APK" = "1" ]; then
  manager_build_output=""
  if ! manager_build_output="$(
    DSAPI_MANAGER_OUT_DIR="$MANAGER_OUT_DIR" \
    DSAPI_MANAGER_VERSION_NAME="$VERSION" \
    DSAPI_MANAGER_VERSION_CODE="$VERSION_CODE" \
    DSAPI_MANAGER_JAVA_RELEASE="$MANAGER_JAVA_RELEASE" \
    DSAPI_ANDROID_JAR="$ANDROID_JAR" \
    DSAPI_AAPT2_ANDROID_JAR="$AAPT2_ANDROID_JAR" \
    DSAPI_ANDROID_BUILD_TOOLS_DIR="$BUILD_TOOLS_DIR" \
    ./scripts/build_android_manager.sh 2>&1
  )"; then
    if [ -n "$manager_build_output" ]; then
      printf '%s\n' "$manager_build_output"
    fi
    echo "ksu_pack_error=manager_build_failed_local"
    exit 2
  fi

  if [ -n "$manager_build_output" ]; then
    printf '%s\n' "$manager_build_output" | sed -n '/^android_manager_/p'
  fi

  MANAGER_APK="$(printf '%s\n' "$manager_build_output" | awk -F= '/^android_manager_apk=/{print $2}' | tail -n 1)"
  if [ -z "$MANAGER_APK" ]; then
    MANAGER_APK="$MANAGER_OUT_DIR/dsapi-manager.apk"
  fi
  if [ ! -f "$MANAGER_APK" ]; then
    echo "ksu_pack_error=manager_apk_missing path=$MANAGER_APK"
    exit 2
  fi
fi

KSU_WITH_ZYGISK="${DSAPI_KSU_WITH_ZYGISK:-auto}"
ZYGISK_SO=""
ZYGISK_ABI=""
detect_android_abi() {
  case "$(uname -m)" in
    aarch64|arm64) echo "arm64-v8a" ;;
    arm*|armv7l) echo "armeabi-v7a" ;;
    x86_64) echo "x86_64" ;;
    i?86|x86) echo "x86" ;;
    *) echo "" ;;
  esac
}
ZYGISK_ABI="$(detect_android_abi)"
if [ "$KSU_WITH_ZYGISK" = "auto" ]; then
  KSU_WITH_ZYGISK=1
fi
if [ "$KSU_WITH_ZYGISK" = "1" ] && [ -n "$ZYGISK_ABI" ]; then
  zygisk_output=""
  if zygisk_output="$(./scripts/build_zygisk_loader.sh 2>&1)"; then
    [ -n "$zygisk_output" ] && printf '%s\n' "$zygisk_output" | sed -n '/^zygisk_loader_/p'
    ZYGISK_SO="$(printf '%s\n' "$zygisk_output" | awk -F= '/^zygisk_loader_so=/{print $2}' | tail -n 1)"
    [ -n "$ZYGISK_SO" ] || ZYGISK_SO="artifacts/ksu_module/zygisk_loader/libdsapi_zygisk_loader.so"
    if [ ! -f "$ZYGISK_SO" ]; then
      echo "ksu_pack_warn=zygisk_loader_output_missing path=$ZYGISK_SO"
      ZYGISK_SO=""
    fi
  else
    printf '%s\n' "$zygisk_output"
    if [ "${DSAPI_KSU_WITH_ZYGISK:-auto}" = "1" ]; then
      echo "ksu_pack_error=zygisk_loader_build_failed"
      exit 2
    fi
    echo "ksu_pack_warn=zygisk_loader_build_failed_auto_skip"
  fi
fi

rm -rf "$STAGE_DIR"
mkdir -p "$MOD_DIR/bin"

cp -a "$TEMPLATE_DIR/." "$MOD_DIR/"
cp "$DSAPID_BIN" "$MOD_DIR/bin/dsapid"
cp "$DSAPICTL_BIN" "$MOD_DIR/bin/dsapictl"
cp "$DSAPI_TOUCH_DEMO_BIN" "$MOD_DIR/bin/dsapi_touch_demo"
if [ -n "$MANAGER_APK" ]; then
  cp "$MANAGER_APK" "$MOD_DIR/manager.apk"
fi
if [ -n "$ZYGISK_SO" ] && [ -n "$ZYGISK_ABI" ] && [ -f "$ZYGISK_SO" ]; then
  mkdir -p "$MOD_DIR/zygisk"
  cp "$ZYGISK_SO" "$MOD_DIR/zygisk/$ZYGISK_ABI.so"
fi

if [ -d "ksu/capability_examples" ]; then
  mkdir -p "$MOD_DIR/capability_examples"
  cp "ksu/capability_examples/"*.sh "$MOD_DIR/capability_examples/" 2>/dev/null || true
fi

if [ -d "ksu/module_examples" ]; then
  mkdir -p "$MOD_DIR/module_zips"
  for module_dir in ksu/module_examples/*; do
    [ -d "$module_dir" ] || continue
    module_name="$(basename "$module_dir")"
    module_zip_rel="$MOD_DIR/module_zips/$module_name.zip"
    if ! zip_tree "ksu/module_examples" "$module_name" "$module_zip_rel"; then
      echo "ksu_pack_error=module_zip_pack_failed module=$module_name"
      exit 2
    fi
  done
fi

SEED_DIR="$MOD_DIR/runtime_seed/releases/$RELEASE_ID"
mkdir -p "$SEED_DIR/bin" "$SEED_DIR/capabilities" "$SEED_DIR/android" "$MOD_DIR/android"
cp "$DSAPID_BIN" "$SEED_DIR/bin/dsapid"
cp "$DSAPICTL_BIN" "$SEED_DIR/bin/dsapictl"
cp "$DSAPI_TOUCH_DEMO_BIN" "$SEED_DIR/bin/dsapi_touch_demo"
cp "$ADAPTER_DEX" "$SEED_DIR/android/directscreen-adapter-dex.jar"
cp "$ADAPTER_DEX" "$MOD_DIR/android/directscreen-adapter-dex.jar"

if [ -d "$TEMPLATE_DIR/capabilities" ]; then
  cp "$TEMPLATE_DIR/capabilities/"*.sh "$SEED_DIR/capabilities/" 2>/dev/null || true
fi

sed \
  -e "s/{{MODULE_ID}}/$KSU_MODULE_ID/g" \
  -e "s/{{MODULE_NAME}}/$KSU_MODULE_NAME/g" \
  -e "s/{{VERSION}}/$VERSION/g" \
  -e "s/{{VERSION_CODE}}/$VERSION_CODE/g" \
  "$TEMPLATE_DIR/module.prop.in" > "$MOD_DIR/module.prop"
rm -f "$MOD_DIR/module.prop.in"

chmod 0755 "$MOD_DIR/service.sh" "$MOD_DIR/post-fs-data.sh" "$MOD_DIR/uninstall.sh"
chmod 0755 "$MOD_DIR/action.sh"
chmod 0755 "$MOD_DIR/bin/dsapid" "$MOD_DIR/bin/dsapictl" "$MOD_DIR/bin/dsapi_touch_demo" "$MOD_DIR/bin/dsapi_service_ctl.sh" "$MOD_DIR/bin/dsapi_ksu_lib.sh"
chmod 0755 "$MOD_DIR/capability_examples/"*.sh 2>/dev/null || true
chmod 0755 "$SEED_DIR/bin/dsapid" "$SEED_DIR/bin/dsapictl" "$SEED_DIR/bin/dsapi_touch_demo"
chmod 0755 "$SEED_DIR/capabilities/"*.sh 2>/dev/null || true
chmod 0444 "$SEED_DIR/android/directscreen-adapter-dex.jar" "$MOD_DIR/android/directscreen-adapter-dex.jar" 2>/dev/null || true
if [ -f "$MOD_DIR/manager.apk" ]; then
  chmod 0444 "$MOD_DIR/manager.apk" 2>/dev/null || true
fi
if [ -n "$ZYGISK_ABI" ] && [ -f "$MOD_DIR/zygisk/$ZYGISK_ABI.so" ]; then
  chmod 0444 "$MOD_DIR/zygisk/$ZYGISK_ABI.so" 2>/dev/null || true
fi

mkdir -p "$OUT_ROOT"
rm -f "$ZIP_PATH"
if command -v zip >/dev/null 2>&1; then
  (
    cd "$MOD_DIR"
    zip -qr "$ROOT_DIR/$ZIP_PATH" .
  )
elif command -v python >/dev/null 2>&1; then
  MOD_DIR_ABS="$MOD_DIR"
  ZIP_PATH_ABS="$ROOT_DIR/$ZIP_PATH"
  python - "$MOD_DIR_ABS" "$ZIP_PATH_ABS" <<'PY'
import os
import sys
import zipfile

mod_dir = sys.argv[1]
zip_path = sys.argv[2]

with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for base, _, files in os.walk(mod_dir):
        for name in files:
            full = os.path.join(base, name)
            rel = os.path.relpath(full, mod_dir)
            zf.write(full, rel)
PY
else
  echo "ksu_pack_error=zip_tool_missing"
  exit 2
fi

echo "ksu_pack_status=ok"
echo "ksu_pack_zip=$ZIP_PATH"
echo "ksu_pack_module_id=$KSU_MODULE_ID"
echo "ksu_pack_module_name=$KSU_MODULE_NAME"
echo "ksu_pack_version=$VERSION"
echo "ksu_pack_release=$RELEASE_ID"
if [ -n "$MANAGER_APK" ]; then
  echo "ksu_pack_manager_apk=embedded"
else
  echo "ksu_pack_manager_apk=not_embedded"
fi
if [ -n "$ZYGISK_ABI" ] && [ -f "$MOD_DIR/zygisk/$ZYGISK_ABI.so" ]; then
  echo "ksu_pack_zygisk=embedded abi=$ZYGISK_ABI"
else
  echo "ksu_pack_zygisk=not_embedded"
fi

# 始终同步一份固定文件名，兼容旧脚本/现有刷包路径。
if [ "$ZIP_PATH" != "$STABLE_ZIP_PATH" ]; then
  mkdir -p "$STABLE_OUT_ROOT"
  cp -f "$ZIP_PATH" "$STABLE_ZIP_PATH"
fi
echo "ksu_pack_stable_zip=$STABLE_ZIP_PATH"

if [ "$KEEP_STAGE" != "1" ] && [ -d "$STAGE_DIR" ]; then
  rm -rf "$STAGE_DIR"
  echo "ksu_pack_stage=cleaned"
else
  echo "ksu_pack_stage=kept"
fi
