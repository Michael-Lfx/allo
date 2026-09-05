//! Planning helpers: clip length follows the beats (speech + related visual
//! events packed into one generation), not a shot-count quota.
//!
//! The accepted per-clip window is a property of the **selected video model**
//! ([`crate::clip_bounds::ClipBounds`]), so every duration helper here takes it
//! as an argument instead of hardcoding one vendor's numbers.

use crate::clip_bounds::ClipBounds;

/// Shortest film the UI / API will plan. A product floor, not a model limit.
pub const MIN_TARGET_DURATION_SECS: u32 = 5;

/// Default target total length when the user does not specify one.
pub const DEFAULT_TARGET_DURATION_SECS: u32 = 45;

/// Max user-facing film target (UI timeline + plan/render clamp).
pub const MAX_TARGET_DURATION_SECS: u32 = 300;

/// Clear spoken Chinese chars/sec — conversational drama, not funeral-slow and not rushed.
/// Daily Mandarin is ~4–5 chars/s; 3.3 leaves room for emotion without padding the clip.
pub(crate) const SPEECH_CJK_CHARS_PER_SEC: f32 = 3.3;
/// Clear spoken English words/sec (aligned with the CJK bias: clear, not drawn-out).
pub(crate) const SPEECH_EN_WORDS_PER_SEC: f32 = 2.3;
/// Breath / reaction beat before the first spoken syllable.
const SPEECH_LEAD_SECS: u32 = 1;
/// Tail seconds after the last spoken syllable so audio is not cut mid-breath.
/// Keep short: reaction/action should fill the landing, not empty hold.
const SPEECH_TAIL_SECS: u32 = 1;
/// A spoken beat needs a lead-in, the line, and a landing. Budget compression
/// must never push dialogue below this even when the model accepts shorter clips.
const DIALOGUE_FLOOR_SECS: u32 = 5;
/// Soft-landing seconds preferred at the end of each shot before a splice.
/// Reserved **from** the user target before budget fitting, then re-applied, so
/// the rendered sum stays near the advertised length (never `target + 2×shots`).
pub const SHOT_SPLICE_TAIL_PADDING_SECS: u32 = 1;

/// [`DIALOGUE_FLOOR_SECS`] pulled inside the selected model's window.
fn dialogue_floor_secs(bounds: ClipBounds) -> u32 {
    bounds.clamp_secs(DIALOGUE_FLOOR_SECS)
}

/// Seedance music caption `(…)` shared by every shot in a scene.
/// Identical wording keeps motif/tempo intent stable across adjacent I2V clips.
pub const DEFAULT_SCENE_BGM_PAREN: &str = "\
(soft continuous cinematic atmospheric underscore, same motif tempo key and instrumentation \
across adjacent shots, stable moderate volume, no sudden genre drop or silence gap)";

/// Default look when the user leaves style empty.
pub const DEFAULT_VISUAL_STYLE: &str = "cinematic film look, believable designed characters, natural wardrobe and lighting, clean healthy facial skin with clear readable features";

/// Resolve user style text; empty → cinematic designed-character default.
pub fn resolve_visual_style(user_style: &str) -> String {
    let t = user_style.trim();
    if t.is_empty() {
        DEFAULT_VISUAL_STYLE.to_string()
    } else {
        t.to_string()
    }
}

/// Style line for three-view identity sheets: keep the look, drop filler that
/// is not about *this* person (generic "designed characters", healthy-skin
/// boilerplate, topology-mesh overlays).
pub fn style_for_three_view_image(user_style: &str) -> String {
    let mut s = resolve_visual_style(user_style);
    const DROP: &[&str] = &[
        "believable designed characters",
        "expressive designed characters",
        "designed characters",
        "clean healthy facial skin with clear readable features",
        "clean healthy facial skin with clear features",
        "clean healthy facial skin",
        "blue topology mesh",
        "topology mesh",
    ];
    for phrase in DROP {
        s = strip_ascii_ci(&s, phrase);
    }
    tidy_comma_list(&s)
}

fn strip_ascii_ci(hay: &str, needle: &str) -> String {
    let n = needle.to_ascii_lowercase();
    if n.is_empty() {
        return hay.to_string();
    }
    let mut remaining = hay;
    let mut out = String::with_capacity(hay.len());
    loop {
        let lower = remaining.to_ascii_lowercase();
        match lower.find(&n) {
            None => {
                out.push_str(remaining);
                break;
            }
            Some(pos) => {
                out.push_str(&remaining[..pos]);
                remaining = &remaining[pos + needle.len()..];
            }
        }
    }
    out
}

