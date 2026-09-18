#!/usr/bin/env bash
# Merge two single-arch Flowy.app bundles into a universal app, DMG, and
# updater tar.gz (+ .sig). Must run on macOS. Used by the ModelScope release
# workflow after native arm64 and x86_64 jobs finish.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "merge-macos-universal.sh must run on macOS" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PRODUCT="Flowy"

ARM64_ROOT=""
INTEL_ROOT=""
VERSION=""
OUT_DIST="$ROOT/dist/desktop"

usage() {
  echo "usage: merge-macos-universal.sh --arm64 <dir> --intel <dir> --version <x.y.z> [--out-dir <dir>]" >&2
  exit 1
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --arm64)
      shift
      [[ "$#" -gt 0 ]] || usage
      ARM64_ROOT="$1"
      shift
      ;;
    --intel)
      shift
      [[ "$#" -gt 0 ]] || usage
      INTEL_ROOT="$1"
      shift
      ;;
    --version)
      shift
      [[ "$#" -gt 0 ]] || usage
      VERSION="$1"
      shift
      ;;
    --out-dir)
      shift
      [[ "$#" -gt 0 ]] || usage
      OUT_DIST="$1"
      shift
      ;;
    *)
      usage
      ;;
  esac
done

[[ -n "$ARM64_ROOT" && -n "$INTEL_ROOT" && -n "$VERSION" ]] || usage

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "TAURI_SIGNING_PRIVATE_KEY is not set" >&2
  exit 1
fi

find_app() {
  local root="$1"
  if [[ -d "$root/$PRODUCT.app" ]]; then
    printf '%s\n' "$root/$PRODUCT.app"
    return 0
  fi
  local found=""
  found="$(find "$root" -type d -name "$PRODUCT.app" 2>/dev/null | head -n 1 || true)"
  if [[ -z "$found" ]]; then
    echo "no $PRODUCT.app under $root" >&2
    return 1
  fi
  printf '%s\n' "$found"
}

is_macho() {
  file -b "$1" 2>/dev/null | grep -q 'Mach-O'
}

relpath_under() {
  local root="$1"
  local full="$2"
  local prefix="${root%/}/"
  printf '%s\n' "${full#"$prefix"}"
}

ARM_APP="$(find_app "$ARM64_ROOT")"
INTEL_APP="$(find_app "$INTEL_ROOT")"

require_slice_arch() {
  local label="$1"
  local app="$2"
  local want="$3"
  local main="$app/Contents/MacOS/$PRODUCT"
  local rg="$app/Contents/Resources/bin/rg"
  if [[ ! -f "$main" ]]; then
    echo "$label missing main binary: $main" >&2
    exit 1
  fi
  local main_archs rg_archs
  main_archs="$(lipo -archs "$main")"
  echo "$label $PRODUCT archs: $main_archs"
  echo "$main_archs" | grep -qw "$want" || {
    echo "$label $PRODUCT must include $want (got: $main_archs)" >&2
    exit 1
  }
  # Reject fat/wrong host-arch sidecars from a cross-compile that bundled the
  # runner's ripgrep instead of the target's (see ensure-bundled-rg.mjs).
  if [[ -f "$rg" ]]; then
    rg_archs="$(lipo -archs "$rg")"
    echo "$label rg archs: $rg_archs"
    if [[ "$rg_archs" != "$want" ]]; then
      echo "$label rg must be exactly $want (got: $rg_archs). Re-run ensure-bundled-rg with TAURI_ENV_TARGET_TRIPLE." >&2
      exit 1
    fi
  fi
}

require_slice_arch "arm64" "$ARM_APP" "arm64"
require_slice_arch "intel" "$INTEL_APP" "x86_64"

BUNDLE_MACOS="$ROOT/target/universal-apple-darwin/release/bundle/macos"
BUNDLE_DMG="$ROOT/target/universal-apple-darwin/release/bundle/dmg"
UNIV_APP="$BUNDLE_MACOS/$PRODUCT.app"

rm -rf "$BUNDLE_MACOS" "$BUNDLE_DMG"
mkdir -p "$BUNDLE_MACOS" "$BUNDLE_DMG"
ditto "$ARM_APP" "$UNIV_APP"

# Fat-binaries for every Mach-O that exists in both slices (main exe, rg, dylibs).
while IFS= read -r -d '' arm_file; do
  rel="$(relpath_under "$UNIV_APP" "$arm_file")"
  intel_file="$INTEL_APP/$rel"
  dest="$UNIV_APP/$rel"
  [[ -f "$intel_file" ]] || continue
  is_macho "$dest" || continue
  is_macho "$intel_file" || continue
  tmp="$dest.lipo.$$"
  lipo -create "$dest" "$intel_file" -output "$tmp"
  mv -f "$tmp" "$dest"
  chmod +x "$dest" || true
done < <(find "$UNIV_APP" -type f -print0)

while IFS= read -r -d '' intel_file; do
  rel="$(relpath_under "$INTEL_APP" "$intel_file")"
  dest="$UNIV_APP/$rel"
  if [[ ! -e "$dest" ]]; then
    mkdir -p "$(dirname "$dest")"
    ditto "$intel_file" "$dest"
  fi
done < <(find "$INTEL_APP" -type f -print0)

MAIN_BIN="$UNIV_APP/Contents/MacOS/$PRODUCT"
if [[ ! -f "$MAIN_BIN" ]]; then
  echo "missing universal binary $MAIN_BIN" >&2
  exit 1
fi
archs="$(lipo -archs "$MAIN_BIN")"
echo "universal $PRODUCT archs: $archs"
echo "$archs" | grep -q 'x86_64' || { echo "x86_64 slice missing" >&2; exit 1; }
echo "$archs" | grep -q 'arm64' || { echo "arm64 slice missing" >&2; exit 1; }

codesign --force --deep --sign - --timestamp=none "$UNIV_APP"

TAR="$BUNDLE_MACOS/$PRODUCT.app.tar.gz"
rm -f "$TAR"
tar -C "$BUNDLE_MACOS" -czf "$TAR" "$PRODUCT.app"

sign_artifact() {
  local file="$1"
  (cd "$ROOT" && bun x tauri signer sign "$file")
}

sign_artifact "$TAR"

DMG="$BUNDLE_DMG/${PRODUCT}_${VERSION}_universal.dmg"
rm -f "$DMG"
hdiutil create -volname "$PRODUCT" -srcfolder "$UNIV_APP" -ov -format UDZO "$DMG"
sign_artifact "$DMG"

mkdir -p "$OUT_DIST"
cp -f "$TAR" "$TAR.sig" "$DMG" "$DMG.sig" "$OUT_DIST/"

echo "wrote:"
ls -lah "$TAR" "$TAR.sig" "$DMG" "$DMG.sig"
