#!/usr/bin/env python3
"""Prepare the shared pinned voice suite, with all voice hosts stopped.

--offline confirms that hosts will remain stopped for this operation. Process
checks cannot prevent a host being started afterwards; do not start one until exit.
"""
import argparse
import contextlib
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from http.client import HTTPException
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

MANIFEST_PATH = Path(__file__).resolve().parents[1] / "crates/backend/nomifun-voice-models/resources/voice-suite-manifest.json"
LOCK_NAME = ".voice-suite-write.lock"
HOST_NAMES = {"voice-companion", "nomifun-desktop", "nomifun-web", "nomifun-app", "nomicore", "flowy", "nomi"}
DEFAULT_KEYWORDS = "▁HE Y ▁F L OW Y :1.5 #0.15 @1\n"


def load_manifest():
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    if manifest["schema_version"] != 1:
        raise ValueError("Unsupported voice manifest schema")
    seen = set()
    for item in manifest["files"]:
        relative = item["path"]
        if relative in seen or relative.startswith("/") or any(p in ("", ".", "..") for p in relative.split("/")) or "\\" in relative or ":" in relative:
            raise ValueError("Unsafe or duplicate manifest path")
        seen.add(relative)
        if not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]) or not isinstance(item["size"], int) or item["size"] <= 0:
            raise ValueError("Invalid manifest identity")
        for url in [item["url"], *item.get("fallback_urls", [])]:
            if not re.fullmatch(r"[0-9a-f]{40}", item["revision"]) or item["revision"] not in url or not url.startswith("https://"):
                raise ValueError("Resource source is not pinned")
    return manifest


def detect_default_channel():
    env_channel = os.environ.get("NOMI_CHANNEL", "").strip()
    if env_channel:
        return env_channel
    repo_root = Path(__file__).resolve().parent.parent
    return "dev" if (repo_root / "apps" / "desktop" / "src" / "main.rs").is_file() else "stable"


def channel_leaf(channel):
    return "Nomi" if not channel or channel == "stable" else f"Nomi-{channel}"


def get_default_models_dir(channel=None):
    env_dir = os.environ.get("VOICE_COMPANION_MODEL_DIR", "").strip()
    if env_dir:
        return Path(env_dir)
    for env_name in ("NOMIFUN_DATA_DIR", "FLOWY_DATA_DIR"):
        data_dir = os.environ.get(env_name, "").strip()
        if data_dir:
            return Path(data_dir) / "models"
    leaf = channel_leaf(channel or detect_default_channel())
    if sys.platform == "win32":
        local_app_data = os.environ.get("LOCALAPPDATA")
        return Path(local_app_data) / "Flowy" / leaf / "models" if local_app_data else Path("C:/ProgramData/Flowy/Nomi/models")
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "Flowy" / leaf / "models"
    return Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share"))) / "Flowy" / leaf / "models"


def running_process_names():
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        class PROCESSENTRY32W(ctypes.Structure):
            _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD), ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD), ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", wintypes.LONG), ("dwFlags", wintypes.DWORD), ("szExeFile", wintypes.WCHAR * 260)]
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        kernel.Process32FirstW.argtypes = kernel.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = kernel.CreateToolhelp32Snapshot(2, 0)
        if handle == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            entry = PROCESSENTRY32W()
            entry.dwSize = ctypes.sizeof(entry)
            if not kernel.Process32FirstW(handle, ctypes.byref(entry)):
                raise ctypes.WinError(ctypes.get_last_error())
            names = []
            while True:
                names.append(entry.szExeFile)
                if not kernel.Process32NextW(handle, ctypes.byref(entry)):
                    if ctypes.get_last_error() != 18:
                        raise ctypes.WinError(ctypes.get_last_error())
                    return names
        finally:
            kernel.CloseHandle(handle)
    result = subprocess.run(["ps", "-A", "-o", "comm="], capture_output=True, text=True, check=True, timeout=10)
    if not result.stdout.strip():
        raise RuntimeError("Process enumeration returned no processes")
    return result.stdout.splitlines()


def require_offline(confirmed):
    if not confirmed:
        raise RuntimeError("Close desktop/web hosts and voice-companion, then pass --offline; keep them stopped until this tool exits")
    active = {Path(name.strip()).name.lower().removesuffix(".exe") for name in running_process_names()} & HOST_NAMES
    if active:
        raise RuntimeError("Close active voice hosts before preparing models: " + ", ".join(sorted(active)))


