#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [ -n "${DSAPI_DEX_TOOL_BIN:-}" ] && [ -x "${DSAPI_DEX_TOOL_BIN}" ]; then
  echo "d8_tool=${DSAPI_DEX_TOOL_BIN}"
  exit 0
fi

# Prefer the repo-local wrapper when available (works with Java 21 class files on Termux).
LOCAL_D8_WRAPPER="$ROOT_DIR/.cache/d8/d8-wrapper.sh"
if [ -x "$LOCAL_D8_WRAPPER" ]; then
  echo "ensure_d8_local_wrapper=$LOCAL_D8_WRAPPER"
  echo "d8_tool=$LOCAL_D8_WRAPPER"
  exit 0
fi

BUILD_TOOLS_DIR_DEFAULT="third_party/android-sdk/build-tools/35.0.1"
BUILD_TOOLS_DIR="${DSAPI_ANDROID_BUILD_TOOLS_DIR:-$BUILD_TOOLS_DIR_DEFAULT}"
if [ -x "$BUILD_TOOLS_DIR/d8" ]; then
  echo "d8_tool=$BUILD_TOOLS_DIR/d8"
  exit 0
fi

if ! command -v java >/dev/null 2>&1; then
  echo "ensure_d8_error=java_not_found"
  exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "ensure_d8_error=python3_not_found"
  exit 2
fi

CACHE_DIR="${DSAPI_R8_CACHE_DIR:-${HOME:-/tmp}/.cache/dsapi-build-tools}"
JAR_PATH="$CACHE_DIR/r8.jar"
D8_WRAPPER="$CACHE_DIR/d8"
mkdir -p "$CACHE_DIR"

python3 - "$JAR_PATH" <<'PY'
import pathlib
import re
import sys
import urllib.request

jar_path = pathlib.Path(sys.argv[1])
meta_url = 'https://dl.google.com/dl/android/maven2/com/android/tools/r8/maven-metadata.xml'
xml = urllib.request.urlopen(meta_url, timeout=30).read().decode('utf-8', 'ignore')
versions = re.findall(r'<version>([^<]+)</version>', xml)
if not versions:
    raise SystemExit('ensure_d8_error=no_r8_versions')
version = versions[-1]
url = f'https://dl.google.com/dl/android/maven2/com/android/tools/r8/{version}/r8-{version}.jar'
print(f'ensure_d8_r8_version={version}')
if not jar_path.exists() or jar_path.stat().st_size < 10_000_000:
    print(f'ensure_d8_download={url}')
    with urllib.request.urlopen(url, timeout=180) as r:
        data = r.read()
    jar_path.write_bytes(data)
print(f'ensure_d8_jar={jar_path}')
print(f'ensure_d8_size={jar_path.stat().st_size}')
PY

cat > "$D8_WRAPPER" <<'SH'
#!/bin/sh
exec java -cp "$HOME/.cache/dsapi-build-tools/r8.jar" com.android.tools.r8.D8 "$@"
SH
# 当 CACHE_DIR 不是 HOME 默认路径时，改写 wrapper 中 jar 路径。
if [ "$CACHE_DIR" != "${HOME:-/tmp}/.cache/dsapi-build-tools" ]; then
  sed -i "s#\$HOME/.cache/dsapi-build-tools/r8.jar#$JAR_PATH#g" "$D8_WRAPPER"
fi
chmod +x "$D8_WRAPPER"

echo "d8_tool=$D8_WRAPPER"
