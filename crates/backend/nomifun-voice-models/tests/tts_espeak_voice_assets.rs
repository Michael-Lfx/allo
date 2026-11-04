//! Regression tests for the English Matcha espeak voice assets.
//!
//! Matcha English is phonemized by the vendored piper frontend through
//! `espeak_SetVoiceByName("en-us")`. When `espeak-ng-data` only contains the
//! five root data files and lacks `lang/gmw/en` + `lang/gmw/en-US`, that call
//! throws a C++ exception which the sherpa C API does not catch and the voice
//! companion daemon aborts. These tests pin the voice definitions into the
//! required/download/checksum lists so installs and repairs always fetch them.

use nomifun_voice_models::{layout, verifier};

const ESPEAK_VOICE_ASSETS: [(&str, &str); 2] = [
    (
        "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en",
        "4605d5330801de3641c6e366d15f129ea1f5ffbce8722642aba01ace07ab9c83",
    ),
    (
        "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en-US",
        "41534c2a22df5dd4f1052ff9e1a33a3ea7bff5a26b5c02bdad5ba8ddb7524704",
    ),
];

#[test]
fn installed_tts_layout_requires_espeak_voice_definitions() {
    for (path, _hash) in ESPEAK_VOICE_ASSETS {
        assert!(
            layout::EXPECTED_TTS_FILES.contains(&path),
            "EXPECTED_TTS_FILES must require {path}"
        );
        assert!(
            verifier::DOWNLOAD_TTS_FILES.contains(&path),
            "DOWNLOAD_TTS_FILES must download {path}"
        );
    }
}

#[test]
fn tts_checksum_manifest_pins_espeak_voice_definitions() {
    let manifest = verifier::default_tts_manifest();
    for (path, hash) in ESPEAK_VOICE_ASSETS {
        assert_eq!(
            manifest.get(path).map(String::as_str),
            Some(hash),
            "{path} must be pinned in the TTS checksum manifest"
        );
    }
    for path in verifier::DOWNLOAD_TTS_FILES {
        assert!(
            manifest.contains_key(*path),
            "every downloaded TTS file needs a pinned hash: {path}"
        );
    }
}

#[test]
fn tts_download_list_and_layout_stay_in_sync() {
    for path in layout::EXPECTED_TTS_FILES {
        assert!(
            verifier::DOWNLOAD_TTS_FILES.contains(path),
            "layout expects {path} but the downloader never fetches it"
        );
    }
    for path in verifier::DOWNLOAD_TTS_FILES {
        assert!(
            layout::EXPECTED_TTS_FILES.contains(path),
            "downloader fetches {path} but the layout does not verify it"
        );
    }
}

#[test]
fn missing_espeak_voice_definitions_mark_tts_not_ready() {
    let temp = tempfile::tempdir().expect("create temp dir");
    for rel in layout::EXPECTED_TTS_FILES {
        let full = temp.path().join(rel);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("create dirs");
        std::fs::write(&full, vec![0u8; 16]).expect("write stub");
    }

    let (_ready, _details, _total) = layout::inspect_tts_files(temp.path());
    assert!(_ready, "fixture must start from a complete layout");

    for (path, _hash) in ESPEAK_VOICE_ASSETS {
        std::fs::remove_file(temp.path().join(path)).expect("remove voice definition");
    }

    let (ready_after, details_after, _total_after) = layout::inspect_tts_files(temp.path());
    assert!(!ready_after, "a suite without the espeak voice definitions is not ready");
    for (path, _hash) in ESPEAK_VOICE_ASSETS {
        let detail = details_after
            .iter()
            .find(|detail| detail.path == path)
            .expect("voice definition detail");
        assert!(!detail.exists, "{path} must be reported missing");
    }
}
