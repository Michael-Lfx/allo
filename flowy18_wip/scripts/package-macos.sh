#!/bin/sh
# Assemble a signed macOS app for one compile-time brand.
# Usage: scripts/package-macos.sh [brand-id]
# SIGN_IDENTITY must be a codesign identity. Certificates stay out of the repo.
set -eu

ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
BRAND="${1:-${FLOWY_BRAND:-flowy}}"
if [ -z "${SIGN_IDENTITY:-}" ]; then
  echo "SIGN_IDENTITY is required so the package is actually signed" >&2
  exit 1
fi

META="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps)"
TARGET="$(printf '%s' "$META" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
VERSION="$(python3 -c 'import pathlib,re,sys; text=pathlib.Path(sys.argv[1]).read_text(); print(re.search(r"(?m)^version = \"([^\"]+)\"", text).group(1))' "$ROOT/Cargo.toml")"

export FLOWY_BRAND="$BRAND"
(cd "$ROOT" && cargo build --release -p flowy-app)

BRAND_DIR="$ROOT/brands/$BRAND"
eval "$(python3 - "$BRAND_DIR/brand.toml" <<'PY'
import pathlib, shlex, sys
fields = {}
for raw in pathlib.Path(sys.argv[1]).read_text().splitlines():
    line = raw.strip()
    if not line or line.startswith("#") or "=" not in line:
        continue
    key, value = line.split("=", 1)
    fields[key.strip()] = value.strip().strip('"')
needed = {
    "PRODUCT": "product",
    "BUNDLE_ID": "bundle_id",
    "ICON_NAME": "icon",
    "SCHEME": "url_scheme",
}
missing = [name for name, key in needed.items() if not fields.get(key)]
if missing:
    raise SystemExit("brand.toml missing " + ", ".join(missing))
for name, key in needed.items():
    print(f"{name}={shlex.quote(fields[key])}")
PY
)"

BIN="$TARGET/release/flowy-app"
if [ ! -x "$BIN" ]; then
  echo "missing binary $BIN" >&2
  exit 1
fi
ICON="$BRAND_DIR/$ICON_NAME"
if [ ! -f "$ICON" ]; then
  echo "missing icon $ICON" >&2
  exit 1
fi

APP="$ROOT/dist/$PRODUCT.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/flowy-app"
cp "$ICON" "$APP/Contents/Resources/icon.icns"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>flowy-app</string>
  <key>CFBundleIconFile</key>
  <string>icon</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleDisplayName</key>
  <string>${PRODUCT}</string>
  <key>CFBundleName</key>
  <string>${PRODUCT}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key>
      <string>${BUNDLE_ID}</string>
      <key>CFBundleURLSchemes</key>
      <array>
        <string>${SCHEME}</string>
      </array>
    </dict>
  </array>
</dict>
</plist>
EOF
codesign --force --sign "$SIGN_IDENTITY" --identifier "$BUNDLE_ID" "$APP"

DIST="$ROOT/dist"
mkdir -p "$DIST"
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) PLATFORM_KEY="darwin-aarch64" ;;
  x86_64) PLATFORM_KEY="darwin-x86_64" ;;
  *)
    echo "unsupported macOS arch: $ARCH" >&2
    exit 1
    ;;
esac

ARTIFACT_NAME="${PRODUCT}_${VERSION}_macos.app.tar.gz"
ARTIFACT="$DIST/$ARTIFACT_NAME"
TAR_ROOT="$DIST/.app-stage"
rm -rf "$TAR_ROOT"
mkdir -p "$TAR_ROOT"
cp -R "$APP" "$TAR_ROOT/"
(
  cd "$TAR_ROOT"
  tar -czf "$ARTIFACT" "$(basename "$APP")"
)
rm -rf "$TAR_ROOT"

ROLLOUT="${OTA_ROLLOUT:-100}"
PUB_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
NOTES="${OTA_NOTES:-}"
SIGNATURE="${OTA_SIGNATURE:-}"
DOWNLOAD_URL="${OTA_DOWNLOAD_URL:-}"
if [ -z "$DOWNLOAD_URL" ]; then
  PREFIX="$(python3 - "$BRAND_DIR/brand.toml" <<'PY'
import pathlib,sys
fields={}
for raw in pathlib.Path(sys.argv[1]).read_text().splitlines():
    line=raw.strip()
    if not line or line.startswith('#') or '=' not in line: continue
    k,v=line.split('=',1)
    fields[k.strip()]=v.strip().strip('"')
print(fields.get('ota_prefix','allo'))
PY
)"
  DOWNLOAD_URL="https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo?Revision=master&FilePath=${PREFIX}/macos/v${VERSION}/${ARTIFACT_NAME}"
fi

MANIFEST="$DIST/latest-macos.json"
python3 - "$MANIFEST" "$VERSION" "$PUB_DATE" "$NOTES" "$SIGNATURE" "$DOWNLOAD_URL" "$PLATFORM_KEY" "$ROLLOUT" <<'PY'
import json,sys
path, version, pub_date, notes, signature, url, platform, rollout = sys.argv[1:9]
payload = {
    "version": version,
    "notes": notes,
    "pub_date": pub_date,
    "platforms": {
        platform: {
            "signature": signature,
            "url": url,
        }
    },
    "rollout": int(rollout),
}
pathlib = __import__('pathlib')
pathlib.Path(path).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(path)
PY

echo "$APP"
echo "$ARTIFACT"
echo "$MANIFEST"
