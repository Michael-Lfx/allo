//! Fill missing per-character voice bibles so Seedance keeps timbre consistent across shots.

use std::collections::HashSet;
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
        if assign_distinct_tts_voices(characters) {
            changed = true;
        }
        Ok(changed)
    }
}

fn finalize_caption_clause(vp: &mut VoiceProfile, name: &str) {
    vp.normalize(name);
}

/// Flowy Cloud Qwen3-TTS ids (must match `FLOWY_CLOUD_TTS_VOICES` in the UI).
pub(crate) const FLOWY_TTS_FEMALE: &[&str] = &["Cherry", "Serena", "Chelsie"];
pub(crate) const FLOWY_TTS_MALE: &[&str] = &["Ethan"];
pub(crate) const FLOWY_TTS_ALL: &[&str] = &["Cherry", "Serena", "Ethan", "Chelsie"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceBucket {
    Female,
    Male,
    Unknown,
}

/// Give each cast member a distinct known TTS voice, gender-matched when possible.
///
/// Seedance `reference_audio` clones whatever wav we bake. Binding everyone to
/// Cherry made every speaker sound the same and drifted when text lock was off.
pub(crate) fn assign_distinct_tts_voices(characters: &mut [CharacterInScene]) -> bool {
    let mut used: HashSet<String> = HashSet::new();
    for ch in characters.iter() {
        if let Some(v) = ch
            .voice_profile
            .as_ref()
            .and_then(|vp| vp.tts_voice.as_deref())
            .and_then(canonical_tts_voice)
        {
            used.insert(v.to_string());
        }
    }

    let mut changed = false;
    for ch in characters.iter_mut() {
        if ch.voice_profile.is_none() {
            continue;
        }
        let already = ch
            .voice_profile
            .as_ref()
            .and_then(|vp| vp.tts_voice.as_deref())
            .and_then(canonical_tts_voice);
        if let Some(canon) = already {
            let vp = ch.voice_profile.as_mut().unwrap();
            if vp.tts_voice.as_deref() != Some(canon) {
                vp.tts_voice = Some(canon.to_string());
                changed = true;
            }
            continue;
        }
        let bucket = infer_cast_voice_bucket(ch);
        let voice = pick_tts_voice(bucket, &used);
        used.insert(voice.to_string());
        ch.voice_profile.as_mut().unwrap().tts_voice = Some(voice.to_string());
        changed = true;
    }
    changed
}

pub(crate) fn canonical_tts_voice(raw: &str) -> Option<&'static str> {
    let t = raw.trim();
    FLOWY_TTS_ALL
        .iter()
        .copied()
        .find(|v| v.eq_ignore_ascii_case(t))
}

fn pick_tts_voice(bucket: VoiceBucket, used: &HashSet<String>) -> &'static str {
    let preferred: &[&str] = match bucket {
        VoiceBucket::Female => FLOWY_TTS_FEMALE,
        VoiceBucket::Male => FLOWY_TTS_MALE,
        VoiceBucket::Unknown => FLOWY_TTS_ALL,
    };
    if let Some(v) = preferred.iter().copied().find(|v| !used.contains(*v)) {
        return v;
    }
    // Gender pool exhausted: reuse inside that gender. Do not steal the other
    // sex's unused id (a second man sounding like Cherry is worse than two Ethans).
    if !matches!(bucket, VoiceBucket::Unknown) && !preferred.is_empty() {
        return preferred[used.len() % preferred.len()];
    }
    if let Some(v) = FLOWY_TTS_ALL.iter().copied().find(|v| !used.contains(*v)) {
        return v;
    }
    FLOWY_TTS_ALL[used.len() % FLOWY_TTS_ALL.len()]
}

fn infer_cast_voice_bucket(ch: &CharacterInScene) -> VoiceBucket {
    let ident = ch.identifier_in_scene.trim();
    let feat = ch.static_features.trim();
    let dynf = ch.dynamic_features.as_deref().unwrap_or("");
    let blob = format!("{ident} {feat} {dynf}");
    let lower = blob.to_ascii_lowercase();
    let female = evidence_female(ident, &blob, &lower);
    let male = evidence_male(ident, &blob, &lower);
    match (female, male) {
        (true, false) => VoiceBucket::Female,
        (false, true) => VoiceBucket::Male,
        (true, true) if blob.contains('女') && !blob.contains('男') => VoiceBucket::Female,
        (true, true) if blob.contains('男') && !blob.contains('女') => VoiceBucket::Male,
        _ => VoiceBucket::Unknown,
    }
}

