#!/usr/bin/env bash
# Upload this channel's updater packages + latest-{channel}.json to the current
# GitHub repository's Release for TAG.
set -euo pipefail

TAG="${1:?tag}"
CHANNEL="${2:?channel}"
DIST="${3:-dist/desktop}"
REPO="${GITHUB_REPOSITORY:-Michael-Lfx/allo}"

case "$CHANNEL" in
  windows|macos|linux) ;;
  *)
    echo "unknown OTA channel: $CHANNEL" >&2
    exit 1
    ;;
esac

test -d "$DIST" || {
  echo "dist dir not found: $DIST" >&2
  exit 1
}

bun run make:latest --host github --repo "$REPO" --channel "$CHANNEL" --from-dir "$DIST" --collect
cp "$DIST/latest.json" "$DIST/latest-${CHANNEL}.json"

assets=()
shopt -s nullglob
for path in "$DIST"/*; do
  name="$(basename "$path")"
  case "$name" in
    *-setup.exe|*-setup.exe.sig|*.app.tar.gz|*.app.tar.gz.sig|*.AppImage|*.AppImage.sig)
      assets+=("$path")
      ;;
  esac
done
assets+=("$DIST/latest-${CHANNEL}.json")

if [[ "${#assets[@]}" -lt 2 ]]; then
  echo "no updater artifacts to upload from $DIST" >&2
  exit 1
fi

if ! gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  gh release create "$TAG" --repo "$REPO" --title "$TAG" --notes "Flowy $TAG"
fi

gh release upload "$TAG" --repo "$REPO" "${assets[@]}" --clobber
echo "GitHub Release $TAG updated with ${#assets[@]} asset(s) for $CHANNEL"
