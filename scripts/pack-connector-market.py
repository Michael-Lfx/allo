#!/usr/bin/env python3
"""把示例连接器市场树打成确定性 zip，供 `market/add`（`source_kind: "zip"`）使用。

为什么需要这个脚本：`zip` 源只收 http(s) 地址（本地路径是 `directory` 的活，
见 `nomifun-app/src/market_source.rs` 的 `normalize_zip_requires_http`），所以
"面向所有人"的形态必然是一个可下载的归档；而归档必须是**确定性**的——同一棵树
重复打包得到字节相同的结果，sha256 才能当版本标识，客户端也才能靠
`X-Linked-Etag` 判断"没变就不重下"（doc `30`，官方市场同理）。

用法：
    python scripts/pack-connector-market.py                       # 默认树 + dist-market/
    python scripts/pack-connector-market.py --root <目录> --out <目录>
    python scripts/pack-connector-market.py --check               # 只校验确定性，不落盘
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_ROOT = REPO_ROOT / "docs/agent-store/examples/github-token-demo"
DEFAULT_OUT = REPO_ROOT / "dist-market"

# 固定时间戳与权限：决定"同一棵树 → 同一串字节"的三个变量里，这两个最容易随环境漂移。
FIXED_DATE = (1980, 1, 1, 0, 0, 0)
FIXED_MODE = 0o644


def collect(root: Path) -> list[tuple[str, bytes]]:
    """树内所有文件，按 POSIX 相对路径排序；目录本身不进归档（解包自会建父目录）。"""
    entries: list[tuple[str, bytes]] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        entries.append((rel, path.read_bytes()))
    return entries


def market_name(root: Path) -> str:
    """市场身份来自索引里的 `name`，不是目录名——目录名只是放它的地方。"""
    manifest = root / ".codebuddy-connector/connectors.json"
    if manifest.is_file():
        try:
            name = json.loads(manifest.read_text(encoding="utf-8")).get("name")
        except (json.JSONDecodeError, UnicodeDecodeError):
            name = None
        if isinstance(name, str) and name:
            return name
    return root.name


def build(root: Path, out_dir: Path) -> tuple[Path, str, int]:
    entries = collect(root)
    if not entries:
        raise SystemExit(f"市场树是空的：{root}")

    out_dir.mkdir(parents=True, exist_ok=True)
    archive = out_dir / f"{market_name(root)}.zip"

    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for rel, body in entries:
            info = zipfile.ZipInfo(rel, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (FIXED_MODE & 0xFFFF) << 16
            zf.writestr(info, body)

    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    return archive, digest, len(entries)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="市场树根目录")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT, help="归档输出目录")
    parser.add_argument("--check", action="store_true", help="只校验确定性，不落盘")
    args = parser.parse_args()

    root = args.root.resolve()
    if not root.is_dir():
        raise SystemExit(f"市场树不存在：{root}")

    if args.check:
        first = build(root, args.out)
        first_digest = first[1]
        second = build(root, args.out)
        if first_digest != second[1]:
            print("✗ 非确定性：两次打包摘要不同", file=sys.stderr)
            return 1
        print(f"✓ 确定性打包：{first[2]} 个文件，sha256={first_digest}")
        return 0

    archive, digest, count = build(root, args.out)
    print(f"✓ {archive}")
    print(f"  {count} 个文件，{archive.stat().st_size} 字节")
    print(f"  sha256={digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
