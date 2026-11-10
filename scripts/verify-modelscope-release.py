#!/usr/bin/env python3
"""Verify a published desktop ModelScope updater manifest and artifact payloads."""

from __future__ import annotations

import argparse
import hashlib
import json
import time
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import parse_qs, quote, unquote, urlparse

PLATFORM_FOLDERS = {
    "windows-x86_64": "windows",
    "windows-aarch64": "windows",
    "darwin-x86_64": "macos",
    "darwin-aarch64": "macos",
    "linux-x86_64": "linux",
    "linux-aarch64": "linux",
}

PLATFORM_CHANNELS = ("windows", "macos", "linux")
KNOWN_API_HOSTS = ("modelscope.cn", "modelscope.ai")


def normalize_api_host(value: str) -> str:
    host = value.strip().lower()
    for scheme in ("https://", "http://"):
        if host.startswith(scheme):
            host = host[len(scheme) :]
    if host.startswith("www."):
        host = host[4:]
    host = host.split("/", 1)[0]
    if host not in KNOWN_API_HOSTS:
        raise SystemExit(
            f"ERROR: unsupported ModelScope API host: {value} "
            f"(expected {' or '.join(KNOWN_API_HOSTS)})"
        )
    return host


def normalize_channel_root(value: str) -> str:
    root = value.strip().strip("/")
    if not root or not all(ch.isalnum() or ch in "-_" for ch in root):
        raise SystemExit(f"ERROR: invalid --channel-root {value!r}")
    return root


def channel_manifest_url(
    channel: str,
    repo: str = "flowy2025/flowyaipc",
    api_host: str = "modelscope.cn",
    prefix: str = "allo",
    channel_root: str = "channels",
) -> str:
    path = f"{prefix}/{channel_root}/{channel}/latest.json"
    return (
        f"https://{api_host}/api/v1/models/{repo}/repo"
        f"?Revision=master&FilePath={quote(path, safe='/')}"
    )


def artifact_basename_from_url(url: str) -> str:
    if not url:
        return ""
    parsed = urlparse(url)
    file_path = parse_qs(parsed.query).get("FilePath", [""])[0]
    if file_path:
        return Path(unquote(file_path)).name
    return Path(unquote(parsed.path)).name


