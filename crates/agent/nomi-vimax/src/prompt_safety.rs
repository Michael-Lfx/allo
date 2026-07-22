//! Soften / rewrite image prompts so DashScope content inspection is less likely
//! to reject generations (`DataInspectionFailed`).
//!
//! Important: prefer **positive** wording. Listing banned topics ("no gore", "not nude")
//! often still trips cloud safety filters.

/// Positive-only preamble for the first attempt.
const SAFETY_PREFIX: &str =
    "Family-friendly stylized cinematic illustration, all-ages animated film look. ";

/// Stronger positive preamble for lexical retry.
const SAFETY_PREFIX_STRICT: &str =
    "Strictly all-ages stylized animation still, soft lighting, peaceful cinematic framing. ";

/// Last-resort prompt when even rewritten text fails inspection (output-side filter).
pub const ULTRA_SAFE_FALLBACK_PROMPT: &str = "Wide 16:9 stylized animated film still, soft daylight, peaceful everyday scene with fully clothed cartoon characters, gentle expressions, clean storybook illustration style.";

/// (needle, replacement) — Chinese and English risk phrases softened in-place.
const REPLACEMENTS: &[(&str, &str)] = &[
    ("bloodstained", "dramatic red accents"),
    ("blood-stained", "dramatic red accents"),
    ("bloody", "dramatic"),
    ("blood", "red fabric accent"),
    ("gore", "intense drama"),
    ("gory", "dramatic"),
    ("corpse", "fallen stylized figure"),
    ("dead body", "fallen stylized figure"),
    ("dead bodies", "fallen stylized figures"),
    ("decapitat", "dramatic silhouette"),
    ("dismember", "abstract debris"),
    ("murder", "tense confrontation"),
    ("suicide", "emotional moment"),
    ("torture", "tense conflict"),
    ("slaughter", "chaotic chase"),
    ("massacre", "crowded tense scene"),
    ("gunshot", "sudden sound effect cue"),
    ("shooting", "tense chase"),
    ("stabbing", "close confrontation"),
    ("stabbed", "struck"),
    ("kill him", "confront him"),
    ("kill her", "confront her"),
    ("kill them", "confront them"),
    ("killing", "confronting"),
    ("killed", "defeated"),
    ("assassin", "mysterious agent"),
    ("execution", "dramatic climax"),
    ("behead", "dramatic silhouette"),
    ("nude", "fully clothed"),
    ("naked", "fully clothed"),
    ("nsfw", "safe for work"),
    ("erotic", "elegant"),
    ("sexual", "emotional"),
    ("porn", "portrait"),
    ("sexy lingerie", "elegant outfit"),
    ("lingerie", "elegant outfit"),
    ("drug deal", "secret meeting"),
    ("cocaine", "prop package"),
    ("heroin", "prop package"),
    ("weapon aimed", "tense standoff"),
    ("pointing a gun", "tense standoff"),
    ("pointing gun", "tense standoff"),
    ("assault rifle", "prop device"),
    ("machine gun", "prop device"),
    ("handgun", "prop device"),
    ("pistol", "prop device"),
    ("rifle", "prop staff"),
    ("shotgun", "prop staff"),
    ("grenade", "prop canister"),
    ("bomb", "prop canister"),
    ("explosion gore", "dust cloud"),
    ("血腥", "紧张氛围"),
    ("血淋淋", "紧张氛围"),
    ("流血", "紧张情绪"),
    ("鲜血", "红色布料点缀"),
    ("尸体", "倒下的风格化身影"),
    ("死尸", "倒下的风格化身影"),
    ("枪杀", "紧张对峙"),
    ("开枪", "紧张对峙"),
    ("枪击", "紧张追逐"),
    ("砍杀", "近距离对峙"),
    ("刺杀", "近距离对峙"),
    ("杀戮", "激烈冲突"),
    ("屠杀", "混乱追逐"),
    ("谋杀", "紧张对峙"),
    ("自杀", "情绪低落的时刻"),
    ("虐待", "紧张冲突"),
    ("酷刑", "紧张冲突"),
    ("裸体", "穿着完整服装"),
    ("裸露", "穿着完整服装"),
    ("色情", "优雅氛围"),
    ("性爱", "情感交流"),
    ("情色", "优雅氛围"),
    ("枪支", "道具装置"),
    ("手枪", "道具装置"),
    ("步枪", "道具权杖"),
    ("机关枪", "道具装置"),
    ("炸弹", "道具罐子"),
    ("爆炸碎片", "烟尘"),
    ("毒品", "道具包裹"),
    ("贩毒", "秘密会面"),
];

