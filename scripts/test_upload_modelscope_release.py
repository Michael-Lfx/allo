#!/usr/bin/env python3
"""Pure-function tests for upload-modelscope-release helpers (no network)."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


def _load():
    path = Path(__file__).resolve().parent / "upload-modelscope-release.py"
    spec = importlib.util.spec_from_file_location("upload_modelscope_release", path)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


mod = _load()


class UploadHelpersTest(unittest.TestCase):
    def test_filter_channel_drops_cross_os(self):
        manifest = {
            "version": "1.2.3",
            "platforms": {
                "windows-x86_64": {"url": "u", "signature": "s"},
                "linux-x86_64": {"url": "u2", "signature": "s2"},
            },
        }
        filtered, dropped = mod.filter_manifest_to_channel(manifest, "windows")
        self.assertEqual(set(filtered["platforms"]), {"windows-x86_64"})
        self.assertEqual(dropped, ["linux-x86_64"])

    def test_merge_remote_same_version(self):
        local = {
            "version": "1.2.3",
            "platforms": {"windows-x86_64": {"url": "a", "signature": "sa"}},
        }
        remote = {
            "version": "1.2.3",
            "platforms": {"windows-aarch64": {"url": "b", "signature": "sb"}},
        }
        merged = mod.merge_remote_same_channel(local, remote, "windows")
        self.assertEqual(set(merged["platforms"]), {"windows-x86_64", "windows-aarch64"})

    def test_merge_remote_rejects_other_version(self):
        local = {"version": "1.2.3", "platforms": {"windows-x86_64": {"url": "a", "signature": "sa"}}}
        remote = {"version": "9.9.9", "platforms": {"windows-aarch64": {"url": "b", "signature": "sb"}}}
        merged = mod.merge_remote_same_channel(local, remote, "windows")
        self.assertEqual(set(merged["platforms"]), {"windows-x86_64"})

    def test_filter_local_platforms(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Flowy_1.2.3_x64-setup.exe").write_bytes(b"pkg")
            manifest = {
                "version": "1.2.3",
                "platforms": {
                    "windows-x86_64": {
                        "url": "https://modelscope.cn/api/v1/models/r/repo?FilePath=allo/windows/v1.2.3/Flowy_1.2.3_x64-setup.exe",
                        "signature": "s",
                    },
                    "windows-aarch64": {
                        "url": "https://modelscope.cn/api/v1/models/r/repo?FilePath=allo/windows/v1.2.3/Flowy_1.2.3_aarch64-setup.exe",
                        "signature": "s",
                    },
                },
            }
            filtered, dropped = mod.filter_manifest_to_local_platforms(manifest, root)
            self.assertEqual(set(filtered["platforms"]), {"windows-x86_64"})
            self.assertEqual(dropped, ["windows-aarch64"])

    def test_channel_yml_and_sha256(self):
        yml = mod.build_channel_yml({"version": "1.0.0", "pub_date": "t", "notes": "n"}, "linux")
        self.assertIn('version: "1.0.0"', yml)
        self.assertIn("channel: linux", yml)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "f.bin"
            path.write_bytes(b"abc")
            self.assertEqual(
                mod.sha256_file(path),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            )


if __name__ == "__main__":
    unittest.main()
