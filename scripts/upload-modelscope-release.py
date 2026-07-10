#!/usr/bin/env python3
"""Upload Flowy (allo) Tauri updater artifacts to ModelScope.

Model repo layout (root = ``allo/`` under flowy2025/flowyaipc):

    allo/
    ├── alpha.yml                      # channel pointer (version metadata)
    ├── channels/alpha/latest.json     # Tauri updater manifest (client endpoint)
    └── v{version}/                    # signed updater packages + .sig

Run ``bun run make:latest --host modelscope --collect`` first to build
``latest.json`` with ModelScope download URLs and copy artifacts to dist/desktop/.

Requires ``MODELSCOPE_TOKEN`` and ``pip install modelscope``.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

DEFAULT_REPO = "flowy2025/flowyaipc"
DEFAULT_PREFIX = "allo"
DEFAULT_CHANNEL = "alpha"
DEFAULT_ENV_FILE = Path(__file__).resolve().parent.parent / "apps/desktop/signing/.env.modelscope"

# Updater artifacts Tauri can install (manual-only bundles like .dmg/.deb are skipped).
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
    """Public ModelScope repo file URL (same shape as hermes-agent-ultra)."""
    return (
        f"https://modelscope.cn/api/v1/models/{repo}/repo"
        f"?Revision=master&FilePath={quote(path_in_repo, safe='/')}"
    )


def artifact_basename_from_url(url: str) -> str:
    return url.rsplit("/", 1)[-1].split("?")[0] if url else ""


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


def merge_remote_platforms(manifest: dict, remote: dict) -> dict:
    """Fill missing platforms from an existing remote latest.json (multi-machine release)."""
    merged = dict(manifest)
    local_platforms = dict(manifest.get("platforms") or {})
    remote_platforms = dict(remote.get("platforms") or {})
    if str(remote.get("version", "")).strip() != str(manifest.get("version", "")).strip():
        return merged
    for key, entry in remote_platforms.items():
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
    """Collect signed updater packages from dist (exclude standalone .sig-only paths)."""
    found: list[Path] = []
    for path in sorted(dist_dir.iterdir()):
        if not path.is_file():
            continue
        name = path.name
        if name.endswith(".sig") or name == "latest.json" or name == "alpha.yml":
            continue
        if any(name.endswith(suffix) for suffix in UPDATER_SUFFIXES):
            sig = dist_dir / f"{name}.sig"
            if not sig.is_file():
                print(f"  [WARN] missing .sig for updater artifact: {name}", file=sys.stderr)
            found.append(path)
    return found


def build_alpha_yml(manifest: dict, channel: str) -> str:
    """Minimal channel pointer — clients read channels/{channel}/latest.json, not this file."""
    lines = [
        f"version: \"{manifest.get('version', '')}\"",
        f"channel: {channel}",
        f"pub_date: \"{manifest.get('pub_date', '')}\"",
        f"manifest: channels/{channel}/latest.json",
    ]
    notes = manifest.get("notes")
    if isinstance(notes, str) and notes.strip():
        # YAML block scalar for multi-line release notes
        lines.append("notes: |")
        for note_line in notes.strip().splitlines():
            lines.append(f"  {note_line}")
    else:
        lines.append('notes: ""')
    return "\n".join(lines) + "\n"


def upload_file(api, local_path: Path, remote_path: str, repo: str, message: str) -> None:
    from modelscope.hub.api import HubApi  # noqa: F401 — imported in main()

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
        default=DEFAULT_CHANNEL,
        help=f"Release channel subdirectory (default: {DEFAULT_CHANNEL})",
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
        help="Merge platform entries from the existing ModelScope latest.json (multi-platform release)",
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
            "  3) bun run build:win --config apps/desktop/tauri.updater.conf.json\n"
            "  4) bun run make:latest --host modelscope --channel alpha --collect\n"
            "  5) bun run upload:modelscope\n"
        )

    latest_path = dist_dir / "latest.json"
    if not latest_path.is_file():
        raise SystemExit(
            f"ERROR: {latest_path} not found. Run:\n"
            "  bun run make:latest --host modelscope --channel alpha --collect"
        )

    manifest = json.loads(latest_path.read_text(encoding="utf-8"))
    version = str(manifest.get("version", "")).strip()
    if not version:
        raise SystemExit("ERROR: latest.json missing 'version'")

    version_tag = version if version.startswith("v") else f"v{version}"
    prefix: str = args.prefix.strip("/")
    channel: str = args.channel
    repo: str = args.repo

    artifacts = collect_updater_artifacts(dist_dir)
    if not artifacts:
        raise SystemExit(f"ERROR: no updater artifacts found in {dist_dir}")

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
            manifest = merge_remote_platforms(manifest, remote)
            added = set((manifest.get("platforms") or {}).keys()) - before
            if added:
                print(f"  Merged {len(added)} platform(s) from remote manifest: {', '.join(sorted(added))}")

    platforms = manifest.get("platforms") or {}
    if not platforms:
        raise SystemExit(
            "ERROR: no uploadable platform entries remain after filtering.\n"
            "Ensure dist/desktop/ contains the updater package(s) referenced in latest.json."
        )

    # Rewrite local latest.json to the filtered/merged manifest used for upload.
    latest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    remote_version_prefix = f"{prefix}/{version_tag}"
    remote_latest = f"{prefix}/channels/{channel}/latest.json"
    remote_alpha = f"{prefix}/alpha.yml"

    alpha_yml = build_alpha_yml(manifest, channel)
    alpha_local = dist_dir / "alpha.yml"
    alpha_local.write_text(alpha_yml, encoding="utf-8")

    print(f"Release {version_tag} → ModelScope {repo}/{prefix}/")
    print(f"  Endpoint: {modelscope_file_url(repo, remote_latest)}")
    print(f"  Artifacts ({len(artifacts)}):")
    for a in artifacts:
        print(f"    - {a.name} ({a.stat().st_size:,} bytes)")
    print(f"  Manifest: {remote_latest}")
    print(f"  Pointer:  {remote_alpha}")

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

    for artifact in artifacts:
        remote_path = f"{remote_version_prefix}/{artifact.name}"
        try:
            upload_file(
                api,
                artifact,
                remote_path,
                repo,
                f"Release {version_tag}: {artifact.name}",
            )
        except Exception as exc:  # noqa: BLE001 — surface per-file errors, continue
            print(f"  [FAIL] {artifact.name}: {exc}", file=sys.stderr)
            fail_count += 1

    for label, local, remote in (
        ("latest.json", latest_path, remote_latest),
        ("alpha.yml", alpha_local, remote_alpha),
    ):
        try:
            upload_file(api, local, remote, repo, f"Release {version_tag}: update {label}")
        except Exception as exc:  # noqa: BLE001
            print(f"  [FAIL] {label}: {exc}", file=sys.stderr)
            fail_count += 1

    print(f"\nUpload complete — failures: {fail_count}")
    if fail_count:
        raise SystemExit(f"ERROR: {fail_count} file(s) failed to upload")


if __name__ == "__main__":
    main()
