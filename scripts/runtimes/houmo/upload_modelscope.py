#!/usr/bin/env python3
"""Upload the Houmo HLIELLama xh2 runtime archive to ModelScope.

Mirrors ``D:\\herdsman\\scripts\\runtimes\\llama-cpp\\upload_modelscope.py``:
the same dataset (``flowy2025/local-model-runtimes``, ``repo_type=dataset``)
now carries the linux-aarch64 Houmo runtime under ``houmo/linux-aarch64/``.

Tokens resolve like this repository's ``scripts/upload-modelscope-release.py``:
``MODELSCOPE_TOKEN`` first, then the CN ``MODELSCOPE_CN_TOKEN``, with
``apps/desktop/signing/.env.modelscope`` loaded as a fallback source.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

RUNTIME_DATASET = "flowy2025/local-model-runtimes"
REMOTE_PATH = "houmo/linux-aarch64/houmo-HLIELLama-xh2_2.1.1_linux_aarch64.tar.gz"
DEFAULT_ARCHIVE = Path(
    r"C:\Users\Administration\Downloads\houmo-HLIELLama-xh2_2.1.1_linux_aarch64.tar.gz"
)
DEFAULT_ENV_FILE = (
    Path(__file__).resolve().parents[3] / "apps/desktop/signing/.env.modelscope"
)


def clear_proxy_env() -> None:
    for key in (
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ):
        os.environ.pop(key, None)


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


def resolve_token() -> str:
    for env in ("MODELSCOPE_TOKEN", "MODELSCOPE_CN_TOKEN"):
        token = os.environ.get(env, "").strip()
        if token:
            return token
    raise SystemExit(
        "ERROR: MODELSCOPE_TOKEN (or MODELSCOPE_CN_TOKEN) not set. Add it to "
        "apps/desktop/signing/.env.modelscope or export it in your shell."
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(16 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_url(path_in_repo: str) -> str:
    return (
        "https://modelscope.cn/datasets/"
        f"{RUNTIME_DATASET}/resolve/master/{path_in_repo}"
    )


def artifact_info(path: Path, path_in_repo: str) -> dict:
    return {
        "file": str(path),
        "path_in_repo": path_in_repo,
        "file_name": path.name,
        "file_size": path.stat().st_size,
        "sha_256": sha256_file(path),
        "resolve_url": resolve_url(path_in_repo),
    }


def upload_file(api, local_path: Path, path_in_repo: str, dry_run: bool) -> dict:
    info = artifact_info(local_path, path_in_repo)
    if dry_run:
        print(
            json.dumps(
                {"dry_run": True, "repo_id": RUNTIME_DATASET, "repo_type": "dataset", **info},
                ensure_ascii=False,
            )
        )
        return info
    api.upload_file(
        path_or_fileobj=str(local_path),
        path_in_repo=path_in_repo,
        repo_id=RUNTIME_DATASET,
        repo_type="dataset",
        commit_message=f"upload: {path_in_repo}",
    )
    print(
        json.dumps(
            {"uploaded": True, "repo_id": RUNTIME_DATASET, "repo_type": "dataset", **info},
            ensure_ascii=False,
        )
    )
    return info


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Upload the Houmo HLIELLama runtime to ModelScope dataset "
        "flowy2025/local-model-runtimes."
    )
    parser.add_argument(
        "archive",
        nargs="?",
        type=Path,
        default=DEFAULT_ARCHIVE,
        help=f"Local tar.gz path (default: {DEFAULT_ARCHIVE})",
    )
    parser.add_argument(
        "--path-in-repo",
        default=REMOTE_PATH,
        help=f"Remote path inside the dataset (default: {REMOTE_PATH})",
    )
    parser.add_argument(
        "--env-file",
        type=Path,
        default=DEFAULT_ENV_FILE,
        help="Optional env file with MODELSCOPE_TOKEN (default: "
        "apps/desktop/signing/.env.modelscope)",
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--metadata-out", type=Path, help="Write upload metadata JSON")
    args = parser.parse_args()

    local_path = args.archive
    if not local_path.is_file():
        raise SystemExit(f"missing archive: {local_path}")

    load_env_file(args.env_file)
    clear_proxy_env()

    token = "" if args.dry_run else resolve_token()
    api = None
    if not args.dry_run:
        try:
            from modelscope.hub.api import HubApi
        except ImportError:
            raise SystemExit("ERROR: modelscope package not installed. Run: pip install modelscope")
        api = HubApi()
        api.login(token)

    info = upload_file(api, local_path, args.path_in_repo, args.dry_run)

    if args.metadata_out:
        args.metadata_out.parent.mkdir(parents=True, exist_ok=True)
        args.metadata_out.write_text(
            json.dumps([info], indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
