#!/usr/bin/env python3
"""Assemble a short-drama voice bank from redistributable sample clips.

Sources (all fetched, not live TTS at runtime):
  - Piper official speaker samples (MIT) via hf-mirror
  - CosyVoice / OpenVoice demo wavs (Apache-2.0) via jsDelivr
  - Windows SAPI installed voices (optional local fill)

Output: allo/staging/voice-bank/ + staging/voice-bank-<ver>.zip
"""
from __future__ import annotations

import json
import os
import shutil
import ssl
import subprocess
import tarfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "staging" / "voice-bank"
WAV_DIR = OUT / "wav"
TMP = OUT / "_tmp"
VERSION = "1.1.0"
USER_AGENT = "allo-voice-bank-builder/1.0"
HF = "https://hf-mirror.com/rhasspy/piper-voices/resolve/main"
SSL_CTX = ssl.create_default_context()

# VCTK speaker-id → gender from the public CSTR speaker-info table.
VCTK_GENDER = {
    "p225": "female", "p226": "male", "p227": "male", "p228": "female",
    "p229": "female", "p230": "female", "p231": "female", "p232": "male",
    "p233": "female", "p234": "female", "p236": "female", "p237": "male",
    "p238": "female", "p239": "female", "p240": "female", "p241": "male",
    "p243": "male", "p244": "female", "p245": "male", "p246": "female",
    "p247": "male", "p248": "female", "p249": "female", "p250": "female",
    "p251": "male", "p252": "male", "p253": "female", "p254": "male",
    "p255": "male", "p256": "male", "p257": "female", "p258": "male",
    "p259": "male", "p260": "male", "p261": "female", "p262": "female",
    "p263": "female", "p264": "female", "p265": "female", "p266": "female",
    "p267": "female", "p268": "female", "p269": "female", "p270": "male",
    "p271": "male", "p272": "male", "p273": "male", "p274": "male",
    "p275": "male", "p276": "female", "p277": "male", "p278": "female",
    "p279": "male", "p281": "male", "p282": "female", "p283": "male",
    "p284": "male", "p285": "female", "p286": "male", "p287": "male",
    "p288": "female", "p292": "male", "p293": "female", "p294": "female",
    "p295": "female", "p297": "female", "p298": "male", "p299": "female",
    "p300": "female", "p301": "female", "p302": "male", "p303": "female",
    "p304": "female", "p305": "female", "p306": "female", "p307": "female",
    "p308": "female", "p310": "female", "p311": "male", "p312": "female",
    "p313": "female", "p314": "female", "p316": "male", "p317": "female",
    "p318": "female", "p323": "female", "p326": "male", "p329": "female",
    "p330": "female", "p333": "female", "p334": "male", "p335": "female",
    "p336": "female", "p339": "female", "p340": "female", "p341": "female",
    "p343": "female", "p345": "male", "p347": "female", "p351": "female",
    "p360": "male", "p361": "female", "p362": "female", "p363": "male",
    "p364": "male", "p374": "male", "p376": "female",
}

ARCTIC_GENDER = {
    "slt": "female", "clb": "female", "lnh": "female", "awb": "male",
    "bdl": "male", "rms": "male", "jmk": "male", "ksp": "male",
    "rxr": "male", "slt_arctic": "female",
}

