#!/usr/bin/env python3
"""Pure-function tests for ModelScope release verification helpers."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


def _load():
    path = Path(__file__).resolve().parent / "verify-modelscope-release.py"
    spec = importlib.util.spec_from_file_location("verify_modelscope_release", path)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


mod = _load()


class VerifyHelpersTest(unittest.TestCase):
    def test_accepts_expected_ai_origin(self):
        mod.validate_artifact_origin(
            "https://modelscope.ai/api/v1/models/flowy2025/flowy/repo"
            "?Revision=master&FilePath=allo/windows/v1.7.0/UniBuddy_1.7.0_x64-setup.exe",
            api_host="modelscope.ai",
            repo="flowy2025/flowy",
            prefix="allo",
            folder="windows",
            version_tag="v1.7.0",
        )

    def test_rejects_cn_artifact_when_verifying_ai(self):
        with self.assertRaisesRegex(ValueError, "wrong host"):
            mod.validate_artifact_origin(
                "https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo"
                "?Revision=master&FilePath=allo/windows/v1.7.0/UniBuddy_1.7.0_x64-setup.exe",
                api_host="modelscope.ai",
                repo="flowy2025/flowy",
                prefix="allo",
                folder="windows",
                version_tag="v1.7.0",
            )

    def test_rejects_wrong_repo_and_file_prefix(self):
        with self.assertRaisesRegex(ValueError, "wrong repo"):
            mod.validate_artifact_origin(
                "https://modelscope.ai/api/v1/models/flowy2025/flowyaipc/repo"
                "?Revision=master&FilePath=allo/windows/v1.7.0/x.exe",
                api_host="modelscope.ai",
                repo="flowy2025/flowy",
                prefix="allo",
                folder="windows",
                version_tag="v1.7.0",
            )

        with self.assertRaisesRegex(ValueError, "must point under"):
            mod.validate_artifact_origin(
                "https://modelscope.ai/api/v1/models/flowy2025/flowy/repo"
                "?Revision=master&FilePath=other/windows/v1.7.0/x.exe",
                api_host="modelscope.ai",
                repo="flowy2025/flowy",
                prefix="allo",
                folder="windows",
                version_tag="v1.7.0",
            )


if __name__ == "__main__":
    unittest.main()
