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
    {
        "rel_path": "tts/matcha-icefall-zh-baker/model-steps-3.onnx",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-zh-baker/resolve/main/model-steps-3.onnx",
        "sha256": "78a9fd17e9a6c20198dca4e69d18f941303f8ab39ec61dde04cfc7595262ddfd",
        "desc": "Matcha Chinese acoustic model",
    },
    {
        "rel_path": "tts/matcha-icefall-zh-baker/tokens.txt",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-zh-baker/resolve/main/tokens.txt",
        "sha256": "56209b2bf609d5ac1d66ede6dae7bf5254bd3f8aa24c4a6823713d5b884d87ba",
        "desc": "Matcha Chinese tokens",
    },
    {
        "rel_path": "tts/matcha-icefall-zh-baker/lexicon.txt",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-zh-baker/resolve/main/lexicon.txt",
        "sha256": "38b886d46aefa50da6322a64d72fd595d5f4fae1051adb160d647541b1e0a4a2",
        "desc": "Matcha Chinese lexicon",
    },
    {
        "rel_path": "tts/matcha-icefall-zh-baker/date.fst",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-zh-baker/resolve/main/date.fst",
        "sha256": "eb8aa079ae3cb81d8f4404992f39d61a0cb990947512b5b8d1e54d1f6980e718",
        "desc": "Matcha Chinese date normalizer",
    },
    {
        "rel_path": "tts/matcha-icefall-zh-baker/number.fst",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-zh-baker/resolve/main/number.fst",
        "sha256": "743f402181fcfebf76cc2f0546b71fa26476e626fbe4e460fb7b4c3a7a8bd5bd",
        "desc": "Matcha Chinese number normalizer",
    },
    {
        "rel_path": "tts/matcha-icefall-zh-baker/phone.fst",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-zh-baker/resolve/main/phone.fst",
        "sha256": "1ac2b6fa56b1442320c4de7db08353bab8963a2b57f365eebcdd3a2d3562f8d7",
        "desc": "Matcha Chinese phone normalizer",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/model-steps-3.onnx",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/model-steps-3.onnx",
        "sha256": "4d7771c0ec063ca74f2fa92ba0ecfe14f73fa3f61c236d9469b92e102d8e9574",
        "desc": "Matcha English acoustic model",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/tokens.txt",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/tokens.txt",
        "sha256": "bcb4a50830e9402112fe8cb57d53ae8523908868a5015f2040dde5f5fd231697",
        "desc": "Matcha English tokens",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/en_dict",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/en_dict",
        "sha256": "71bd330ba8a2e3e8076e631508208ef49449d6147c17b7bd2b4b1e1468292e35",
        "desc": "espeak-ng English dictionary",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/phontab",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/phontab",
        "sha256": "886f3fa402cb0ba73d483aa8ad000af47a6b7cc06293c75a97913fba68a530f6",
        "desc": "espeak-ng English phoneme table",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/phonindex",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/phonindex",
        "sha256": "3ca7b8fa3b42624e4b0f152707e7a39245fce569aa99ea47c055d9e622fcf0c4",
        "desc": "espeak-ng English phoneme index",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/phondata",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/phondata",
        "sha256": "4e0288957874029a8c3c9f41a8f517ad4bf18127046decbdd4b9d1d6807ce3a3",
        "desc": "espeak-ng English phoneme data",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/intonations",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/intonations",
        "sha256": "3f8af65fd3eda9759a10f021d61361c120871f463515229c925995c7f90918cc",
        "desc": "espeak-ng English intonations",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/lang/gmw/en",
        "sha256": "4605d5330801de3641c6e366d15f129ea1f5ffbce8722642aba01ace07ab9c83",
        "desc": "espeak-ng English language definition",
    },
    {
        "rel_path": "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en-US",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/csukuangfj/matcha-icefall-en_US-ljspeech/resolve/main/espeak-ng-data/lang/gmw/en-US",
        "sha256": "41534c2a22df5dd4f1052ff9e1a33a3ea7bff5a26b5c02bdad5ba8ddb7524704",
        "desc": "espeak-ng English language definition-US",
    },
    {
        "rel_path": "tts/vocos-22khz-univ.onnx",
        "url_modelscope": None,  # ModelScope does not host the TTS subtree; use the HF mirror.
        "url_hf": "https://hf-mirror.com/k2-fsa/sherpa-onnx-models/resolve/main/vocoder-models/vocos-22khz-univ.onnx",
        "sha256": "0574a135aa1db2de6e181050db2ec528496cacd4a4701fc5d7faf9f9804c0081",
        "desc": "Vocos vocoder",
    },
]


def detect_default_channel() -> str:
    """Pick the data channel when the operator did not pass --channel.

    A source checkout is a development environment by default; installed/release
    operators can always pass --channel stable explicitly.
    """
    env_channel = os.environ.get("NOMI_CHANNEL", "").strip()
    if env_channel:
        return env_channel
    repo_root = Path(__file__).resolve().parent.parent
    if (repo_root / "apps" / "desktop" / "src" / "main.rs").is_file():
        return "dev"
    return "stable"


def channel_leaf(channel: str) -> str:
    return "Nomi" if not channel or channel == "stable" else f"Nomi-{channel}"


