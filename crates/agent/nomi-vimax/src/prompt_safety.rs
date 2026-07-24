//! Soften / rewrite image prompts so DashScope content inspection is less likely
//! to reject generations (`DataInspectionFailed`).
//!
//! Important: prefer **positive** wording. Listing banned topics ("no gore", "not nude")
//! often still trips cloud safety filters.

use crate::planning::wants_stylized_non_photoreal;

/// Positive-only preamble for the first attempt (live-action / cinematic default).
const SAFETY_PREFIX: &str =
    "Family-friendly all-ages cinematic still, fully clothed characters, tasteful framing. ";

/// Positive-only preamble when the user Style is anime/animation/illustration.
const SAFETY_PREFIX_STYLIZED: &str =
    "Family-friendly all-ages stylized illustration still matching the requested Style, fully clothed characters, tasteful framing. ";

/// Stronger positive preamble for lexical retry.
const SAFETY_PREFIX_STRICT: &str =
    "Strictly all-ages cinematic still, soft lighting, peaceful framing, fully clothed. ";

const SAFETY_PREFIX_STRICT_STYLIZED: &str =
    "Strictly all-ages stylized illustration still matching Style, soft lighting, peaceful framing, fully clothed. ";

/// Vacant set / prop plates must NOT get the "characters" safety prefix (that causes people in env/prop images).
const SAFETY_PREFIX_VACANT: &str =
    "Family-friendly vacant plate, architecture furniture props lighting only, completely unoccupied, no people. ";

const SAFETY_PREFIX_VACANT_STRICT: &str =
    "Strictly vacant unoccupied set or object plate, soft daylight, zero people zero faces. ";

const SAFETY_PREFIX_VACANT_STYLIZED: &str =
    "Family-friendly vacant stylized plate matching Style, architecture furniture props lighting only, completely unoccupied, no people. ";

const SAFETY_PREFIX_VACANT_STRICT_STYLIZED: &str =
    "Strictly vacant unoccupied stylized set or object plate matching Style, soft daylight, zero people zero faces. ";

/// Last-resort prompt when even rewritten text fails inspection (output-side filter).
/// Keep cinematic — never push "cartoon characters" (that causes kids≠adults style split).
pub const ULTRA_SAFE_FALLBACK_PROMPT: &str = "Wide 16:9 family-friendly cinematic still, soft daylight, peaceful everyday scene with fully clothed characters of consistent film style (adults and children share the same cinematic rendering), gentle expressions, clear readable faces.";

pub const ULTRA_SAFE_FALLBACK_PROMPT_STYLIZED: &str = "Wide 16:9 family-friendly stylized illustration still matching the requested Style, soft daylight, peaceful everyday scene with fully clothed characters of consistent drawn style (adults and children share the same look), gentle expressions, clear readable faces.";

/// Vacant fallback — never mention characters/people.
pub const ULTRA_SAFE_VACANT_FALLBACK_PROMPT: &str = "Wide 16:9 vacant film location or isolated object plate, soft daylight, empty architecture and props only, completely unoccupied, no people no faces no silhouettes.";

pub const ULTRA_SAFE_VACANT_FALLBACK_PROMPT_STYLIZED: &str = "Wide 16:9 vacant stylized location or isolated object plate matching Style, soft daylight, empty architecture and props only, completely unoccupied, no people no faces no silhouettes.";

/// True when this prompt is an empty-set / prop bible (must not inject "characters").
pub fn looks_like_vacant_world_prompt(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "vacant",
        "empty set",
        "empty-set",
        "empty film location",
        "location plate",
        "zero people",
        "zero humans",
        "object only",
        "object-only",
        "prop bible",
        "unoccupied",
        "no people",
        "空场",
        "无人",
        "仅物体",
        "道具圣经",
    ];
    NEEDLES.iter().any(|n| p.contains(n))
}

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
4. Preserve the requested visual style from the original prompt when safe (photoreal, cinematic, illustration, anime, animation, etc.). If the original asked for anime/animation/illustration, KEEP that look — do NOT convert it to live-action cinematic.
5. CRITICAL: If the prompt includes children/kids/teens, keep them in the SAME visual style as adults. Never convert only children into a different medium than adults.
6. Output ONLY the rewritten prompt text. No quotes, no markdown, no explanation.
7. Keep under 500 characters."#;