# Extra Apache/MIT/CC demo clips. Prefer human recordings over live TTS.
EXTRA_CLIPS = [
    {
        "id": "zh_f_cosy_zero",
        "url": "https://cdn.jsdelivr.net/gh/FunAudioLLM/CosyVoice@main/asset/zero_shot_prompt.wav",
        "language": "zh", "gender": "female", "age": "adult", "pitch": "mid-high",
        "tags": ["成年", "清亮", "人声"], "source": "cosyvoice_apache",
    },
    {
        "id": "zh_m_cosy_cross",
        "url": "https://cdn.jsdelivr.net/gh/FunAudioLLM/CosyVoice@main/asset/cross_lingual_prompt.wav",
        "language": "zh", "gender": "male", "age": "adult", "pitch": "mid",
        "tags": ["成年", "男中音", "人声"], "source": "cosyvoice_apache",
    },
    {
        "id": "zh_m_f5_taiyi",
        "url": "https://cdn.jsdelivr.net/gh/SWivid/F5-TTS@main/src/f5_tts/infer/examples/basic/basic_ref_zh.wav",
        "language": "zh", "gender": "male", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声", "戏剧"], "source": "f5tts_mit",
        "transcript": "对，这就是我，万人敬仰的太乙真人。",
    },
    {
        "id": "zh_f_funasr_ex",
        "url": "https://isv-data.oss-cn-hangzhou.aliyuncs.com/ics/MaaS/ASR/test_audio/asr_example_zh.wav",
        "language": "zh", "gender": "female", "age": "adult", "pitch": "mid-high",
        "tags": ["成年", "人声", "播报"], "source": "funasr_mit",
    },
    {
        "id": "zh_funasr_hotword",
        "url": "https://isv-data.oss-cn-hangzhou.aliyuncs.com/ics/MaaS/ASR/test_audio/asr_example_hotword.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "funasr_mit",
    },
    {
        "id": "zh_funasr_vad",
        "url": "https://isv-data.oss-cn-hangzhou.aliyuncs.com/ics/MaaS/ASR/test_audio/vad_example.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声", "对话"], "source": "funasr_mit",
    },
    {
        "id": "zh_f_paddle",
        "url": "https://paddlespeech.bj.bcebos.com/PaddleAudio/zh.wav",
        "language": "zh", "gender": "female", "age": "adult", "pitch": "mid-high",
        "tags": ["成年", "人声"], "source": "paddlespeech_apache",
    },
    {
        "id": "zh_paraformer_punc",
        "url": (
            "https://www.modelscope.cn/models/iic/"
            "speech_paraformer-large-vad-punc_asr_nat-zh-cn-16k-common-vocab8404-pytorch/"
            "resolve/master/example/asr_example.wav"
        ),
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "funasr_mit",
    },
    {
        "id": "zh_sensevoice",
        "url": "https://www.modelscope.cn/models/iic/SenseVoiceSmall/resolve/master/example/zh.mp3",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "sensevoice_apache",
    },
    {
        "id": "zh_yue_sensevoice",
        "url": "https://www.modelscope.cn/models/iic/SenseVoiceSmall/resolve/master/example/yue.mp3",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["粤语", "人声"], "source": "sensevoice_apache",
    },
    {
        "id": "zh_camp_spk1",
        "url": (
            "https://www.modelscope.cn/models/iic/speech_campplus_sv_zh-cn_16k-common/"
            "resolve/master/examples/speaker1_a_cn_16k.wav"
        ),
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声", "说话人1"], "source": "campplus_apache",
    },
    {
        "id": "zh_camp_spk2",
        "url": (
            "https://www.modelscope.cn/models/iic/speech_campplus_sv_zh-cn_16k-common/"
            "resolve/master/examples/speaker2_a_cn_16k.wav"
        ),
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声", "说话人2"], "source": "campplus_apache",
    },
    {
        "id": "zh_aishell_s0724",
        "url": "https://cdn.jsdelivr.net/gh/wenet-e2e/wenet@main/test/resources/aishell-BAC009S0724W0121.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声", "aishell"], "source": "aishell_apache",
    },
    {
        "id": "zh_f_emotivoice",
        "url": "https://cdn.jsdelivr.net/gh/netease-youdao/EmotiVoice@main/assets/audio/emotivoice_intro_cn.wav",
        "language": "zh", "gender": "female", "age": "adult", "pitch": "mid-high",
        "tags": ["成年", "人声", "旁白"], "source": "emotivoice_apache",
    },
    {
        "id": "zh_sherpa_1",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-2023-09-14/resolve/main/test_wavs/1.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "sherpa_apache",
    },
    {
        "id": "zh_sherpa_2",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-2023-09-14/resolve/main/test_wavs/2.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "sherpa_apache",
    },
    {
        "id": "zh_m_sichuan",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-2023-09-14/resolve/main/test_wavs/3-sichuan.wav",
        "language": "zh", "gender": "male", "age": "adult", "pitch": "mid-low",
        "tags": ["四川话", "人声", "方言"], "source": "sherpa_apache",
    },
    {
        "id": "zh_tianjin",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-2023-09-14/resolve/main/test_wavs/4-tianjin.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["天津话", "人声", "方言"], "source": "sherpa_apache",
    },
    {
        "id": "zh_henan",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-2023-09-14/resolve/main/test_wavs/5-henan.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["河南话", "人声", "方言"], "source": "sherpa_apache",
    },
    {
        "id": "zh_sherpa_mix",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-2023-09-14/resolve/main/test_wavs/6-zh-en.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["中英混合", "人声"], "source": "sherpa_apache",
    },
    {
        "id": "zh_sherpa_zip0",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/main/test_wavs/0.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "sherpa_apache",
    },
    {
        "id": "zh_sherpa_zip1",
        "url": "https://hf-mirror.com/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/main/test_wavs/1.wav",
        "language": "zh", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["成年", "人声"], "source": "sherpa_apache",
    },
    {
        "id": "en_f_openvoice_0",
        "url": "https://cdn.jsdelivr.net/gh/myshell-ai/OpenVoice@main/resources/demo_speaker0.mp3",
        "language": "en", "gender": "female", "age": "young", "pitch": "mid-high",
        "tags": ["young", "bright"], "source": "openvoice_mit",
    },
    {
        "id": "en_m_openvoice_1",
        "url": "https://cdn.jsdelivr.net/gh/myshell-ai/OpenVoice@main/resources/demo_speaker1.mp3",
        "language": "en", "gender": "male", "age": "adult", "pitch": "mid",
        "tags": ["adult", "clear"], "source": "openvoice_mit",
    },
    {
        "id": "en_f_openvoice_2",
        "url": "https://cdn.jsdelivr.net/gh/myshell-ai/OpenVoice@main/resources/demo_speaker2.mp3",
        "language": "en", "gender": "female", "age": "adult", "pitch": "mid",
        "tags": ["adult", "warm"], "source": "openvoice_mit",
    },
    {
        "id": "en_f_openvoice_ref",
        "url": "https://cdn.jsdelivr.net/gh/myshell-ai/OpenVoice@main/resources/example_reference.mp3",
        "language": "en", "gender": "female", "age": "young", "pitch": "high",
        "tags": ["young", "bright"], "source": "openvoice_mit",
    },
    {
        "id": "en_f_f5_nature",
        "url": "https://cdn.jsdelivr.net/gh/SWivid/F5-TTS@main/src/f5_tts/infer/examples/basic/basic_ref_en.wav",
        "language": "en", "gender": "female", "age": "adult", "pitch": "mid",
        "tags": ["adult", "narration"], "source": "f5tts_mit",
        "transcript": "Some call me nature, others call me mother nature.",
    },
    {
        "id": "en_f5_country",
        "url": "https://cdn.jsdelivr.net/gh/SWivid/F5-TTS@main/src/f5_tts/infer/examples/multi/country.flac",
        "language": "en", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["story", "country"], "source": "f5tts_mit",
    },
    {
        "id": "en_f5_main",
        "url": "https://cdn.jsdelivr.net/gh/SWivid/F5-TTS@main/src/f5_tts/infer/examples/multi/main.flac",
        "language": "en", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["story", "main"], "source": "f5tts_mit",
    },
    {
        "id": "en_f5_town",
        "url": "https://cdn.jsdelivr.net/gh/SWivid/F5-TTS@main/src/f5_tts/infer/examples/multi/town.flac",
        "language": "en", "gender": "unknown", "age": "adult", "pitch": "mid",
        "tags": ["story", "town"], "source": "f5tts_mit",
    },
]