@contextlib.contextmanager
def directory_write_lease(directory):
    directory.mkdir(parents=True, exist_ok=True)
    path = directory.resolve() / LOCK_NAME
    with path.open("a+b") as handle:
        if os.name == "nt":
            import ctypes
            import msvcrt
            from ctypes import wintypes
            class OVERLAPPED(ctypes.Structure):
                _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t), ("Offset", wintypes.DWORD), ("OffsetHigh", wintypes.DWORD), ("hEvent", wintypes.HANDLE)]
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.LockFileEx.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(OVERLAPPED)]
            kernel.UnlockFileEx.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(OVERLAPPED)]
            overlapped = OVERLAPPED()
            raw = msvcrt.get_osfhandle(handle.fileno())
            # Rust std::fs::File::try_lock uses this whole-file exclusive range.
            if not kernel.LockFileEx(raw, 3, 0, 0xffffffff, 0xffffffff, ctypes.byref(overlapped)):
                raise RuntimeError("Voice model directory is busy or cannot be locked")
            try:
                yield
            finally:
                if not kernel.UnlockFileEx(raw, 0, 0xffffffff, 0xffffffff, ctypes.byref(overlapped)):
                    raise ctypes.WinError(ctypes.get_last_error())
        else:
            import fcntl
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            try:
                yield
            finally:
                fcntl.flock(handle, fcntl.LOCK_UN)


def compute_file_sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def valid(path, item):
    return path.is_file() and path.stat().st_size == item["size"] and compute_file_sha256(path) == item["sha256"]


def transaction_paths(target, item):
    identity = f"{target.name}.{item['sha256']}.{item['size']}"
    verified = target.with_name(identity + ".verified")
    return verified, verified.with_name(verified.name + ".part"), target.with_name(identity + ".backup")


def recover(target, item):
    verified, part, backup = transaction_paths(target, item)
    if backup.exists():
        if valid(target, item):
            backup.unlink()
        elif not target.exists():
            backup.rename(target)
        else:
            raise RuntimeError(f"recovery: {item['path']}: target and backup conflict; preserve both for recovery")


def promote(target, item):
    verified, part, backup = transaction_paths(target, item)
    if not valid(verified, item):
        raise RuntimeError(f"verify: {item['path']}: staged size/SHA256 mismatch")
    if backup.exists():
        raise RuntimeError(f"recovery: {item['path']}: unresolved backup")
    had_target = target.exists()
    if had_target:
        target.rename(backup)
    try:
        verified.rename(target)
        if not valid(target, item):
            raise RuntimeError(f"verify: {item['path']}: promoted size/SHA256 mismatch")
    except BaseException:
        if had_target:
            if target.exists():
                target.rename(verified)
            backup.rename(target)
        raise
    if had_target:
        backup.unlink()


def download_file_with_resume(item, target):
    for index, url in enumerate([item["url"], *item.get("fallback_urls", [])]):
        try:
            download_from_source(item, target, url)
            break
        except (RuntimeError, URLError, TimeoutError, HTTPException) as error:
            if index == len(item.get("fallback_urls", [])):
                detail = str(error) if isinstance(error, RuntimeError) else type(error).__name__
                raise RuntimeError(f"download: {item['path']}: source {index + 1} failed: {detail}") from error
            print(f"Retrying {item['path']} with pinned source {index + 2}", flush=True)
    promote(target, item)


def download_from_source(item, target, url):
    target.parent.mkdir(parents=True, exist_ok=True)
    verified, part, backup = transaction_paths(target, item)
    if valid(verified, item):
        return
    if part.exists() and part.stat().st_size >= item["size"]:
        if valid(part, item):
            part.replace(verified)
            return
        part.unlink()
    offset = part.stat().st_size if part.exists() else 0
    for attempt in range(2):
        request = Request(url, headers={"User-Agent": "Flowy-Voice-Models-Installer/2.0", "Accept-Encoding": "identity"})
        if offset:
            request.add_header("Range", f"bytes={offset}-")
        try:
            response = urlopen(request, timeout=30)
        except HTTPError as error:
            if error.code == 416 and offset and attempt == 0:
                part.unlink()
                offset = 0
                continue
            raise RuntimeError(f"download: {item['path']}: HTTP {error.code}") from None
        with response:
            if response.status == 206:
                match = re.fullmatch(r"bytes (\d+)-(\d+)/(\d+)", response.headers.get("Content-Range", ""))
                if not match or tuple(map(int, match.groups())) != (offset, item["size"] - 1, item["size"]):
                    raise RuntimeError(f"download: {item['path']}: invalid Content-Range")
            elif response.status == 200:
                offset = 0
            else:
                raise RuntimeError(f"download: {item['path']}: unexpected HTTP status")
            length = response.headers.get("Content-Length")
            if length is not None and int(length) != item["size"] - offset:
                raise RuntimeError(f"download: {item['path']}: incorrect Content-Length")
            downloaded = offset
            with part.open("ab" if offset else "wb") as output:
                while chunk := response.read(65536):
                    downloaded += len(chunk)
                    if downloaded > item["size"]:
                        raise RuntimeError(f"download: {item['path']}: oversized body")
                    output.write(chunk)
                output.flush()
                os.fsync(output.fileno())
        if not valid(part, item):
            # Full bad objects must not poison the next Range attempt.
            if part.stat().st_size == item["size"]:
                part.unlink()
            raise RuntimeError(f"verify: {item['path']}: downloaded size/SHA256 mismatch")
        part.replace(verified)
        return


