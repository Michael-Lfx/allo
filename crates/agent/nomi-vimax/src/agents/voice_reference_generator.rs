//! Synthesize per-character voice reference clips (TTS category=8) for Seedance `reference_audio`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::backends::FlowyVimaxServices;
use crate::domain::{CharacterInScene, VoiceProfile};
use crate::error::{VimaxError, VimaxResult};
use crate::session::write_text_artifact;
use super::voice_profile_generator::canonical_tts_voice;

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
            let Some(vp) = ch.voice_profile.as_ref().filter(|v| v.is_usable()) else {
                continue;
            };
            let id_safe = safe_file_stem(&ch.identifier_in_scene);
            let char_dir = portraits_dir.join(format!(
                "{}_{}",
                ch.idx,
                safe_component(&ch.identifier_in_scene)
            ));
            let tts_voice = resolve_tts_voice(vp);
            let instruct = voice_design_instruct(ch);
            let cache_tag = instruct_cache_tag(&instruct);
            // Cache key includes TTS id + instruct hash so a new bible does not
            // reuse a Cherry wav baked under the old unversioned name.
            let wav_path = char_dir.join(format!("{id_safe}_voice_ref_{tts_voice}{cache_tag}.wav"));
            if crate::media_local::is_usable_audio_file(&wav_path) {
                register_voice_ref(registry, ch, &wav_path);
                continue;
            }
            tokio::fs::create_dir_all(&char_dir).await?;
            let sample_text = voice_reference_sample_line(ch);
            let language = infer_tts_language(&sample_text, ch);
            let (bytes, _mime) = match self
                .services
                .api
                .speech_synthesis(
                    &self.services.session,
                    &model,
                    &sample_text,
                    Some(&tts_voice),
                    "wav",
                    language,
                    instruct.as_deref(),
                )
                .await
            {
                Ok(ok) => ok,
                Err(e) if instruct.is_some() && tts_rejected_instructions(&e) => {
                    tracing::info!(
                        character = %ch.identifier_in_scene,
                        error = %e,
                        "TTS ignored voice-design instructions; retrying with preset voice only"
                    );
                    self.services
                        .api
                        .speech_synthesis(
                            &self.services.session,
                            &model,
                            &sample_text,
                            Some(&tts_voice),
                            "wav",
                            language,
                            None,
                        )
                        .await
                        .map_err(|e2| {
                            VimaxError::msg(format!("TTS for {}: {e2}", ch.identifier_in_scene))
                        })?
                }
                Err(e) => {
                    return Err(VimaxError::msg(format!(
                        "TTS for {}: {e}",
                        ch.identifier_in_scene
                    )));
                }
            };
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
                &char_dir.join(format!("{id_safe}_voice_ref_{tts_voice}{cache_tag}_sample.txt")),
                &sample_text,
            )
            .await;
            if let Some(instruct) = instruct.as_deref() {
                let _ = write_text_artifact(
                    &char_dir.join(format!(
                        "{id_safe}_voice_ref_{tts_voice}{cache_tag}_instruct.txt"
                    )),
                    instruct,
                )
                .await;
            }
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
        pick_tts_model(&catalog.cloud)
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

fn resolve_tts_voice(vp: &VoiceProfile) -> String {
    vp.tts_voice
        .as_deref()
        .and_then(canonical_tts_voice)
        .unwrap_or(DEFAULT_TTS_VOICE)
        .to_string()
}

fn pick_tts_model(catalog: &[nomifun_cloud::ClawModelEntry]) -> Option<String> {
    let usable: Vec<&nomifun_cloud::ClawModelEntry> = catalog
        .iter()
        .filter(|m| !m.id.trim().is_empty())
        .collect();
    if usable.is_empty() {
        return None;
    }
    let score = |m: &nomifun_cloud::ClawModelEntry| -> u8 {
        let blob = format!("{} {}", m.id, m.name).to_ascii_lowercase();
        if blob.contains("voice-design") || blob.contains("voicedesign") || blob.contains("-vd") {
            3
        } else if blob.contains("instruct") {
            2
        } else {
            1
        }
    };
    usable
        .into_iter()
        .max_by_key(|m| score(m))
        .map(|m| m.id.clone())
}

