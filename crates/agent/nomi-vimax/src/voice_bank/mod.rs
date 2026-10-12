//! Curated short-drama speaker clips used as Seedance / Wan `reference_audio`.
//!
//! The zip lives on ModelScope. The client compares `version.json` with the
//! local `manifest.json`, downloads when behind, and atomically replaces
//! `{data_dir}/voice-bank/current/`. Runtime generation copies a clip into the
//! film portraits dir and skips Qwen3-TTS whenever a match exists.

mod assign;
mod install;
mod manifest;

pub use assign::{assign_unique_voices, CharacterVoiceNeed};
pub use install::{
    current_dir, ensure_installed, load_local_bank, VoiceBankInstallError,
};
pub use manifest::{
    parse_version, version_newer, VoiceAge, VoiceBankManifest, VoiceEntry, VoiceGender,
    DEFAULT_VOICE_BANK_VERSION_URL, VOICE_BANK_DIR_ENV, VOICE_BANK_VERSION_ENV, VOICE_BANK_ZIP_ENV,
};

use std::collections::HashMap;
use std::path::Path;

use crate::domain::CharacterInScene;
use crate::error::{VimaxError, VimaxResult};
use crate::planning::OutputLanguage;

const VOICE_REF_VIEW: &str = "voice_ref";
const VOICE_REF_CACHE_VER: &str = "_bank1";

/// Bind one distinctive bank clip per speaking cast member.
///
/// Syncs the ModelScope pack first (fail-soft). Characters that already have a
/// usable `voice_ref` in `registry` are left alone so a mid-film bank bump
/// cannot change locked speaker identity.
pub async fn bind_clips(
    characters: &[CharacterInScene],
    portraits_dir: &Path,
    registry: &mut HashMap<String, HashMap<String, HashMap<String, String>>>,
    film_language: OutputLanguage,
) -> VimaxResult<usize> {
    let needs_bind = characters.iter().any(|ch| {
        ch.needs_identity_assets()
            && ch.voice_profile.as_ref().is_some_and(|v| v.is_usable())
            && !crate::agents::has_usable_voice_ref(registry, &ch.identifier_in_scene)
    });
    if !needs_bind {
        return Ok(0);
    }
    if let Err(err) = ensure_installed().await {
        tracing::warn!(
            error = %err,
            "voice bank sync failed; existing local pack will still be used if present"
        );
    }
    let Some(bank) = load_local_bank() else {
        return Ok(0);
    };
    let assignments = assign_unique_voices(characters, &bank.manifest, film_language);
    if assignments.is_empty() {
        return Ok(0);
    }
    tokio::fs::create_dir_all(portraits_dir)
        .await
        .map_err(VimaxError::from)?;
    let mut bound = 0usize;
    for (ch, voice) in assignments {
        if crate::agents::has_usable_voice_ref(registry, &ch.identifier_in_scene) {
            continue;
        }
        let src = bank.root.join(&voice.file);
        if !crate::media_local::is_usable_audio_file(&src) {
            tracing::warn!(
                voice_id = %voice.id,
                path = %src.display(),
                "voice bank clip missing or unreadable; skipping"
            );
            continue;
        }
        let char_dir = portraits_dir.join(format!(
            "{}_{}",
            ch.idx,
            safe_component(&ch.identifier_in_scene)
        ));
        tokio::fs::create_dir_all(&char_dir)
            .await
            .map_err(VimaxError::from)?;
        let dest = char_dir.join(format!(
            "{}_voice_ref_bank_{}{}{}.wav",
            safe_component(&ch.identifier_in_scene),
            voice.id,
            language_tag(film_language),
            VOICE_REF_CACHE_VER
        ));
        if !crate::media_local::is_usable_audio_file(&dest) {
            tokio::fs::copy(&src, &dest)
                .await
                .map_err(VimaxError::from)?;
        }
        if !crate::media_local::is_usable_audio_file(&dest) {
            continue;
        }
        register_voice_ref(registry, ch, &dest, &voice.id);
        bound += 1;
    }
    Ok(bound)
}

fn language_tag(lang: OutputLanguage) -> &'static str {
    match lang {
        OutputLanguage::English => "_en",
        OutputLanguage::Chinese | OutputLanguage::Unspecified => "_zh",
    }
}

fn register_voice_ref(
    registry: &mut HashMap<String, HashMap<String, HashMap<String, String>>>,
    ch: &CharacterInScene,
    wav_path: &Path,
    voice_id: &str,
) {
    let mut item = HashMap::new();
    item.insert("path".into(), wav_path.to_string_lossy().into_owned());
    item.insert(
        "description".into(),
        format!(
            "Voice bank clip `{voice_id}` for <{}> — bind speaker timbre via reference_audio.",
            ch.identifier_in_scene
        ),
    );
    item.insert("voice_bank_id".into(), voice_id.to_string());
    registry
        .entry(ch.identifier_in_scene.clone())
        .or_default()
        .insert(VOICE_REF_VIEW.into(), item);
}

fn is_path_safe_ideograph(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0x3040..=0x30FF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u)
}

fn safe_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || is_path_safe_ideograph(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').chars().take(80).collect::<String>();
    if out.is_empty() {
        "asset".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::VoiceProfile;

    fn silent_wav(path: &Path) {
        // 44-byte header + 280 bytes of silence so `is_usable_audio_file` passes.
        let mut bytes = Vec::from(&b"RIFF"[..]);
        bytes.extend_from_slice(&316u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24000u32.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&280u32.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0u8, 280));
        std::fs::write(path, bytes).unwrap();
    }

    fn ch(idx: i32, name: &str, features: &str) -> CharacterInScene {
        CharacterInScene {
            idx,
            identifier_in_scene: name.into(),
            is_visible: true,
            static_features: features.into(),
            dynamic_features: None,
            voice_profile: Some(VoiceProfile {
                timbre: features.into(),
                volume: Some("normal".into()),
                pitch: Some("mid".into()),
                speaking_style: "平稳".into(),
                caption_clause: None,
                tts_voice: None,
            }),
        }
    }

    #[tokio::test]
    async fn bind_skips_when_ref_already_registered() {
        let tmp = tempfile::tempdir().unwrap();
        let wav = tmp.path().join("existing.wav");
        silent_wav(&wav);
        let chars = vec![ch(0, "李薇", "成年女性")];
        let mut registry = HashMap::new();
        register_voice_ref(&mut registry, &chars[0], &wav, "already");
        let n = bind_clips(
            &chars,
            tmp.path(),
            &mut registry,
            OutputLanguage::Chinese,
        )
        .await
        .unwrap();
        assert_eq!(n, 0);
        let path = registry["李薇"]["voice_ref"]["voice_bank_id"].as_str();
        assert_eq!(path, "already");
    }
}