fn tidy_comma_list(s: &str) -> String {
    s.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// True when the user asked for anime / animation / cartoon / illustration (non-photoreal).
/// Used so cast/safety locks do NOT force cinematic/live-action look over the user's choice.
///
/// Important: negated mentions like "NOT anime" / "no cartoon" must NOT count as requesting stylization
/// (otherwise safety prefixes flip cinematic prompts into illustration mode).
pub fn wants_stylized_non_photoreal(user_style: &str) -> bool {
    let raw = user_style.trim();
    if raw.is_empty() {
        return false;
    }
    // Strong live-action phrasing does not itself request stylization; only positive needles do.
    let lower_raw = raw.to_ascii_lowercase();

    const EN: &[&str] = &[
        "anime",
        "animation",
        "animated",
        "cartoon",
        "toon",
        "manga",
        "manhwa",
        "webtoon",
        "donghua",
        "comic",
        "cel-shad",
        "cel shad",
        "illustration",
        "illustrated",
        "storybook",
        "hand-drawn",
        "hand drawn",
        "2d art",
        "2d animation",
        "painted",
        "watercolor",
        "ink-wash",
        "ink wash",
        "oil painting",
        "oil-paint",
        "impasto",
        "pixar",
        "disney",
        "ghibli",
        "chibi",
        "claymation",
        "plasticine",
        "stop-motion",
        "stop motion",
        "pixel-art",
        "pixel art",
        "isometric",
        "diorama",
        "blind-box",
        "blind box",
        "paper-cut",
        "papercut",
        "ukiyo-e",
        "ukiyo e",
        "low-poly",
        "low poly",
        "shinkai",
        "game-engine",
        "game engine",
        "unreal engine",
        "toon-shaded",
        "toon shaded",
        "lego",
        "brickfilm",
        "felted",
        "wool felt",
        "crayon",
        "charcoal",
        "line-art",
        "line art",
        "dunhuang",
        "stained-glass",
        "stained glass",
        "art nouveau",
        "pop art",
        "shadow puppet",
        "gouache",
        "shoujo",
        "shojo",
        "voxel",
        "origami",
        "glitch",
    ];
    let positive_en = EN.iter().any(|n| positive_style_needle(&lower_raw, n));
    const ZH: &[&str] = &[
        "动画",
        "动漫",
        "二次元",
        "国漫",
        "卡通",
        "漫画",
        "韩漫",
        "插画",
        "手绘",
        "水彩",
        "水墨",
        "油画",
        "日式",
        "赛璐璐",
        "绘本",
        "黏土",
        "定格",
        "像素",
        "盲盒",
        "潮玩",
        "等距",
        "漫剧",
        "浮世绘",
        "剪纸",
        "乐高",
        "毛毡",
        "蜡笔",
        "炭笔",
        "敦煌",
        "皮影",
        "折纸",
        "体素",
        "三渲二",
        "线稿",
        "波普",
        "少女漫",
        "连环画",
        "故障",
    ];
    let positive_zh = ZH.iter().any(|n| positive_style_needle_zh(raw, n));
    positive_en || positive_zh
}

/// True when `needle` appears as a positive style request (not after not/no/never/forbidden).
fn positive_style_needle(lower: &str, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    let mut start = 0;
    while let Some(rel) = lower.get(start..).and_then(|s| s.find(&needle)) {
        let abs = start + rel;
        let before = &lower[..abs];
        // Use the current clause (after last . ; ! ? or newline) so "FORBIDDEN: a, b, c"
        // still negates later list items.
        let clause_start = after_last_clause_delim(before);
        let clause = before.get(clause_start..).unwrap_or("").trim_start();
        let negated = clause.contains("not ")
            || clause.contains("no ")
            || clause.contains("never ")
            || clause.contains("without ")
            || clause.contains("forbid")
            || clause.contains("avoid ")
            || clause.contains("禁止")
            || clause.contains("不要")
            || clause.contains("并非")
            || clause.contains("非 ");
        if !negated {
            return true;
        }
        start = abs + needle.len();
    }
    false
}

fn positive_style_needle_zh(raw: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(rel) = raw.get(start..).and_then(|s| s.find(needle)) {
        let abs = start + rel;
        let before = &raw[..abs];
        // CRITICAL: `rfind` returns a byte index; multi-byte delimiters like '。'
        // must advance by `len_utf8()`, not `+ 1` (that panics mid-char).
        let clause_start = after_last_clause_delim(before);
        let clause = before.get(clause_start..).unwrap_or("").trim_start();
        let negated = clause.contains('不')
            || clause.contains('非')
            || clause.contains('无')
            || clause.contains("禁止")
            || clause.contains("避免")
            || clause.contains('别')
            || clause.to_ascii_lowercase().contains("forbid")
            || clause.to_ascii_lowercase().contains("not ");
        if !negated {
            return true;
        }
        start = abs + needle.len();
    }
    false
}

/// Byte index just after the last clause delimiter in `before` (UTF-8 safe).
fn after_last_clause_delim(before: &str) -> usize {
    const DELIMS: &[char] = &['。', '；', '！', '？', '.', ';', '!', '?', '\n'];
    let Some(i) = before.rfind(DELIMS) else {
        return 0;
    };
    let ch = before[i..].chars().next().expect("rfind lands on char start");
    i + ch.len_utf8()
}

/// Compact Style noun-phrase shared by every bible image (cast / set / prop).
pub fn production_style_phrase(user_style: &str) -> String {
    let style = resolve_visual_style(user_style);
    if wants_stylized_non_photoreal(&style) {
        enrich_stylized_style_for_portraits(&style)
            .chars()
            .take(140)
            .collect()
    } else {
        style.chars().take(120).collect()
    }
}

/// Subject-agnostic medium lock — one rendering medium for cast, sets, and props.
/// Portrait sheets add face/identity clauses separately; vacant plates must NOT.
pub fn production_medium_lock_line(user_style: &str) -> String {
    if wants_stylized_non_photoreal(user_style) {
        "Medium: high-detail animation/illustration matching Style — same drawn look for cast, sets, and props; not a flat paper sticker; not photoreal live-action."
            .into()
    } else {
        "Medium: live-action cinematic photography — same film look for cast, sets, and props; not anime, not manga, not cartoon, not illustration."
            .into()
    }
}

/// Canonical production-look lock for bible images (three-view, environment, prop).
///
/// T2I calls do not share a session. Cast three-views, vacant environments, and
/// catalog prop plates are text-to-image: consistency comes from this identical
/// medium contract on every bible prompt. Do not img2img from a look plate —
/// Seedream copies that plate's layout as a background and warps subject scale.
/// Subject content (faces / architecture / objects) stays in the per-asset template.
pub fn production_look_lock(user_style: &str) -> String {
    let phrase = production_style_phrase(user_style);
    let medium = production_medium_lock_line(user_style);
    if wants_stylized_non_photoreal(user_style) {
        format!(
            "PRODUCTION LOOK LOCK (cast + sets + props share ONE medium): {phrase}. {medium} \
Do NOT switch to live-action photoreal. Do NOT flatten into paper-doll cutouts."
        )
    } else {
        format!(
            "PRODUCTION LOOK LOCK (cast + sets + props share ONE medium): {phrase}. {medium} \
Absolutely NOT anime/manga/cartoon/cel-shaded/illustration."
        )
    }
}

/// Short style clause for shot/video image prompts (survives 800-char Z-Image truncate).
///
/// Bible plates (vacant env/prop) must use [`production_look_lock`] instead — the
/// face-finish suffix here would pull empty-set models toward portraits.
pub fn style_prompt_clause(user_style: &str) -> String {
    let look = production_look_lock(user_style);
    if wants_stylized_non_photoreal(user_style) {
        look
    } else {
        format!(
            "{look} Faces: clean healthy skin, clear sharp features (no melt/blur, no dirt or weird makeup)."
        )
    }
}

/// Face finish for character bible sheets — clean healthy skin, sharp features (avoid collapse).
pub const PORTRAIT_FACE_GUIDANCE: &str = "\
Face finish: clean, healthy, evenly lit skin with clear sharp eyes, brows, nose, and mouth. \
Do NOT add dirt, blemishes, rashes, heavy pores, aging wrinkles, or strange makeup. \
Do NOT melt, warp, or heavy-blur the face. Do NOT make a plastic doll or cheap cartoon unless Style asks for animation.";

/// Face finish when the user requested animation / illustration.
pub const PORTRAIT_FACE_GUIDANCE_STYLIZED: &str = "\
Face finish: premium animated-film character design — clean healthy skin tones, clear VOLUME under soft light, sharp readable eyes/brows/nose/mouth, hair strand detail. \
Do NOT add dirt, blemishes, weird makeup, or distorted facial features. \
Do NOT flatten into a paper cutout or blank sticker face. Do NOT render photoreal live-action skin.";

/// Extra face lock for child / teen characters (models often age or over-make kids).
pub const PORTRAIT_CHILD_FACE_GUIDANCE: &str = "\
CHILD FACE LOCK: age-correct child/teen face — smooth healthy skin, soft natural cheeks, age-appropriate features. \
No adult contour makeup, no heavy lipstick/eyeshadow, no aged wrinkles, no dirt/blemishes, no uncanny warped proportions. \
Keep expression natural and clear; identity must stay cute/clean, not grotesque.";

/// Force adults and children to share one rendering style (models often anime-ify kids otherwise).
pub const CAST_STYLE_LOCK: &str = "\
CAST STYLE LOCK: every character of every age must share the SAME Style, shading, materials, and finish. \
Children/teens use age-correct proportions but must NOT become anime/chibi/cartoon/comic while adults stay cinematic.";

/// Cast lock when the production Style is already animation/illustration.
pub const CAST_STYLE_LOCK_STYLIZED: &str = "\
CAST STYLE LOCK: every character of every age must share the SAME premium animation Style with equal detail and volume. \
Children/teens use age-correct proportions but the SAME drawn look as adults — do NOT mix photoreal adults with stylized kids or vice versa.";

/// Compact locks for portrait image prompts (survive 800-char truncate).
pub const PORTRAIT_FACE_SHORT: &str =
    "Clean healthy skin, sharp eyes/brows/nose/mouth; no dirt, no weird makeup, no melt.";

pub const PORTRAIT_FACE_SHORT_STYLIZED: &str =
    "Premium animated-film faces with volume + hair detail; clean skin, sharp features; no dirt/weird makeup; no flat paper-doll.";

pub const PORTRAIT_CHILD_FACE_SHORT: &str =
    "Child/teen: smooth healthy skin, natural soft features; no adult makeup, no aging, no dirt, no warped face.";

pub const CAST_STYLE_SHORT: &str =
    "All ages share the SAME cinematic style (no anime-only kids).";

pub const CAST_STYLE_SHORT_STYLIZED: &str =
    "All ages share the SAME detailed animation Style (volume+fabric folds; never flat cel paper doll; never mix photoreal).";

/// Enrich vague anime/animation presets so image models aim at high-detail film look, not flat cutouts.
pub fn enrich_stylized_style_for_portraits(user_style: &str) -> String {
    let base = resolve_visual_style(user_style);
    if !wants_stylized_non_photoreal(&base) {
        return base;
    }
    let lower = base.to_ascii_lowercase();
    // Already asks for detail/volume — keep as-is (still clip later).
    if lower.contains("volume")
        || lower.contains("fabric fold")
        || lower.contains("high-detail")
        || lower.contains("high detail")
        || lower.contains("theatrical")
        || base.contains("体积")
        || base.contains("高细节")
    {
        return base;
    }
    format!(
        "{base}; theatrical animated-film character design with clear volume, soft painted shading, \
hair strand detail, fabric folds and material contrast — NOT flat paper-doll / empty cel cutout"
    )
}

/// Style text for portrait sheets (may be long; prefer `portrait_image_style_clause` in image prompts).
pub fn portrait_style_for_generation(user_style: &str) -> String {
    let base = enrich_stylized_style_for_portraits(user_style);
    if wants_stylized_non_photoreal(&base) {
        format!("{base}. {PORTRAIT_FACE_GUIDANCE_STYLIZED} {CAST_STYLE_LOCK_STYLIZED}")
    } else {
        format!("{base}. {PORTRAIT_FACE_GUIDANCE} {CAST_STYLE_LOCK}")
    }
}

/// Short Style field for portrait image prompts (ViMax-style: Features first, Style short).
pub fn portrait_style_line_for_image(user_style: &str) -> String {
    let resolved = enrich_stylized_style_for_portraits(user_style);
    resolved.chars().take(120).collect()
}

/// One-line medium lock so Style does not drown Features.
/// Same medium contract as environment / prop bibles ([`production_medium_lock_line`]).
pub fn portrait_medium_lock_line(user_style: &str) -> String {
    production_medium_lock_line(user_style)
}

/// Short style block for three-view image generation (theme/features get priority in the template).
pub fn portrait_image_style_clause(user_style: &str) -> String {
    let style = portrait_style_line_for_image(user_style);
    let medium = portrait_medium_lock_line(user_style);
    format!("{style}. {medium}")
}

/// Prompt fragments for the three-view template — style-aware (do NOT force anime on cinematic).
pub struct PortraitSheetPromptParts {
    pub style_lead: String,
    pub sheet_kind: String,
    pub background: String,
    pub quality_block: String,
    pub medium_lock: String,
}

pub fn portrait_sheet_prompt_parts(user_style: &str) -> PortraitSheetPromptParts {
    let style = enrich_stylized_style_for_portraits(user_style);
    if wants_stylized_non_photoreal(&style) {
        PortraitSheetPromptParts {
            style_lead: format!(
                "STYLE FIRST: {style}. Render as high-detail animation/illustration matching this Style."
            ),
            sheet_kind: "animated/illustrated character turnaround bible".into(),
            background: "Clean studio backdrop (soft gradient white/light-gray) with subtle contact shadow."
                .into(),
            quality_block: "\
QUALITY (high-detail stylized — avoid cheap flat look):
- Match the requested Style with clear VOLUME and form under soft light.
- Rich surface detail: hair strands/layers, fabric folds and seams, material contrast, accessories.
- Soft painted shading + gentle rim/fill light; clean healthy skin; sharp readable facial features.
- FORBIDDEN: flat paper cutout, sticker/chibi low-detail, empty blank faces, muddy blur, dirtied/blemished faces, weird makeup."
                .into(),
            medium_lock: "\
MEDIUM LOCK: keep the requested animation/illustration Style for ALL panels. \
Do NOT switch to photoreal live-action. Do NOT output a cheap flat cartoon sticker."
                .into(),
        }
    } else {
        PortraitSheetPromptParts {
            style_lead: format!(
                "STYLE FIRST: {style}. Render as LIVE-ACTION cinematic cast continuity photos — absolutely NOT anime/manga/cartoon."
            ),
            sheet_kind: "live-action film cast continuity photo board".into(),
            background:
                "Neutral photo studio backdrop with soft cinematic key/fill light and realistic contact shadow."
                    .into(),
            quality_block: "\
QUALITY (live-action cinematic — high detail):
- Photoreal / cinematic film look with realistic human anatomy and proportions.
- Clean healthy skin (not dirty, not heavily pore-mapped, not aged unless Features say so), individual hair strands, fabric weave, seams, accessories.
- Natural photographic lighting and shallow depth cues; sharp eyes, brows, nose, mouth; no strange makeup unless Features ask for it.
- FORBIDDEN: anime, manga, cartoon, chibi, 2D model-sheet, cel shading, flat paper doll, illustration lineart, sticker look, intentional face dirt/blemishes/ugliness."
                .into(),
            medium_lock: "\
MEDIUM LOCK: LIVE-ACTION cinematic continuity photography only. \
Do NOT draw anime/manga/cartoon/2D animation. Do NOT output a stylized illustration unless Style explicitly asks for it."
                .into(),
        }
    }
}

/// Story/world excerpt for portrait THEME LOCK (keep short for image prompt budget).
pub fn portrait_theme_excerpt(script_or_story: &str) -> String {
    let compact: String = script_or_story
        .split_whitespace()
        .take(50)
        .collect::<Vec<_>>()
        .join(" ");
    compact.chars().take(160).collect()
}

/// Extra clause when the character looks like a child (features / name heuristics).
/// Pass `user_style` so animation productions are not forced back to cinematic.
pub fn child_style_lock_if_needed(identifier: &str, features: &str) -> String {
    child_style_lock_if_needed_for_style(identifier, features, "")
}

/// Style-aware child lock (prefer this when Style is known).
pub fn child_style_lock_if_needed_for_style(
    identifier: &str,
    features: &str,
    user_style: &str,
) -> String {
    if !looks_like_child_character(identifier, features) {
        return String::new();
    }
    let face = PORTRAIT_CHILD_FACE_GUIDANCE;
    if wants_stylized_non_photoreal(user_style) {
        format!(
            " Young character: keep child proportions, but render in the SAME animation/illustration Style as adult cast — never switch only kids to a different look. {face}"
        )
    } else {
        format!(
            " Young character: keep child proportions, but render in the SAME Style as adult cast — cinematic character design, not anime/cartoon/chibi. {face}"
        )
    }
}

/// Compact face + age guidance for three-view image prompts.
pub fn portrait_face_clause_for_character(
    identifier: &str,
    features: &str,
    user_style: &str,
) -> String {
    let base = if wants_stylized_non_photoreal(user_style) {
        PORTRAIT_FACE_SHORT_STYLIZED
    } else {
        PORTRAIT_FACE_SHORT
    };
    if looks_like_child_character(identifier, features) {
        format!("{base} {PORTRAIT_CHILD_FACE_SHORT}")
    } else {
        base.to_string()
    }
}

/// Heuristic: child / kid / 小孩 / age cues in id or features.
///
/// Explicit ages win: `28岁` is an adult even if the prose says 少女/girl.
/// Bare `岁` is not a child cue — it appears in every Chinese age string.
pub fn looks_like_child_character(identifier: &str, features: &str) -> bool {
    let blob = format!("{identifier} {features}");
    if let Some(age) = parse_character_age_years(&blob) {
        return age < 18;
    }
    let lower = blob.to_ascii_lowercase();
    const CJK_NEEDLES: &[&str] = &[
        "小孩",
        "儿童",
        "孩子",
        "男童",
        "女童",
        "男孩",
        "女孩",
        "幼儿",
        "少年",
        "少女",
        "小学生",
    ];
    if CJK_NEEDLES.iter().any(|n| blob.contains(n)) {
        return true;
    }
    const EN_WORDS: &[&str] = &[
        "child",
        "kid",
        "kids",
        "boy",
        "girl",
        "toddler",
        "infant",
        "baby",
        "teen",
        "teenager",
        "preteen",
        "schoolgirl",
        "schoolboy",
    ];
    EN_WORDS.iter().any(|w| has_ascii_word(&lower, w))
}

/// Parse `28岁` / `8 岁` / `age 7` / `7-year-old`. None when no numeric age is present.
pub fn parse_character_age_years(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if chars[i] != '岁' || i == 0 {
            continue;
        }
        let mut j = i;
        while j > 0 && chars[j - 1].is_whitespace() {
            j -= 1;
        }
        let end = j;
        while j > 0 && chars[j - 1].is_ascii_digit() {
            j -= 1;
        }
        if j < end {
            if let Ok(n) = chars[j..end].iter().collect::<String>().parse::<u32>() {
                if n > 0 && n < 120 {
                    return Some(n);
                }
            }
        }
    }
    let lower = text.to_ascii_lowercase();
    for pat in ["year-old", "years old", "years-old"] {
        if let Some(pos) = lower.find(pat) {
            if let Some(n) = digits_immediately_before(&lower, pos) {
                return Some(n);
            }
        }
    }
    if let Some(pos) = lower.find("age ") {
        let rest = &lower[pos + 4..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            if n > 0 && n < 120 {
                return Some(n);
            }
        }
    }
    None
}

