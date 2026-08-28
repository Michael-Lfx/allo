#!/usr/bin/env python3
"""Roll back a ModelScope OTA channel pointer to a previous history snapshot.

Usage:
  MODELSCOPE_TOKEN=... python scripts/rollback-modelscope-release.py \\
    --channel windows --to-version 1.0.9

Snapshots are written on each successful ``--manifest-only`` / full upload as:
  allo/channels/{channel}/history/v{version}.json
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import quote

DEFAULT_REPO = "flowy2025/flowyaipc"
DEFAULT_PREFIX = "allo"
DEFAULT_ENV_FILE = Path(__file__).resolve().parent.parent / "apps/desktop/signing/.env.modelscope"
PLATFORM_CHANNELS = ("windows", "macos", "linux")


def load_env_file(path: Path) -> None:
    if not path.is_file():
        return
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export ") :].strip()
        if "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        if key and key not in os.environ:
            os.environ[key] = value


def modelscope_file_url(repo: str, path_in_repo: str) -> str:
    return (
        f"https://modelscope.cn/api/v1/models/{repo}/repo"
        f"?Revision=master&FilePath={quote(path_in_repo, safe='/')}"
    )


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(url, headers={"User-Agent": "flowy-release-rollback"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.loads(response.read().decode("utf-8"))


def build_channel_yml(manifest: dict, channel: str) -> str:
    lines = [
        f'version: "{manifest.get("version", "")}"',
        f"channel: {channel}",
        f'pub_date: "{manifest.get("pub_date", "")}"',
        f"manifest: channels/{channel}/latest.json",
    ]
    notes = manifest.get("notes")
    if isinstance(notes, str) and notes.strip():
        lines.append("notes: |")
        for note_line in notes.strip().splitlines():
            lines.append(f"  {note_line}")
    else:
        lines.append('notes: ""')
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Roll back ModelScope channel pointer")
    parser.add_argument("--repo", default=DEFAULT_REPO)
    parser.add_argument("--prefix", default=DEFAULT_PREFIX)
    parser.add_argument("--channel", required=True, choices=PLATFORM_CHANNELS)
    parser.add_argument(
        "--to-version",
        required=True,
        help="Target version with or without v prefix (must exist under channels/.../history/)",
    )
    parser.add_argument("--env-file", default=str(DEFAULT_ENV_FILE))
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    load_env_file(Path(args.env_file))
    version = args.to_version.strip()
    if version.startswith("v"):
        version = version[1:]
    version_tag = f"v{version}"
    prefix = args.prefix.strip("/")
    repo = args.repo
    channel = args.channel

    history_path = f"{prefix}/channels/{channel}/history/{version_tag}.json"
    history_url = modelscope_file_url(repo, history_path)
    try:
        manifest = fetch_json(history_url)
    except (urllib.error.URLError, TimeoutError, OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"ERROR: could not fetch history snapshot {history_path}: {exc}") from exc

    if str(manifest.get("version", "")).strip() != version:
        raise SystemExit(
            f"ERROR: history snapshot version {manifest.get('version')!r} != requested {version!r}"
        )

    print(f"Rollback {channel} -> {version_tag}")
    print(f"  Snapshot: {history_url}")
    print(f"  Platforms: {', '.join(sorted((manifest.get('platforms') or {}).keys()))}")

    if args.dry_run:
        print("Dry run — no uploads performed.")
        return

    token = os.environ.get("MODELSCOPE_TOKEN")
    if not token:
        raise SystemExit("ERROR: MODELSCOPE_TOKEN not set")

    try:
        from modelscope.hub.api import HubApi
    except ImportError as exc:
        raise SystemExit("ERROR: modelscope package not installed. Run: pip install modelscope") from exc

    tmp = Path(".rollback-modelscope")
    tmp.mkdir(exist_ok=True)
    latest_local = tmp / "latest.json"
    channel_local = tmp / "channel.yml"
    latest_local.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    channel_local.write_text(build_channel_yml(manifest, channel), encoding="utf-8")

    api = HubApi()
    api.login(token)
    for local, remote, label in (
        (channel_local, f"{prefix}/channels/{channel}/channel.yml", "channel.yml"),
        (latest_local, f"{prefix}/channels/{channel}/latest.json", "latest.json"),
    ):
        api.upload_file(
            path_or_fileobj=str(local),
            path_in_repo=remote,
            repo_id=repo,
            repo_type="model",
            commit_message=f"Rollback {channel} channel pointer to {version_tag}",
        )
        print(f"  [OK] {label} -> {remote}")

    print(f"Rollback complete — {channel} now points at {version_tag}")


if __name__ == "__main__":
    main()