def get_default_models_dir(channel: str | None = None) -> Path:
    env_dir = os.environ.get("VOICE_COMPANION_MODEL_DIR")
    if env_dir and env_dir.strip():
        return Path(env_dir.strip())
    for env_name in ("NOMIFUN_DATA_DIR", "FLOWY_DATA_DIR"):
        data_dir = os.environ.get(env_name)
        if data_dir and data_dir.strip():
            return Path(data_dir.strip()) / "models"

    leaf = channel_leaf(channel or detect_default_channel())
    if sys.platform == "win32":
        local_app_data = os.environ.get("LOCALAPPDATA")
        if local_app_data:
            return Path(local_app_data) / "Flowy" / leaf / "models"
        return Path("C:/ProgramData/Flowy/Nomi/models")
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "Flowy" / leaf / "models"
    data_home = os.environ.get("XDG_DATA_HOME")
    if data_home:
        return Path(data_home) / "Flowy" / leaf / "models"
    return Path.home() / ".local" / "share" / "Flowy" / leaf / "models"


def compute_file_sha256(filepath: Path) -> str:
    hasher = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            hasher.update(chunk)
    return hasher.hexdigest().lower()


def hf_official_url(url: str) -> str | None:
    """Return the official HuggingFace equivalent of an hf-mirror URL."""
    prefix = "https://hf-mirror.com/"
    if url.startswith(prefix):
        return "https://huggingface.co/" + url[len(prefix):]
    return None


def format_size(bytes_val: int) -> str:
    if bytes_val < 1024:
        return f"{bytes_val} B"
    elif bytes_val < 1024 * 1024:
        return f"{bytes_val / 1024:.1f} KB"
    else:
        return f"{bytes_val / (1024 * 1024):.2f} MB"


def download_file_with_resume(url: str, target_path: Path, desc: str, expected_sha: str = None) -> bool:
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
            # Range Not Satisfiable: file might already be complete or corrupted
            if part_path.exists():
                part_sha = compute_file_sha256(part_path)
                if expected_sha and (
                    part_sha == expected_sha
                    or (part_sha == "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3")
                ):
                    if target_path.exists():
                        target_path.unlink()
                    part_path.rename(target_path)
                    return True
                # If hash doesn't match, remove corrupted part file and redownload from scratch
                part_path.unlink()
                req_fresh = Request(url)
                req_fresh.add_header("User-Agent", "Flowy-Voice-Models-Installer/1.0")
                try:
                    with urlopen(req_fresh, timeout=30) as fresh_resp, open(part_path, "wb") as out_f:
                        while chunk := fresh_resp.read(65536):
                            out_f.write(chunk)
                    if part_path.exists():
                        if target_path.exists():
                            target_path.unlink()
                        part_path.rename(target_path)
                        return True
                except Exception as retry_err:
                    print(f"\n  [Error] Retry after 416 failed: {retry_err}")
                    return False
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
        help="Model repository source: 'modelscope' (fastest in China) or 'hf' (HuggingFace); TTS always uses the HF mirror",
    )
    parser.add_argument(
        "--channel",
        choices=["stable", "dev"],
        default=None,
        help="Data channel (default: auto; source checkout uses dev, installed/release uses stable)",
    )
    args = parser.parse_args()
    channel = args.channel or detect_default_channel()

    models_dir = (
        Path(args.output_dir)
        if args.output_dir
        else get_default_models_dir(channel)
    )
    print("=" * 72)
    print("Flowy Voice Models Downloader")
    print(f"Target Directory: {models_dir.resolve()}")
    print(f"Channel: {channel}")
    print(f"Preferred Mirror: {args.mirror}")
    print("=" * 72)
    if channel == "dev" and not args.output_dir:
        print("Using the dev data root. Pass --channel stable for a release install root.")

    models_dir.mkdir(parents=True, exist_ok=True)

    verified_manifest = {}
    success_count = 0

    for idx, item in enumerate(MODELS_MANIFEST, start=1):
        rel_path = item["rel_path"]
        desc = item["desc"]
        expected_sha = item["sha256"]
        url = item["url_modelscope"] if args.mirror == "modelscope" else item["url_hf"]
        # ModelScope does not host the offline TTS subtree; its entries declare
        # url_modelscope=None, so fall back to the HF mirror for those files.
        if not url:
            url = item["url_hf"]
        candidate_urls = [url]
        official_url = hf_official_url(url)
        if official_url and official_url != url:
            candidate_urls.append(official_url)
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

        # Download with an hf-mirror -> huggingface.co fallback.
        ok = False
        for candidate_index, candidate_url in enumerate(candidate_urls):
            if candidate_index > 0:
                print(f"  [Fallback] Trying {candidate_url.split('/')[2]}...")
            ok = download_file_with_resume(candidate_url, target_path, desc, expected_sha)
            if ok:
                break
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

    # Point custom output directories at the desktop app's active data root.
    desktop_models_dir = get_default_models_dir(channel)
    if desktop_models_dir != models_dir:
        print("\nTip: Desktop app model directory is " + str(desktop_models_dir) + ".")
        print("If you copied models elsewhere, set VOICE_COMPANION_MODEL_DIR for the app.")

    print(f"SUCCESS: All {success_count} voice models downloaded and verified!")
    print(f"Models directory is Ready for Flowy AI Mate & Desktop Voice Companion.")
    print("=" * 72)


if __name__ == "__main__":
    main()