FLEURS_TSV_URL = "https://hf-mirror.com/datasets/google/fleurs/resolve/main/data/cmn_hans_cn/dev.tsv"
FLEURS_TAR_URL = "https://hf-mirror.com/datasets/google/fleurs/resolve/main/data/cmn_hans_cn/audio/dev.tar.gz"
FLEURS_PER_GENDER = 14


def which(name: str) -> str | None:
    return shutil.which(name)


def find_ffmpeg() -> str:
    for cand in (
        which("ffmpeg"),
        os.path.expandvars(r"%LOCALAPPDATA%\ffmpeg\ffmpeg.exe"),
        os.path.expandvars(r"%LOCALAPPDATA%\Flowy\Nomi\bin\ffmpeg.exe"),
    ):
        if cand and Path(cand).is_file():
            return cand
    raise SystemExit("ffmpeg not found")


def http_get(url: str, dest: Path | None = None, timeout: int = 90) -> bytes | None:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, context=SSL_CTX, timeout=timeout) as resp:
            if dest is None:
                return resp.read()
            dest.parent.mkdir(parents=True, exist_ok=True)
            with dest.open("wb") as out:
                while True:
                    chunk = resp.read(256 * 1024)
                    if not chunk:
                        break
                    out.write(chunk)
            return dest.read_bytes() if dest.stat().st_size < 4_000_000 else b""
    except (urllib.error.URLError, TimeoutError, ssl.SSLError, OSError) as exc:
        print(f"  GET fail {url}: {exc}")
        return None


