//! Fill missing per-character voice bibles so Seedance keeps timbre consistent across shots.

use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::{CharacterInScene, VoiceProfile};
use crate::error::VimaxResult;
use crate::json_util::complete_and_parse_llm_json;

use super::formats::VOICE_PROFILES;

pub struct VoiceProfileGenerator {
    chat: Arc<dyn VimaxChat>,
}

impl VoiceProfileGenerator {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    /// Ensure every character has a usable, normalized [`VoiceProfile`].
    /// Returns `true` if any were filled or caption clauses were rewritten.
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

        if !missing.is_empty() {
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

            #[derive(Deserialize)]
            struct VoiceItem {
                idx: i32,
                voice_profile: VoiceProfile,
            }
            #[derive(Deserialize)]
            struct Resp {
                voices: Vec<VoiceItem>,
            }
            match complete_and_parse_llm_json::<Resp>(self.chat.as_ref(), &system, &user).await {
                Ok(resp) => {
                    for item in resp.voices {
                        if !item.voice_profile.is_usable() {
                            continue;
                        }
                        if let Some(ch) = characters.iter_mut().find(|c| c.idx == item.idx) {
                            ch.voice_profile = Some(item.voice_profile);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "voice_profile LLM JSON failed after retries; applying heuristic fallbacks"
                    );
                }
            }

            for ch in characters.iter_mut() {
                if ch.voice_profile.as_ref().is_some_and(|v| v.is_usable()) {
                    continue;
                }
                ch.voice_profile = Some(heuristic_voice_profile(ch));
            }
        }

        // Always normalize so existing freeform caption_clause becomes the
        // canonical FIXED SPEAKER VOICE fingerprint reused on every shot.
        let mut changed = false;
        for i in 0..characters.len() {
            let needs_heuristic = characters[i]
                .voice_profile
                .as_ref()
                .is_none_or(|v| !v.is_usable());
            if needs_heuristic {
                let fallback = heuristic_voice_profile(&characters[i]);
                characters[i].voice_profile = Some(fallback);
                changed = true;
                continue;
            }
            let name = characters[i].identifier_in_scene.clone();
            let vp = characters[i].voice_profile.as_mut().unwrap();
            let before = vp.caption_clause.clone();
            vp.normalize(&name);
            if before.as_deref() != vp.caption_clause.as_deref() {
                changed = true;
            }
        }
        Ok(changed)
    }
}

fn finalize_caption_clause(vp: &mut VoiceProfile, name: &str) {
    vp.normalize(name);
}

pub(crate) fn heuristic_voice_profile(ch: &CharacterInScene) -> VoiceProfile {
    let name = ch.identifier_in_scene.trim();
    let static_f = ch.static_features.trim();
    let lower = format!("{name} {static_f}").to_ascii_lowercase();
    // Distinctive acoustic fingerprints — vague "女中音" alone drifts across shots.
    let (timbre, pitch, volume, style) = if looks_childish(&lower) {
        (
            "偏细清亮的少年/少女声，气声轻、共鸣偏头腔，吐字清楚不发虚",
            "high",
            "normal",
            "语速略快但咬字完整，情绪克制，跨镜头绝不改音色",
        )
    } else if looks_feminine(&lower) {
        (
            "清亮偏柔的成年女中音，胸腹共鸣均衡，略带稳定气声，无鼻音",
            "mid-high",
            "normal",
            "语速平稳、句尾轻落，情绪基线冷静，跨镜头音色音量锁定",
        )
    } else if looks_elder(&lower) {
        (
            "沉稳略沙的中老年嗓音，胸腔共鸣偏重，气流稍糙但不破音",
            "low",
            "normal",
            "语速从容、停顿自然，情绪克制，跨镜头音色锁定",
        )
    } else {
        (
            "沉稳清晰的成年男中音，胸腔共鸣适中，音色干爽不沙不尖",
            "mid",
            "normal",
            "语速自然、咬字清楚，情绪克制，跨镜头音色音量锁定",
        )
    };
    let mut vp = VoiceProfile {
        timbre: timbre.into(),
        volume: Some(volume.into()),
        pitch: Some(pitch.into()),
        speaking_style: style.into(),
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
        assert!(clause.contains("FIXED SPEAKER VOICE"));
        assert!(!clause.is_empty());
    }

    #[test]
    fn heuristic_female_fingerprint_is_distinctive() {
        let ch = CharacterInScene {
            idx: 0,
            identifier_in_scene: "阿琳".into(),
            is_visible: true,
            static_features: "女性，短发".into(),
            dynamic_features: None,
            voice_profile: None,
        };
        let vp = heuristic_voice_profile(&ch);
        assert!(vp.timbre.contains("女"));
        assert!(vp.pitch.as_deref() == Some("mid-high"));
    }
}