REBUILD_INTENT = "kws/.voice-suite-rebuild-intent"
REBUILD_INTENT_TMP = REBUILD_INTENT + ".tmp"


def _intent_document(group):
    return {
        "schema_version": 1,
        "group_id": group["group_id"],
        "files": {path: group["files"][path] for path in sorted(group["files"])},
    }


def _intent_text(group):
    return json.dumps(_intent_document(group), ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"


def _reject_symlink(path):
    if path.is_symlink():
        raise RuntimeError(f"configuration: KWS rebuild intent path must not be a symlink: {path}")


def read_rebuild_intent(directory, manifest):
    """Return the confirmed default group, validating the document identity.

    The document only selects a recovery target; verification hashes always come
    from the shared manifest, never from this file.
    """
    path = directory / REBUILD_INTENT
    if not path.exists() and not path.is_symlink():
        return None
    _reject_symlink(path)
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise RuntimeError(f"configuration: invalid KWS rebuild intent: {error}") from error
    default = next(g for g in manifest["kws_groups"] if g["group_id"] == manifest["default_kws_group"])
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        raise RuntimeError("configuration: unsupported KWS rebuild intent schema")
    if payload.get("group_id") != default["group_id"] or payload.get("files") != default["files"]:
        raise RuntimeError("configuration: conflicting KWS rebuild intent: only the current supported default group may be rebuilt")
    return default


def persist_rebuild_intent(directory, group):
    # Caller holds the shared directory lease; publish before any asset changes.
    path = directory / REBUILD_INTENT
    temporary = directory / REBUILD_INTENT_TMP
    _reject_symlink(directory / "kws")
    path.parent.mkdir(parents=True, exist_ok=True)
    _reject_symlink(directory / "kws")
    _reject_symlink(path)
    _reject_symlink(temporary)
    expected = _intent_text(group)
    if path.exists():
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            raise RuntimeError(f"configuration: invalid KWS rebuild intent: {error}") from error
        if payload == _intent_document(group):
            temporary.unlink(missing_ok=True)
            return
        raise RuntimeError("configuration: conflicting KWS rebuild intent: only the current supported default group may be rebuilt")
    temporary.unlink(missing_ok=True)
    with temporary.open("xb") as output:
        output.write(expected.encode("utf-8"))
        output.flush()
        os.fsync(output.fileno())
    temporary.rename(path)
    if os.name != "nt":
        descriptor = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)


def clear_rebuild_intent(directory):
    for name in (REBUILD_INTENT, REBUILD_INTENT_TMP):
        path = directory / name
        if path.is_symlink() or path.exists():
            path.unlink(missing_ok=True)


def select_kws_group(directory, manifest, rebuild_default=False):
    groups = manifest["kws_groups"]
    default = next(g for g in groups if g["group_id"] == manifest["default_kws_group"])
    intent = read_rebuild_intent(directory, manifest)
    if intent is not None:
        # A persisted confirmation is followed by ordinary retries.
        return intent
    if rebuild_default:
        return default
    paths = set(groups[0]["files"])
    actual = {p: compute_file_sha256(directory / p) for p in paths if (directory / p).is_file()}
    for group in groups:
        if actual == group["files"]:
            return group
    if not actual:
        return default
    marker = directory / "kws/.voice-suite-group"
    if marker.is_file():
        recorded = marker.read_text(encoding="utf-8").strip()
        selected = next((g for g in groups if g["group_id"] == recorded), None)
        if selected is None:
            raise RuntimeError(f"configuration: recorded KWS group {recorded!r} is unsupported; preserve files or explicitly use --rebuild-default-kws to select the supported default")
        identified = [g for g in groups if all(g["files"].get(p) == h for p, h in actual.items())]
        if identified and identified[0]["group_id"] != selected["group_id"]:
            raise RuntimeError(f"configuration: recorded KWS group {recorded!r} contradicts the existing files; preserve files or explicitly use --rebuild-default-kws to select the supported default")
        return selected
    candidates = [g for g in groups if all(g["files"].get(p) == h for p, h in actual.items())]
    if len(candidates) == 1:
        return candidates[0]
    raise RuntimeError("configuration: ambiguous KWS group; preserve files or explicitly use --rebuild-default-kws to select the supported default")