def ffmpeg_to_ref(ffmpeg: str, src: Path, dest: Path) -> bool:
    dest.parent.mkdir(parents=True, exist_ok=True)
    af = (
        "silenceremove=start_periods=1:start_silence=0.08:start_threshold=-38dB:detection=peak,"
        "areverse,"
        "silenceremove=start_periods=1:start_silence=0.08:start_threshold=-38dB:detection=peak,"
        "areverse,"
        "loudnorm=I=-16:LRA=11:TP=-1.5,"
        "apad=pad_dur=2.3,atrim=0:2.3"
    )
    cmd = [
        ffmpeg, "-y", "-i", str(src),
        "-ac", "1", "-ar", "24000", "-af", af,
        "-c:a", "pcm_s16le", str(dest),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0 or not dest.is_file() or dest.stat().st_size < 4096:
        print(f"  ffmpeg fail {src.name}: {(proc.stderr or '')[-300:]}")
        if dest.exists():
            dest.unlink()
        return False
    return True


def guess_age(name: str) -> str:
    n = name.lower()
    if any(k in n for k in ("child", "kid", "boy", "girl")):
        return "child"
    if any(k in n for k in ("young", "teen")):
        return "young"
    if any(k in n for k in ("elder", "old", "senior")):
        return "elder"
    return "adult"


def guess_gender_from_name(name: str) -> str:
    n = name.lower()
    female = ("female", "woman", "girl", "amy", "jenny", "kathleen", "kristin", "cori", "alba", "huihui", "huayan", "xiao_ya", "slt", "clb")
    male = ("male", "man", "boy", "alan", "joe", "john", "danny", "bryce", "kusal", "david", "awb", "bdl", "rms", "chaowen")
    if any(k in n for k in female):
        return "female"
    if any(k in n for k in male):
        return "male"
    return "unknown"


def add_voice(voices: list[dict], entry: dict, ffmpeg: str, src: Path | None) -> None:
    dest = WAV_DIR / f"{entry['id']}.wav"
    cached = dest.exists() and dest.stat().st_size > 4096
    if not cached:
        if src is None or not ffmpeg_to_ref(ffmpeg, src, dest):
            return
    gender = entry["gender"]
    default_pitch = "mid-high" if gender == "female" else "mid"
    voices.append({
        "id": entry["id"],
        "file": f"wav/{entry['id']}.wav",
        "language": entry["language"],
        "gender": gender,
        "age": entry.get("age") or "adult",
        "pitch": entry.get("pitch") or default_pitch,
        "tags": entry.get("tags") or [],
        "source": entry.get("source") or "unknown",
        "transcript": entry.get("transcript") or "",
    })
    mark = "=" if cached else "+"
    print(f"  {mark} {entry['id']} ({entry['language']} {gender} {entry.get('age')})")


def ingest_piper(ffmpeg: str, voices: list[dict]) -> None:
    print("downloading Piper voices.json")
    raw = http_get(f"{HF}/voices.json")
    if not raw:
        print("  skip piper: voices.json missing")
        return
    catalog = json.loads(raw)
    # Unique Chinese (drop huayan-x_low duplicate) + English singles + capped multi-speaker.
    wanted: list[tuple[str, dict, int]] = []
    vctk_cap = 28
    arctic_cap = 18
    l2_cap = 12
    aru_cap = 8
    for key, meta in catalog.items():
        if key.startswith("zh_CN-") and "x_low" not in key:
            wanted.append((key, meta, 1))
        elif key.startswith("en_") and (meta.get("num_speakers") or 1) == 1:
            if any(q in key for q in ("-medium", "-high")):
                wanted.append((key, meta, 1))
        elif key == "en_GB-vctk-medium":
            wanted.append((key, meta, vctk_cap))
        elif key == "en_US-arctic-medium":
            wanted.append((key, meta, arctic_cap))
        elif key == "en_US-l2arctic-medium":
            wanted.append((key, meta, l2_cap))
        elif key == "en_GB-aru-medium":
            wanted.append((key, meta, aru_cap))

    for key, meta, cap in wanted:
        lang_code = meta.get("language", {}).get("code") or key.split("-")[0]
        family = "zh" if lang_code.startswith("zh") else "en"
        name = meta.get("name") or key
        quality = meta.get("quality") or "medium"
        region = lang_code  # zh_CN / en_US
        id_map = meta.get("speaker_id_map") or {}
        # invert: numeric id -> speaker label
        by_id = {int(v): str(k) for k, v in id_map.items()} if id_map else {}
        n_speakers = min(int(meta.get("num_speakers") or 1), cap)
        for spk in range(n_speakers):
            url = f"{HF}/{family}/{region}/{name}/{quality}/samples/speaker_{spk}.mp3"
            raw_path = TMP / f"piper_{key}_{spk}.mp3"
            print(f"Piper {key} speaker {spk}")
            label = by_id.get(spk, f"spk{spk}")
            if "vctk" in key:
                gender = VCTK_GENDER.get(label, "unknown")
            elif "arctic" in key:
                gender = ARCTIC_GENDER.get(label.split("_")[0], guess_gender_from_name(label))
            else:
                gender = guess_gender_from_name(f"{name} {label}")
            if gender == "unknown":
                gender = "female" if spk % 2 == 0 else "male"
            vid = f"{family}_{gender[0]}_piper_{name}_{label}".replace("-", "_")
            cached = WAV_DIR / f"{vid}.wav"
            if cached.exists() and cached.stat().st_size > 4096:
                add_voice(
                    voices,
                    {
                        "id": vid,
                        "language": family,
                        "gender": gender,
                        "age": guess_age(label),
                        "tags": [name, label, "piper"],
                        "source": "piper_mit_sample",
                    },
                    ffmpeg,
                    None,
                )
                continue
            if not http_get(url, raw_path):
                continue
            add_voice(
                voices,
                {
                    "id": vid,
                    "language": family,
                    "gender": gender,
                    "age": guess_age(label),
                    "tags": [name, label, "piper"],
                    "source": "piper_mit_sample",
                },
                ffmpeg,
                raw_path,
            )


def ingest_extra(ffmpeg: str, voices: list[dict]) -> None:
    for clip in EXTRA_CLIPS:
        print("extra", clip["id"])
        dest = WAV_DIR / f"{clip['id']}.wav"
        if dest.exists() and dest.stat().st_size > 4096:
            add_voice(voices, clip, ffmpeg, None)
            continue
        ext = Path(clip["url"].split("?")[0]).suffix or ".wav"
        raw = TMP / f"{clip['id']}{ext}"
        if not http_get(clip["url"], raw, timeout=120):
            continue
        add_voice(voices, clip, ffmpeg, raw)


def _spaced(items: list, n: int) -> list:
    if n <= 0 or not items:
        return []
    if len(items) <= n:
        return items
    step = len(items) / n
    return [items[int(i * step)] for i in range(n)]


def ingest_fleurs_zh(ffmpeg: str, voices: list[dict]) -> None:
    """One evenly spaced clip per gender from FLEURS Mandarin dev (CC-BY-4.0)."""
    tsv_path = TMP / "fleurs_cmn_dev.tsv"
    tar_path = TMP / "fleurs_cmn_dev.tar.gz"
    print("FLEURS Chinese dev")
    if not (tsv_path.exists() and tsv_path.stat().st_size > 1000):
        if http_get(FLEURS_TSV_URL, tsv_path, timeout=120) is None:
            print("  skip fleurs: tsv missing")
            return
    if not (tar_path.exists() and tar_path.stat().st_size > 1_000_000):
        if http_get(FLEURS_TAR_URL, tar_path, timeout=600) is None:
            print("  skip fleurs: tar missing")
            return
    rows_f: list[tuple[str, str]] = []
    rows_m: list[tuple[str, str]] = []
    for line in tsv_path.read_text(encoding="utf-8").splitlines():
        cols = line.split("\t")
        if len(cols) < 7:
            continue
        fn, gender, transcript = cols[1], cols[6].strip().upper(), cols[2]
        if gender == "FEMALE":
            rows_f.append((fn, transcript))
        elif gender == "MALE":
            rows_m.append((fn, transcript))
    picked = [("female", fn, tx) for fn, tx in _spaced(rows_f, FLEURS_PER_GENDER)]
    picked += [("male", fn, tx) for fn, tx in _spaced(rows_m, FLEURS_PER_GENDER)]
    if not picked:
        print("  skip fleurs: no rows")
        return
    want = {fn for _, fn, _ in picked}
    member_by_base: dict[str, str] = {}
    try:
        with tarfile.open(tar_path, "r:gz") as tf:
            for member in tf.getmembers():
                if not member.isfile():
                    continue
                base = Path(member.name).name
                if base in want:
                    member_by_base[base] = member.name
            extract_root = TMP / "fleurs_extract"
            extract_root.mkdir(parents=True, exist_ok=True)
            for gender, fn, transcript in picked:
                vid = f"zh_{gender[0]}_fleurs_{Path(fn).stem[-10:]}"
                dest = WAV_DIR / f"{vid}.wav"
                entry = {
                    "id": vid,
                    "language": "zh",
                    "gender": gender,
                    "age": "adult",
                    "pitch": "mid-high" if gender == "female" else "mid",
                    "tags": ["成年", "人声", "fleurs"],
                    "source": "fleurs_cc_by_4",
                    "transcript": transcript,
                }
                if dest.exists() and dest.stat().st_size > 4096:
                    add_voice(voices, entry, ffmpeg, None)
                    continue
                member_name = member_by_base.get(fn)
                if not member_name:
                    print(f"  missing in tar {fn}")
                    continue
                tf.extract(member_name, path=extract_root)
                src = extract_root / member_name
                add_voice(voices, entry, ffmpeg, src)
    except tarfile.TarError as exc:
        print(f"  skip fleurs tar: {exc}")


def ingest_sapi(ffmpeg: str, voices: list[dict]) -> None:
    ps = which("powershell") or which("pwsh")
    if not ps:
        return
    script = r"""
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$s.GetInstalledVoices() | ForEach-Object {
  $i = $_.VoiceInfo
  Write-Output ($i.Name + '|' + $i.Culture + '|' + $i.Gender)
}
"""
    proc = subprocess.run(
        [ps, "-NoProfile", "-Command", script],
        capture_output=True, text=True,
    )
    if proc.returncode != 0:
        return
    lines = [ln.strip() for ln in proc.stdout.splitlines() if "|" in ln]
    print(f"SAPI voices: {len(lines)}")
    for i, line in enumerate(lines):
        name, culture, gender_raw = (line.split("|") + ["", ""])[:3]
        lang = "zh" if culture.lower().startswith("zh") else "en"
        gender = "female" if "female" in gender_raw.lower() else "male"
        wav_raw = TMP / f"sapi_{i}.wav"
        text = "今晚别等我。我们把话说开。" if lang == "zh" else "Don't wait up. Let's talk this through."
        speak = rf"""
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$s.SelectVoice('{name}')
$s.SetOutputToWaveFile('{wav_raw}')
$s.Speak('{text}')
$s.Dispose()
"""
        print(f"SAPI {name}")
        sp = subprocess.run([ps, "-NoProfile", "-Command", speak], capture_output=True, text=True)
        if sp.returncode != 0 or not wav_raw.is_file():
            print("  sapi fail", (sp.stderr or "")[-200:])
            continue
        vid = f"{lang}_{gender[0]}_sapi_{i}"
        add_voice(
            voices,
            {
                "id": vid,
                "language": lang,
                "gender": gender,
                "age": "adult",
                "tags": [name, "sapi"],
                "source": "windows_sapi",
                "transcript": text,
            },
            ffmpeg,
            wav_raw,
        )


def sha256_file(path: Path) -> str:
    import hashlib
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def write_docs(voices: list[dict]) -> None:
    (OUT / "LICENSE.txt").write_text(
        f"""Flowy short-drama voice bank {VERSION}

This pack is assembled from third-party samples for Seedance/Wan reference_audio:

- FLEURS Mandarin read speech — CC-BY-4.0, Google FLEURS
- AISHELL / WeNet / FunASR / CAM++ / SenseVoice / sherpa-onnx demo wavs (Apache-2.0 / MIT)
- F5-TTS reference clips — MIT
- CosyVoice demo prompts — Apache-2.0, FunAudioLLM/CosyVoice
- OpenVoice demo speakers — MIT, myshell-ai/OpenVoice
- EmotiVoice intro clip — Apache-2.0
- Piper TTS speaker samples — MIT, https://github.com/rhasspy/piper
- Optional Windows SAPI renders generated on the build machine

Replace any clip with a recording you own before shipping if you need a
single-license pack. Runtime clients only consume the wav + manifest.
""",
        encoding="utf-8",
    )
    (OUT / "README.txt").write_text(
        """短剧工坊音色包

1. 确认本目录有 manifest.json 与 wav/*.wav
2. 上传 staging/voice-bank-<version>.zip 与 version.json 到魔搭
   默认路径：datasets/NanoModel/ffmpeg/voices/
3. 客户端用 FLOWY_VOICE_BANK_VERSION_URL 覆盖地址（可选）
4. 升版本：改 version、换 zip，客户端会下载并替换 {data_dir}/voice-bank/current/
""",
        encoding="utf-8",
    )
    (OUT / "manifest.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "version": VERSION,
                "name": "flowy-short-drama-voice-bank",
                "clip_secs": 2.3,
                "sample_rate_hz": 24000,
                "channels": 1,
                "voices": voices,
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def make_zip() -> None:
    zip_path = ROOT / "staging" / f"voice-bank-{VERSION}.zip"
    if zip_path.exists():
        zip_path.unlink()
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for path in sorted(OUT.rglob("*")):
            if path.is_file() and path.parent.name != "_tmp":
                zf.write(path, path.relative_to(OUT).as_posix())
    digest = sha256_file(zip_path)
    version_doc = {
        "version": VERSION,
        "filename": zip_path.name,
        "sha256": digest,
        "size_bytes": zip_path.stat().st_size,
        "zip_url": (
            "https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/"
            f"voices/{zip_path.name}?view=false"
        ),
    }
    text = json.dumps(version_doc, ensure_ascii=False, indent=2) + "\n"
    (OUT / "version.json").write_text(text, encoding="utf-8")
    (ROOT / "staging" / "voice-bank-version.json").write_text(text, encoding="utf-8")
    print(f"zip {zip_path} sha256={digest} bytes={zip_path.stat().st_size}")


def main() -> int:
    ffmpeg = find_ffmpeg()
    print("ffmpeg", ffmpeg)
    WAV_DIR.mkdir(parents=True, exist_ok=True)
    TMP.mkdir(parents=True, exist_ok=True)
    voices: list[dict] = []
    ingest_extra(ffmpeg, voices)
    ingest_fleurs_zh(ffmpeg, voices)
    ingest_piper(ffmpeg, voices)
    ingest_sapi(ffmpeg, voices)
    shutil.rmtree(TMP, ignore_errors=True)
    if len(voices) < 8:
        raise SystemExit(f"too few voices built: {len(voices)}")
    # Dedup ids
    seen = set()
    uniq = []
    for v in voices:
        if v["id"] in seen:
            continue
        seen.add(v["id"])
        uniq.append(v)
    write_docs(uniq)
    make_zip()
    zh = sum(1 for v in uniq if v["language"] == "zh")
    en = sum(1 for v in uniq if v["language"] == "en")
    print(f"built {len(uniq)} voices (zh={zh} en={en}) at {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
