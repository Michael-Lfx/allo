#!/usr/bin/env python3
"""Upload Flowy (allo) Tauri updater artifacts to ModelScope.

Model repo layout (root = ``allo/`` under flowy2025/flowyaipc):

    allo/
    ├── channels/windows/latest.json   # Windows-only updater manifest
    ├── channels/macos/latest.json     # macOS-only updater manifest
    ├── channels/linux/latest.json     # Linux-only updater manifest
    ├── channels/{platform}/channel.yml
    ├── windows/v{version}/...
    ├── macos/v{version}/...
    └── linux/v{version}/...

Shared ``channels/alpha/latest.json`` is deprecated. Each platform channel has an
independent ``version`` so release cadence can diverge.

Run ``bun run make:latest --host modelscope --channel <platform> --collect`` first.

Requires ``MODELSCOPE_TOKEN`` and ``pip install modelscope``.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from urllib.parse import parse_qs, quote, unquote, urlparse

DEFAULT_REPO = "flowy2025/flowyaipc"
DEFAULT_PREFIX = "allo"
DEFAULT_ENV_FILE = Path(__file__).resolve().parent.parent / "apps/desktop/signing/.env.modelscope"
PLATFORM_CHANNELS = ("windows", "macos", "linux")
CHANNEL_KEYS = {
    "windows": frozenset({"windows-x86_64", "windows-aarch64"}),
    "macos": frozenset({"darwin-x86_64", "darwin-aarch64"}),
    "linux": frozenset({"linux-x86_64", "linux-aarch64"}),
}

UPDATER_SUFFIXES = (
    "-setup.exe",
    ".app.tar.gz",
    ".AppImage",
)


def load_env_file(path: Path) -> None:
    """Load KEY=VALUE lines into os.environ when the key is not already set."""
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
    """Public ModelScope repo file URL."""
    return (
        f"https://modelscope.cn/api/v1/models/{repo}/repo"
        f"?Revision=master&FilePath={quote(path_in_repo, safe='/')}"
    )


def artifact_basename_from_url(url: str) -> str:
    if not url:
        return ""
    parsed = urlparse(url)
    file_path = parse_qs(parsed.query).get("FilePath", [""])[0]
    if file_path:
        return Path(unquote(file_path)).name
    return Path(unquote(parsed.path)).name


def platform_folder_for_key(key: str) -> str:
    """Map Tauri platform key to ModelScope directory under allo/."""
    if key.startswith("windows-"):
        return "windows"
    if key.startswith("darwin-"):
        return "macos"
    if key.startswith("linux-"):
        return "linux"
    raise ValueError(f"unsupported platform key: {key}")


def infer_channel_from_manifest(manifest: dict) -> str | None:
    keys = set((manifest.get("platforms") or {}).keys())
    if not keys:
        return None
    for channel, allowed in CHANNEL_KEYS.items():
        if keys <= allowed:
            return channel
    return None


def remote_dir_for_artifact(
    manifest: dict, artifact_name: str, prefix: str, version_tag: str
) -> str:
    """Resolve allo/{platform}/v{version} for a package or its .sig file."""
    base = artifact_name[:-4] if artifact_name.endswith(".sig") else artifact_name
    for key, entry in (manifest.get("platforms") or {}).items():
        if artifact_basename_from_url(str(entry.get("url", ""))) == base:
            return f"{prefix}/{platform_folder_for_key(key)}/{version_tag}"
    raise SystemExit(
        f"ERROR: cannot map artifact {artifact_name} to allo/{{platform}}/{version_tag} "
        "via latest.json platform URLs"
    )


def filter_manifest_to_local_platforms(manifest: dict, dist_dir: Path) -> tuple[dict, list[str]]:
    """Keep only platform entries whose updater artifact exists in dist-dir."""
    platforms = dict(manifest.get("platforms") or {})
    kept: dict[str, dict] = {}
    dropped: list[str] = []
    for key, entry in platforms.items():
        filename = artifact_basename_from_url(str(entry.get("url", "")))
        if filename and (dist_dir / filename).is_file():
            kept[key] = entry
        else:
            dropped.append(key)
    filtered = dict(manifest)
    filtered["platforms"] = kept
    return filtered, dropped


def filter_manifest_to_channel(manifest: dict, channel: str) -> tuple[dict, list[str]]:
    """Drop platform keys that do not belong to this OTA channel."""
    allowed = CHANNEL_KEYS[channel]
    platforms = dict(manifest.get("platforms") or {})
    kept: dict[str, dict] = {}
    dropped: list[str] = []
    for key, entry in platforms.items():
        if key in allowed:
            kept[key] = entry
        else:
            dropped.append(key)
    filtered = dict(manifest)
    filtered["platforms"] = kept
    return filtered, dropped


def merge_remote_same_channel(manifest: dict, remote: dict, channel: str) -> dict:
    """Fill missing same-channel keys from remote (same version only)."""
    merged = dict(manifest)
    local_platforms = dict(manifest.get("platforms") or {})
    remote_platforms = dict(remote.get("platforms") or {})
    if str(remote.get("version", "")).strip() != str(manifest.get("version", "")).strip():
        return merged
    allowed = CHANNEL_KEYS[channel]
    for key, entry in remote_platforms.items():
        if key not in allowed:
            continue
        if key not in local_platforms and entry.get("url") and entry.get("signature"):
            local_platforms[key] = entry
    merged["platforms"] = local_platforms
    return merged


def fetch_remote_latest(repo: str, prefix: str, channel: str) -> dict | None:
    """Best-effort download of the current channel manifest from ModelScope."""
    import urllib.error
    import urllib.request

    url = modelscope_file_url(repo, f"{prefix}/channels/{channel}/latest.json")
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, json.JSONDecodeError, TimeoutError):
        return None


def collect_updater_artifacts(dist_dir: Path) -> list[Path]:
    """Collect updater packages and their required detached signatures."""
    found: list[Path] = []
    for path in sorted(dist_dir.iterdir()):
        if not path.is_file():
            continue
        name = path.name
        if name.endswith(".sig") or name in {"latest.json", "channel.yml", "alpha.yml"}:
            continue
        if any(name.endswith(suffix) for suffix in UPDATER_SUFFIXES):
            sig = dist_dir / f"{name}.sig"
            if not sig.is_file():
                raise SystemExit(f"ERROR: missing .sig for updater artifact: {name}")
            found.extend((path, sig))
    return found


def build_channel_yml(manifest: dict, channel: str) -> str:
    """Minimal channel pointer — clients read channels/{channel}/latest.json."""
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


def upload_file(api, local_path: Path, remote_path: str, repo: str, message: str) -> None:
    api.upload_file(
        path_or_fileobj=str(local_path),
        path_in_repo=remote_path,
        repo_id=repo,
        repo_type="model",
        commit_message=message,
    )
    print(f"  [OK] {local_path.name} -> {remote_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Upload allo Tauri release to ModelScope")
    parser.add_argument("--repo", default=DEFAULT_REPO, help=f"ModelScope repo (default: {DEFAULT_REPO})")
    parser.add_argument(
        "--prefix",
        default=DEFAULT_PREFIX,
        help=f"Path prefix inside repo (default: {DEFAULT_PREFIX})",
    )
    parser.add_argument(
        "--channel",
        default=None,
        choices=PLATFORM_CHANNELS,
        help="OTA channel: windows | macos | linux (inferred from latest.json when omitted)",
    )
    parser.add_argument(
        "--dist-dir",
        required=True,
        help="Directory with latest.json + signed updater artifacts (e.g. dist/desktop/)",
    )
    parser.add_argument(
        "--env-file",
        default=str(DEFAULT_ENV_FILE),
        help="Optional env file with MODELSCOPE_TOKEN (default: apps/desktop/signing/.env.modelscope)",
    )
    parser.add_argument(
        "--merge-remote",
        action="store_true",
        help="Merge missing same-channel keys from the existing ModelScope latest.json (same version only)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate inputs and print upload plan without uploading",
    )
    args = parser.parse_args()

    load_env_file(Path(args.env_file))

    dist_dir = Path(args.dist_dir)
    if not dist_dir.is_dir():
        raise SystemExit(
            f"ERROR: dist directory not found: {dist_dir}\n\n"
            "Upload runs after a signed updater build. On Windows:\n"
            "  1) Copy apps/desktop/signing/nomifun-updater.key from your key store\n"
            "  2) $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw\n"
            "  3) bun run build:win --config apps/desktop/tauri.updater.conf.json "
            "--config apps/desktop/tauri.channel.windows.conf.json\n"
            "  4) bun run make:latest --host modelscope --channel windows --collect\n"
            "  5) bun run upload:modelscope -- --channel windows\n"
        )

    latest_path = dist_dir / "latest.json"
    if not latest_path.is_file():
        raise SystemExit(
            f"ERROR: {latest_path} not found. Run:\n"
            "  bun run make:latest --host modelscope --channel windows --collect"
        )

    manifest = json.loads(latest_path.read_text(encoding="utf-8"))
    version = str(manifest.get("version", "")).strip()
    if not version:
        raise SystemExit("ERROR: latest.json missing 'version'")

    channel = args.channel or infer_channel_from_manifest(manifest)
    if channel not in PLATFORM_CHANNELS:
        raise SystemExit(
            "ERROR: could not determine OTA channel. Pass --channel windows|macos|linux "
            "(shared alpha channel is deprecated)."
        )

    version_tag = version if version.startswith("v") else f"v{version}"
    prefix: str = args.prefix.strip("/")
    repo: str = args.repo

    artifacts = collect_updater_artifacts(dist_dir)
    if not artifacts:
        raise SystemExit(f"ERROR: no updater artifacts found in {dist_dir}")

    manifest, dropped_channel = filter_manifest_to_channel(manifest, channel)
    if dropped_channel:
        print(
            f"  Note: dropping cross-channel platform(s): {', '.join(dropped_channel)}",
            file=sys.stderr,
        )

    manifest, dropped = filter_manifest_to_local_platforms(manifest, dist_dir)
    if dropped:
        print(
            f"  Note: dropping {len(dropped)} platform(s) not built on this machine: {', '.join(dropped)}",
            file=sys.stderr,
        )

    if args.merge_remote:
        remote = fetch_remote_latest(repo, prefix, channel)
        if remote:
            before = set((manifest.get("platforms") or {}).keys())
            manifest = merge_remote_same_channel(manifest, remote, channel)
            added = set((manifest.get("platforms") or {}).keys()) - before
            if added:
                print(
                    f"  Merged {len(added)} same-channel platform(s) from remote: "
                    f"{', '.join(sorted(added))}"
                )

    platforms = manifest.get("platforms") or {}
    if not platforms:
        raise SystemExit(
            "ERROR: no uploadable platform entries remain after filtering.\n"
            "Ensure dist/desktop/ contains the updater package(s) referenced in latest.json."
        )

    latest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    remote_latest = f"{prefix}/channels/{channel}/latest.json"
    remote_channel_yml = f"{prefix}/channels/{channel}/channel.yml"
    artifact_remotes = [
        (
            artifact,
            f"{remote_dir_for_artifact(manifest, artifact.name, prefix, version_tag)}/{artifact.name}",
        )
        for artifact in artifacts
    ]

    channel_yml = build_channel_yml(manifest, channel)
    channel_local = dist_dir / "channel.yml"
    channel_local.write_text(channel_yml, encoding="utf-8")

    print(f"Release {version_tag} → ModelScope {repo}/{prefix}/channels/{channel}/")
    print(f"  Endpoint: {modelscope_file_url(repo, remote_latest)}")
    print(f"  Artifacts ({len(artifact_remotes)}):")
    for artifact, remote_path in artifact_remotes:
        print(f"    - {artifact.name} ({artifact.stat().st_size:,} bytes) -> {remote_path}")
    print(f"  Manifest: {remote_latest}")
    print(f"  Pointer:  {remote_channel_yml}")

    if args.dry_run:
        print("\nDry run — no uploads performed.")
        return

    token = os.environ.get("MODELSCOPE_TOKEN")
    if not token:
        raise SystemExit(
            "ERROR: MODELSCOPE_TOKEN not set. Add it to apps/desktop/signing/.env.modelscope "
            "(see .env.modelscope.example) or export MODELSCOPE_TOKEN in your shell."
        )

    try:
        from modelscope.hub.api import HubApi
    except ImportError:
        raise SystemExit("ERROR: modelscope package not installed. Run: pip install modelscope")

    api = HubApi()
    api.login(token)
    print(f"\nAuthenticated — uploading to {repo}")

    fail_count = 0

    for artifact, remote_path in artifact_remotes:
        try:
            upload_file(
                api,
                artifact,
                remote_path,
                repo,
                f"Release {channel} {version_tag}: {artifact.name}",
            )
        except Exception as exc:  # noqa: BLE001
            print(f"  [FAIL] {artifact.name}: {exc}", file=sys.stderr)
            fail_count += 1

    if fail_count:
        raise SystemExit(
            f"ERROR: {fail_count} artifact(s) failed; channel manifests were not published"
        )

    for label, local, remote in (
        ("channel.yml", channel_local, remote_channel_yml),
        ("latest.json", latest_path, remote_latest),
    ):
        try:
            upload_file(api, local, remote, repo, f"Release {channel} {version_tag}: update {label}")
        except Exception as exc:  # noqa: BLE001
            print(f"  [FAIL] {label}: {exc}", file=sys.stderr)
            fail_count += 1

    print(f"\nUpload complete — failures: {fail_count}")
    if fail_count:
        raise SystemExit(f"ERROR: {fail_count} file(s) failed to upload")


if __name__ == "__main__":
    main()