pub(crate) fn heuristic_voice_profile(ch: &CharacterInScene) -> VoiceProfile {
    let name = ch.identifier_in_scene.trim();
    let static_f = ch.static_features.trim();
    let lower = format!("{name} {static_f}").to_ascii_lowercase();
    let child = crate::planning::looks_like_child_character(name, static_f);
    let elder = looks_elder(&lower);
    let bucket = infer_cast_voice_bucket(ch);
    // Distinctive acoustic fingerprints — vague "女中音" alone drifts across shots.
    // Never default an unidentified adult to 男中音: that is how 李薇/阿琳 became male.
    let (timbre, pitch, volume, style) = match (child, elder, bucket) {
        (true, _, VoiceBucket::Female) => (
            "偏细清亮的女童/少女声，气声轻、共鸣偏头腔，吐字清楚不发虚",
            "high",
            "normal",
            "语速略快但咬字完整，情绪克制，跨镜头绝不改音色",
        ),
        (true, _, VoiceBucket::Male) => (
            "偏细清亮的男童/少年声，气声轻、共鸣偏头腔，吐字清楚不发虚",
            "high",
            "normal",
            "语速略快但咬字完整，情绪克制，跨镜头绝不改音色",
        ),
        (true, _, VoiceBucket::Unknown) => (
            "偏细清亮的少年声，气声轻、共鸣偏头腔，吐字清楚不发虚",
            "high",
            "normal",
            "语速略快但咬字完整，情绪克制，跨镜头绝不改音色",
        ),
        (_, true, VoiceBucket::Female) => (
            "沉稳略沙的中老年女声，胸腔共鸣偏重，气流稍糙但不破音",
            "low",
            "normal",
            "语速从容、停顿自然，情绪克制，跨镜头音色锁定",
        ),
        (_, true, _) => (
            "沉稳略沙的中老年嗓音，胸腔共鸣偏重，气流稍糙但不破音",
            "low",
            "normal",
            "语速从容、停顿自然，情绪克制，跨镜头音色锁定",
        ),
        (_, _, VoiceBucket::Female) => (
            "清亮偏柔的成年女中音，胸腹共鸣均衡，略带稳定气声，无鼻音",
            "mid-high",
            "normal",
            "语速平稳、句尾轻落，情绪基线冷静，跨镜头音色音量锁定",
        ),
        (_, _, VoiceBucket::Male) => (
            "沉稳清晰的成年男中音，胸腔共鸣适中，音色干爽不沙不尖",
            "mid",
            "normal",
            "语速自然、咬字清楚，情绪克制，跨镜头音色音量锁定",
        ),
        (_, _, VoiceBucket::Unknown) => (
            "清晰稳定的成年声，胸腹共鸣适中，不尖不沙，跨镜头锁定同一音色",
            "mid",
            "normal",
            "语速自然、咬字清楚，情绪克制，跨镜头音色音量锁定",
        ),
    };
    let mut vp = VoiceProfile {
        timbre: timbre.into(),
        volume: Some(volume.into()),
        pitch: Some(pitch.into()),
        speaking_style: style.into(),
        caption_clause: None,
        tts_voice: None,
    };
    finalize_caption_clause(&mut vp, name);
    vp
}

fn evidence_female(ident: &str, blob: &str, lower: &str) -> bool {
    blob.contains('女')
        || blob.contains('她')
        || blob.contains("少女")
        || blob.contains("女士")
        || has_ascii_word(lower, "woman")
        || has_ascii_word(lower, "female")
        || has_ascii_word(lower, "lady")
        || has_ascii_word(lower, "girl")
        || has_ascii_word(lower, "she")
        || feminine_given_name(ident)
}

fn evidence_male(ident: &str, blob: &str, lower: &str) -> bool {
    blob.contains('男')
        || blob.contains('他')
        || blob.contains("男士")
        || has_ascii_word(lower, "man")
        || has_ascii_word(lower, "male")
        || has_ascii_word(lower, "boy")
        || has_ascii_word(lower, "he")
        || masculine_given_name(ident)
}

