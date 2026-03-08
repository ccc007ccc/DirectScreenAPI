#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

API_LEVEL="${1:-35}"
OUT_DIR="${DSAPI_ANDROID_SDK_DIR:-third_party/android-sdk/platforms/android-$API_LEVEL}"
OUT_JAR="$OUT_DIR/android.jar"

if [ -f "$OUT_JAR" ]; then
  echo "android_jar_status=exists path=$OUT_JAR"
  exit 0
fi

mkdir -p "$OUT_DIR" artifacts/run

python - "$API_LEVEL" "$OUT_JAR" <<'PY'
import sys, urllib.request, xml.etree.ElementTree as ET, zipfile, pathlib
api = sys.argv[1]
out_jar = pathlib.Path(sys.argv[2])
repo = 'https://dl.google.com/android/repository/repository2-1.xml'
xml = urllib.request.urlopen(repo, timeout=30).read()
root = ET.fromstring(xml)
path = f'platforms;android-{api}'
url = None
for rp in root.findall('remotePackage'):
    if rp.attrib.get('path') == path:
        comp = rp.find('./archives/archive/complete')
        if comp is not None:
            url = comp.findtext('url')
        break
if not url:
    raise SystemExit(f'android_jar_error=platform_not_found api={api}')
full = 'https://dl.google.com/android/repository/' + url
zip_path = pathlib.Path('artifacts/run') / url
print(f'android_jar_download url={full}')
if not zip_path.exists():
    with urllib.request.urlopen(full, timeout=120) as r, open(zip_path, 'wb') as f:
        while True:
            b = r.read(1 << 20)
            if not b:
                break
            f.write(b)
with zipfile.ZipFile(zip_path, 'r') as z:
    candidates = [n for n in z.namelist() if n.endswith('/android.jar') or n == 'android.jar']
    if not candidates:
        raise SystemExit('android_jar_error=jar_missing_in_archive')
    data = z.read(candidates[0])
out_jar.parent.mkdir(parents=True, exist_ok=True)
out_jar.write_bytes(data)
print(f'android_jar_status=ready path={out_jar} bytes={out_jar.stat().st_size}')
PY
