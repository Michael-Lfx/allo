#!/usr/bin/env bash
# State-preserving GPUI hot patch via dx (dioxus-cli 0.7.10) + gpui hot-patching.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! command -v dx >/dev/null 2>&1; then
  echo "dx not found. Install: cargo install dioxus-cli --version 0.7.10 --locked" >&2
  exit 1
fi

ver="$(dx --version 2>&1 || true)"
case "$ver" in
  *0.7.10*) ;;
  *)
    echo "need dx 0.7.10 (gpui subsecond), got: $ver" >&2
    exit 1
    ;;
esac

serve_help="$(dx serve --help 2>&1)" || {
  echo "failed to inspect dx serve options" >&2
  exit 1
}
if grep -q -- '--hotpatch' <<<"$serve_help"; then
  hot_patch_flag='--hotpatch'
elif grep -q -- '--hot-patch' <<<"$serve_help"; then
  hot_patch_flag='--hot-patch'
else
  echo "dx serve does not expose --hotpatch or --hot-patch" >&2
  exit 1
fi

exec dx serve "$hot_patch_flag" --package flowy-desktop --bin flowy-app --platform macos --renderer native --open false