const LLM_REWRITE_SYSTEM: &str = r#"You rewrite image-generation prompts for Chinese cloud safety filters (DashScope / Z-Image).

Rules:
1. Keep the core scene, characters, camera angle, wardrobe colors, and mood when possible.
2. Soften or remove violence, injury, death, weapons, blood, sexuality, drugs, politics.
3. Use POSITIVE wording only. Never mention banned topics even to forbid them (do not write "no blood", "not nude", "not a real person").
4. Prefer stylized animation / storybook illustration look.
5. Output ONLY the rewritten prompt text. No quotes, no markdown, no explanation.
6. Keep under 500 characters."#;

/// Sanitize an image prompt before the first generation attempt.
pub fn sanitize_image_prompt(prompt: &str) -> String {
    let softened = apply_replacements(prompt);
    let body = softened.trim();
    if body.is_empty() {
        return ULTRA_SAFE_FALLBACK_PROMPT.to_string();
    }
    if body.to_ascii_lowercase().contains("family-friendly")
        || body.contains("全年龄")
        || body.contains("all-ages")
        || body.to_ascii_lowercase().contains("stylized animated")
    {
        return softened;
    }
    format!("{SAFETY_PREFIX}{softened}")
}

/// Aggressive lexical sanitize used after a content-inspection miss.
pub fn sanitize_image_prompt_strict(prompt: &str) -> String {
    let softened = apply_replacements(prompt);
    let stripped = strip_residual_risk(&softened);
    let body = stripped.trim();
    let core = if body.chars().count() > 360 {
        body.chars().take(360).collect::<String>()
    } else {
        body.to_string()
    };
    if core.is_empty() {
        return ULTRA_SAFE_FALLBACK_PROMPT.to_string();
    }
    format!("{SAFETY_PREFIX_STRICT}Scene: {core}")
}

/// Build the user message for LLM safety rewrite.
pub fn llm_rewrite_user_message(original: &str) -> String {
    format!(
        "Rewrite this image prompt to pass all-ages cloud safety filters:\n\n{}",
        original.trim()
    )
}

pub fn llm_rewrite_system_message() -> &'static str {
    LLM_REWRITE_SYSTEM
}

/// Clean LLM rewrite output into a usable image prompt.
pub fn finalize_llm_rewrite(raw: &str, original: &str) -> String {
    let mut t = raw.trim().to_string();
    if let Some(rest) = t.strip_prefix("```") {
        t = rest.to_string();
        if let Some(pos) = t.find("```") {
            t = t[..pos].to_string();
        }
        if let Some((_, body)) = t.split_once('\n') {
            t = body.to_string();
        }
    }
    t = t.trim().trim_matches('"').trim().to_string();
    if t.chars().count() < 12 {
        return sanitize_image_prompt_strict(original);
    }
    sanitize_image_prompt(&t)
}

/// Ultra-safe short prompt that keeps a tiny gist of the original when possible.
pub fn ultra_safe_fallback_prompt(original: &str) -> String {
    let gist = apply_replacements(original);
    let gist = strip_residual_risk(&gist);
    let gist: String = gist.chars().take(120).collect();
    let gist = gist.trim();
    if gist.is_empty() {
        return ULTRA_SAFE_FALLBACK_PROMPT.to_string();
    }
    format!("{ULTRA_SAFE_FALLBACK_PROMPT} Hint of story beat: {gist}")
}

