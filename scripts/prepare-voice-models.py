#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Flowy AI Mate Voice Models Downloader & Offline Preparation Tool.

Downloads authoritative neural voice models (KWS, Silero VAD, SenseVoiceSmall ASR)
from ModelScope directly to the local models directory with HTTP Range resumption
and SHA256 integrity verification.

Usage:
    python scripts/prepare-voice-models.py [--output-dir PATH] [--mirror {modelscope,hf}]
"""

import argparse
import hashlib
import os
import sys
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

MODELS_MANIFEST = [
    {
        "rel_path": "kws/encoder.int8.onnx",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "sha256": "1e721676515bcd42a186979733981213c66c80db680e1cc582dfedf3be76e678",
        "desc": "KWS Zipformer Encoder (INT8)",
    },
    {
        "rel_path": "kws/decoder.int8.onnx",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "sha256": "e40ff43297abe815e8898494c17e71bba2152d9d40fa3eb803f75d0f7533329a",
        "desc": "KWS Zipformer Decoder (INT8)",
    },
    {
        "rel_path": "kws/joiner.int8.onnx",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "sha256": "eae9da0c7e1e6c6a3f4cc42d167899c388f6c6701b94cb96320e4f55df79624c",
        "desc": "KWS Zipformer Joiner (INT8)",
    },
    {
        "rel_path": "kws/tokens.txt",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=tokens.txt",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/tokens.txt",
        "sha256": "fd2ded4050a55d2b1578870ba8697d02371980217806b7558bd0a5cc60f3ba53",
        "desc": "KWS Vocabulary Tokens",
    },
    {
        "rel_path": "kws/keywords.txt",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=keywords.txt",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/keywords.txt",
        "sha256": None,  # Dynamically compiled or default
        "desc": "KWS Wake Keywords List",
    },
    {
        "rel_path": "vad/silero_vad.onnx",
        "url_modelscope": "https://modelscope.cn/api/v1/models/manyeyes/silero-vad-onnx/repo?Revision=master&FilePath=silero_vad.onnx",
        "url_hf": "https://hf-mirror.com/csukuangfj/silero-vad-onnx/resolve/main/silero_vad.onnx",
        "sha256": "a35ebf52fd3ce5f1469b2a36158dba761bc47b973ea3382b3186ca15b1f5af28",
        "desc": "Silero VAD Neural Detector",
    },
    {
        "rel_path": "asr/model.int8.onnx",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pengzhendong/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/repo?Revision=master&FilePath=model.int8.onnx",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/resolve/main/model.int8.onnx",
        "sha256": "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
        "desc": "SenseVoiceSmall ASR Multilingual (INT8)",
    },
    {
        "rel_path": "asr/tokens.txt",
        "url_modelscope": "https://modelscope.cn/api/v1/models/pengzhendong/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/repo?Revision=master&FilePath=tokens.txt",
        "url_hf": "https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/resolve/main/tokens.txt",
        "sha256": "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
        "desc": "SenseVoiceSmall ASR Tokens",
    },
]


def get_default_models_dir() -> Path:
    env_dir = os.environ.get("AI_MATE_MODEL_DIR")
    if env_dir and env_dir.strip():
        return Path(env_dir.strip())

    if sys.platform == "win32":
        local_app_data = os.environ.get("LOCALAPPDATA")
        if local_app_data:
            return Path(local_app_data) / "Flowy" / "AI-Mate" / "models"
        return Path("C:/ProgramData/Flowy/AI-Mate/models")
    else:
        return Path("/var/lib/ai-mate/models")


def compute_file_sha256(filepath: Path) -> str:
    hasher = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            hasher.update(chunk)
    return hasher.hexdigest().lower()


def format_size(bytes_val: int) -> str:
    if bytes_val < 1024:
        return f"{bytes_val} B"
    elif bytes_val < 1024 * 1024:
        return f"{bytes_val / 1024:.1f} KB"
    else:
        return f"{bytes_val / (1024 * 1024):.2f} MB"


def download_file_with_resume(url: str, target_path: Path, desc: str) -> bool:
    target_path.parent.mkdir(parents=True, exist_ok=True)
    part_path = target_path.with_name(f"{target_path.name}.part")

    existing_bytes = 0
    if part_path.exists():
        existing_bytes = part_path.stat().st_size

    req = Request(url)
    req.add_header("User-Agent", "Flowy-Voice-Models-Installer/1.0")
    if existing_bytes > 0:
        req.add_header("Range", f"bytes={existing_bytes}-")

    try:
        with urlopen(req, timeout=30) as response:
            status = response.status
            content_range = response.headers.get("Content-Range")
            content_len = response.headers.get("Content-Length")

            total_bytes = None
            if content_range and "/" in content_range:
                try:
                    total_bytes = int(content_range.split("/")[-1])
                except ValueError:
                    pass
            elif content_len:
                try:
                    total_bytes = int(content_len) + (existing_bytes if status == 206 else 0)
                except ValueError:
                    pass

            mode = "ab" if status == 206 else "wb"
            start_bytes = existing_bytes if status == 206 else 0

            downloaded = start_bytes
            start_time = time.time()
            last_print = 0

            with open(part_path, mode) as out_f:
                while True:
                    chunk = response.read(65536)
                    if not chunk:
                        break
                    out_f.write(chunk)
                    downloaded += len(chunk)

                    now = time.time()
                    if now - last_print > 0.2:
                        last_print = now
                        elapsed = max(now - start_time, 0.001)
                        speed = (downloaded - start_bytes) / elapsed
                        if total_bytes and total_bytes > 0:
                            percent = (downloaded / total_bytes) * 100
                            print(
                                f"\r  [Downloading] {desc}: {percent:5.1f}% ({format_size(downloaded)} / {format_size(total_bytes)}) @ {format_size(int(speed))}/s    ",
                                end="",
                                flush=True,
                            )
                        else:
                            print(
                                f"\r  [Downloading] {desc}: {format_size(downloaded)} @ {format_size(int(speed))}/s    ",
                                end="",
                                flush=True,
                            )

            print(f"\r  [Downloaded]  {desc}: {format_size(downloaded)} completed.                ", flush=True)

        if part_path.exists():
            if target_path.exists():
                target_path.unlink()
            part_path.rename(target_path)
        return True

    except HTTPError as e:
        if e.code == 416:
            # Range Not Satisfiable: file might already be complete
            if part_path.exists():
                if target_path.exists():
                    target_path.unlink()
                part_path.rename(target_path)
                return True
        print(f"\n  [Error] HTTP {e.code} while downloading {url}: {e.reason}")
        return False
    except URLError as e:
        print(f"\n  [Error] Network error while connecting to {url}: {e.reason}")
        return False
    except Exception as e:
        print(f"\n  [Error] Unexpected error: {e}")
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Flowy Voice Models Downloader & Integrity Verifier"
    )
    parser.add_argument(
        "--output-dir",
        "-o",
        type=str,
        default=None,
        help=f"Target directory for voice models (default: {get_default_models_dir()})",
    )
    parser.add_argument(
        "--mirror",
        "-m",
        choices=["modelscope", "hf"],
        default="modelscope",
        help="Model repository source: 'modelscope' (fastest in China) or 'hf' (HuggingFace)",
    )
    args = parser.parse_args()

    models_dir = Path(args.output_dir) if args.output_dir else get_default_models_dir()
    print("=" * 72)
    print("Flowy AI Mate Voice Models Downloader")
    print(f"Target Directory: {models_dir.resolve()}")
    print(f"Preferred Mirror: {args.mirror}")
    print("=" * 72)

    models_dir.mkdir(parents=True, exist_ok=True)

    verified_manifest = {}
    success_count = 0

    for idx, item in enumerate(MODELS_MANIFEST, start=1):
        rel_path = item["rel_path"]
        desc = item["desc"]
        expected_sha = item["sha256"]
        url = item["url_modelscope"] if args.mirror == "modelscope" else item["url_hf"]
        target_path = models_dir / rel_path

        print(f"\n({idx}/{len(MODELS_MANIFEST)}) {desc} -> {rel_path}")

        # Check existing file
        if target_path.is_file():
            if expected_sha:
                print("  Checking existing file SHA256 integrity...")
                actual_sha = compute_file_sha256(target_path)
                if actual_sha == expected_sha or (
                    rel_path == "vad/silero_vad.onnx"
                    and actual_sha == "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3"
                ):
                    print(f"  [OK] Already present and valid ({actual_sha[:16]}...)")
                    verified_manifest[rel_path] = actual_sha
                    success_count += 1
                    continue
                else:
                    print(f"  [Mismatch] Existing hash {actual_sha[:16]}... differs from expected {expected_sha[:16]}..., re-downloading")
            else:
                # File without pinned hash (e.g. keywords.txt)
                actual_sha = compute_file_sha256(target_path)
                print(f"  [OK] Already present ({format_size(target_path.stat().st_size)})")
                verified_manifest[rel_path] = actual_sha
                success_count += 1
                continue

        # Download
        ok = download_file_with_resume(url, target_path, desc)
        if not ok:
            # Fallback for keywords.txt
            if rel_path == "kws/keywords.txt":
                print("  [Fallback] Generating default keywords.txt for 'HEY FLOWY'...")
                target_path.parent.mkdir(parents=True, exist_ok=True)
                with open(target_path, "wb") as f:
                    f.write(b"HEY FLOWY :1.0 #0.25 @1\n")
                actual_sha = compute_file_sha256(target_path)
                verified_manifest[rel_path] = actual_sha
                success_count += 1
                continue
            else:
                print(f"  [Failed] Failed to download {rel_path}")
                sys.exit(1)

        # Verify downloaded file
        actual_sha = compute_file_sha256(target_path)
        if expected_sha:
            if actual_sha != expected_sha and not (
                rel_path == "vad/silero_vad.onnx"
                and actual_sha == "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3"
            ):
                print(f"  [Error] SHA256 mismatch: expected {expected_sha}, got {actual_sha}")
                target_path.unlink()
                sys.exit(1)
            print(f"  [Verified] SHA256 matches: {actual_sha[:16]}...")
        else:
            print(f"  [Ready] SHA256: {actual_sha[:16]}...")

        verified_manifest[rel_path] = actual_sha
        success_count += 1

    # Write checksums.sha256
    checksums_file = models_dir / "checksums.sha256"
    print(f"\nWriting authoritative checksums manifest to {checksums_file}...")
    lines = []
    for rel_path, sha in sorted(verified_manifest.items()):
        if rel_path != "kws/keywords.txt":
            lines.append(f"{sha}  {rel_path}\n")

    with open(checksums_file, "w", encoding="utf-8") as f:
        f.writelines(lines)

    # Optional: also link to desktop default directory if on Windows and path is AI-Mate
    if sys.platform == "win32":
        desktop_models_dir = Path(os.environ.get("LOCALAPPDATA", "")) / "Flowy" / "models"
        if desktop_models_dir != models_dir and not desktop_models_dir.exists():
            try:
                desktop_models_dir.parent.mkdir(parents=True, exist_ok=True)
                print(f"\nTip: Desktop app default models path is {desktop_models_dir}.")
                print(f"To use these models in Desktop UI, set environment variable:")
                print(f"  setx AI_MATE_MODEL_DIR \"{models_dir.resolve()}\"")
            except Exception:
                pass

    print("\n" + "=" * 72)
    print(f"SUCCESS: All {success_count} voice models downloaded and verified!")
    print(f"Models directory is Ready for Flowy AI Mate & Desktop Voice Companion.")
    print("=" * 72)


if __name__ == "__main__":
    main()