def validate_artifact_origin(
    url: str,
    *,
    api_host: str,
    repo: str,
    prefix: str,
    folder: str,
    version_tag: str,
) -> None:
    """Require an artifact URL to belong to the ModelScope origin being verified."""
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname != api_host:
        raise ValueError(f"artifact URL points at the wrong host: {url}")

    expected_api_path = f"/api/v1/models/{repo}/repo"
    if unquote(parsed.path).rstrip("/") != expected_api_path:
        raise ValueError(f"artifact URL points at the wrong repo: {url}")

    file_path = parse_qs(parsed.query).get("FilePath", [""])[0].lstrip("/")
    expected_prefix = f"{prefix}/{folder}/{version_tag}/"
    if not file_path.startswith(expected_prefix) or not Path(file_path).name:
        raise ValueError(
            f"artifact URL must point under {expected_prefix}, got "
            f"{file_path or '<missing FilePath>'}"
        )


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(
        url,
        headers={"Cache-Control": "no-cache", "User-Agent": "flowy-release-verifier"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.loads(response.read().decode("utf-8"))


def remote_content_length(url: str) -> int | None:
    request = urllib.request.Request(
        url,
        method="HEAD",
        headers={"Cache-Control": "no-cache", "User-Agent": "flowy-release-verifier"},
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            length = response.headers.get("Content-Length")
            if length and length.isdigit():
                return int(length)
    except (urllib.error.URLError, TimeoutError, OSError):
        pass
    request = urllib.request.Request(
        url,
        headers={
            "Cache-Control": "no-cache",
            "User-Agent": "flowy-release-verifier",
            "Range": "bytes=0-0",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            content_range = response.headers.get("Content-Range", "")
            if "/" in content_range:
                total = content_range.rsplit("/", 1)[-1]
                if total.isdigit():
                    return int(total)
    except (urllib.error.URLError, TimeoutError, OSError):
        return None
    return None


def sha256_url(url: str, expected_size: int | None = None) -> tuple[str, int]:
    """Download remote body and return (sha256, byte_count)."""
    request = urllib.request.Request(
        url,
        headers={"Cache-Control": "no-cache", "User-Agent": "flowy-release-verifier"},
    )
    digest = hashlib.sha256()
    total = 0
    with urllib.request.urlopen(request, timeout=600) as response:
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
            total += len(chunk)
            if expected_size is not None and total > expected_size:
                raise ValueError(f"downloaded more bytes than expected ({expected_size})")
    return digest.hexdigest(), total


def load_metadata_hashes(path: Path) -> dict[str, dict]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    files = payload.get("files") or []
    return {str(item.get("name", "")): item for item in files if item.get("name")}


def main() -> None:
    parser = argparse.ArgumentParser(description="Verify the published ModelScope updater manifest")
    parser.add_argument("--version", required=True, help="Expected version without the v prefix")
    parser.add_argument("--platform", action="append", required=True, help="Required platform key")
    parser.add_argument(
        "--channel",
        choices=PLATFORM_CHANNELS,
        help="OTA channel (sets default --url to <prefix>/<channel-root>/<channel>/latest.json)",
    )
    parser.add_argument("--prefix", default="allo", help="Repo path prefix (default: allo)")
    parser.add_argument(
        "--channel-root",
        default="channels",
        help="Manifest directory under --prefix (default: channels; UniBuddy: unibuddy_channels)",
    )
    parser.add_argument("--url", default=None, help="Updater manifest URL")
    parser.add_argument("--repo", default="flowy2025/flowyaipc", help="ModelScope model repo")
    parser.add_argument(
        "--api-host",
        default="modelscope.cn",
        help="Public ModelScope host: modelscope.cn or modelscope.ai",
    )
    parser.add_argument("--attempts", type=int, default=5, help="Maximum fetch attempts")
    parser.add_argument("--retry-delay", type=float, default=10, help="Seconds between attempts")
    parser.add_argument(
        "--check-artifacts",
        action="store_true",
        help="Verify each platform artifact URL is reachable and Content-Length is present",
    )
    parser.add_argument(
        "--metadata",
        default=None,
        help="Optional release-metadata.json; when set with --check-artifacts, compare size/sha256",
    )
    parser.add_argument(
        "--hash-artifacts",
        action="store_true",
        help="Download artifacts and verify sha256 against --metadata (implies --check-artifacts)",
    )
    args = parser.parse_args()

    if args.hash_artifacts and not args.metadata:
        raise SystemExit("ERROR: --hash-artifacts requires --metadata")

    check_artifacts = args.check_artifacts or args.hash_artifacts
    metadata_by_name = load_metadata_hashes(Path(args.metadata)) if args.metadata else {}

    prefix = args.prefix.strip("/")
    if not prefix:
        raise SystemExit("ERROR: --prefix must not be empty")
    api_host = normalize_api_host(args.api_host)
    channel_root = normalize_channel_root(args.channel_root)
    if args.url:
        url = args.url
    elif args.channel:
        url = channel_manifest_url(args.channel, args.repo, api_host, prefix, channel_root)
    else:
        folder = PLATFORM_FOLDERS.get(args.platform[0])
        if not folder:
            raise SystemExit(f"ERROR: unsupported platform key: {args.platform[0]}")
        url = channel_manifest_url(folder, args.repo, api_host, prefix, channel_root)

    last_error = "manifest was not fetched"
    for attempt in range(1, args.attempts + 1):
        try:
            manifest = fetch_json(url)

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
                entry = platforms.get(key) or {}
                entry_url = str(entry.get("url", ""))
                validate_artifact_origin(
                    entry_url,
                    api_host=api_host,
                    repo=args.repo,
                    prefix=prefix,
                    folder=folder,
                    version_tag=version_tag,
                )
                if not entry.get("signature"):
                    raise ValueError(f"{key} missing signature")

                if check_artifacts:
                    remote_size = remote_content_length(entry_url)
                    if remote_size is None:
                        raise ValueError(f"{key} artifact not reachable or missing Content-Length: {entry_url}")
                    if remote_size <= 0:
                        raise ValueError(f"{key} artifact has empty Content-Length")

                    name = artifact_basename_from_url(entry_url)
                    meta = metadata_by_name.get(name)
                    if meta and meta.get("size") is not None and int(meta["size"]) != remote_size:
                        raise ValueError(
                            f"{key} size mismatch for {name}: remote={remote_size} metadata={meta['size']}"
                        )

                    if args.hash_artifacts:
                        if not meta or not meta.get("sha256"):
                            raise ValueError(f"{key} missing sha256 in metadata for {name}")
                        digest, total = sha256_url(entry_url, expected_size=int(meta["size"]))
                        if total != int(meta["size"]):
                            raise ValueError(
                                f"{key} downloaded size mismatch for {name}: got {total} expected {meta['size']}"
                            )
                        if digest != meta["sha256"]:
                            raise ValueError(
                                f"{key} sha256 mismatch for {name}: remote={digest} metadata={meta['sha256']}"
                            )
                        print(f"  hash ok: {key} {name} ({total:,} bytes)")

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
