//! Assign one unused bank clip per character (gender × age × timbre, no reuse).

use crate::domain::CharacterInScene;
use crate::planning::{looks_like_child_character, parse_character_age_years, OutputLanguage};

use super::manifest::{VoiceAge, VoiceBankManifest, VoiceEntry, VoiceGender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterVoiceNeed {
    pub gender: VoiceGender,
    pub age: VoiceAge,
    pub pitch: Option<&'static str>,
}

pub fn infer_need(ch: &CharacterInScene) -> CharacterVoiceNeed {
    let ident = ch.identifier_in_scene.trim();
    let feat = ch.static_features.trim();
    let dynf = ch.dynamic_features.as_deref().unwrap_or("");
    let blob = format!("{ident} {feat} {dynf}");
    let gender = infer_gender(ident, &blob);
    let age = infer_age(ident, feat, &blob);
    let pitch = ch
        .voice_profile
        .as_ref()
        .and_then(|vp| vp.pitch.as_deref())
        .and_then(normalize_pitch);
    CharacterVoiceNeed {
        gender,
        age,
        pitch,
    }
}

/// Unique assignment: never give two speakers the same clip while unused
/// voices remain in that gender. Opposite-gender clips are a last resort only
/// when the gender pool is empty (unknown-gender extras).
pub fn assign_unique_voices<'a>(
    characters: &'a [CharacterInScene],
    manifest: &'a VoiceBankManifest,
    film_language: OutputLanguage,
) -> Vec<(&'a CharacterInScene, &'a VoiceEntry)> {
    let lang = match film_language {
        OutputLanguage::English => "en",
        OutputLanguage::Chinese | OutputLanguage::Unspecified => "zh",
    };
    let mut used = std::collections::HashSet::<String>::new();
    // Preserve already-bound bank ids across re-runs of the same in-memory cast.
    for ch in characters {
        if let Some(id) = ch
            .voice_profile
            .as_ref()
            .and_then(|vp| vp.tts_voice.as_deref())
        {
            if manifest.voices.iter().any(|v| v.id == id) {
                used.insert(id.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for ch in characters {
        if !ch.needs_identity_assets() {
            continue;
        }
        if !ch.voice_profile.as_ref().is_some_and(|v| v.is_usable()) {
            continue;
        }
        let need = infer_need(ch);
        let Some(voice) = pick_voice(&manifest.voices, &used, need, lang) else {
            continue;
        };
        used.insert(voice.id.clone());
        out.push((ch, voice));
    }
    out
}

fn pick_voice<'a>(
    voices: &'a [VoiceEntry],
    used: &std::collections::HashSet<String>,
    need: CharacterVoiceNeed,
    lang: &str,
) -> Option<&'a VoiceEntry> {
    let mut best: Option<(&'a VoiceEntry, i32)> = None;
    for voice in voices {
        if used.contains(&voice.id) {
            continue;
        }
        let Some(score) = score_voice(voice, need, lang) else {
            continue;
        };
        match best {
            None => best = Some((voice, score)),
            Some((_, best_score)) if score > best_score => best = Some((voice, score)),
            Some(_) => {}
        }
    }
    if let Some((voice, _)) = best {
        return Some(voice);
    }
    // Gender pool exhausted: reuse inside that gender (same policy as TTS).
    let mut pool: Vec<&VoiceEntry> = voices
        .iter()
        .filter(|v| gender_ok(v.gender, need.gender) && language_ok(&v.language, lang))
        .collect();
    if pool.is_empty() {
        pool = voices
            .iter()
            .filter(|v| gender_ok(v.gender, need.gender))
            .collect();
    }
    if pool.is_empty() {
        return None;
    }
    Some(pool[used.len() % pool.len()])
}

fn score_voice(voice: &VoiceEntry, need: CharacterVoiceNeed, lang: &str) -> Option<i32> {
    if !gender_ok(voice.gender, need.gender) {
        return None;
    }
    let mut score = 0;
    if language_exact(&voice.language, lang) {
        score += 80;
    } else if language_ok(&voice.language, lang) {
        score += 40;
    }
    // Cross-language clips still score: Seedance/Wan use timbre, not lexicon.
    if voice.gender == need.gender {
        score += 50;
    } else if need.gender == VoiceGender::Unknown {
        score += 10;
    }
    score += age_score(voice.age, need.age);
    if let Some(want) = need.pitch {
        if let Some(have) = voice.pitch.as_deref() {
            if normalize_pitch(have) == Some(want) {
                score += 8;
            }
        }
    }
    Some(score)
}

fn language_ok(voice_lang: &str, want: &str) -> bool {
    let v = voice_lang.trim().to_ascii_lowercase();
    if v.is_empty() {
        return true;
    }
    v.starts_with(want)
        || (want == "zh"
            && (v.starts_with("cmn")
                || v.starts_with("yue")
                || v.contains("chinese")
                || v.contains("cantonese")))
}

fn language_exact(voice_lang: &str, want: &str) -> bool {
    let v = voice_lang.trim().to_ascii_lowercase();
    v.starts_with(want)
}

fn gender_ok(voice: VoiceGender, need: VoiceGender) -> bool {
    match need {
        VoiceGender::Unknown => true,
        VoiceGender::Female => voice == VoiceGender::Female || voice == VoiceGender::Unknown,
        VoiceGender::Male => voice == VoiceGender::Male || voice == VoiceGender::Unknown,
    }
}

fn age_score(voice: VoiceAge, need: VoiceAge) -> i32 {
    if voice == need {
        return 30;
    }
    match (need, voice) {
        (VoiceAge::Child, VoiceAge::Young) | (VoiceAge::Young, VoiceAge::Child) => 12,
        (VoiceAge::Young, VoiceAge::Adult) | (VoiceAge::Adult, VoiceAge::Young) => 10,
        (VoiceAge::Adult, VoiceAge::Mature) | (VoiceAge::Mature, VoiceAge::Adult) => 10,
        (VoiceAge::Mature, VoiceAge::Elder) | (VoiceAge::Elder, VoiceAge::Mature) => 12,
        (VoiceAge::Elder, VoiceAge::Adult) | (VoiceAge::Adult, VoiceAge::Elder) => 4,
        _ => 0,
    }
}

fn infer_gender(ident: &str, blob: &str) -> VoiceGender {
    let lower = blob.to_ascii_lowercase();
    let female = blob.contains('女')
        || blob.contains('她')
        || blob.contains("少女")
        || blob.contains("女士")
        || has_ascii_word(&lower, "woman")
        || has_ascii_word(&lower, "female")
        || has_ascii_word(&lower, "lady")
        || has_ascii_word(&lower, "girl")
        || feminine_given_name(ident);
    let male = blob.contains('男')
        || blob.contains('他')
        || blob.contains("男士")
        || has_ascii_word(&lower, "man")
        || has_ascii_word(&lower, "male")
        || has_ascii_word(&lower, "boy")
        || masculine_given_name(ident);
    match (female, male) {
        (true, false) => VoiceGender::Female,
        (false, true) => VoiceGender::Male,
        (true, true) if blob.contains('女') && !blob.contains('男') => VoiceGender::Female,
        (true, true) if blob.contains('男') && !blob.contains('女') => VoiceGender::Male,
        _ => VoiceGender::Unknown,
    }
}

fn infer_age(ident: &str, feat: &str, blob: &str) -> VoiceAge {
    if let Some(age) = parse_character_age_years(blob) {
        return match age {
            0..=13 => VoiceAge::Child,
            14..=24 => VoiceAge::Young,
            25..=39 => VoiceAge::Adult,
            40..=54 => VoiceAge::Mature,
            _ => VoiceAge::Elder,
        };
    }
    let lower = blob.to_ascii_lowercase();
    if blob.contains("老年")
        || blob.contains("年迈")
        || blob.contains("爷爷")
        || blob.contains("奶奶")
        || blob.contains("外公")
        || blob.contains("外婆")
        || has_ascii_word(&lower, "elder")
        || has_ascii_word(&lower, "elderly")
    {
        return VoiceAge::Elder;
    }
    if blob.contains("中年")
        || blob.contains("大叔")
        || blob.contains("阿姨")
        || has_ascii_word(&lower, "middle-aged")
        || has_ascii_word(&lower, "middle aged")
    {
        return VoiceAge::Mature;
    }
    // `looks_like_child_character` treats 少女/少年 as children; short-drama
    // leads of that name are young adults. Only send true child cues here.
    if is_true_child(ident, feat, blob) {
        return VoiceAge::Child;
    }
    if blob.contains("少女")
        || blob.contains("少年")
        || blob.contains("年轻")
        || blob.contains("青年")
        || has_ascii_word(&lower, "young")
        || has_ascii_word(&lower, "teen")
    {
        return VoiceAge::Young;
    }
    VoiceAge::Adult
}

fn is_true_child(ident: &str, feat: &str, blob: &str) -> bool {
    if looks_like_child_character(ident, feat)
        && !(blob.contains("少女") || blob.contains("少年"))
    {
        return true;
    }
    blob.contains("小孩")
        || blob.contains("儿童")
        || blob.contains("男童")
        || blob.contains("女童")
        || blob.contains("小学生")
        || blob.contains("幼儿")
}

fn normalize_pitch(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "high" | "高" => Some("high"),
        "mid-high" | "midhigh" | "中高" => Some("mid-high"),
        "mid" | "medium" | "中" => Some("mid"),
        "mid-low" | "midlow" | "中低" => Some("mid-low"),
        "low" | "低" => Some("low"),
        _ => None,
    }
}

fn feminine_given_name(ident: &str) -> bool {
    const CHARS: &[char] = &[
        '薇', '琳', '婷', '娟', '娜', '芳', '丽', '雪', '梅', '燕', '霞', '媛', '妮',
        '妍', '怡', '佳', '雯', '静', '颖', '玲', '凤', '琴', '珍', '玉', '红', '英',
        '慧', '兰',
    ];
    ident.chars().any(|c| CHARS.contains(&c))
}

fn masculine_given_name(ident: &str) -> bool {
    const CHARS: &[char] = &[
        '强', '伟', '军', '刚', '勇', '辉', '峰', '磊', '斌', '涛', '杰', '鹏', '浩',
        '宇', '轩', '博', '龙', '虎',
    ];
    ident.chars().any(|c| CHARS.contains(&c))
}

fn has_ascii_word(hay: &str, needle: &str) -> bool {
    hay.split(|c: char| !c.is_ascii_alphabetic())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::VoiceProfile;

    fn voice(id: &str, gender: VoiceGender, age: VoiceAge) -> VoiceEntry {
        VoiceEntry {
            id: id.into(),
            file: format!("wav/{id}.wav"),
            language: "zh".into(),
            gender,
            age,
            pitch: None,
            tags: Vec::new(),
            source: None,
            transcript: None,
        }
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

    #[test]
    fn two_men_do_not_share_a_clip() {
        let manifest = VoiceBankManifest {
            schema_version: 1,
            version: "1".into(),
            name: None,
            voices: vec![
                voice("m1", VoiceGender::Male, VoiceAge::Adult),
                voice("m2", VoiceGender::Male, VoiceAge::Adult),
                voice("f1", VoiceGender::Female, VoiceAge::Adult),
            ],
        };
        let chars = vec![
            ch(0, "阿强", "成年男性"),
            ch(1, "张伟", "成年男性"),
            ch(2, "李薇", "成年女性"),
        ];
        let assigned = assign_unique_voices(&chars, &manifest, OutputLanguage::Chinese);
        assert_eq!(assigned.len(), 3);
        let ids: Vec<&str> = assigned.iter().map(|(_, v)| v.id.as_str()).collect();
        assert_ne!(ids[0], ids[1]);
        assert!(ids[0].starts_with('m') && ids[1].starts_with('m'));
        assert_eq!(ids[2], "f1");
    }

    #[test]
    fn young_woman_prefers_young_not_child() {
        let need = infer_need(&ch(0, "苏糖", "年轻女性，长发"));
        assert_eq!(need.gender, VoiceGender::Female);
        assert_eq!(need.age, VoiceAge::Young);
    }

    #[test]
    fn child_keyword_stays_child() {
        let need = infer_need(&ch(0, "小明", "8岁男孩"));
        assert_eq!(need.gender, VoiceGender::Male);
        assert_eq!(need.age, VoiceAge::Child);
    }

    #[test]
    fn chinese_cast_falls_back_to_english_timbre() {
        let manifest = VoiceBankManifest {
            schema_version: 1,
            version: "1".into(),
            name: None,
            voices: vec![
                voice("zh_m1", VoiceGender::Male, VoiceAge::Adult),
                VoiceEntry {
                    id: "en_m1".into(),
                    file: "wav/en_m1.wav".into(),
                    language: "en".into(),
                    gender: VoiceGender::Male,
                    age: VoiceAge::Adult,
                    pitch: None,
                    tags: Vec::new(),
                    source: None,
                    transcript: None,
                },
            ],
        };
        let chars = vec![ch(0, "阿强", "成年男性"), ch(1, "张伟", "成年男性")];
        let assigned = assign_unique_voices(&chars, &manifest, OutputLanguage::Chinese);
        let ids: Vec<&str> = assigned.iter().map(|(_, v)| v.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "zh_m1");
        assert_eq!(ids[1], "en_m1");
    }
}