def verify_keywords(directory):
    tokens = {line.split()[0] for line in (directory / "kws/tokens.txt").read_text(encoding="utf-8").splitlines() if line.strip()}
    path = directory / "kws/keywords.txt"
    content = path.read_text(encoding="utf-8") if path.exists() else DEFAULT_KEYWORDS
    count = 0
    for line in content.splitlines():
        words = line.split()
        if not words or words[0].startswith("#"):
            continue
        phrase = []
        for word in words:
            if word.startswith((":", "#", "@")):
                break
            phrase.append(word)
        if not phrase or any(word not in tokens for word in phrase):
            raise RuntimeError("configuration: kws/keywords.txt contains tokens absent from selected KWS vocabulary; edit keywords explicitly")
        count += 1
    if not count:
        raise RuntimeError("configuration: kws/keywords.txt contains no wake phrases")
    if not path.exists():
        with path.open("x", encoding="utf-8", newline="\n") as output:
            output.write(content)


def prepare(directory, manifest, rebuild_default=False):
    default = next(g for g in manifest["kws_groups"] if g["group_id"] == manifest["default_kws_group"])
    if rebuild_default:
        persist_rebuild_intent(directory, default)
    for item in manifest["files"]:
        recover(directory / item["path"], item)
    group = select_kws_group(directory, manifest, rebuild_default)
    # Block an unrecoverable compatibility group before changing other resources.
    for item in manifest["files"]:
        expected = group["files"].get(item["path"])
        target = directory / item["path"]
        if expected and expected != item["sha256"] and (not target.is_file() or compute_file_sha256(target) != expected):
            raise RuntimeError(f"configuration: {item['path']}: selected KWS group has no verified repair source")
    for item in manifest["files"]:
        target = directory / item["path"]
        if item["path"] in group["files"]:
            expected = group["files"][item["path"]]
            if target.is_file() and compute_file_sha256(target) == expected:
                continue
            if expected != item["sha256"]:
                raise RuntimeError(f"configuration: {item['path']}: selected KWS group has no verified repair source")
        if not valid(target, item):
            print(f"Repairing {item['path']}", flush=True)
            download_file_with_resume(item, target)
    if any(not (directory / path).is_file() or compute_file_sha256(directory / path) != sha for path, sha in group["files"].items()):
        raise RuntimeError("verify: KWS group mismatch")
    verify_keywords(directory)
    marker = directory / "kws/.voice-suite-group"
    group_text = group["group_id"] + "\n"
    if not marker.exists() or marker.read_text(encoding="utf-8") != group_text:
        marker.write_text(group_text, encoding="utf-8", newline="\n")
    for item in manifest["files"]:
        if item["path"] not in group["files"] and not valid(directory / item["path"], item):
            raise RuntimeError(f"verify: {item['path']}: final verification failed")
    checksums = directory / "checksums.sha256"
    text = "".join(f"{group['files'].get(item['path'], item['sha256'])}  {item['path']}\n" for item in manifest["files"])
    if not checksums.exists() or checksums.read_text(encoding="utf-8") != text:
        checksums.write_text(text, encoding="utf-8", newline="\n")
    clear_rebuild_intent(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", "-o")
    parser.add_argument("--channel", choices=["stable", "dev"])
    parser.add_argument("--offline", action="store_true", help="Confirm all voice hosts are stopped and will remain stopped")
    parser.add_argument("--rebuild-default-kws", action="store_true", help="Explicitly rebuild an ambiguous KWS set using the supported default group; keep custom keywords")
    parser.add_argument("--mirror", "-m", choices=["modelscope", "hf"], help="Deprecated: each resource uses only its verified pinned manifest source")
    args = parser.parse_args()
    try:
        require_offline(args.offline)
        manifest = load_manifest()
        directory = Path(args.output_dir) if args.output_dir else get_default_models_dir(args.channel)
        with directory_write_lease(directory):
            require_offline(args.offline)
            prepare(directory, manifest, args.rebuild_default_kws)
        print("Voice model files verified. Start the host separately for device and voice capability checks.")
        return 0
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        print(f"Failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