fn apply_replacements(prompt: &str) -> String {
    let mut out = prompt.to_string();
    for (from, to) in REPLACEMENTS {
        if from.is_ascii() {
            out = replace_ascii_case_insensitive(&out, from, to);
        } else if out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

fn replace_ascii_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower = haystack.to_ascii_lowercase();
    let needle_l = needle.to_ascii_lowercase();
    if !lower.contains(&needle_l) {
        return haystack.to_string();
    }
    let mut result = String::with_capacity(haystack.len());
    let lower_bytes = lower.as_bytes();
    let n = needle_l.as_bytes();
    let require_boundary = needle_l.len() <= 5;
    let mut i = 0;
    while i < haystack.len() {
        if i + n.len() <= lower_bytes.len() && &lower_bytes[i..i + n.len()] == n {
            let prev_ok = i == 0 || !is_ascii_word_byte(lower_bytes[i - 1]);
            let next_ok = i + n.len() >= lower_bytes.len()
                || !is_ascii_word_byte(lower_bytes[i + n.len()]);
            if !require_boundary || (prev_ok && next_ok) {
                result.push_str(replacement);
                i += n.len();
                continue;
            }
        }
        let ch = haystack[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

fn is_ascii_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn strip_residual_risk(s: &str) -> String {
    const DROP: &[&str] = &[
        "blood", "gore", "nude", "naked", "nsfw", "porn", "kill", "murder", "corpse", "gun",
        "pistol", "rifle", "weapon", "血腥", "尸体", "裸体", "枪杀", "自杀", "毒品", "色情",
        "real person", "photorealistic", "photograph of a real",
    ];
    let mut out = s.to_string();
    for d in DROP {
        if d.is_ascii() {
            out = replace_ascii_case_insensitive(&out, d, "");
        } else {
            out = out.replace(d, "");
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when upstream rejected the prompt/output for content inspection.
pub fn is_image_content_inspection_err(msg: &str) -> bool {
    let s = msg.to_ascii_lowercase();
    s.contains("datainspectionfailed")
        || s.contains("inappropriate content")
        || s.contains("inappropriate-content")
        || s.contains("output data may contain")
        || s.contains("内容安全")
        || s.contains("敏感内容")
        || s.contains("不当内容")
        || s.contains("all channel models failed")
        || s.contains("所有渠道模型均失败")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softens_blood_and_adds_prefix() {
        let out = sanitize_image_prompt("A bloody fight with a corpse on the floor");
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("family-friendly") || lower.contains("stylized"));
        assert!(!lower.contains("bloody"));
        assert!(!lower.contains("corpse"));
        // Positive wording — avoid banned-topic negation lists.
        assert!(!lower.contains("no gore"));
        assert!(!lower.contains("no nudity"));
    }

    #[test]
    fn softens_chinese_risk_words() {
        let out = sanitize_image_prompt("战场上出现尸体和血腥场面，有人持枪");
        assert!(out.contains("倒下的风格化身影") || out.contains("紧张"));
        assert!(!out.contains("尸体"));
        assert!(!out.contains("血腥"));
    }

    #[test]
    fn strict_mode_strips_residual() {
        let out = sanitize_image_prompt_strict("kill the enemy with a gun");
        let lower = out.to_ascii_lowercase();
        assert!(!lower.contains("kill"));
        assert!(!lower.contains("gun"));
    }

    #[test]
    fn detects_inspection_errors() {
        assert!(is_image_content_inspection_err(
            "DataInspectionFailed: Output data may contain inappropriate content"
        ));
        assert!(is_image_content_inspection_err("All channel models failed"));
        assert!(is_image_content_inspection_err("所有渠道模型均失败：upstream 400"));
    }

    #[test]
    fn finalize_llm_rewrite_strips_fences() {
        let out = finalize_llm_rewrite("```\nA calm village street at dusk\n```", "bloody fight");
        assert!(out.to_ascii_lowercase().contains("village") || out.contains("calm"));
    }
}