/// Sanitize an image prompt before the first generation attempt.
pub fn sanitize_image_prompt(prompt: &str) -> String {
    let softened = apply_replacements(prompt);
    let body = softened.trim();
    let vacant = looks_like_vacant_world_prompt(body);
    let stylized = wants_stylized_non_photoreal(body);
    if body.is_empty() {
        return if vacant {
            if stylized {
                ULTRA_SAFE_VACANT_FALLBACK_PROMPT_STYLIZED.to_string()
            } else {
                ULTRA_SAFE_VACANT_FALLBACK_PROMPT.to_string()
            }
        } else if stylized {
            ULTRA_SAFE_FALLBACK_PROMPT_STYLIZED.to_string()
        } else {
            ULTRA_SAFE_FALLBACK_PROMPT.to_string()
        };
    }
    let prefix = if vacant {
        if stylized {
            SAFETY_PREFIX_VACANT_STYLIZED
        } else {
            SAFETY_PREFIX_VACANT
        }
    } else if stylized {
        SAFETY_PREFIX_STYLIZED
    } else {
        SAFETY_PREFIX
    };
    let mut out = if body.to_ascii_lowercase().contains("family-friendly")
        || body.contains("全年龄")
        || body.contains("all-ages")
    {
        // Still strip accidental "characters" injection risk for vacant plates.
        if vacant {
            format!(
                "{}{softened}",
                if stylized {
                    SAFETY_PREFIX_VACANT_STYLIZED
                } else {
                    SAFETY_PREFIX_VACANT
                }
            )
        } else {
            softened
        }
    } else {
        format!("{prefix}{softened}")
    };
    // Cloud filters often anime-ify kids; reinforce cast-wide style lock (not for vacant plates).
    if !vacant
        && crate::planning::looks_like_child_character("", &out)
        && !out.to_ascii_lowercase().contains("same visual style")
        && !out.to_ascii_lowercase().contains("cast style lock")
        && !out.to_ascii_lowercase().contains("same animation")
        && !out.to_ascii_lowercase().contains("same drawn")
    {
        if stylized {
            out.push_str(
                " Adults and children share the same requested Style (animation/illustration; not photoreal for kids only).",
            );
        } else {
            out.push_str(
                " Adults and children share the same cinematic visual style (not anime for kids only).",
            );
        }
    }
    out
}