fn feminine_given_name(ident: &str) -> bool {
    const CHARS: &[char] = &[
        '薇', '琳', '婷', '娟', '娜', '芳', '丽', '雪', '梅', '燕', '霞', '媛', '妮',
        '妍', '怡', '佳', '雯', '静', '颖', '玲', '凤', '琴', '珍', '玉', '红', '英',
        '慧', '兰',
    ];
    ident.chars().any(|c| CHARS.contains(&c)) || english_given_name_in(ident, FEMALE_EN_NAMES)
}

fn masculine_given_name(ident: &str) -> bool {
    const CHARS: &[char] = &[
        '强', '伟', '军', '刚', '勇', '辉', '峰', '磊', '斌', '涛', '杰', '鹏', '浩',
        '宇', '轩', '博', '龙', '虎',
    ];
    ident.chars().any(|c| CHARS.contains(&c)) || english_given_name_in(ident, MALE_EN_NAMES)
}

const FEMALE_EN_NAMES: &[&str] = &[
    "alice", "amy", "ann", "anna", "emma", "lily", "lucy", "mary", "sarah", "linda",
    "joyce", "jane", "jenny", "lisa", "kate", "emily", "sophia", "olivia", "mia",
    "chloe", "grace", "helen", "nancy", "susan", "wendy", "vivian",
];

const MALE_EN_NAMES: &[&str] = &[
    "bob", "tom", "john", "jack", "mike", "david", "james", "robert", "michael",
    "william", "jim", "bill", "steve", "peter", "paul", "mark", "tony", "ethan",
];

fn english_given_name_in(ident: &str, names: &[&str]) -> bool {
    let lower = ident.to_ascii_lowercase();
    names.iter().any(|n| has_ascii_word(&lower, n))
}

fn has_ascii_word(hay: &str, needle: &str) -> bool {
    hay.split(|c: char| !c.is_ascii_alphabetic())
        .any(|w| w.eq_ignore_ascii_case(needle))
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

    fn ch(idx: i32, name: &str, features: &str) -> CharacterInScene {
        CharacterInScene {
            idx,
            identifier_in_scene: name.into(),
            is_visible: true,
            static_features: features.into(),
            dynamic_features: None,
            voice_profile: None,
        }
    }

    #[test]
    fn heuristic_uses_given_name_when_features_omit_sex() {
        let vp = heuristic_voice_profile(&ch(0, "李薇", "长发"));
        assert!(vp.timbre.contains("女"), "{}", vp.timbre);
        let vp = heuristic_voice_profile(&ch(1, "阿琳", ""));
        assert!(vp.timbre.contains("女"), "{}", vp.timbre);
        let vp = heuristic_voice_profile(&ch(2, "阿强", "短发"));
        assert!(vp.timbre.contains("男"), "{}", vp.timbre);
    }

    #[test]
    fn heuristic_does_not_default_unknown_adult_to_male() {
        let vp = heuristic_voice_profile(&ch(0, "林晚", "长发"));
        assert!(!vp.timbre.contains("男中音"), "{}", vp.timbre);
        assert!(vp.timbre.contains("成年声"), "{}", vp.timbre);
    }

    #[test]
    fn assign_distinct_tts_voices_matches_gender_and_avoids_collapse() {
        let mut chars = vec![
            ch(0, "李薇", "成年女性"),
            ch(1, "阿琳", "成年女性"),
            ch(2, "阿强", "成年男性"),
        ];
        for c in &mut chars {
            c.voice_profile = Some(heuristic_voice_profile(c));
        }
        assert!(assign_distinct_tts_voices(&mut chars));
        let v0 = chars[0]
            .voice_profile
            .as_ref()
            .and_then(|v| v.tts_voice.as_deref())
            .unwrap();
        let v1 = chars[1]
            .voice_profile
            .as_ref()
            .and_then(|v| v.tts_voice.as_deref())
            .unwrap();
        let v2 = chars[2]
            .voice_profile
            .as_ref()
            .and_then(|v| v.tts_voice.as_deref())
            .unwrap();
        assert_ne!(v0, v1);
        assert!(FLOWY_TTS_FEMALE.contains(&v0), "{v0}");
        assert!(FLOWY_TTS_FEMALE.contains(&v1), "{v1}");
        assert_eq!(v2, "Ethan");
    }
}
