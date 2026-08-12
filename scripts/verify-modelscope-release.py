#!/usr/bin/env python3
"""Verify a published Flowy ModelScope updater manifest with cache retries."""

from __future__ import annotations

import argparse
import json
import time
import urllib.error
import urllib.request
from urllib.parse import quote

PLATFORM_FOLDERS = {
    "windows-x86_64": "windows",
    "windows-aarch64": "windows",
    "darwin-x86_64": "macos",
    "darwin-aarch64": "macos",
    "linux-x86_64": "linux",
    "linux-aarch64": "linux",
}

PLATFORM_CHANNELS = ("windows", "macos", "linux")


def channel_manifest_url(channel: str) -> str:
    path = f"allo/channels/{channel}/latest.json"
    return (
        "https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo"
        f"?Revision=master&FilePath={quote(path, safe='/')}"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Verify the published ModelScope updater manifest")
    parser.add_argument("--version", required=True, help="Expected version without the v prefix")
    parser.add_argument("--platform", action="append", required=True, help="Required platform key")
    parser.add_argument(
        "--channel",
        choices=PLATFORM_CHANNELS,
        help="OTA channel (sets default --url to allo/channels/<channel>/latest.json)",
    )
    parser.add_argument("--url", default=None, help="Updater manifest URL")
    parser.add_argument("--attempts", type=int, default=5, help="Maximum fetch attempts")
    parser.add_argument("--retry-delay", type=float, default=10, help="Seconds between attempts")
    args = parser.parse_args()

    if args.url:
        url = args.url
    elif args.channel:
        url = channel_manifest_url(args.channel)
    else:
        # Infer channel from the first required platform key.
        folder = PLATFORM_FOLDERS.get(args.platform[0])
        if not folder:
            raise SystemExit(f"ERROR: unsupported platform key: {args.platform[0]}")
        url = channel_manifest_url(folder)

    last_error = "manifest was not fetched"
    for attempt in range(1, args.attempts + 1):
        try:
            request = urllib.request.Request(
                url,
                headers={"Cache-Control": "no-cache", "User-Agent": "flowy-release-verifier"},
            )
            with urllib.request.urlopen(request, timeout=30) as response:
                manifest = json.loads(response.read().decode("utf-8"))

            actual_version = str(manifest.get("version", ""))
            if actual_version != args.version:
                raise ValueError(f"expected version {args.version}, got {actual_version or '<missing>'}")

            platforms = manifest.get("platforms") or {}
            missing = sorted(set(args.platform) - set(platforms))
            if missing:
                raise ValueError(f"missing platform entries: {', '.join(missing)}")

            version_tag = args.version if args.version.startswith("v") else f"v{args.version}"
            for key in args.platform:
                folder = PLATFORM_FOLDERS.get(key)
                if not folder:
                    raise ValueError(f"unsupported platform key: {key}")
                entry_url = str((platforms.get(key) or {}).get("url", ""))
                needle = f"/{folder}/{version_tag}/"
                if needle not in entry_url:
                    raise ValueError(
                        f"{key} url does not use platform path ...{needle}...: {entry_url or '<missing>'}"
                    )

            print(f"Verified ModelScope v{args.version} via {url}: {', '.join(args.platform)}")
            return
        except (OSError, TimeoutError, ValueError, json.JSONDecodeError, urllib.error.URLError) as exc:
            last_error = str(exc)
            if attempt < args.attempts:
                print(f"Attempt {attempt}/{args.attempts} failed: {last_error}; retrying...")
                time.sleep(args.retry_delay)

    raise SystemExit(f"ERROR: ModelScope verification failed: {last_error}")


if __name__ == "__main__":
    main()