/// Aggressive lexical sanitize used after a content-inspection miss.
pub fn sanitize_image_prompt_strict(prompt: &str) -> String {
    let softened = apply_replacements(prompt);
    let stripped = strip_residual_risk(&softened);
    let body = stripped.trim();
    let vacant = looks_like_vacant_world_prompt(prompt) || looks_like_vacant_world_prompt(body);
    let stylized = wants_stylized_non_photoreal(prompt) || wants_stylized_non_photoreal(body);
    let core = if body.chars().count() > 360 {
        body.chars().take(360).collect::<String>()
    } else {
        body.to_string()
    };
    if core.is_empty() {
        return if vacant {
            if stylized {
                ULTRA_SAFE_VACANT_FALLBACK_PROMPT_STYLIZED.to_string()
            } else {
                ULTRA_SAFE_VACANT_FALLBACK_PROMPT.to_string()
            }
        } else if stylized {
            ULTRA_SAFE_FALLBACK_PROMPT_STYLIZED.to_string()
        } else {
            ULTRA_SAFE_FALLBACK_PROMPT.to_string()
        };
    }
    let prefix = if vacant {
        if stylized {
            SAFETY_PREFIX_VACANT_STRICT_STYLIZED
        } else {
            SAFETY_PREFIX_VACANT_STRICT
        }
    } else if stylized {
        SAFETY_PREFIX_STRICT_STYLIZED
    } else {
        SAFETY_PREFIX_STRICT
    };
    format!("{prefix}Scene: {core}")
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
    let vacant = looks_like_vacant_world_prompt(original);
    let stylized = wants_stylized_non_photoreal(original);
    let gist = apply_replacements(original);
    let gist = strip_residual_risk(&gist);
    let gist: String = gist.chars().take(120).collect();
    let gist = gist.trim();
    let base = if vacant {
        if stylized {
            ULTRA_SAFE_VACANT_FALLBACK_PROMPT_STYLIZED
        } else {
            ULTRA_SAFE_VACANT_FALLBACK_PROMPT
        }
    } else if stylized {
        ULTRA_SAFE_FALLBACK_PROMPT_STYLIZED
    } else {
        ULTRA_SAFE_FALLBACK_PROMPT
    };
    if gist.is_empty() {
        return base.to_string();
    }
    format!("{base} Hint: {gist}")
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
    // Schema / missing-field errors must NOT trigger safety rewrites (would
    // burn retries while still sending a broken body, e.g. Seedream MissingParameter).
    if s.contains("missingparameter")
        || s.contains("missing parameter")
        || s.contains("missing `prompt`")
        || s.contains("missing \"prompt\"")
        || s.contains("invalidparameter")
        || s.contains("invalid_param")
        || s.contains("invalid request")
    {
        return false;
    }
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
        assert!(lower.contains("family-friendly") || lower.contains("all-ages") || lower.contains("cinematic"));
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
        assert!(!is_image_content_inspection_err(
            "API error 500: All channel models failed: upstream 400: MissingParameter missing `prompt`"
        ));
    }

    #[test]
    fn finalize_llm_rewrite_strips_fences() {
        let out = finalize_llm_rewrite("```\nA calm village street at dusk\n```", "bloody fight");
        assert!(out.to_ascii_lowercase().contains("village") || out.contains("calm"));
    }

    #[test]
    fn vacant_plates_do_not_get_characters_prefix() {
        let out = sanitize_image_prompt(
            "Wide 16:9 vacant film location plate. Completely unoccupied. Zero people. Location: INT. CAFE",
        );
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("vacant") || lower.contains("unoccupied"));
        assert!(
            !lower.contains("fully clothed characters"),
            "vacant sanitize must not ask for characters: {out}"
        );
        assert!(looks_like_vacant_world_prompt(&out) || lower.contains("no people"));
    }

    #[test]
    fn vacant_ultra_safe_has_no_characters() {
        let out = ultra_safe_fallback_prompt("vacant empty-set plate INT. OFFICE zero people");
        let lower = out.to_ascii_lowercase();
        assert!(!lower.contains("fully clothed characters"));
        assert!(lower.contains("vacant") || lower.contains("unoccupied") || lower.contains("empty"));
    }

    #[test]
    fn anime_prompts_do_not_get_forced_cinematic_prefix() {
        let out = sanitize_image_prompt(
            "CRITICAL Visual style (MUST MATCH): stylized anime / animated film look. Full-body FRONT character bible.",
        );
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("anime") || lower.contains("animated") || lower.contains("stylized"));
        assert!(
            !lower.contains("cinematic still"),
            "anime sanitize must not force cinematic still: {out}"
        );
        assert!(
            !lower.contains("not anime for kids only"),
            "anime sanitize must not ban anime: {out}"
        );
    }

    #[test]
    fn cinematic_anti_anime_prompt_keeps_cinematic_safety_prefix() {
        let out = sanitize_image_prompt(
            "STYLE FIRST: cinematic film look. LIVE-ACTION cast continuity. FORBIDDEN: anime, manga, cartoon.",
        );
        let lower = out.to_ascii_lowercase();
        assert!(
            lower.contains("cinematic still") || lower.starts_with("family-friendly all-ages cinematic"),
            "cinematic prompt must keep cinematic safety prefix: {out}"
        );
        assert!(
            !lower.contains("stylized illustration still"),
            "anti-anime wording must not flip safety into illustration mode: {out}"
        );
    }
}