/// Natural-language voice design for Qwen3-TTS-Instruct / VD.
///
/// Seedance clones this wav; the wav itself must sound like the character, not
/// a generic Cherry reading director notes.
fn voice_design_instruct(ch: &CharacterInScene) -> Option<String> {
    let vp = ch.voice_profile.as_ref().filter(|v| v.is_usable())?;
    let mut bits = Vec::new();
    let timbre = vp.timbre.trim();
    if !timbre.is_empty() {
        bits.push(timbre.to_string());
    }
    if let Some(pitch) = vp
        .pitch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        bits.push(format!("pitch {pitch}"));
    }
    let feat = crate::planning::clip_at_break(ch.static_features.trim(), 36);
    if !feat.is_empty() {
        bits.push(feat);
    }
    let style = vp.speaking_style.trim();
    if !style.is_empty()
        && !style.contains("跨镜头")
        && !style.contains("音色锁定")
        && !style.contains("never change")
    {
        bits.push(crate::planning::clip_at_break(style, 28));
    }
    let instruct = bits.join("。");
    let instruct = instruct.trim();
    (!instruct.is_empty()).then(|| instruct.to_string())
}

fn instruct_cache_tag(instruct: &Option<String>) -> String {
    let Some(text) = instruct.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("_{:08x}", hasher.finish() as u32)
}

fn tts_rejected_instructions(err: &nomifun_cloud::ServerClientError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("instructions")
        || s.contains("optimize_instructions")
        || ((s.contains("invalidparameter") || s.contains("unexpected") || s.contains("unknown"))
            && s.contains("instruct"))
}

fn voice_reference_sample_line(ch: &CharacterInScene) -> String {
    // Phonetic coverage only. Never paste speaking_style / director notes —
    // Qwen3-TTS will read those instructions aloud and every character will
    // recite the same bible ("跨镜头音色锁定").
    let child = crate::planning::looks_like_child_character(
        &ch.identifier_in_scene,
        &ch.static_features,
    );
    let cjk = text_looks_cjk(&ch.identifier_in_scene)
        || ch.static_features.chars().any(is_cjk_char)
        || ch
            .dynamic_features
            .as_deref()
            .is_some_and(|s| s.chars().any(is_cjk_char));
    if cjk {
        if child {
            "妈妈，今天风有点大，我们先回家吧。".into()
        } else {
            "今晚别等我。这件事我已经想清楚了，我们把话说开。".into()
        }
    } else if child {
        "Mom, the wind is strong. Let's go home first.".into()
    } else {
        "Don't wait up tonight. I already made up my mind — let's talk this through.".into()
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

fn is_path_safe_ideograph(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0x3040..=0x30FF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u)
}

fn safe_file_stem(s: &str) -> String {
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

fn safe_component(s: &str) -> String {
    safe_file_stem(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::VoiceProfile;

    fn ch(name: &str, features: &str, style: &str) -> CharacterInScene {
        CharacterInScene {
            idx: 0,
            identifier_in_scene: name.into(),
            is_visible: true,
            static_features: features.into(),
            dynamic_features: None,
            voice_profile: Some(VoiceProfile {
                timbre: "清亮".into(),
                volume: Some("normal".into()),
                pitch: Some("mid".into()),
                speaking_style: style.into(),
                caption_clause: None,
                tts_voice: Some("Serena".into()),
            }),
        }
    }

    #[test]
    fn sample_line_is_speech_not_director_notes() {
        let line = voice_reference_sample_line(&ch(
            "李薇",
            "成年女性",
            "语速平稳、跨镜头音色音量锁定",
        ));
        assert!(!line.contains("跨镜头"));
        assert!(!line.contains("语速平稳"));
        assert!(!line.contains("李薇"));
        assert!(line.contains("想清楚") || line.contains("话说"));
    }

    #[test]
    fn resolve_tts_voice_uses_profile_id() {
        let ch = ch("李薇", "成年女性", "平稳");
        let vp = ch.voice_profile.as_ref().unwrap();
        assert_eq!(resolve_tts_voice(vp), "Serena");
    }

    #[test]
    fn voice_design_instruct_uses_timbre_not_director_notes() {
        let line = voice_design_instruct(&ch(
            "赵无极",
            "中青年男性，温润伪善",
            "语速平稳、跨镜头音色音量锁定",
        ))
        .expect("instruct");
        assert!(line.contains("清亮"));
        assert!(!line.contains("跨镜头"));
        assert!(line.contains("中青年男性") || line.contains("温润"));
    }

    #[test]
    fn pick_tts_model_prefers_instruct() {
        let catalog = vec![
            nomifun_cloud::ClawModelEntry {
                id: "AIPC-qwen3-tts".into(),
                name: "qwen3-tts".into(),
                ..Default::default()
            },
            nomifun_cloud::ClawModelEntry {
                id: "AIPC-qwen3-tts-instruct".into(),
                name: "qwen3-tts-instruct-flash".into(),
                ..Default::default()
            },
        ];
        assert_eq!(
            pick_tts_model(&catalog).as_deref(),
            Some("AIPC-qwen3-tts-instruct")
        );
    }
}
