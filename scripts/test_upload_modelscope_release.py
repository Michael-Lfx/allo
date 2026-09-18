#!/usr/bin/env python3
"""Pure-function tests for upload-modelscope-release helpers (no network)."""

from __future__ import annotations

import importlib.util
import os
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
                        "url": "https://www.modelscope.ai/api/v1/models/r/repo?FilePath=allo/windows/v1.2.3/Flowy_1.2.3_x64-setup.exe",
                        "signature": "s",
                    },
                    "windows-aarch64": {
                        "url": "https://www.modelscope.ai/api/v1/models/r/repo?FilePath=allo/windows/v1.2.3/Flowy_1.2.3_aarch64-setup.exe",
                        "signature": "s",
                    },
                },
            }
            filtered, dropped = mod.filter_manifest_to_local_platforms(manifest, root)
            self.assertEqual(set(filtered["platforms"]), {"windows-x86_64"})
            self.assertEqual(dropped, ["windows-aarch64"])

    def test_remote_dir_maps_tauri_arm64_setup_exe(self):
        manifest = {
            "version": "1.1.1",
            "platforms": {
                "windows-x86_64": {
                    "url": "https://www.modelscope.ai/api/v1/models/r/repo?FilePath=allo/windows/v1.1.1/Flowy_1.1.1_x64-setup.exe",
                    "signature": "s",
                }
            },
        }
        self.assertEqual(
            mod.remote_dir_for_artifact(manifest, "Flowy_1.1.1_arm64-setup.exe", "allo", "v1.1.1"),
            "allo/windows/v1.1.1",
        )
        self.assertEqual(
            mod.remote_dir_for_artifact(manifest, "Flowy_1.1.1_arm64-setup.exe.sig", "allo", "v1.1.1"),
            "allo/windows/v1.1.1",
        )

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

    def test_file_url_defaults_to_international_hub(self):
        saved_endpoint = os.environ.pop("MODELSCOPE_ENDPOINT", None)
        saved_domain = os.environ.pop("MODELSCOPE_DOMAIN", None)
        try:
            url = mod.modelscope_file_url("flowy2025/flowyaipc", "allo/channels/windows/latest.json")
            self.assertTrue(url.startswith("https://www.modelscope.ai/api/v1/models/flowy2025/flowyaipc/repo"))
            self.assertIn("FilePath=allo/channels/windows/latest.json", url)
        finally:
            if saved_endpoint is not None:
                os.environ["MODELSCOPE_ENDPOINT"] = saved_endpoint
            if saved_domain is not None:
                os.environ["MODELSCOPE_DOMAIN"] = saved_domain

    def test_file_url_honors_china_endpoint(self):
        url = mod.modelscope_file_url(
            "flowy2025/flowyaipc",
            "allo/channels/windows/latest.json",
            endpoint="https://modelscope.cn",
        )
        self.assertTrue(url.startswith("https://modelscope.cn/api/v1/models/"))


if __name__ == "__main__":
    unittest.main()