fn digits_immediately_before(lower: &str, pos: usize) -> Option<u32> {
    let bytes = lower.as_bytes();
    let mut i = pos;
    while i > 0 && (bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b'-') {
        i -= 1;
    }
    let end = i;
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i >= end {
        return None;
    }
    let n = std::str::from_utf8(&bytes[i..end]).ok()?.parse::<u32>().ok()?;
    (n > 0 && n < 120).then_some(n)
}

fn has_ascii_word(haystack: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let h = haystack.as_bytes();
    let w = word.as_bytes();
    let mut i = 0;
    while i + w.len() <= h.len() {
        if &h[i..i + w.len()] == w {
            let before_ok = i == 0 || !h[i - 1].is_ascii_alphabetic();
            let after = i + w.len();
            let after_ok = after >= h.len() || !h[after].is_ascii_alphabetic();
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Truncate at a sentence/clause break so Seedance never sees half a CJK word.
pub fn clip_at_break(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.is_empty() || max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    let taken: String = s.chars().take(max).collect();
    const BREAKS: &[char] = &['。', '！', '？', '；', '!', '?', ';', '，', ',', '、', '.', '—'];
    if let Some((idx, ch)) = taken.char_indices().rev().find(|(_, c)| BREAKS.contains(c)) {
        let keep = idx + ch.len_utf8();
        if taken[..keep].chars().count() > max / 3 {
            return taken[..keep].trim().to_string();
        }
    }
    taken.trim().to_string()
}

/// One-line look for Seedance R2V. Reference images already carry the medium;
/// stacked "not anime" negatives steal attention from the motion beat.
pub fn video_style_clause(user_style: &str) -> String {
    let resolved = resolve_visual_style(user_style);
    if wants_stylized_non_photoreal(&resolved) {
        let short = clip_at_break(&resolved, 80);
        format!("Look: {short}.")
    } else {
        let custom = user_style.trim();
        let lower = custom.to_ascii_lowercase();
        if custom.is_empty()
            || lower == "cinematic"
            || lower.contains("cinematic film look")
        {
            "Look: live-action cinematic photography.".into()
        } else {
            let short = clip_at_break(&resolved, 72);
            format!("Look: live-action cinematic photography, {short}.")
        }
    }
}

/// Clamp a user-provided target into a practical range.
pub fn normalize_target_duration_secs(raw: Option<u32>) -> u32 {
    raw.unwrap_or(DEFAULT_TARGET_DURATION_SECS)
        .clamp(MIN_TARGET_DURATION_SECS, MAX_TARGET_DURATION_SECS)
}

/// Suggested shot count for a **single scene budget** (not the whole film).
///
/// Soft hint only: `ideal` prices beats near the drama clip length (~12s) so a
/// user duration is fillable by **longer clips**, not extra splices. The
/// storyboard LLM still chooses count from beats — this is not a minimum quota.
/// `max_shots` is a hard cap for post-LLM truncation, with slack for speech
/// that cannot fit one clip (吞字).
pub fn suggested_shot_count(bounds: ClipBounds, budget_secs: u32) -> (u32, u32) {
    let budget = budget_secs.max(bounds.min_secs());
    let min_to_fill = budget.div_ceil(bounds.max_secs());
    let beat = bounds.typical_beat_secs().max(1);
    // Floor, not round-up: leftover seconds lengthen packed clips instead of
    // inventing another splice. `min_to_fill` still raises the count when even
    // max-length clips cannot reach the budget.
    let ideal = (budget / beat).max(min_to_fill).clamp(1, 6);
    // Slack: two extra shots so a long line can still split instead of 吞字.
    // Cap stays tight so leftover seconds are not spent inventing filler cuts.
    let max_shots = min_to_fill
        .max(ideal)
        .saturating_add(2)
        .min(budget / bounds.min_secs())
        .clamp(1, 6);
    let ideal = if ideal.saturating_mul(bounds.max_secs()) < budget {
        min_to_fill.min(max_shots)
    } else {
        ideal.min(max_shots)
    };
    (ideal.max(1), max_shots)
}

/// Normalize a scene-level BGM brief into a Seedance `(music)` caption.
///
/// Empty → [`DEFAULT_SCENE_BGM_PAREN`]. Already-parenthesized text is kept
/// (length-clamped). Plain prose is wrapped in parentheses.
pub fn format_scene_bgm_paren(brief: &str) -> String {
    let t = brief.trim();
    if t.is_empty() {
        return DEFAULT_SCENE_BGM_PAREN.to_string();
    }
    let inner = if t.starts_with('(') && t.ends_with(')') && t.len() >= 2 {
        t[1..t.len() - 1].trim()
    } else {
        t
    };
    let clipped: String = inner.chars().take(220).collect();
    format!("({clipped})")
}

/// Pull the first `(…)` music caption from storyboard `audio_desc` values, if any.
pub fn extract_bgm_paren_from_audio_descs<'a, I>(audio_descs: I) -> Option<String>
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    for audio in audio_descs {
        let Some(raw) = audio.map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        if let Some(paren) = first_paren_span(raw) {
            // Skip tiny non-music parentheses (e.g. emotion cues).
            if paren.chars().count() >= 12 {
                return Some(format_scene_bgm_paren(&paren));
            }
        }
    }
    None
}

fn first_paren_span(s: &str) -> Option<String> {
    let start = s.find('(')?;
    let end = s[start + 1..].find(')')? + start + 1;
    if end <= start + 1 {
        return None;
    }
    Some(s[start..=end].to_string())
}

/// Resolve the canonical scene BGM caption used by every shot in the scene.
pub fn resolve_scene_bgm_paren(
    existing_brief: Option<&str>,
    audio_descs: &[Option<&str>],
) -> String {
    if let Some(brief) = existing_brief.map(str::trim).filter(|s| !s.is_empty()) {
        return format_scene_bgm_paren(brief);
    }
    extract_bgm_paren_from_audio_descs(audio_descs.iter().copied())
        .unwrap_or_else(|| DEFAULT_SCENE_BGM_PAREN.to_string())
}

/// Split a film-level target across N scenes (each ≥ one model-minimum clip).
pub fn allocate_scene_budgets(
    bounds: ClipBounds,
    total_secs: u32,
    scene_count: usize,
) -> Vec<u32> {
    let n = scene_count.max(1);
    let total = normalize_target_duration_secs(Some(total_secs));
    let base = (total / n as u32).max(bounds.min_secs());
    let mut budgets = vec![base; n];
    let mut rem = total.saturating_sub(base.saturating_mul(n as u32));
    for b in &mut budgets {
        if rem == 0 {
            break;
        }
        *b = b.saturating_add(1);
        rem -= 1;
    }
    budgets
}

/// Suggested scene count for a whole film (idea/novel multi-scene).
pub fn suggested_scene_count(bounds: ClipBounds, total_secs: u32) -> (u32, u32) {
    let total = normalize_target_duration_secs(Some(total_secs));
    // ~10–12s per scene for short drama.
    let ideal = ((total + 10) / 12).clamp(1, 5);
    let max_scenes = (total / bounds.min_secs()).clamp(1, 6);
    (ideal.min(max_scenes), max_scenes)
}

/// Dominant natural language for planning narrative outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLanguage {
    Chinese,
    English,
    Unspecified,
}

/// Detect output language from user creative text (idea / script / novel / requirement).
///
/// Prefers Chinese when there is a meaningful amount of CJK, even if English style
/// presets or schema text are mixed in.
pub fn detect_output_language(samples: &[&str]) -> OutputLanguage {
    let mut cjk = 0u32;
    let mut latin = 0u32;
    for sample in samples {
        for ch in sample.chars() {
            if is_cjk_speech_char(ch) {
                cjk += 1;
            } else if ch.is_ascii_alphabetic() {
                latin += 1;
            }
        }
    }
    if cjk == 0 && latin == 0 {
        return OutputLanguage::Unspecified;
    }
    // Strong Chinese signal, or CJK at least half as common as Latin letters.
    if cjk >= 6 || (cjk > 0 && cjk.saturating_mul(2) >= latin) {
        OutputLanguage::Chinese
    } else {
        OutputLanguage::English
    }
}

/// Hard language lock for planning LLM system/user prompts.
pub fn language_lock_clause(lang: OutputLanguage) -> String {
    match lang {
        OutputLanguage::Chinese => "\
[OUTPUT_LANGUAGE — MUST FOLLOW]
The user's creative input is predominantly Chinese (中文).
Write ALL natural-language narrative content in 简体中文: story, script, scene headings, action lines, dialogue, character names when originally Chinese, visual_desc, audio_desc, motion_desc, environment/prop descriptions, and any other prose fields.
JSON keys, enum tokens (large|medium|small), and cam_idx/idx numbers stay as the schema requires (English keys OK).
Do NOT default to English prose just because this instruction is written in English."
            .into(),
        OutputLanguage::English => "\
[OUTPUT_LANGUAGE — MUST FOLLOW]
The user's creative input is predominantly English.
Write ALL natural-language narrative content in English: story, script, dialogue, visual_desc, audio_desc, and other prose fields.
JSON keys and enum tokens stay as the schema requires."
            .into(),
        OutputLanguage::Unspecified => "\
[OUTPUT_LANGUAGE — MUST FOLLOW]
Match the language of the user's creative input for ALL natural-language narrative fields (story, script, dialogue, descriptions).
JSON keys and enum tokens stay as the schema requires. Do not translate the user's language into another language."
            .into(),
    }
}

/// Detect language from samples and return the lock block.
pub fn language_lock_for_sources(samples: &[&str]) -> String {
    language_lock_clause(detect_output_language(samples))
}

/// Detect language from a single planning user message (may include XML tags).
pub fn language_lock_for_text(text: &str) -> String {
    language_lock_for_sources(&[text])
}

/// Prepend a language lock derived from `sources` onto a requirement string.
pub fn with_language_lock(base: &str, sources: &[&str]) -> String {
    let lock = language_lock_for_sources(sources);
    let base = base.trim();
    if base.is_empty() {
        lock
    } else if base.contains("[OUTPUT_LANGUAGE") {
        base.to_string()
    } else {
        format!("{lock}\n\n{base}")
    }
}

/// True when the session has an explicit finished-film budget (legacy / API).
pub fn has_explicit_duration_budget(target_secs: Option<u32>) -> bool {
    target_secs.is_some_and(|s| s > 0)
}

/// Spoken-payload budget for one clip, in the user's terms (chars / words).
///
/// Derived from the selected model's window so a longer-clip model really does
/// buy longer lines instead of the planner guessing a vendor number.
pub fn speech_budget_line(bounds: ClipBounds) -> String {
    let clip_max = bounds.max_secs();
    let clip_min = bounds.min_secs();
    let speak_window = clip_max.saturating_sub(SPEECH_LEAD_SECS + SPEECH_TAIL_SECS);
    let max_cjk_chars = (speak_window as f32 * SPEECH_CJK_CHARS_PER_SEC).floor() as u32;
    let max_en_words = (speak_window as f32 * SPEECH_EN_WORDS_PER_SEC).floor() as u32;
    let action_max = bounds.glance_secs();
    let pack = bounds.pack_target_secs();
    format!(
        "Dialogue MUST finish inside the same shot's {clip_min}–{clip_max}s clip: \
Chinese ~{SPEECH_CJK_CHARS_PER_SEC} chars/sec or English ~{SPEECH_EN_WORDS_PER_SEC} words/sec, \
leave ~{SPEECH_LEAD_SECS}s lead-in and ~{SPEECH_TAIL_SECS}s tail after the last word, then land on a \
visible reaction/action beat (no empty hold); hard max ≲{max_cjk_chars} Chinese chars / \
≲{max_en_words} English words. Prefer packing a line + reaction into one ~{pack}s clip when the \
spoken payload still fits. A single sit/stand/glance stays {clip_min}–{action_max}s \
(do not pad it to {clip_max}s). If the line cannot be spoken clearly inside {clip_max}s, SPLIT \
or shorten — never rush (吞字)."
    )
}

/// Clip-length rules for the planning prompts (storyboard + shot decompose).
///
/// Planning runs **before** the renderer allocates clip lengths from the scene
/// budget, so any absolute second a planner writes would contradict the clip it
/// lands on — the bug this rule exists to prevent. Planners order their beats;
/// the renderer lays them on the real timeline.
pub fn clip_length_rules(bounds: ClipBounds) -> String {
    let (clip_min, clip_max) = (bounds.min_secs(), bounds.max_secs());
    let pack = bounds.pack_target_secs();
    let glance = bounds.glance_secs();
    format!(
        "Each shot renders as ONE clip of {clip_min}–{clip_max}s (typically ~{pack}s when it holds \
2–3 related story beats). ONE ROW = ONE VIDEO: each storyboard JSON object is one generated file. \
Slice rows by NARRATIVE (a dramatic unit: line+reaction, a turn, a payoff), never by tripod position. \
A reverse-angle / insert / push-in is coverage of the SAME beat — write CUT TO inside that row, do not \
open a new row because the camera moved. Pack a line + reaction + a small action into the SAME row when \
they are the same story unit and the spoken payload still fits. A single glance/sit/stand stays \
{clip_min}–{glance}s. Use a new row only when the story itself moves on, or the next events cannot fit \
without rushing speech (吞字). NEVER write absolute seconds or timecodes (no \"0-4s:\", no \"4-7s\", no \
\"前3秒\"): the renderer decides the clip length and would contradict them. When a clip contains \
consecutive events, write them in order (\"…；然后…\" / \"…, then …\", and \"CUT TO\" on a camera \
change) and let the renderer pace them. Do not pad. Do not rush speech (吞字)."
    )
}

fn film_pacing_model_decides_block(bounds: ClipBounds) -> String {
    let (clip_min, clip_max) = (bounds.min_secs(), bounds.max_secs());
    let pack = bounds.pack_target_secs();
    format!(
        "[VIDEO_PACING — MUST FOLLOW]\n\
         - Do NOT target a fixed finished runtime. Let scene count and story length follow the idea \
(ViMax-style: the model decides duration).\n\
         - Shot count follows story beats — do not pad or split to hit a quota. Prefer fewer, richer \
clips (~{pack}s) over many 5–8s fragments. Honor an explicit user duration/scene/shot count.\n\
         - Each rendered shot clip is {clip_min}–{clip_max}s (hard range of the selected video model). \
Pack 2–3 related story beats into one clip when they are the same narrative unit and speech still fits; \
{clip_max}s only when speech or a continuous action needs it. Do not start a new clip because the camera moved.\n\
         - Speech pacing (clear, language-aware): Chinese ~{SPEECH_CJK_CHARS_PER_SEC} chars/sec, \
English ~{SPEECH_EN_WORDS_PER_SEC} words/sec; leave ~{SPEECH_LEAD_SECS}s before speech starts and \
~{SPEECH_TAIL_SECS}s after the last word — then land on a visible reaction/action beat (no empty hold). \
If a line cannot finish clearly inside {clip_max}s, SPLIT or shorten — never rush (吞字).\n\
         [DIRECTOR_DENSITY — MUST FOLLOW]\n\
         - Short-film information density: every clip must advance plot, relationship, OR a \
distinct visual surprise. Forbid repeated establishing shots and filler pauses.\n\
         - Write a mental beat sheet before prose: hook → escalation → turn → payoff. Each scene needs \
at least one concrete conflict beat and one filmable visual motif that can recur.\n\
         - Prefer show-don't-tell actions over long explanatory dialogue; keep spoken lines short and punchy.\n\
         [MUSIC_ARC — MUST FOLLOW]\n\
         - Plan one continuous underscore mood for the film (motif / tempo / intensity arc). \
Adjacent shots must feel like one soundtrack, not a new track per cut."
    )
}

/// Beat-matched clip guidance: length follows content, neither a max-length pad
/// nor a cut quota.
fn beat_matched_pacing_lines(bounds: ClipBounds) -> String {
    let (clip_min, clip_max) = (bounds.min_secs(), bounds.max_secs());
    let pack = bounds.pack_target_secs();
    let typical = bounds.typical_beat_secs();
    let glance = bounds.glance_secs();
    format!(
        "         - **BEAT-MATCHED PACING** (not a shot-count quota):\n\
           * Clip length follows the beats it holds — a glance/reaction may be {clip_min}s; \
a spoken line plus its reaction typically ~{typical}s; pack 2–3 related beats toward ~{pack}s \
when the spoken payload still fits.\n\
           * Prefer fewer clips. Leftover seconds should lengthen a packed clip (up to {pack}s), \
not invent another shot.\n\
           * Do NOT stretch thin content to {clip_max}s. Do NOT split one story beat into \
micro-cuts just to raise shot count, and do NOT start a new clip because the camera \
moved (reverse / insert / push-in belong inside the same row as CUT TO).\n\
           * Empty holds, slow pans, and \"character looks around\" are FORBIDDEN.\n\
           * A single sit/stand/walk with no dialogue stays {clip_min}–{glance}s — \
do not pad it to {clip_max}s.\n\
           * Never write absolute seconds or timecodes into a shot description \
(no \"0-3s:\" / \"前3秒\"): the renderer allocates each clip's length and lays consecutive \
beats on that timeline itself. Order the beats instead.\n\
           * Language-aware delivery: Chinese ~{SPEECH_CJK_CHARS_PER_SEC} chars/sec, \
English ~{SPEECH_EN_WORDS_PER_SEC} words/sec (clear — not rushed, not drawn-out). \
If a line cannot finish clearly inside {clip_max}s, SPLIT or shorten — never 吞字.\n"
    )
}

fn scene_pacing_model_decides_block(
    bounds: ClipBounds,
    scene_idx: usize,
    scene_count: usize,
) -> String {
    let scene_num = scene_idx + 1;
    let (clip_min, clip_max) = (bounds.min_secs(), bounds.max_secs());
    let speak_window_max = clip_max.saturating_sub(SPEECH_LEAD_SECS + SPEECH_TAIL_SECS);
    let max_cjk_chars = (speak_window_max as f32 * SPEECH_CJK_CHARS_PER_SEC).floor() as u32;
    let max_en_words = (speak_window_max as f32 * SPEECH_EN_WORDS_PER_SEC).floor() as u32;
    let silent_beat_max = bounds.pack_target_secs();
    let cross_scene = cross_scene_opening_line(scene_idx, scene_count);
    let beat_pacing = beat_matched_pacing_lines(bounds);
    let continuity = shot_continuity_lines();
    let density = director_density_lines();
    format!(
        "[VIDEO_PACING — MUST FOLLOW]\n\
         - This is scene {scene_num}/{scene_count}. Shot count follows the scene script — do NOT pad or \
truncate to hit a runtime quota. Prefer packing related beats into fewer clips.\n\
         - Each shot clip is {clip_min}–{clip_max}s (selected video model).\n\
{beat_pacing}\
         - Plan visual beats AND audio beats together: dialogue/SFX in audio_desc MUST finish inside the \
same shot's duration — no unfinished lines, mid-sentence cuts, swallowed syllables (吞字), or \
\"and then…\" requiring another clip.\n\
         - EVERY shot MUST have a non-empty audio_desc (spoken lines and/or ambient SFX+BGM). Never leave audio_desc null.\n\
         - Speech budget per shot: hard max ≲ {max_cjk_chars} Chinese chars / ≲ {max_en_words} English words \
for a {clip_max}s clip, after reserving ~{SPEECH_LEAD_SECS}s lead-in + ~{SPEECH_TAIL_SECS}s tail. \
If a speech beat is longer, you MUST SPLIT into another shot (or shorten the line) — never cram past \
the {clip_max}s model limit.\n\
         - After the last spoken word, land on a clear reaction/action beat within ~{SPEECH_TAIL_SECS}s — \
do NOT pad with empty static holds or \"磨叽\" waiting.\n\
         - Prefer purposeful coverage that raises information density; silent/action beats may be \
{clip_min}–{silent_beat_max}s with rich in-shot motion. For dialogue, pack the line and its reaction \
into the SAME row when they are one conversation beat and the spoken payload still fits.\n\
         - NARRATIVE FIRST: a new storyboard row is a new story unit (turn, time jump, new conflict), \
never a new tripod position. Reverse / insert / angle change that still belongs to this beat stays \
in this row as CUT TO (one row = one generated video). cam_idx is the opening setup of the row, not \
a reason to emit another object.\n\
{continuity}\
         {cross_scene}\
{density}\
         [BGM_CONTINUITY — MUST FOLLOW]\n\
         - All shots in THIS SCENE share ONE continuous underscore: same motif, tempo feel, instrumentation, \
and volume intention. Write the same BGM phrase into every audio_desc (or a clear \"same underscore as prior shot\"). \
Do NOT invent a new music style per shot — abrupt BGM changes between cuts are forbidden."
    )
}

/// Continuity rules the renderer can actually deliver.
///
/// The renderer feeds shot N's last frame to shot N+1 as reference Image 1.
/// Telling the LLM that *every* pair must "open from the previous ending state"
/// makes each new shot re-stage that pose before it moves, which reads as a
/// stutter at the splice. Identity continuity always holds; **compositional**
/// continuity is only correct when the camera does not change.
fn shot_continuity_lines() -> String {
    "         - SHOT CONTINUITY — these rules apply BETWEEN storyboard rows (file splices), \
not as a reason to add a row. Camera moves inside a row are CUT TO in that row's visual_desc.\n\
           * IDENTITY (always): cast faces, wardrobe, hair, props, set dressing, time of day, \
weather, and lighting mood carry over unchanged between adjacent shots unless the story explicitly \
changes them.\n\
           * SAME opening cam_idx = SAME TAKE into the next *row*: that next file continues this \
row's exit framing — open exactly where this row ended and move on from there. Keep each named \
person on the SAME screen side (left/right).\n\
           * NEW opening cam_idx = A CUT into the next *row*: do NOT replay or re-describe this \
row's ending pose. Open on the next action already in progress from the new angle. Keep the \
180-degree axis: who is screen-left vs screen-right MUST match the previous row unless THIS row \
explicitly writes 反打 / 过肩 / reverse as an in-file CUT TO. Flipping left/right across a file \
splice looks like a teleport.\n\
           * A reverse angle of the SAME beat is NOT a new row — pack it in this row as CUT TO.\n"
        .to_string()
}

fn cross_scene_opening_line(scene_idx: usize, scene_count: usize) -> String {
    if scene_idx == 0 || scene_count <= 1 {
        return String::new();
    }
    "         - CROSS-SCENE OPENING: This is NOT the first scene. Carry cast identity / wardrobe / \
lighting mood from the previous scene when the story is consistent (camera or location may change). \
The renderer feeds the previous ending frame as continuity Image 1 for IDENTITY only — do NOT restage \
or replay that pose as the opening picture. Start the new scene's action already in progress. \
Re-staging the previous last frame is what makes the scene join look frozen.\n"
        .into()
}

fn director_density_lines() -> String {
    "         [DIRECTOR_DENSITY — MUST FOLLOW]\n\
         - Each clip must change something the audience can see or hear (new info, new emotion, new action). \
Ban back-to-back redundant wide establishes and repeated \"looks around slowly\" beats.\n\
         - **NARRATIVE FIRST / ONE ROW = ONE VIDEO**: Slice JSON objects by story unit, not by camera. \
Each object is one generated file. Pack 2–3 *related* story beats into that SAME object when they are \
one dramatic unit AND the spoken payload still fits:\n\
           * She opens the door, sees him, and the line lands — one row (even if you CUT TO his face)\n\
           * Line + the other person's reaction, reverse included — one row, write CUT TO\n\
           * A glance that is the whole beat stays its own short row\n\
           A new row is a new story beat (turn, time jump, new conflict) or a line that would 吞字. \
Never emit a new row because the camera moved. Never emit a micro-shot the renderer would delete.\n"
        .into()
}

/// Scene-level pacing when the user did not set a finished-film duration.
pub fn enrich_requirement_for_scene_model_decides(
    bounds: ClipBounds,
    user_requirement: &str,
    scene_idx: usize,
    scene_count: usize,
) -> String {
    let base = user_requirement.trim();
    let base = strip_duration_constraint_blocks(base);
    let base = with_language_lock(&base, &[&base, user_requirement]);
    format!(
        "{base}\n\n{}",
        scene_pacing_model_decides_block(bounds, scene_idx, scene_count)
    )
}

/// Film-level constraints (develop story / write multi-scene script).
///
/// `None` / `0` means ViMax-style: the model decides length from the story.
pub fn enrich_requirement_for_film(
    bounds: ClipBounds,
    user_requirement: &str,
    target_secs: Option<u32>,
) -> String {
    let base = with_language_lock(user_requirement, &[user_requirement]);
    if !has_explicit_duration_budget(target_secs) {
        return format!("{base}\n\n{}", film_pacing_model_decides_block(bounds));
    }
    let (clip_min, clip_max) = (bounds.min_secs(), bounds.max_secs());
    let pack = bounds.pack_target_secs();
    let target = normalize_target_duration_secs(target_secs);
    let (ideal_scenes, max_scenes) = suggested_scene_count(bounds, target);
    let per_scene = (target / ideal_scenes.max(1)).max(clip_min);
    let block = format!(
        "[VIDEO_DURATION_CONSTRAINTS — MUST FOLLOW]\n\
         - Target finished film length ≈ {target} seconds TOTAL (hard planning budget).\n\
         - Prefer about {ideal_scenes} scenes (hard upper bound {max_scenes}); ~{per_scene}s per scene.\n\
         - Each rendered shot clip is {clip_min}–{clip_max}s (hard range of the selected video model).\n\
         - Keep the whole story compact so total scenes × shots × {clip_min}s stays near {target}s.\n\
         - Do NOT write more plot/dialogue than can be spoken and shown inside that total runtime.\n\
         - Speech pacing (clear, language-aware): Chinese ~{SPEECH_CJK_CHARS_PER_SEC} chars/sec, \
English ~{SPEECH_EN_WORDS_PER_SEC} words/sec; leave ~{SPEECH_LEAD_SECS}s before speech starts and \
~{SPEECH_TAIL_SECS}s after the last word — then land on a visible reaction/action beat (no empty hold).\n\
         [DIRECTOR_DENSITY — MUST FOLLOW]\n\
         - Short-film information density: every clip (typically ~{pack}s, packing 2–3 related beats) must advance plot, \
relationship, OR a distinct visual surprise. Forbid repeated establishing shots and filler pauses.\n\
         - Write a mental beat sheet before prose: hook → escalation → turn → payoff. Each scene needs \
at least one concrete conflict beat and one filmable visual motif that can recur.\n\
         - Prefer show-don't-tell actions over long explanatory dialogue; keep spoken lines short and punchy.\n\
         [MUSIC_ARC — MUST FOLLOW]\n\
         - Plan one continuous underscore mood for the film (motif / tempo / intensity arc). \
Adjacent shots must feel like one soundtrack, not a new track per cut."
    );
    format!("{base}\n\n{block}")
}

/// Scene-level constraints for storyboard design (budget already allocated).
pub fn enrich_requirement_for_scene(
    bounds: ClipBounds,
    user_requirement: &str,
    scene_budget_secs: u32,
    scene_idx: usize,
    scene_count: usize,
    film_total_secs: u32,
) -> String {
    let (clip_min, clip_max) = (bounds.min_secs(), bounds.max_secs());
    let budget = scene_budget_secs.max(clip_min);
    let (ideal, max_shots) = suggested_shot_count(bounds, budget);
    let per_shot = clip_duration_secs(bounds, Some(budget), ideal as usize);
    // Speakable window inside one clip (lead + tail reserved).
    let speak_window_max = clip_max.saturating_sub(SPEECH_LEAD_SECS + SPEECH_TAIL_SECS);
    let speak_window_typical =
        per_shot.saturating_sub(SPEECH_LEAD_SECS + SPEECH_TAIL_SECS);
    let max_cjk_chars =
        (speak_window_max as f32 * SPEECH_CJK_CHARS_PER_SEC).floor() as u32;
    let max_en_words =
        (speak_window_max as f32 * SPEECH_EN_WORDS_PER_SEC).floor() as u32;
    let per_shot_cjk =
        (speak_window_typical as f32 * SPEECH_CJK_CHARS_PER_SEC).floor() as u32;
    let per_shot_en =
        (speak_window_typical as f32 * SPEECH_EN_WORDS_PER_SEC).floor() as u32;
    let silent_beat_max = bounds.pack_target_secs();
    let beat_pacing = beat_matched_pacing_lines(bounds);
    let continuity = shot_continuity_lines();
    let density = director_density_lines();
    let base = user_requirement.trim();
    // Strip a previous film-level block so we don't double-confuse the LLM with two totals.
    let base = strip_duration_constraint_blocks(base);
    let base = with_language_lock(&base, &[&base, user_requirement]);
    let cross_scene = cross_scene_opening_line(scene_idx, scene_count);
    let block = format!(
        "[VIDEO_DURATION_CONSTRAINTS — MUST FOLLOW]\n\
         - This is scene {scene_num}/{scene_count} of a film targeting ≈ {film_total_secs}s total.\n\
         - THIS SCENE budget ≈ {budget} seconds of finished video (NOT the whole film).\n\
         - Each shot clip is {clip_min}–{clip_max}s (selected video model).\n\
{beat_pacing}\
         - Follow the scene's beats; a budget this size often lands around {ideal} shots \
(~{pack}s each). HARD UPPER BOUND: {max_shots} shots — merge if you would exceed it. Do not invent \
filler shots to hit {ideal}; leftover seconds should lengthen packed clips, not add cuts.\n\
         - Plan visual beats AND audio beats together: dialogue/SFX in audio_desc MUST finish inside the \
same shot's duration — no unfinished lines, mid-sentence cuts, swallowed syllables (吞字), or \
\"and then…\" requiring another clip.\n\
         - EVERY shot MUST have a non-empty audio_desc (spoken lines and/or ambient SFX+BGM). Never leave audio_desc null.\n\
         - Speech budget per shot: keep spoken Chinese ≲ {per_shot_cjk} chars / English ≲ {per_shot_en} words \
(hard max ≲ {max_cjk_chars} Chinese chars / ≲ {max_en_words} English words for a {clip_max}s clip, \
after reserving ~{SPEECH_LEAD_SECS}s lead-in + ~{SPEECH_TAIL_SECS}s tail). \
If a speech beat is longer, you MUST SPLIT into another shot (or shorten the line) — never cram past \
the {clip_max}s model limit.\n\
         - After the last spoken word, land on a clear reaction/action beat within ~{SPEECH_TAIL_SECS}s — \
do NOT pad with empty static holds or \"磨叽\" waiting.\n\
         - Prefer purposeful coverage that raises information density; silent/action beats may be \
{clip_min}–{silent_beat_max}s with rich in-shot motion. For dialogue, pack the line and its reaction \
into the SAME row when they are one conversation beat and the spoken payload still fits.\n\
         - NARRATIVE FIRST: a new storyboard row is a new story unit (turn, time jump, new conflict), \
never a new tripod position. Reverse / insert / angle change that still belongs to this beat stays \
in this row as CUT TO (one row = one generated video). cam_idx is the opening setup of the row, not \
a reason to emit another object.\n\
{continuity}\
         {cross_scene}\
         - If you would create more than {max_shots} shots, merge beats instead.\n\
{density}\
         [BGM_CONTINUITY — MUST FOLLOW]\n\
         - All shots in THIS SCENE share ONE continuous underscore: same motif, tempo feel, instrumentation, \
and volume intention. Write the same BGM phrase into every audio_desc (or a clear \"same underscore as prior shot\"). \
Do NOT invent a new music style per shot — abrupt BGM changes between cuts are forbidden.",
        scene_num = scene_idx + 1,
        pack = bounds.pack_target_secs(),
        cross_scene = cross_scene,
        beat_pacing = beat_pacing,
        continuity = continuity,
        density = density,
    );
    format!("{base}\n\n{block}")
}

/// Single-scene script2video (whole target = this scene).
pub fn enrich_requirement_for_planning(
    bounds: ClipBounds,
    user_requirement: &str,
    target_secs: Option<u32>,
) -> String {
    if !has_explicit_duration_budget(target_secs) {
        return enrich_requirement_for_scene_model_decides(bounds, user_requirement, 0, 1);
    }
    let target = normalize_target_duration_secs(target_secs);
    enrich_requirement_for_scene(bounds, user_requirement, target, 0, 1, target)
}

fn strip_duration_constraint_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in s.lines() {
        let t = line.trim();
        if t.starts_with("[VIDEO_DURATION_CONSTRAINTS")
            || t.starts_with("[VIDEO_PACING")
            || t.starts_with("[OUTPUT_LANGUAGE")
            || t.starts_with("[DIRECTOR_DENSITY")
            || t.starts_with("[MUSIC_ARC")
            || t.starts_with("[BGM_CONTINUITY")
        {
            skipping = true;
            continue;
        }
        if skipping {
            // End skip when we hit a blank line after the block, or a new [SECTION]
            if t.is_empty() {
                skipping = false;
            } else if t.starts_with('[') && t.ends_with(']') {
                skipping = false;
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Per-shot clip duration for render: spread scene budget across shots.
pub fn clip_duration_secs(
    bounds: ClipBounds,
    target_total: Option<u32>,
    shot_count: usize,
) -> u32 {
    let n = shot_count.max(1) as u32;
    let target = normalize_target_duration_secs(target_total);
    bounds.clamp_secs(target / n)
}

/// Allocate per-shot durations that sum as close as possible to the scene budget
/// while respecting the selected model's clip window.
///
/// Prefer this over repeating [`clip_duration_secs`] when shot lengths should vary
/// slightly so the last clip absorbs remainder seconds.
///
/// Soft-landing ([`SHOT_SPLICE_TAIL_PADDING_SECS`]) is reserved from `target` first
/// so the final sum stays near the user budget instead of overshooting by
/// `padding × shot_count`.
pub fn allocate_clip_durations(
    bounds: ClipBounds,
    target_total: Option<u32>,
    shot_count: usize,
) -> Vec<u32> {
    let n = shot_count.max(1);
    let target = normalize_target_duration_secs(target_total);
    let (fit_budget, will_pad) = fit_budget_reserving_splice_padding(bounds, target, n);
    let base = clip_duration_secs(bounds, Some(fit_budget), n);
    let mut durs = vec![base; n];
    // Cap total near fit budget when base*n would overshoot (min-clip floor).
    let planned: u32 = base.saturating_mul(n as u32);
    if planned <= fit_budget {
        let mut rem = fit_budget.saturating_sub(planned);
        for d in durs.iter_mut().rev() {
            if rem == 0 {
                break;
            }
            let room = bounds.max_secs().saturating_sub(*d);
            let add = rem.min(room);
            *d += add;
            rem -= add;
        }
    }
    reapply_splice_tail_padding(bounds, &mut durs, fit_budget, target, will_pad);
    durs
}

/// Carve soft-landing room out of the advertised target when there is enough
/// headroom above `min_clip × shots`.
fn fit_budget_reserving_splice_padding(
    bounds: ClipBounds,
    target: u32,
    shot_count: usize,
) -> (u32, bool) {
    let n = shot_count.max(1) as u32;
    let pad_total = SHOT_SPLICE_TAIL_PADDING_SECS.saturating_mul(n);
    let min_content = bounds.min_secs().saturating_mul(n);
    if target >= pad_total.saturating_add(min_content) {
        (target - pad_total, true)
    } else {
        (target, false)
    }
}

/// Add splice-tail padding to finalized per-shot durations (≤ model max).
fn apply_shot_splice_tail_padding(bounds: ClipBounds, durs: &mut [u32]) {
    for d in durs.iter_mut() {
        *d = bounds.saturating_add_within(*d, SHOT_SPLICE_TAIL_PADDING_SECS);
    }
}

/// Re-apply reserved soft-landing only when content still fits the pre-pad budget
/// (or fill leftover seconds toward `target` without exceeding it).
fn reapply_splice_tail_padding(
    bounds: ClipBounds,
    durs: &mut [u32],
    fit_budget: u32,
    target: u32,
    will_pad: bool,
) {
    if !will_pad || durs.is_empty() {
        return;
    }
    let sum: u32 = durs.iter().sum();
    if sum <= fit_budget {
        apply_shot_splice_tail_padding(bounds, durs);
        return;
    }
    if sum >= target {
        return;
    }
    // Content overshot the reserved fit budget but is still under target — spend
    // only the remaining spare seconds on soft landings.
    let mut rem = target - sum;
    for d in durs.iter_mut() {
        if rem == 0 {
            break;
        }
        let room = bounds
            .max_secs()
            .saturating_sub(*d)
            .min(SHOT_SPLICE_TAIL_PADDING_SECS)
            .min(rem);
        *d += room;
        rem -= room;
    }
}

/// Estimate spoken seconds from `audio_desc` (dialogue only).
///
/// Prefers quoted / braced dialogue payloads so stage directions, SFX, and BGM
/// do not dominate. Uses language-aware rates (CJK chars vs English words).
pub fn estimate_speech_secs(audio_desc: &str) -> u32 {
    let t = audio_desc.trim();
    if t.is_empty() {
        return 0;
    }
    let spoken = extract_spoken_payload(t);
    let mut cjk = 0u32;
    let mut en_words = 0u32;
    let mut in_word = false;
    for ch in spoken.chars() {
        if is_cjk_speech_char(ch) {
            cjk += 1;
            in_word = false;
        } else if ch.is_ascii_alphabetic() {
            if !in_word {
                en_words += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    let cjk_secs = if cjk == 0 {
        0.0
    } else {
        cjk as f32 / SPEECH_CJK_CHARS_PER_SEC
    };
    let en_secs = if en_words == 0 {
        0.0
    } else {
        en_words as f32 / SPEECH_EN_WORDS_PER_SEC
    };
    // Mixed lines: take the sum (both streams rarely overlap).
    (cjk_secs + en_secs).ceil() as u32
}

/// Prefer dialogue inside 「」 / “” / "" / `{…}`. Unquoted ambient/BGM/SFX is not speech.
fn extract_spoken_payload(audio_desc: &str) -> String {
    let mut chunks: Vec<String> = Vec::new();
    let chars: Vec<char> = audio_desc.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let open = chars[i];
        let close = match open {
            '「' => Some('」'),
            '“' => Some('”'),
            '"' => Some('"'),
            '{' => Some('}'),
            _ => None,
        };
        if let Some(close) = close {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != close {
                i += 1;
            }
            if i > start {
                chunks.push(chars[start..i].iter().collect());
            }
            if i < chars.len() {
                i += 1; // consume closer
            }
            continue;
        }
        i += 1;
    }
    if !chunks.is_empty() {
        return chunks.join(" ");
    }
    let stripped = strip_non_speech_markup(audio_desc);
    if looks_like_ambient_only(&stripped) {
        String::new()
    } else {
        stripped
    }
}

/// Drop `(BGM)` / `（…）` / `<SFX>` so they never inflate speech estimates.
fn strip_non_speech_markup(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '(' => Some(')'),
            '（' => Some('）'),
            '<' => Some('>'),
            _ => None,
        };
        if let Some(close) = close {
            i += 1;
            while i < chars.len() && chars[i] != close {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn looks_like_ambient_only(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if text_looks_like_dialogue(t) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    t.contains("底噪")
        || t.contains("背景音乐")
        || t.contains("拟音")
        || lower.contains("room tone")
        || lower.contains("underscore")
        || lower.contains("ambient")
        || lower.contains("bgm")
}

pub(crate) fn is_cjk_speech_char(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified
        | '\u{3400}'..='\u{4DBF}' // Extension A
        | '\u{F900}'..='\u{FAFF}' // Compatibility
        | '\u{3007}'              // Ideographic number zero
    )
}

/// Content-aware duration for one shot, clamped to the selected model's window.
///
/// Spoken audio (language-aware) sets the floor when present. Visual variation
/// adds a little headroom for continuous camera moves — not for verbose
/// `motion_desc` prose. Caps at the beat length ([`ClipBounds::preferred_max_secs`])
/// unless speech itself needs more (up to [`ClipBounds::max_secs`]).
pub fn estimate_shot_need_secs(
    bounds: ClipBounds,
    audio_desc: Option<&str>,
    motion_desc: &str,
    variation_type: &str,
) -> u32 {
    let audio = audio_desc.unwrap_or("").trim();
    let mut speech = estimate_speech_secs(audio);
    // Only mine motion text when it looks like it carries spoken lines
    // (quotes / dialogue verbs) — never treat camera verbs like "hold/pan" as speech.
    if speech == 0 && text_looks_like_dialogue(motion_desc) {
        speech = estimate_speech_secs(motion_desc);
    }
    let speech_need = if speech == 0 {
        0
    } else {
        speech
            .saturating_add(SPEECH_LEAD_SECS)
            .saturating_add(SPEECH_TAIL_SECS)
            .max(dialogue_floor_secs(bounds))
    };
    // Variation is about on-screen change, not prompt length.
    let visual_extra = match variation_type.trim().to_ascii_lowercase().as_str() {
        "large" => 2, // continuous move needs room to travel
        "medium" => 1,
        _ => 0,
    };
    let visual_need = bounds.saturating_add_within(bounds.min_secs(), visual_extra);
    let beat_cap = bounds.preferred_max_secs();
    let combined = bounds.clamp_secs(speech_need.max(visual_need));
    if speech_need > beat_cap {
        combined
    } else {
        combined.min(beat_cap)
    }
}

/// True when text carries spoken lines (quotes / dialogue verbs) rather than
/// pure camera direction like "hold/pan".
pub fn text_looks_like_dialogue(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    t.contains('「')
        || t.contains('」')
        || t.contains('"')
        || t.contains('“')
        || t.contains('”')
        || t.contains('{')
        || lower.contains("says")
        || lower.contains("said")
        || lower.contains("dialogue")
        || lower.contains("speech")
        || lower.contains("voice")
        || lower.contains("whisper")
        || lower.contains("shouts")
        || lower.contains("台词")
        || lower.contains("说道")
        || lower.contains("喊道")
        || lower.contains("怒吼")
        || lower.contains("说话")
        || lower.contains("轻声")
}

/// Allocate per-shot durations from content needs (audio + motion), then fit the
/// scene budget.
///
/// Honors dialogue floors first. When needs exceed the user target, Phase 1
/// trims surplus above each shot's content need; Phase 2 may compress further
/// but **never below** [`DIALOGUE_FLOOR_SECS`] for dialogue-heavy shots or the
/// model minimum otherwise. That can make the rendered sum slightly exceed
/// `target` — preferred over cutting spoken lines mid-sentence.
///
/// When content is *shorter* than the user target, leftover seconds are **not**
/// dumped onto existing clips (that is what made shots feel slow). The film may
/// land under budget; planning prompts already ask the LLM to write enough beats.
///
/// Soft-landing ([`SHOT_SPLICE_TAIL_PADDING_SECS`]) is reserved from an explicit
/// `target` before fitting and re-applied only when content still fits. Model-
/// decides mode (no budget) uses content length as-is — no extra pad.
pub fn allocate_clip_durations_for_content(
    bounds: ClipBounds,
    target_total: Option<u32>,
    needs: &[u32],
) -> Vec<u32> {
    if needs.is_empty() {
        if !has_explicit_duration_budget(target_total) {
            return vec![bounds.min_secs()];
        }
        return allocate_clip_durations(bounds, target_total, 1);
    }
    if !has_explicit_duration_budget(target_total) {
        return needs.iter().map(|&n| bounds.clamp_secs(n)).collect();
    }
    let target = normalize_target_duration_secs(target_total);
    let (fit_budget, will_pad) =
        fit_budget_reserving_splice_padding(bounds, target, needs.len());
    let mut durs: Vec<u32> = needs.iter().map(|&n| bounds.clamp_secs(n)).collect();
    let sum: u32 = durs.iter().sum();

    if sum < fit_budget {
        tracing::info!(
            target,
            fit_budget,
            rendered = sum,
            needs = ?needs,
            "content-sized clips under budget; not padding shots (avoids slow holds)"
        );
    } else if sum > fit_budget {
        // Phase 1: shrink surplus above each shot's content floor.
        let mut excess = sum - fit_budget;
        let mut order: Vec<usize> = (0..durs.len()).collect();
        order.sort_by_key(|&i| needs[i]); // smallest need first
        while excess > 0 {
            let mut progressed = false;
            for &i in &order {
                if excess == 0 {
                    break;
                }
                let floor = bounds.clamp_secs(needs[i]);
                if durs[i] > floor {
                    durs[i] -= 1;
                    excess -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        // Phase 2: still over → compress toward dialogue-safe floors (not the bare model min).
        // Prefer cutting longer shots first, but never drop dialogue below the dialogue floor.
        let sum2: u32 = durs.iter().sum();
        if sum2 > fit_budget {
            let mut excess = sum2 - fit_budget;
            let mut order: Vec<usize> = (0..durs.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(durs[i]));
            while excess > 0 {
                let mut progressed = false;
                for &i in &order {
                    if excess == 0 {
                        break;
                    }
                    let floor = content_compress_floor(bounds, needs[i]);
                    if durs[i] > floor {
                        durs[i] -= 1;
                        excess -= 1;
                        progressed = true;
                    }
                }
                if !progressed {
                    break;
                }
            }
            let final_sum: u32 = durs.iter().sum();
            if final_sum > fit_budget {
                tracing::info!(
                    target,
                    fit_budget,
                    rendered = final_sum,
                    needs = ?needs,
                    durations = ?durs,
                    "kept dialogue-safe clip floors; rendered length exceeds reserved fit budget"
                );
            } else {
                tracing::info!(
                    target,
                    fit_budget,
                    needs = ?needs,
                    durations = ?durs,
                    "compressed clip durations to fit reserved budget (dialogue floors preserved)"
                );
            }
        }
    }
    reapply_splice_tail_padding(bounds, &mut durs, fit_budget, target, will_pad);
    durs
}

/// Floor used when Phase-2 budget compression must still leave room for speech.
fn content_compress_floor(bounds: ClipBounds, need: u32) -> u32 {
    let dialogue_floor = dialogue_floor_secs(bounds);
    if need >= dialogue_floor {
        dialogue_floor
    } else {
        bounds.min_secs()
    }
}

/// Hard max shots for a budget (for post-LLM truncation).
pub fn max_shots_for_budget(bounds: ClipBounds, budget_secs: u32) -> usize {
    suggested_shot_count(bounds, budget_secs).1 as usize
}

/// Hard max scenes for a film-level budget (for post-LLM truncation).
pub fn max_scenes_for_budget(bounds: ClipBounds, total_secs: u32) -> usize {
    suggested_scene_count(bounds, total_secs).1 as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Window of the models integrated today (Seedance 2.0, MiniMax-H3 ⊂ 4–15s).
    const SEEDANCE: ClipBounds = ClipBounds::new(5, 15);

    fn within(secs: u32) -> bool {
        (SEEDANCE.min_secs()..=SEEDANCE.max_secs()).contains(&secs)
    }

    #[test]
    fn detect_output_language_prefers_chinese_input() {
        assert_eq!(
            detect_output_language(&["一个程序员发现自己的影子有了意识"]),
            OutputLanguage::Chinese
        );
        assert_eq!(
            detect_output_language(&["A programmer discovers his shadow is alive"]),
            OutputLanguage::English
        );
        // Mixed: Chinese idea + English style preset → still Chinese.
        assert_eq!(
            detect_output_language(&[
                "雨夜咖啡馆里的重逢",
                "cinematic film look, believable designed characters"
            ]),
            OutputLanguage::Chinese
        );
        assert!(language_lock_for_sources(&["你好世界"]).contains("简体中文"));
        assert!(language_lock_for_sources(&["hello world story"]).contains("English"));
    }

    #[test]
    fn resolve_visual_style_defaults_to_cinematic_soft_faces() {
        let s = resolve_visual_style("");
        let lower = s.to_ascii_lowercase();
        assert!(lower.contains("cinematic") || lower.contains("film"));
        assert!(lower.contains("clean") || lower.contains("clear") || lower.contains("healthy"));
        assert!(!lower.contains("anime"));
    }

    #[test]
    fn three_view_style_drops_identity_irrelevant_filler() {
        let s = style_for_three_view_image("");
        let lower = s.to_ascii_lowercase();
        assert!(lower.contains("cinematic") || lower.contains("film"));
        assert!(lower.contains("wardrobe") || lower.contains("lighting"));
        assert!(!lower.contains("designed characters"));
        assert!(!lower.contains("character"));
        assert!(!lower.contains("clean healthy facial skin"));
        assert!(!lower.contains("topology"));
        let custom = style_for_three_view_image(
            "rainy neon alley, believable designed characters, clean healthy facial skin with clear readable features, wet asphalt",
        );
        let custom_l = custom.to_ascii_lowercase();
        assert!(custom_l.contains("rainy neon alley"));
        assert!(custom_l.contains("wet asphalt"));
        assert!(!custom_l.contains("designed characters"));
        assert!(!custom_l.contains("clean healthy facial skin"));
    }

    #[test]
    fn detects_animation_style_presets() {
        assert!(wants_stylized_non_photoreal(
            "stylized anime / animated film look, clearly drawn characters, storybook colors"
        ));
        assert!(wants_stylized_non_photoreal("日式动画风格"));
        assert!(wants_stylized_non_photoreal(
            "painted illustration style, detailed brushwork"
        ));
        assert!(wants_stylized_non_photoreal(
            "soft 3D claymation / stop-motion look, rounded plasticine forms"
        ));
        assert!(wants_stylized_non_photoreal(
            "Chinese ink-wash painting animation, expressive brush strokes"
        ));
        assert!(wants_stylized_non_photoreal(
            "Korean webtoon / manhwa illustration style, clean digital linework"
        ));
        assert!(wants_stylized_non_photoreal(
            "premium pixel-art animation, deliberate low-resolution mosaic"
        ));
        assert!(wants_stylized_non_photoreal("黏土定格动画风格"));
        assert!(wants_stylized_non_photoreal(
            "3D CG animation style, game-engine quality render, semi-realistic stylized characters"
        ));
        assert!(wants_stylized_non_photoreal(
            "Japanese anime style, cel shading, clean crisp line art"
        ));
        assert!(wants_stylized_non_photoreal(
            "toon-shaded 3D animation, 2D anime lighting on 3D models"
        ));
        assert!(wants_stylized_non_photoreal(
            "LEGO brickfilm stop-motion animation, visible plastic studs"
        ));
        assert!(wants_stylized_non_photoreal(
            "voxel 3D animation, cubic blocky forms"
        ));
        assert!(wants_stylized_non_photoreal("glitch art video, datamosh pixel smear"));
        assert!(!wants_stylized_non_photoreal("cinematic film look"));
        assert!(!wants_stylized_non_photoreal(""));
        // Negated mentions must not flip cinematic prompts into stylized mode.
        assert!(!wants_stylized_non_photoreal(
            "cinematic film look. absolutely NOT anime, NOT manga, NOT cartoon"
        ));
        assert!(!wants_stylized_non_photoreal(
            "LIVE-ACTION continuity photos. FORBIDDEN: anime, manga, cartoon, cel shading"
        ));
    }

    #[test]
    fn chinese_ideographic_period_does_not_panic_in_style_detect() {
        // Regression: `rfind('。') + 1` used to slice mid-char and panic.
        let style = "写实电影感光影。日式动画风格，清晰体积与赛璐璐边缘";
        assert!(wants_stylized_non_photoreal(style));
        let _ = style_prompt_clause(style);
        let negated = "写实电影感。禁止动画、禁止动漫、不要卡通";
        assert!(!wants_stylized_non_photoreal(negated));
    }

    #[test]
    fn cinematic_sheet_parts_are_not_anime_model_sheets() {
        let parts = portrait_sheet_prompt_parts("cinematic film look, believable designed characters");
        let blob = format!(
            "{} {} {} {}",
            parts.style_lead, parts.sheet_kind, parts.quality_block, parts.medium_lock
        )
        .to_ascii_lowercase();
        assert!(blob.contains("live-action") || blob.contains("cinematic"));
        assert!(blob.contains("not anime") || blob.contains("forbidden"));
        assert!(!blob.contains("theatrical animated-film character design"));
        assert!(!wants_stylized_non_photoreal(&blob));
    }

    #[test]
    fn anime_sheet_parts_keep_stylized_medium() {
        let parts = portrait_sheet_prompt_parts(
            "theatrical anime / animated-film character design, clear volume",
        );
        let blob = format!("{} {}", parts.sheet_kind, parts.medium_lock).to_ascii_lowercase();
        assert!(blob.contains("animated") || blob.contains("illustration"));
        assert!(wants_stylized_non_photoreal("theatrical anime / animated-film character design"));
    }

    #[test]
    fn portrait_style_asks_for_clean_face_not_dirt() {
        let s = portrait_style_for_generation("cinematic");
        let lower = s.to_ascii_lowercase();
        assert!(lower.contains("clean") || lower.contains("healthy"));
        assert!(lower.contains("dirt") || lower.contains("blemish") || lower.contains("makeup"));
        assert!(lower.contains("cast style lock") || lower.contains("same style"));
        assert!(!s.contains("非真人") && !s.contains("无明星"));
        assert!(!lower.contains("celebrity") && !lower.contains("real-person"));
    }

    #[test]
    fn portrait_image_clause_honors_anime_and_does_not_force_cinematic() {
        let anime = "stylized anime / animated film look, clearly drawn characters, storybook colors";
        let s = portrait_image_style_clause(anime);
        let lower = s.to_ascii_lowercase();
        assert!(lower.contains("anime") || lower.contains("animated"));
        assert!(
            !lower.contains("same cinematic style"),
            "anime portraits must not force cinematic lock: {s}"
        );
        assert!(
            !lower.contains("no anime-only kids"),
            "anime portraits must not ban anime: {s}"
        );
        assert!(
            lower.contains("animation")
                || lower.contains("illustration")
                || lower.contains("volume"),
            "anime portraits should keep stylized medium: {s}"
        );
        let line = portrait_style_line_for_image(anime);
        assert!(line.chars().count() <= 120);
        assert!(portrait_medium_lock_line(anime).to_ascii_lowercase().contains("animation"));
        assert!(portrait_medium_lock_line("cinematic film look")
            .to_ascii_lowercase()
            .contains("live-action"));
    }

    #[test]
    fn production_look_lock_is_one_medium_for_cast_and_world() {
        let cine = production_look_lock("cinematic film look");
        let cine_l = cine.to_ascii_lowercase();
        assert!(cine.contains("PRODUCTION LOOK LOCK"));
        assert!(cine_l.contains("live-action") || cine_l.contains("cinematic"));
        assert!(cine_l.contains("sets") && cine_l.contains("props"));
        assert!(cine_l.contains("not anime") || cine_l.contains("absolutely not anime"));
        assert!(
            !cine_l.contains("faces:"),
            "vacant world plates reuse this lock — no face-finish: {cine}"
        );

        let anime = production_look_lock(
            "stylized anime / animated film look, clearly drawn characters, storybook colors",
        );
        let anime_l = anime.to_ascii_lowercase();
        assert!(anime_l.contains("animation") || anime_l.contains("illustration"));
        assert!(anime_l.contains("not photoreal") || anime_l.contains("do not switch to live-action"));
        assert!(!anime_l.contains("same cinematic style"));

        // Shot clause may add a face suffix; the bible lock itself must stay subject-agnostic.
        let shot = style_prompt_clause("cinematic film look");
        assert!(shot.contains("PRODUCTION LOOK LOCK"));
        assert!(shot.to_ascii_lowercase().contains("faces:"));
        assert_eq!(production_medium_lock_line("cinematic"), portrait_medium_lock_line("cinematic"));
    }

    #[test]
    fn portrait_image_clause_is_compact_and_keeps_theme_room() {
        let s = portrait_image_style_clause("cinematic wuxia ink");
        assert!(s.chars().count() < 280, "too long for image budget: {}", s.chars().count());
        assert!(!s.contains("非真人") && !s.to_ascii_lowercase().contains("celebrity"));
        let theme = portrait_theme_excerpt(
            "INT. ANCIENT TEMPLE - NIGHT. A young swordsman in travel-stained hanfu kneels before incense.",
        );
        assert!(theme.to_ascii_lowercase().contains("temple") || theme.contains("hanfu") || theme.contains("swordsman"));
    }

    #[test]
    fn detects_child_features_for_style_lock() {
        assert!(looks_like_child_character("小明", "8岁男孩，黑短发"));
        assert!(looks_like_child_character("Amy", "a young girl, age 7"));
        assert!(!looks_like_child_character("王经理", "中年男性，西装"));
        assert!(
            !looks_like_child_character("林铮", "28 岁中国女性，身高约 172cm"),
            "adult age must not trip the child lock via a bare 岁"
        );
        assert!(!looks_like_child_character("阿强", "boyfriend of the lead, 32"));
        assert_eq!(parse_character_age_years("28 岁中国女性"), Some(28));
        assert_eq!(parse_character_age_years("8岁男孩"), Some(8));
        assert_eq!(parse_character_age_years("a young girl, age 7"), Some(7));
    }

    #[test]
    fn clip_at_break_avoids_mid_clause_garbage() {
        let s = clip_at_break("鹅蛋脸，下颌线清晰，鼻梁高而挺直，嘴唇偏薄。身着战甲。", 18);
        assert!(!s.ends_with("小"), "{s}");
        assert!(s.contains('，') || s.contains('。'), "{s}");
        let short = clip_at_break("短", 80);
        assert_eq!(short, "短");
    }

    #[test]
    fn video_style_clause_is_one_positive_line() {
        let cine = video_style_clause("cinematic film look");
        assert!(cine.starts_with("Look:"));
        assert!(!cine.contains("PRODUCTION LOOK LOCK"));
        assert!(!cine.to_ascii_lowercase().contains("not anime"));
        let anime = video_style_clause("stylized anime / animated film look");
        assert!(anime.to_ascii_lowercase().contains("anim"));
    }

    #[test]
    fn enrich_scene_uses_budget_not_film_total_as_shot_target() {
        let s = enrich_requirement_for_scene(SEEDANCE, "funny", 10, 1, 3, 30);
        assert!(s.contains("funny"));
        assert!(s.contains("10"));
        assert!(s.contains("scene 2/3"));
        assert!(s.contains("HARD UPPER BOUND"));
        assert!(s.contains("BEAT-MATCHED PACING"));
        // Should not claim THIS SCENE is 30s.
        assert!(s.contains("30"));
        assert!(s.contains("THIS SCENE budget"));
        assert!(s.contains("SHOT CONTINUITY"));
        assert!(s.contains("CROSS-SCENE OPENING"));
        assert!(s.contains("DIRECTOR_DENSITY"));
        assert!(s.contains("BGM_CONTINUITY"));
        assert!(
            s.contains("already in progress") || s.contains("IDENTITY only"),
            "cross-scene must not restage the previous still: {s}"
        );
        assert!(
            !s.contains("continue from that still"),
            "cross-scene still-replay leftover: {s}"
        );
        assert!(
            !s.contains("ONE STRONG VISUAL EVENT PER CLIP"),
            "one-event-per-clip leftover: {s}"
        );
        assert!(s.contains("ONE ROW = ONE VIDEO"), "{s}");
        assert!(s.contains("NARRATIVE FIRST"), "{s}");
        assert!(s.contains("2–3") || s.contains("2-3"), "{s}");
        assert!(
            !s.contains("the packer folds"),
            "merge belongs in the storyboard row, not a later packer: {s}"
        );
        assert!(
            !s.contains("when they share a camera"),
            "rows must not be sliced by tripod: {s}"
        );
    }

    #[test]
    fn scene_prompt_quotes_the_selected_model_window() {
        let short_model = ClipBounds::new(4, 8);
        let s = enrich_requirement_for_scene(short_model, "funny", 20, 0, 1, 20);
        assert!(s.contains("4–8s"), "{s}");
        assert!(!s.contains("15s"), "must not leak another model's ceiling: {s}");
    }

    #[test]
    fn continuity_block_separates_identity_from_composition() {
        let s = shot_continuity_lines();
        assert!(s.contains("IDENTITY (always)"));
        assert!(s.contains("SAME opening cam_idx") || s.contains("SAME cam_idx"));
        assert!(s.contains("NEW opening cam_idx") || s.contains("NEW cam_idx"));
        assert!(
            s.contains("BETWEEN storyboard rows") || s.contains("file splices"),
            "cam_idx rules must not be a reason to add a row: {s}"
        );
        assert!(
            s.contains("do NOT replay") || s.contains("Do NOT replay"),
            "a cut must not re-stage the previous ending pose: {s}"
        );
    }

    #[test]
    fn short_budget_allows_only_one_or_two_shots() {
        let (ideal, max) = suggested_shot_count(SEEDANCE, 8);
        assert!(ideal <= 2);
        assert!(max <= 2);
    }

    #[test]
    fn sixty_second_budget_allows_enough_shots_to_fill() {
        let (ideal, max) = suggested_shot_count(SEEDANCE, 60);
        assert!(ideal >= 4, "ideal={ideal}");
        assert!(max >= 4, "max={max}");
        assert!(ideal * SEEDANCE.max_secs() >= 60);
        assert!(max * SEEDANCE.max_secs() >= 60);
    }

    #[test]
    fn forty_second_budget_prefers_fewer_longer_clips() {
        let (ideal, max) = suggested_shot_count(SEEDANCE, 40);
        // 40 / 12s drama beat = 3 clips (~13s each), not 5×8s fragments.
        assert_eq!(ideal, 3, "ideal={ideal}");
        assert!(max >= 3 && max <= 6, "max={max}");
        assert!(ideal * SEEDANCE.max_secs() >= 40);
    }

    #[test]
    fn shot_count_follows_a_long_clip_model() {
        // A model that accepts 30s clips needs fewer shots for the same budget.
        let long_clips = ClipBounds::new(10, 30);
        let (ideal, max) = suggested_shot_count(long_clips, 60);
        assert!(max <= 6, "max={max}");
        assert!(ideal * long_clips.max_secs() >= 60);
    }

    #[test]
    fn allocate_scene_budgets_sum_near_total() {
        let budgets = allocate_scene_budgets(SEEDANCE, 30, 3);
        assert_eq!(budgets.len(), 3);
        assert!(budgets.iter().sum::<u32>() >= 30);
        assert!(budgets.iter().all(|&b| b >= SEEDANCE.min_secs()));
    }

    #[test]
    fn clip_duration_never_below_min() {
        assert_eq!(
            clip_duration_secs(SEEDANCE, Some(20), 10),
            SEEDANCE.min_secs()
        );
        assert!(clip_duration_secs(SEEDANCE, Some(60), 3) >= SEEDANCE.min_secs());
    }

    #[test]
    fn allocate_clip_durations_respects_bounds_and_absorbs_remainder() {
        let durs = allocate_clip_durations(SEEDANCE, Some(30), 3);
        assert_eq!(durs.len(), 3);
        assert!(durs.iter().all(|&d| within(d)));
        // Soft-landing is reserved from target then re-applied → sum stays ≈ 30.
        assert_eq!(durs.iter().sum::<u32>(), 30);
        assert!(durs.iter().all(|&d| d == 10));
    }

    #[test]
    fn max_scenes_scales_with_budget() {
        assert!(max_scenes_for_budget(SEEDANCE, 15) <= 3);
        assert!(max_scenes_for_budget(SEEDANCE, 60) >= 3);
    }

    #[test]
    fn estimate_speech_secs_cjk_and_english() {
        // ~17 CJK chars @ 3.3/s → ceil(17/3.3)=6s
        let cjk: String = "他看着窗外轻声说道今天的风很温柔对吗".chars().cycle().take(17).collect();
        assert_eq!(estimate_speech_secs(&cjk), 6);
        // Quoted payload only (ignore stage directions outside 「」)
        assert_eq!(
            estimate_speech_secs("环境底噪。李薇：「今晚别等我」"),
            estimate_speech_secs("今晚别等我")
        );
        // Ambient / BGM copy must not count as speech.
        assert_eq!(
            estimate_speech_secs("环境底噪与连贯电影感背景音乐，配合画面动作的细微拟音"),
            0
        );
        // ~14 English words @ 2.3/wps → ceil(14/2.3)=7s
        let en = "one two three four five six seven eight nine ten \
eleven twelve thirteen fourteen";
        assert_eq!(estimate_speech_secs(en), 7);
        assert_eq!(estimate_speech_secs(""), 0);
        assert_eq!(estimate_speech_secs("   "), 0);
    }

    #[test]
    fn estimate_shot_need_includes_speech_tail() {
        // 17 CJK → ceil(17/3.3)=6s speech + 1s lead + 1s tail = 8
        let line: String = "中".chars().cycle().take(17).collect();
        assert_eq!(line.chars().count(), 17);
        let need = estimate_shot_need_secs(SEEDANCE, Some(&line), "slow pan", "small");
        assert_eq!(need, 8);
        // Shorter line: 9 CJK → ceil(9/3.3)=3 + 1 + 1 = 5 (dialogue floor)
        let mid: String = "中".chars().cycle().take(9).collect();
        let mid_need = estimate_shot_need_secs(SEEDANCE, Some(&mid), "slow pan", "small");
        assert_eq!(mid_need, 5);
        // No dialogue → visual floor (model min)
        let silent = estimate_shot_need_secs(SEEDANCE, None, "hold", "small");
        assert_eq!(silent, SEEDANCE.min_secs());
        // Verbose motion_desc must not inflate a silent shot
        let verbose_motion = "hold ".repeat(80);
        assert_eq!(
            estimate_shot_need_secs(SEEDANCE, None, &verbose_motion, "small"),
            SEEDANCE.min_secs()
        );
        // Long dialogue clamped to the model ceiling
        let long: String = "中".chars().cycle().take(80).collect();
        let capped =
            estimate_shot_need_secs(SEEDANCE, Some(&long), "walk across room", "large");
        assert_eq!(capped, SEEDANCE.max_secs());
        // Brief dialogue still gets dialogue floor, and stays at beat length
        let brief = estimate_shot_need_secs(SEEDANCE, Some("你好"), "nod", "small");
        assert!(brief >= DIALOGUE_FLOOR_SECS);
        assert!(brief <= SEEDANCE.preferred_max_secs());
        // English quoted line: 6 words → ceil(6/2.3)=3 + lead + tail = 5
        let en_need = estimate_shot_need_secs(
            SEEDANCE,
            Some(r#"Alice: "Don't wait up tonight.""#),
            "nod",
            "small",
        );
        assert_eq!(en_need, 5);
    }

    #[test]
    fn shot_need_never_leaves_a_narrow_model_window() {
        let narrow = ClipBounds::new(4, 8);
        let long: String = "中".chars().cycle().take(80).collect();
        let capped = estimate_shot_need_secs(narrow, Some(&long), "walk", "large");
        assert_eq!(capped, narrow.max_secs());
        let silent = estimate_shot_need_secs(narrow, None, "hold", "small");
        assert_eq!(silent, narrow.min_secs());
    }

    #[test]
    fn allocate_for_content_protects_dialogue_floors() {
        // Dialogue-heavy shot needs ~12s; silent needs 5s; budget 20s.
        // Spare seconds are NOT dumped onto clips (that caused slow holds).
        let needs = vec![12, 5];
        let durs = allocate_clip_durations_for_content(SEEDANCE, Some(20), &needs);
        assert_eq!(durs.len(), 2);
        assert!(durs[0] >= 12);
        assert!(durs[1] >= SEEDANCE.min_secs());
        assert!(durs.iter().all(|&d| within(d)));
        let sum: u32 = durs.iter().sum();
        assert!(sum >= 17);
        assert!(sum <= 20);
        assert!(durs[0] >= durs[1]);
    }

    #[test]
    fn allocate_does_not_pad_short_content_to_fill_budget() {
        let needs = vec![5, 5];
        let durs = allocate_clip_durations_for_content(SEEDANCE, Some(40), &needs);
        // A 40s budget buys nothing beyond each shot's content plus its soft
        // landing — leftover seconds must not become slow holds.
        assert!(durs
            .iter()
            .zip(&needs)
            .all(|(&d, &need)| d <= need + SHOT_SPLICE_TAIL_PADDING_SECS));
        assert!(durs.iter().sum::<u32>() < 20);
    }

    #[test]
    fn allocate_for_content_fits_target_when_floors_overshoot() {
        // Dialogue floors that exceed the reserved fit budget still land near
        // the advertised target after soft-landing is re-applied only when safe.
        let needs = vec![12, 12];
        let durs = allocate_clip_durations_for_content(SEEDANCE, Some(18), &needs);
        assert_eq!(durs.iter().sum::<u32>(), 18);
        assert!(durs.iter().all(|&d| d >= DIALOGUE_FLOOR_SECS));
        assert!(durs.iter().all(|&d| within(d)));
    }

    #[test]
    fn allocate_for_content_caps_four_max_clips_to_forty() {
        let needs = vec![15, 15, 15, 15];
        let durs = allocate_clip_durations_for_content(SEEDANCE, Some(40), &needs);
        // Budget + reserved soft-landing re-applied → sum stays at the user target.
        assert_eq!(durs.iter().sum::<u32>(), 40);
        assert!(durs.iter().all(|&d| d >= DIALOGUE_FLOOR_SECS));
        assert!(durs.iter().all(|&d| d <= SEEDANCE.max_secs()));
    }

    #[test]
    fn allocate_for_content_never_cuts_dialogue_below_floor() {
        // Extreme under-budget: prefer dialogue-safe floors over bare-min silent clips.
        let needs = vec![12, 12];
        let durs = allocate_clip_durations_for_content(SEEDANCE, Some(12), &needs);
        assert!(durs.iter().all(|&d| d >= DIALOGUE_FLOOR_SECS));
        assert!(durs.iter().sum::<u32>() <= 12);
        assert!(durs.iter().all(|&d| within(d)));
    }

    #[test]
    fn fifty_five_second_target_does_not_gain_two_secs_per_shot() {
        // Regression: max-shot packing must stay at the user target after soft-landing.
        let needs = vec![5; 11];
        let durs = allocate_clip_durations_for_content(SEEDANCE, Some(55), &needs);
        assert_eq!(durs.len(), 11);
        assert_eq!(durs.iter().sum::<u32>(), 55);
    }

    #[test]
    fn splice_tail_padding_respects_the_model_ceiling() {
        let narrow = ClipBounds::new(5, 12);
        let mut durs = vec![5, 13, 14, 15];
        apply_shot_splice_tail_padding(narrow, &mut durs);
        assert_eq!(durs, vec![6, 12, 12, 12]);

        let mut wide = vec![5, 14, 15];
        apply_shot_splice_tail_padding(SEEDANCE, &mut wide);
        assert_eq!(wide, vec![6, 15, 15]);
    }

    #[test]
    fn scene_bgm_paren_is_stable_and_extractable() {
        assert!(format_scene_bgm_paren("").starts_with('('));
        assert_eq!(
            format_scene_bgm_paren("(soft piano underscore, same motif)"),
            "(soft piano underscore, same motif)"
        );
        let descs = [
            Some("李薇：「走」 <脚步声> (gentle piano motif, steady tempo)"),
            Some("环境底噪"),
        ];
        let extracted = extract_bgm_paren_from_audio_descs(descs);
        assert!(extracted.as_ref().is_some_and(|s| s.contains("piano")));
        let resolved = resolve_scene_bgm_paren(None, &descs);
        assert_eq!(resolved, extracted.unwrap());
        assert_eq!(
            resolve_scene_bgm_paren(Some("warm strings underscore"), &[]),
            "(warm strings underscore)"
        );
    }

    #[test]
    fn enrich_film_asks_for_density_and_music_arc() {
        let s = enrich_requirement_for_film(SEEDANCE, "雨夜重逢", Some(45));
        assert!(s.contains("DIRECTOR_DENSITY"));
        assert!(s.contains("MUSIC_ARC"));
        assert!(s.contains("45"));
        assert!(s.contains("VIDEO_DURATION_CONSTRAINTS"));
    }

    #[test]
    fn enrich_film_without_budget_lets_model_decide() {
        let s = enrich_requirement_for_film(SEEDANCE, "雨夜重逢", None);
        assert!(s.contains("VIDEO_PACING"));
        assert!(s.contains("beats") || s.contains("BEAT"));
        assert!(!s.contains("hard planning budget"));
        assert!(!s.contains("VIDEO_DURATION_CONSTRAINTS"));
        let planning = enrich_requirement_for_planning(SEEDANCE, "funny", None);
        assert!(planning.contains("VIDEO_PACING"));
        assert!(!planning.contains("THIS SCENE budget"));
    }

    #[test]
    fn allocate_clip_durations_without_budget_follows_content() {
        let needs = vec![6, 9, 7];
        let durs = allocate_clip_durations_for_content(SEEDANCE, None, &needs);
        assert_eq!(durs.len(), 3);
        assert!(durs.iter().all(|&d| within(d)));
        for (d, n) in durs.iter().zip(needs.iter()) {
            assert_eq!(*d, *n);
        }
    }
}
