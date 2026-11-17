"""Isolated behavioral checks; never reads or writes installed model directories."""
import copy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

SPEC = importlib.util.spec_from_file_location("prepare_voice_models", Path(__file__).with_name("prepare-voice-models.py"))
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


def entry(path, data):
    return {"path": path, "size": len(data), "sha256": hashlib.sha256(data).hexdigest(), "url": "https://example.invalid/pinned"}


class Response(io.BytesIO):
    def __init__(self, data, status=200, headers=None):
        super().__init__(data)
        self.status = status
        self.headers = headers or {"Content-Length": str(len(data))}


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.item = entry("asr/model.onnx", b"good model")
        self.target = self.root / self.item["path"]
        self.target.parent.mkdir()

    def test_shared_manifest_complete_and_pinned(self):
        manifest = m.load_manifest()
        self.assertEqual(manifest, json.loads(m.MANIFEST_PATH.read_text(encoding="utf-8")))
        self.assertEqual(len(manifest["files"]), 23)
        self.assertEqual(sum(f["path"].startswith("tts/") for f in manifest["files"]), 16)
        default = next(g for g in manifest["kws_groups"] if g["group_id"] == manifest["default_kws_group"])
        self.assertEqual(default["files"], {f["path"]: f["sha256"] for f in manifest["files"] if f["path"].startswith("kws/")})
        self.assertEqual(len(default["files"]), 4)

    def test_data_roots_and_override(self):
        with patch.dict(os.environ, {"LOCALAPPDATA": "C:/isolated", "NOMI_CHANNEL": "dev"}, clear=True), patch.object(sys, "platform", "win32"):
            self.assertEqual(m.get_default_models_dir(), Path("C:/isolated/Flowy/Nomi-dev/models"))
            self.assertEqual(m.get_default_models_dir("stable"), Path("C:/isolated/Flowy/Nomi/models"))
            os.environ["VOICE_COMPANION_MODEL_DIR"] = "D:/isolated/models"
            self.assertEqual(m.get_default_models_dir(), Path("D:/isolated/models"))

    def fixture_suite(self):
        data = {"kws/encoder.int8.onnx": b"encoder", "kws/decoder.int8.onnx": b"decoder", "kws/joiner.int8.onnx": b"joiner", "kws/tokens.txt": "▁HE 0\nY 1\n▁F 2\nL 3\nOW 4\n".encode(), "asr/model.onnx": b"good model"}
        files = [entry(p, b) for p, b in data.items()]
        group = {"group_id": "default", "files": {f["path"]: f["sha256"] for f in files if f["path"].startswith("kws/")}}
        manifest = {"files": files, "kws_groups": [group], "default_kws_group": "default"}
        for p, b in data.items():
            target = self.root / p
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b)
        return manifest, data

    def test_correct_files_zero_requests_and_unchanged_mtime(self):
        manifest, data = self.fixture_suite()
        keywords = self.root / "kws/keywords.txt"
        keywords.write_text("▁HE Y :2 #0.3 @CUSTOM\n", encoding="utf-8")
        before = {p: (self.root / p).stat().st_mtime_ns for p in data}
        with patch.object(m, "urlopen", side_effect=AssertionError("unexpected GET")):
            m.prepare(self.root, manifest)
            m.prepare(self.root, manifest)
        self.assertEqual(before, {p: (self.root / p).stat().st_mtime_ns for p in data})
        self.assertEqual(keywords.read_text(encoding="utf-8"), "▁HE Y :2 #0.3 @CUSTOM\n")

    def test_only_damaged_file_requested(self):
        manifest, data = self.fixture_suite()
        self.target.write_bytes(b"bad")
        before = {p: (self.root / p).stat().st_mtime_ns for p in data if p != self.item["path"]}
        with patch.object(m, "urlopen", return_value=Response(b"good model")) as fetch:
            m.prepare(self.root, manifest)
        self.assertEqual(fetch.call_count, 1)
        self.assertEqual(before, {p: (self.root / p).stat().st_mtime_ns for p in before})

    def test_bad_body_never_replaces_old_target(self):
        self.target.write_bytes(b"old usable")
        for body in (b"<html>bad!", b"short"):
            with patch.object(m, "urlopen", return_value=Response(body)), self.assertRaises(RuntimeError):
                m.download_file_with_resume(self.item, self.target)
            self.assertEqual(self.target.read_bytes(), b"old usable")

    def test_range_and_unknown_part_identity(self):
        verified, part, backup = m.transaction_paths(self.target, self.item)
        unknown = self.target.with_name(self.target.name + ".part")
        unknown.write_bytes(b"foreign")
        part.write_bytes(b"good")
        with patch.object(m, "urlopen", return_value=Response(b" model", 206, {"Content-Range": "bytes 4-9/10", "Content-Length": "6"})) as fetch:
            m.download_file_with_resume(self.item, self.target)
        self.assertEqual(fetch.call_args.args[0].get_header("Range"), "bytes=4-")
        self.assertEqual(self.target.read_bytes(), b"good model")
        self.assertEqual(unknown.read_bytes(), b"foreign")

    def test_real_http_fallback_bad_hash_and_partial_resume(self):
        owner = self
        requests = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                requests.append((self.path, self.headers.get("Range")))
                # Target must survive every failed source until verified promotion.
                owner.assertEqual(owner.target.read_bytes(), b"old usable")
                if self.path == "/bad":
                    body = b"bad model!"
                elif self.path == "/short":
                    body = b"good"
                elif self.headers.get("Range") == "bytes=4-":
                    self.send_response(206)
                    self.send_header("Content-Range", "bytes 4-9/10")
                    self.send_header("Content-Length", "6")
                    self.end_headers()
                    self.wfile.write(b" model")
                    return
                else:
                    body = b"good model"
                self.send_response(200)
                self.send_header("Content-Length", "10")
                self.end_headers()
                self.wfile.write(body)

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f"http://127.0.0.1:{server.server_port}"
            for route, expected_range in (("bad", None), ("short", "bytes=4-")):
                requests.clear()
                self.target.write_bytes(b"old usable")
                item = {**self.item, "url": base + "/" + route, "fallback_urls": [base + "/good"]}
                m.download_file_with_resume(item, self.target)
                self.assertEqual(requests, [("/" + route, None), ("/good", expected_range)])
                self.assertEqual(self.target.read_bytes(), b"good model")
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    def test_wrong_206_preserves_target_and_part(self):
        self.target.write_bytes(b"old")
        _, part, _ = m.transaction_paths(self.target, self.item)
        part.write_bytes(b"good")
        with patch.object(m, "urlopen", return_value=Response(b" model", 206, {"Content-Range": "bytes 0-5/10"})), self.assertRaises(RuntimeError):
            m.download_file_with_resume(self.item, self.target)
        self.assertEqual(part.read_bytes(), b"good")
        self.assertEqual(self.target.read_bytes(), b"old")

    def test_range_200_restarts_and_416_retries_cleanly(self):
        for responses in ([Response(b"good model")], [HTTPError("https://example.invalid", 416, "range", {}, None), Response(b"good model")]):
            _, part, _ = m.transaction_paths(self.target, self.item)
            part.write_bytes(b"good")
            with patch.object(m, "urlopen", side_effect=responses):
                m.download_file_with_resume(self.item, self.target)
            self.assertEqual(self.target.read_bytes(), b"good model")

    def test_complete_part_commits_without_network(self):
        _, part, _ = m.transaction_paths(self.target, self.item)
        part.write_bytes(b"good model")
        with patch.object(m, "urlopen", side_effect=AssertionError("unexpected GET")):
            m.download_file_with_resume(self.item, self.target)
        self.assertEqual(self.target.read_bytes(), b"good model")

    def test_failed_promotion_restores_old_file(self):
        self.target.write_bytes(b"old")
        verified, _, backup = m.transaction_paths(self.target, self.item)
        verified.write_bytes(b"good model")
        original = Path.rename
        def rename(path, to):
            if path == verified:
                raise PermissionError("injected promotion failure")
            return original(path, to)
        with patch.object(Path, "rename", rename), self.assertRaises(PermissionError):
            m.promote(self.target, self.item)
        self.assertEqual(self.target.read_bytes(), b"old")
        self.assertEqual(verified.read_bytes(), b"good model")
        self.assertFalse(backup.exists())

    def test_crash_recovery_and_conflict_preserves_evidence(self):
        verified, _, backup = m.transaction_paths(self.target, self.item)
        backup.write_bytes(b"old")
        verified.write_bytes(b"good model")
        m.recover(self.target, self.item)
        self.assertEqual(self.target.read_bytes(), b"old")
        m.promote(self.target, self.item)
        backup.write_bytes(b"old")
        m.recover(self.target, self.item)
        self.assertFalse(backup.exists())
        self.target.write_bytes(b"bad")
        backup.write_bytes(b"old")
        with self.assertRaises(RuntimeError):
            m.recover(self.target, self.item)
        self.assertEqual(backup.read_bytes(), b"old")

    def test_complete_legacy_group_kept_partial_blocked_and_mix_rejected(self):
        manifest, data = self.fixture_suite()
        legacy = copy.deepcopy(manifest["kws_groups"][0])
        legacy["group_id"] = "legacy"
        for p in legacy["files"]:
            legacy["files"][p] = hashlib.sha256(b"legacy" + data[p]).hexdigest()
            (self.root / p).write_bytes(b"legacy" + data[p])
        manifest["kws_groups"].append(legacy)
        # Select entire legacy as a group; never accept per-file cross-group aliases.
        self.assertEqual(m.select_kws_group(self.root, manifest), legacy)
        (self.root / "kws/encoder.int8.onnx").unlink()
        with patch.object(m, "urlopen", side_effect=AssertionError("unexpected GET")), self.assertRaisesRegex(RuntimeError, "no verified repair source"):
            m.prepare(self.root, manifest)
        (self.root / "kws/encoder.int8.onnx").write_bytes(data["kws/encoder.int8.onnx"])
        with self.assertRaisesRegex(RuntimeError, "ambiguous"):
            m.select_kws_group(self.root, manifest)
        self.assertEqual(m.select_kws_group(self.root, manifest, True)["group_id"], "default")

    def test_empty_default_ambiguous_and_marker_recovery(self):
        manifest, data = self.fixture_suite()
        for p in manifest["kws_groups"][0]["files"]:
            (self.root / p).write_bytes(b"damaged")
        with self.assertRaisesRegex(RuntimeError, "ambiguous"):
            m.select_kws_group(self.root, manifest)
        (self.root / "kws/.voice-suite-group").write_text("default\n")
        self.assertEqual(m.select_kws_group(self.root, manifest)["group_id"], "default")
        for item in manifest["files"]:
            item["url"] = "https://example.invalid/" + item["path"]
        with patch.object(m, "urlopen", side_effect=lambda req, **kw: Response(data[req.full_url.removeprefix("https://example.invalid/")])) as fetch:
            m.prepare(self.root, manifest)
        self.assertEqual(fetch.call_count, 4)
        self.assertTrue(all(m.valid(self.root / f["path"], f) for f in manifest["files"]))
        for p in manifest["kws_groups"][0]["files"]:
            (self.root / p).unlink()
        self.assertEqual(m.select_kws_group(self.root, manifest)["group_id"], "default")

    def test_complete_legacy_prepare_zero_requests(self):
        manifest, data = self.fixture_suite()
        legacy = copy.deepcopy(manifest["kws_groups"][0])
        legacy["group_id"] = "legacy"
        for p in legacy["files"]:
            content = data[p] + b"\n"
            (self.root / p).write_bytes(content)
            legacy["files"][p] = hashlib.sha256(content).hexdigest()
        manifest["kws_groups"].append(legacy)
        before = {p: (self.root / p).stat().st_mtime_ns for p in data}
        with patch.object(m, "urlopen", side_effect=AssertionError("unexpected GET")):
            m.prepare(self.root, manifest)
        self.assertEqual(before, {p: (self.root / p).stat().st_mtime_ns for p in data})
        self.assertEqual((self.root / "kws/.voice-suite-group").read_text(), "legacy\n")

    def test_marker_cannot_bypass_final_group_hashes(self):
        manifest, data = self.fixture_suite()
        (self.root / "kws/.voice-suite-group").write_text("default\n")
        (self.root / "kws/encoder.int8.onnx").write_bytes(b"bad")
        with patch.object(m, "download_file_with_resume", return_value=None), self.assertRaisesRegex(RuntimeError, "KWS group mismatch"):
            m.prepare(self.root, manifest)

    def test_confirmed_partial_legacy_rebuild_survives_interruption(self):
        manifest, data = self.fixture_suite()
        legacy = copy.deepcopy(manifest["kws_groups"][0])
        legacy["group_id"] = "legacy"
        for p in legacy["files"]:
            content = b"legacy" + data[p]
            legacy["files"][p] = hashlib.sha256(content).hexdigest()
            (self.root / p).write_bytes(content)
        manifest["kws_groups"].append(legacy)
        (self.root / "kws/encoder.int8.onnx").unlink()
        (self.root / "kws/.voice-suite-group").write_text("legacy")
        default = next(g for g in manifest["kws_groups"] if g["group_id"] == manifest["default_kws_group"])
        repaired = []
        def download(item, target):
            intent = json.loads((self.root / "kws/.voice-suite-rebuild-intent").read_text(encoding="utf-8"))
            self.assertEqual(intent, {"schema_version": 1, "group_id": default["group_id"], "files": default["files"]})
            if repaired:
                raise RuntimeError("interrupted")
            target.write_bytes(data[item["path"]])
            repaired.append(item["path"])
        with m.directory_write_lease(self.root), patch.object(m, "download_file_with_resume", side_effect=download):
            with self.assertRaisesRegex(RuntimeError, "interrupted"):
                m.prepare(self.root, manifest, True)
        # The confirmed target is still present after the failed second item.
        remaining = json.loads((self.root / "kws/.voice-suite-rebuild-intent").read_text(encoding="utf-8"))
        self.assertEqual(remaining["group_id"], "default")
        with m.directory_write_lease(self.root), patch.object(m, "download_file_with_resume", side_effect=lambda item, target: target.write_bytes(data[item["path"]])):
            # An ordinary retry follows the persisted confirmation without --rebuild-default-kws.
            m.prepare(self.root, manifest)
        self.assertFalse((self.root / "kws/.voice-suite-rebuild-intent").exists())
        self.assertFalse((self.root / "kws/.voice-suite-rebuild-intent.tmp").exists())
        self.assertTrue(all(m.valid(self.root / f["path"], f) for f in manifest["files"]))

    def test_conflicting_or_forged_rebuild_intent_is_rejected(self):
        manifest, data = self.fixture_suite()
        default = next(g for g in manifest["kws_groups"] if g["group_id"] == manifest["default_kws_group"])
        intent = self.root / "kws/.voice-suite-rebuild-intent"
        before = {p: (self.root / p).read_bytes() for p in data}
        intent.write_text(json.dumps({"schema_version": 1, "group_id": "legacy", "files": default["files"]}), encoding="utf-8")
        with m.directory_write_lease(self.root), self.assertRaisesRegex(RuntimeError, "conflicting"):
            m.prepare(self.root, manifest, True)
        forged = dict(default["files"])
        forged["kws/encoder.int8.onnx"] = "0" * 64
        intent.write_text(json.dumps({"schema_version": 1, "group_id": default["group_id"], "files": forged}), encoding="utf-8")
        with m.directory_write_lease(self.root), self.assertRaisesRegex(RuntimeError, "conflicting"):
            m.prepare(self.root, manifest)
        self.assertEqual(before, {p: (self.root / p).read_bytes() for p in data})

    def test_mixed_kws_without_intent_still_blocks(self):
        manifest, data = self.fixture_suite()
        (self.root / "kws/encoder.int8.onnx").write_bytes(b"foreign encoder")
        with patch.object(m, "urlopen", side_effect=AssertionError("unexpected GET")), self.assertRaisesRegex(RuntimeError, "ambiguous"):
            m.prepare(self.root, manifest)
        self.assertEqual((self.root / "kws/encoder.int8.onnx").read_bytes(), b"foreign encoder")

    def test_rebuild_intent_symlink_is_rejected(self):
        manifest, _ = self.fixture_suite()
        outside = self.root / "outside-intent"
        outside.write_text("not used", encoding="utf-8")
        intent = self.root / "kws/.voice-suite-rebuild-intent"
        try:
            os.symlink(outside, intent)
        except OSError as error:
            self.skipTest(f"symlink unsupported: {error}")
        with self.assertRaisesRegex(RuntimeError, "symlink"):
            m.read_rebuild_intent(self.root, manifest)
        self.assertEqual(outside.read_text(encoding="utf-8"), "not used")

    def test_bad_keywords_preserved(self):
        manifest, _ = self.fixture_suite()
        keywords = self.root / "kws/keywords.txt"
        keywords.write_text("INVALID @CUSTOM\n", encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "absent"):
            m.prepare(self.root, manifest)
        self.assertEqual(keywords.read_text(), "INVALID @CUSTOM\n")

    def test_offline_gate_and_exact_process_names(self):
        with patch.object(m, "running_process_names", return_value=["voice-companion.exe"]):
            with self.assertRaises(RuntimeError):
                m.require_offline(False)
            with self.assertRaises(RuntimeError):
                m.require_offline(True)
        with patch.object(m, "running_process_names", return_value=["my-flowy-editor.exe"]):
            m.require_offline(True)
        with patch.object(m, "running_process_names", side_effect=OSError("enumeration denied")), self.assertRaises(OSError):
            m.require_offline(True)

    def test_rust_python_lock_interoperability(self):
        source = self.root / "lock_probe.rs"
        source.write_text('''use std::{fs::OpenOptions, io::{self, Write}};
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let file = OpenOptions::new().read(true).write(true).create(true).open(path).unwrap();
    if file.try_lock().is_err() { std::process::exit(23); }
    println!("locked"); io::stdout().flush().unwrap();
    let mut line = String::new(); io::stdin().read_line(&mut line).unwrap();
}
''', encoding="utf-8")
        executable = self.root / ("lock_probe.exe" if os.name == "nt" else "lock_probe")
        version = subprocess.run(["rustc", "--version"], capture_output=True, text=True, check=True, timeout=10)
        print(version.stdout.strip())
        compiled = subprocess.run(["rustc", str(source), "-o", str(executable)], capture_output=True, timeout=60)
        self.assertEqual(compiled.returncode, 0, compiled.stderr)
        command = [str(executable), str(self.root / m.LOCK_NAME)]
        with m.directory_write_lease(self.root):
            child = subprocess.run(command, input=b"\n", capture_output=True, timeout=10)
            self.assertEqual(child.returncode, 23, child.stderr)
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(child.stdout.readline().strip(), "locked")
            with self.assertRaises((RuntimeError, OSError)):
                with m.directory_write_lease(self.root):
                    self.fail("Python acquired Rust-owned lock")
            _, error = child.communicate("release\n", timeout=10)
            self.assertEqual(child.returncode, 0, error)
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate()
        with m.directory_write_lease(self.root):
            pass
        if os.name == "nt":
            host = self.root / "voice-companion.exe"
            executable.rename(host)
            child = subprocess.Popen([str(host), str(self.root / m.LOCK_NAME)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                self.assertEqual(child.stdout.readline().strip(), "locked")
                with self.assertRaisesRegex(RuntimeError, "voice-companion"):
                    m.require_offline(True)
            finally:
                child.communicate("release\n", timeout=10)

    def test_os_lock_contends_across_process_and_releases(self):
        code = "import importlib.util,sys; from pathlib import Path; s=importlib.util.spec_from_file_location('m',sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m);\nwith m.directory_write_lease(Path(sys.argv[2])): pass"
        command = [sys.executable, "-c", code, str(Path(m.__file__)), str(self.root)]
        with m.directory_write_lease(self.root):
            child = subprocess.run(command, capture_output=True, timeout=10)
            self.assertNotEqual(child.returncode, 0, child.stdout)
        self.assertTrue((self.root / m.LOCK_NAME).exists())
        child = subprocess.run(command, capture_output=True, timeout=10)
        self.assertEqual(child.returncode, 0, child.stderr)


if __name__ == "__main__":
    unittest.main()
