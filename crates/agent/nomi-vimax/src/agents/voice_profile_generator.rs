//! Fill missing per-character voice bibles so Seedance keeps timbre consistent across shots.

use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::{CharacterInScene, VoiceProfile};
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

use super::formats::VOICE_PROFILES;

pub struct VoiceProfileGenerator {
    chat: Arc<dyn VimaxChat>,
}

impl VoiceProfileGenerator {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    /// Ensure every character has a usable [`VoiceProfile`]. Returns `true` if any were filled/changed.
    pub async fn ensure_voice_profiles(
        &self,
        characters: &mut [CharacterInScene],
        script: &str,
        style: &str,
    ) -> VimaxResult<bool> {
        let missing: Vec<usize> = characters
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.voice_profile.as_ref().is_some_and(|v| v.is_usable()))
            .map(|(i, _)| i)
            .collect();
        if missing.is_empty() {
            return Ok(false);
        }

        let style = crate::planning::resolve_visual_style(style);
        let characters_str = missing
            .iter()
            .map(|&i| {
                let c = &characters[i];
                format!(
                    "- idx={} identifier_in_scene={} static_features={} dynamic_features={}",
                    c.idx,
                    c.identifier_in_scene,
                    c.static_features.trim(),
                    c.dynamic_features.as_deref().unwrap_or("").trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let system = include_str!(
            "../../prompts/voice_profile_generator__system_prompt_template.txt"
        )
        .replace("{format_instructions}", VOICE_PROFILES);
        let user = include_str!(
            "../../prompts/voice_profile_generator__human_prompt_template.txt"
        )
        .replace("{script}", script)
        .replace("{style}", &style)
        .replace("{characters_str}", &characters_str);

        match self.chat.complete_text(&system, &user).await {
            Ok(raw) => {
                #[derive(Deserialize)]
                struct VoiceItem {
                    idx: i32,
                    voice_profile: VoiceProfile,
                }
                #[derive(Deserialize)]
                struct Resp {
                    voices: Vec<VoiceItem>,
                }
                match parse_llm_json::<Resp>(&raw) {
                    Ok(resp) => {
                        for item in resp.voices {
                            if !item.voice_profile.is_usable() {
                                continue;
                            }
                            if let Some(ch) = characters.iter_mut().find(|c| c.idx == item.idx) {
                                let mut vp = item.voice_profile;
                                finalize_caption_clause(&mut vp, &ch.identifier_in_scene);
                                ch.voice_profile = Some(vp);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "voice_profile LLM JSON parse failed; applying heuristic fallbacks"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "voice_profile LLM call failed; applying heuristic fallbacks"
                );
            }
        }

        let mut changed = false;
        for ch in characters.iter_mut() {
            if ch.voice_profile.as_ref().is_some_and(|v| v.is_usable()) {
                // Still count as changed if we just filled it above (missing was non-empty).
                changed = true;
                continue;
            }
            ch.voice_profile = Some(heuristic_voice_profile(ch));
            changed = true;
        }
        Ok(changed)
    }
}

fn finalize_caption_clause(vp: &mut VoiceProfile, name: &str) {
    if vp
        .caption_clause
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return;
    }
    vp.caption_clause = Some(vp.seedance_clause(name));
}

pub(crate) fn heuristic_voice_profile(ch: &CharacterInScene) -> VoiceProfile {
    let name = ch.identifier_in_scene.trim();
    let static_f = ch.static_features.trim();
    let lower = format!("{name} {static_f}").to_ascii_lowercase();
    let (timbre, pitch, volume) = if looks_childish(&lower) {
        (
            "清亮偏细的少年/少女声，稚气但吐字清楚",
            "high",
            "normal",
        )
    } else if looks_feminine(&lower) {
        ("清亮柔和的女中音，气息稳定", "mid-high", "normal")
    } else if looks_elder(&lower) {
        ("沉稳略沙的中老年嗓音，语气从容", "low", "normal")
    } else {
        ("沉稳清晰的男中音，共鸣适中", "mid", "normal")
    };
    let mut vp = VoiceProfile {
        timbre: timbre.into(),
        volume: Some(volume.into()),
        pitch: Some(pitch.into()),
        speaking_style: "语速自然、情绪克制、跨镜头音色音量保持一致".into(),
        caption_clause: None,
    };
    finalize_caption_clause(&mut vp, name);
    vp
}

fn looks_feminine(s: &str) -> bool {
    s.contains('女')
        || s.contains("woman")
        || s.contains("girl")
        || s.contains("lady")
        || s.contains("she ")
        || s.contains('她')
}

fn looks_childish(s: &str) -> bool {
    s.contains('童')
        || s.contains('孩')
        || s.contains("child")
        || s.contains("kid")
        || s.contains("boy")
        || s.contains("girl")
        || s.contains("少年")
        || s.contains("少女")
}

fn looks_elder(s: &str) -> bool {
    s.contains('老')
        || s.contains("elder")
        || s.contains("old man")
        || s.contains("old woman")
        || s.contains("中年")
        || s.contains("年迈")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_builds_usable_clause() {
        let ch = CharacterInScene {
            idx: 0,
            identifier_in_scene: "李薇".into(),
            is_visible: true,
            static_features: "年轻女性，长发".into(),
            dynamic_features: None,
            voice_profile: None,
        };
        let vp = heuristic_voice_profile(&ch);
        assert!(vp.is_usable());
        let clause = vp.seedance_clause("李薇");
        assert!(clause.contains("李薇"));
        assert!(!clause.is_empty());
    }
}
