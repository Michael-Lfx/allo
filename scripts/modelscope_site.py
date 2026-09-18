"""Resolve the ModelScope hub used for desktop OTA publish/verify.

China (modelscope.cn) and international (modelscope.ai) are separate sites with
separate tokens. Default is the international hub; override with
``MODELSCOPE_ENDPOINT`` (``MODELSCOPE_DOMAIN`` is accepted as a deprecated alias).
"""

from __future__ import annotations

import os
from urllib.parse import quote, urlparse

DEFAULT_MODELSCOPE_ENDPOINT = "https://www.modelscope.ai"
ENV_ENDPOINT = "MODELSCOPE_ENDPOINT"
ENV_DOMAIN_LEGACY = "MODELSCOPE_DOMAIN"


def normalize_modelscope_endpoint(raw: str | None) -> str:
    value = (raw or "").strip().rstrip("/")
    if not value:
        return DEFAULT_MODELSCOPE_ENDPOINT
    if "://" not in value:
        value = f"https://{value}"
    parsed = urlparse(value)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise ValueError(f"invalid ModelScope endpoint: {raw!r}")
    host = parsed.hostname.lower() if parsed.hostname else ""
    if host in {"modelscope.ai", "www.modelscope.ai"}:
        return "https://www.modelscope.ai"
    if host in {"modelscope.cn", "www.modelscope.cn"}:
        return "https://modelscope.cn"
    return f"{parsed.scheme}://{parsed.netloc}"


def modelscope_endpoint() -> str:
    raw = os.environ.get(ENV_ENDPOINT) or os.environ.get(ENV_DOMAIN_LEGACY)
    return normalize_modelscope_endpoint(raw)


def modelscope_file_url(repo: str, path_in_repo: str, endpoint: str | None = None) -> str:
    """Public ModelScope repo file URL on the configured hub."""
    base = normalize_modelscope_endpoint(endpoint or modelscope_endpoint())
    return (
        f"{base}/api/v1/models/{repo}/repo"
        f"?Revision=master&FilePath={quote(path_in_repo, safe='/')}"
    )
