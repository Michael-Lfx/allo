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

exec dx serve --hot-patch -p flowy-desktop --bin flowy-app --platform macos --renderer native --open false
