//! Synthesize per-character voice reference clips (TTS category=8) for Seedance `reference_audio`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::backends::FlowyVimaxServices;
use crate::domain::CharacterInScene;
use crate::error::{VimaxError, VimaxResult};
use crate::session::write_text_artifact;

const VOICE_REF_VIEW: &str = "voice_ref";
const DEFAULT_TTS_VOICE: &str = "Cherry";

pub struct VoiceReferenceGenerator {
    services: FlowyVimaxServices,
}

impl VoiceReferenceGenerator {
    pub fn new(services: FlowyVimaxServices) -> Self {
        Self { services }
    }

    /// Ensure every visible cast member with a voice bible has a local `*_voice_ref.wav`.
    pub async fn ensure_voice_references(
        &self,
        characters: &[CharacterInScene],
        portraits_dir: &Path,
        registry: &mut HashMap<String, HashMap<String, HashMap<String, String>>>,
    ) -> VimaxResult<usize> {
        self.services.require_token().await?;
        let model = self.resolve_tts_model().await?;
        let mut generated = 0usize;
        tokio::fs::create_dir_all(portraits_dir).await?;

        for ch in characters {
            if !ch.is_visible {
                continue;
            }
            let vp = ch.voice_profile.as_ref().filter(|v| v.is_usable());
            if vp.is_none() {
                continue;
            }
            let id_safe = safe_file_stem(&ch.identifier_in_scene);
            let char_dir = portraits_dir.join(format!(
                "{}_{}",
                ch.idx,
                safe_component(&ch.identifier_in_scene)
            ));
            let wav_path = char_dir.join(format!("{id_safe}_voice_ref.wav"));
            if crate::media_local::is_usable_audio_file(&wav_path) {
                register_voice_ref(registry, ch, &wav_path);
                continue;
            }
            tokio::fs::create_dir_all(&char_dir).await?;
            let sample_text = voice_reference_sample_line(ch);
            let language = infer_tts_language(&sample_text, ch);
            let (bytes, _mime) = self
                .services
                .api
                .speech_synthesis(
                    &self.services.session,
                    &model,
                    &sample_text,
                    Some(DEFAULT_TTS_VOICE),
                    "wav",
                    language,
                )
                .await
                .map_err(|e| VimaxError::msg(format!("TTS for {}: {e}", ch.identifier_in_scene)))?;
            if bytes.len() < 256 {
                tracing::warn!(
                    character = %ch.identifier_in_scene,
                    bytes = bytes.len(),
                    "TTS voice reference too small; skipping"
                );
                continue;
            }
            crate::media_local::write_audio_bytes_atomic(&wav_path, &bytes).await?;
            let _ = write_text_artifact(
                &char_dir.join(format!("{id_safe}_voice_ref_sample.txt")),
                &sample_text,
            )
            .await;
            register_voice_ref(registry, ch, &wav_path);
            generated += 1;
        }
        Ok(generated)
    }

    async fn resolve_tts_model(&self) -> VimaxResult<String> {
        let catalog = self
            .services
            .api
            .get_available_models_claw(
                &self.services.session,
                Some(nomifun_cloud::MODEL_CATEGORY_TTS),
            )
            .await
            .map_err(|e| VimaxError::msg(e.to_string()))?;
        catalog
            .cloud
            .first()
            .map(|m| m.id.clone())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| VimaxError::msg("no Flowy TTS model (category=8) in catalog"))
    }
}

pub fn has_usable_voice_ref(
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    identifier: &str,
) -> bool {
    voice_ref_abs_path(registry, identifier, Path::new("."))
        .map(|p| crate::media_local::is_usable_audio_file(&p))
        .unwrap_or(false)
}

pub fn voice_ref_abs_path(
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    identifier: &str,
    film_root: &Path,
) -> Option<PathBuf> {
    registry
        .get(identifier)
        .and_then(|views| views.get(VOICE_REF_VIEW))
        .and_then(|item| item.get("path"))
        .map(|rel| resolve_voice_ref_path(rel, film_root))
        .filter(|p| crate::media_local::is_usable_audio_file(p))
}

fn register_voice_ref(
    registry: &mut HashMap<String, HashMap<String, HashMap<String, String>>>,
    ch: &CharacterInScene,
    wav_path: &Path,
) {
    let rel = wav_path.to_string_lossy().to_string();
    let desc = format!(
        "Voice reference clip for <{}> — bind speaker timbre via reference_audio on shot video.",
        ch.identifier_in_scene
    );
    let mut item = HashMap::new();
    item.insert("path".into(), rel);
    item.insert("description".into(), desc);
    registry
        .entry(ch.identifier_in_scene.clone())
        .or_default()
        .insert(VOICE_REF_VIEW.into(), item);
}

fn resolve_voice_ref_path(stored: &str, film_root: &Path) -> PathBuf {
    let trimmed = stored.trim();
    let p = PathBuf::from(trimmed);
    if p.is_absolute() {
        return p;
    }
    let from_film = film_root.join(trimmed);
    if from_film.is_file() {
        return from_film;
    }
    p
}

fn voice_reference_sample_line(ch: &CharacterInScene) -> String {
    let name = ch.identifier_in_scene.trim();
    let style = ch
        .voice_profile
        .as_ref()
        .map(|v| v.speaking_style.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    if text_looks_cjk(name) || style.chars().any(is_cjk_char) {
        if style.is_empty() {
            format!("你好，我是{name}。这是我的声音参考。")
        } else {
            format!("你好，我是{name}。{style}")
        }
    } else if style.is_empty() {
        format!("Hello, I'm {name}. This is my voice reference.")
    } else {
        format!("Hello, I'm {name}. {style}")
    }
}

fn infer_tts_language(sample: &str, ch: &CharacterInScene) -> &'static str {
    if text_looks_cjk(sample)
        || ch.static_features.chars().any(is_cjk_char)
        || ch
            .dynamic_features
            .as_deref()
            .is_some_and(|s| s.chars().any(is_cjk_char))
    {
        "Chinese"
    } else {
        "English"
    }
}

fn text_looks_cjk(s: &str) -> bool {
    s.chars().any(is_cjk_char)
}

fn is_cjk_char(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

fn safe_file_stem(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = raw.trim_matches('_');
    if trimmed.is_empty() {
        "asset".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn safe_component(s: &str) -> String {
    safe_file_stem(s)
}
