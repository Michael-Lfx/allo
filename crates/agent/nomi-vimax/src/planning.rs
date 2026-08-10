//! Planning helpers: Seedance clips are 5–15s — keep shot counts low and budgets real.

/// Minimum seconds the Flowy / Seedance video API accepts for I2V (and what we bill).
pub const MIN_CLIP_DURATION_SECS: u32 = 5;

/// Max per clip — Seedance 2.x accepts up to 15s.
pub const MAX_CLIP_DURATION_SECS: u32 = 15;

/// Default target total length when the user does not specify one.
pub const DEFAULT_TARGET_DURATION_SECS: u32 = 45;

/// Max user-facing film target (UI timeline + plan/render clamp).
pub const MAX_TARGET_DURATION_SECS: u32 = 300;

/// Clear spoken Chinese chars/sec for Seedance (deliberately slower than
/// conversational chat — rushed clips swallow syllables / 吞字).
const SPEECH_CJK_CHARS_PER_SEC: f32 = 1.7;
/// Clear spoken English words/sec (same conservative bias as CJK).
const SPEECH_EN_WORDS_PER_SEC: f32 = 1.35;
/// Breath / reaction beat before the first spoken syllable.
const SPEECH_LEAD_SECS: u32 = 1;
/// Tail seconds after the last spoken syllable so audio is not cut mid-breath.
const SPEECH_TAIL_SECS: u32 = 5;
/// Dialogue shots should not be shorter than this even when the line is brief.
const MIN_DIALOGUE_CLIP_SECS: u32 = 9;
/// Soft-landing seconds preferred at the end of each shot before a splice.
/// Reserved **from** the user target before budget fitting, then re-applied, so
/// the rendered sum stays near the advertised length (never `target + 2×shots`).
pub const SHOT_SPLICE_TAIL_PADDING_SECS: u32 = 2;

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

/// Short style clause for image prompts (survives 800-char Z-Image truncate).
pub fn style_prompt_clause(user_style: &str) -> String {
    let style = resolve_visual_style(user_style);
    let clipped: String = style.chars().take(120).collect();
    if wants_stylized_non_photoreal(&style) {
        let enriched = enrich_stylized_style_for_portraits(&style);
        let clipped: String = enriched.chars().take(140).collect();
        format!(
            "MUST MATCH Visual style (non-photoreal, detailed volume): {clipped}. Keep the SAME drawn look for every character and set; do NOT switch to live-action photoreal; avoid flat paper-doll cutouts."
        )
    } else {
        format!(
            "Visual style: {clipped}. Faces: clean healthy skin, clear sharp features (no melt/blur, no dirt or weird makeup)."
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
Do NOT flatten into a paper cutout or blank sticker face. Do NOT render photoreal live-action skin or celebrity likeness.";

/// Extra face lock for child / teen characters (models often age or over-make kids).
pub const PORTRAIT_CHILD_FACE_GUIDANCE: &str = "\
CHILD FACE LOCK: age-correct child/teen face — smooth healthy skin, soft natural cheeks, age-appropriate features. \
No adult contour makeup, no heavy lipstick/eyeshadow, no aged wrinkles, no dirt/blemishes, no uncanny warped proportions. \
Keep expression natural and clear; identity must stay cute/clean, not grotesque.";

/// Not a real-person / celebrity likeness (Seedance privacy + originality).
pub const PORTRAIT_NON_REAL_PERSON: &str = "\
IDENTITY SAFETY: fictional designed character only — NOT a real-person portrait, NOT photoreal ID-photo, NOT a celebrity/star likeness, NOT a recognizable famous face. Original character design with a clean natural face. \
非真人肖像，无明星样貌，虚构角色造型，禁止做成可辨认的真人/明星脸；面部保持干净自然，不要刻意丑化或污化。";

/// Force adults and children to share one rendering style (models often anime-ify kids otherwise).
pub const CAST_STYLE_LOCK: &str = "\
CAST STYLE LOCK: every character of every age must share the SAME Style, shading, materials, and finish. \
Children/teens use age-correct proportions but must NOT become anime/chibi/cartoon/comic while adults stay cinematic.";

/// Cast lock when the production Style is already animation/illustration.
pub const CAST_STYLE_LOCK_STYLIZED: &str = "\
CAST STYLE LOCK: every character of every age must share the SAME premium animation Style with equal detail and volume. \
Children/teens use age-correct proportions but the SAME drawn look as adults — do NOT mix photoreal adults with stylized kids or vice versa.";

/// Compact locks for portrait image prompts (survive 800-char truncate).
pub const PORTRAIT_IDENTITY_SHORT: &str =
    "非真人肖像，无明星样貌; fictional designed character, clean natural face, not celebrity likeness.";

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
        format!(
            "{base}. {PORTRAIT_NON_REAL_PERSON} {PORTRAIT_FACE_GUIDANCE_STYLIZED} {CAST_STYLE_LOCK_STYLIZED}"
        )
    } else {
        format!("{base}. {PORTRAIT_NON_REAL_PERSON} {PORTRAIT_FACE_GUIDANCE} {CAST_STYLE_LOCK}")
    }
}

/// Short Style field for portrait image prompts (ViMax-style: Features first, Style short).
pub fn portrait_style_line_for_image(user_style: &str) -> String {
    let resolved = enrich_stylized_style_for_portraits(user_style);
    resolved.chars().take(120).collect()
}

/// One-line medium lock so Style does not drown Features.
pub fn portrait_medium_lock_line(user_style: &str) -> String {
    if wants_stylized_non_photoreal(user_style) {
        "Medium: match Style as high-detail animation/illustration with volume — not a flat paper sticker; not photoreal live-action."
            .into()
    } else {
        "Medium: live-action cinematic cast portrait — not anime, not manga, not cartoon model-sheet."
            .into()
    }
}

/// Short style block for three-view image generation (theme/features get priority in the template).
pub fn portrait_image_style_clause(user_style: &str) -> String {
    let style = portrait_style_line_for_image(user_style);
    let medium = portrait_medium_lock_line(user_style);
    if wants_stylized_non_photoreal(user_style) {
        format!("{style}. {PORTRAIT_IDENTITY_SHORT} {medium}")
    } else {
        format!("{style}. {PORTRAIT_IDENTITY_SHORT} {medium}")
    }
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
pub fn looks_like_child_character(identifier: &str, features: &str) -> bool {
    let blob = format!("{identifier} {features}").to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
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
        "岁",
    ];
    NEEDLES.iter().any(|n| blob.contains(n))
}

/// Clamp a user-provided target into a practical range.
pub fn normalize_target_duration_secs(raw: Option<u32>) -> u32 {
    raw.unwrap_or(DEFAULT_TARGET_DURATION_SECS)
        .clamp(MIN_CLIP_DURATION_SECS, MAX_TARGET_DURATION_SECS)
}

/// Suggested shot count for a **single scene budget** (not the whole film).
///
/// Seedance clips are 5–15s. Prefer ~12–13s clips so `ideal × ~13s ≈ budget`.
/// `max_shots` is high enough to fill the budget at MAX length, but `ideal` is
/// not forced up to that floor (forcing it caused 4×15s≈60s when target was 40s
/// once dialogue floors refused to shrink).
pub fn suggested_shot_count(budget_secs: u32) -> (u32, u32) {
    let budget = budget_secs.max(MIN_CLIP_DURATION_SECS);
    let min_to_fill =
        (budget + MAX_CLIP_DURATION_SECS - 1) / MAX_CLIP_DURATION_SECS;
    // Aim ~13s/clip: 40→3, 60→5, 30→3. (`+6` rounds toward nearest).
    let ideal = ((budget + 6) / 13).clamp(1, 6);
    let max_shots = (budget / MIN_CLIP_DURATION_SECS)
        .max(min_to_fill)
        .clamp(1, 8);
    // Only raise ideal toward min_to_fill when even max-length ideal clips
    // cannot reach the budget (e.g. ideal=2 → 30s < 40s target).
    let ideal = if ideal.saturating_mul(MAX_CLIP_DURATION_SECS) < budget {
        min_to_fill.min(max_shots)
    } else {
        ideal.min(max_shots)
    };
    (ideal.max(1), max_shots)
}

/// Split a film-level target across N scenes (each ≥5s).
pub fn allocate_scene_budgets(total_secs: u32, scene_count: usize) -> Vec<u32> {
    let n = scene_count.max(1);
    let total = normalize_target_duration_secs(Some(total_secs));
    let base = (total / n as u32).max(MIN_CLIP_DURATION_SECS);
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
pub fn suggested_scene_count(total_secs: u32) -> (u32, u32) {
    let total = normalize_target_duration_secs(Some(total_secs));
    // ~10–15s per scene.
    let ideal = ((total + 12) / 15).clamp(1, 5);
    let max_scenes = (total / MIN_CLIP_DURATION_SECS).clamp(1, 6);
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

/// Film-level constraints (develop story / write multi-scene script).
pub fn enrich_requirement_for_film(user_requirement: &str, target_secs: Option<u32>) -> String {
    let target = normalize_target_duration_secs(target_secs);
    let (ideal_scenes, max_scenes) = suggested_scene_count(target);
    let per_scene = (target / ideal_scenes.max(1)).max(MIN_CLIP_DURATION_SECS);
    let base = with_language_lock(user_requirement, &[user_requirement]);
    let block = format!(
        "[VIDEO_DURATION_CONSTRAINTS — MUST FOLLOW]\n\
         - Target finished film length ≈ {target} seconds TOTAL (hard planning budget).\n\
         - Prefer about {ideal_scenes} scenes (hard upper bound {max_scenes}); ~{per_scene}s per scene.\n\
         - Each rendered shot clip is {MIN_CLIP_DURATION_SECS}–{MAX_CLIP_DURATION_SECS}s (Seedance hard range).\n\
         - Keep the whole story compact so total scenes × shots × {MIN_CLIP_DURATION_SECS}s stays near {target}s.\n\
         - Do NOT write more plot/dialogue than can be spoken and shown inside that total runtime.\n\
         - Speech pacing guide (clear delivery, avoid rush/吞字): ~{SPEECH_CJK_CHARS_PER_SEC} Chinese chars/sec \
or ~{SPEECH_EN_WORDS_PER_SEC} English words/sec; leave ~{SPEECH_LEAD_SECS}s before speech starts and \
~{SPEECH_TAIL_SECS}s after the last word so audio is not cut off."
    );
    format!("{base}\n\n{block}")
}

/// Scene-level constraints for storyboard design (budget already allocated).
pub fn enrich_requirement_for_scene(
    user_requirement: &str,
    scene_budget_secs: u32,
    scene_idx: usize,
    scene_count: usize,
    film_total_secs: u32,
) -> String {
    let budget = scene_budget_secs.max(MIN_CLIP_DURATION_SECS);
    let (ideal, max_shots) = suggested_shot_count(budget);
    let per_shot = clip_duration_secs(Some(budget), ideal as usize);
    // Speakable window inside one Seedance clip (lead + tail reserved).
    let speak_window_max =
        MAX_CLIP_DURATION_SECS.saturating_sub(SPEECH_LEAD_SECS + SPEECH_TAIL_SECS);
    let speak_window_typical =
        per_shot.saturating_sub(SPEECH_LEAD_SECS + SPEECH_TAIL_SECS);
    let max_cjk_chars =
        (speak_window_max as f32 * SPEECH_CJK_CHARS_PER_SEC).floor() as u32;
    let max_en_words =
        (speak_window_max as f32 * SPEECH_EN_WORDS_PER_SEC).floor() as u32;
    let per_shot_cjk =
        (speak_window_typical as f32 * SPEECH_CJK_CHARS_PER_SEC).floor() as u32;
    let base = user_requirement.trim();
    // Strip a previous film-level block so we don't double-confuse the LLM with two totals.
    let base = strip_duration_constraint_blocks(base);
    let base = with_language_lock(&base, &[&base, user_requirement]);
    let block = format!(
        "[VIDEO_DURATION_CONSTRAINTS — MUST FOLLOW]\n\
         - This is scene {scene_num}/{scene_count} of a film targeting ≈ {film_total_secs}s total.\n\
         - THIS SCENE budget ≈ {budget} seconds of finished video (NOT the whole film).\n\
         - Each shot clip is {MIN_CLIP_DURATION_SECS}–{MAX_CLIP_DURATION_SECS}s (Seedance). Typical render ≈ {per_shot}s.\n\
         - Prefer about {ideal} shots; HARD UPPER BOUND: {max_shots} shots for this scene.\n\
         - Shot count × clip length should land near this scene's {budget}s budget \
(Seedance max {MAX_CLIP_DURATION_SECS}s/clip — do NOT under-shoot with only 1–2 short clips when the budget is much larger).\n\
         - Plan visual beats AND audio beats together: dialogue/SFX in audio_desc MUST finish inside the \
same shot's duration — no unfinished lines, mid-sentence cuts, swallowed syllables (吞字), or \
\"and then…\" requiring another clip.\n\
         - EVERY shot MUST have a non-empty audio_desc (spoken lines and/or ambient SFX+BGM). Never leave audio_desc null.\n\
         - Speech budget per shot at ≈{per_shot}s: keep spoken Chinese ≲ {per_shot_cjk} chars \
(hard max ≲ {max_cjk_chars} chars / ≲ {max_en_words} English words for a {MAX_CLIP_DURATION_SECS}s clip, \
after reserving ~{SPEECH_LEAD_SECS}s lead-in + ~{SPEECH_TAIL_SECS}s tail). \
If a speech beat is longer, you MUST SPLIT into another shot (or shorten the line) — never cram past \
the {MAX_CLIP_DURATION_SECS}s Seedance limit.\n\
         - Prefer fewer longer shots for silent/action beats; for dialogue, prioritize clear pacing over \
merging — pack reaction into the same framing only when the spoken payload still fits the speech budget.\n\
         - Reuse cam_idx whenever possible. Prefer in-shot motion over cutting.\n\
         - SHOT CONTINUITY (this scene only): for every adjacent pair of shots, shot N+1 must open from \
shot N's ending state so Seedance can match-cut (first frame of next = last frame of previous). \
Camera/angle may change; cast identity, wardrobe, lighting mood, and set must carry over. \
Do NOT require continuity from the previous scene's final shot into this scene's first shot.\n\
         - If you would create more than {max_shots} shots, merge beats instead.",
        scene_num = scene_idx + 1,
    );
    format!("{base}\n\n{block}")
}

/// Single-scene script2video (whole target = this scene).
pub fn enrich_requirement_for_planning(user_requirement: &str, target_secs: Option<u32>) -> String {
    let target = normalize_target_duration_secs(target_secs);
    enrich_requirement_for_scene(user_requirement, target, 0, 1, target)
}

fn strip_duration_constraint_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in s.lines() {
        let t = line.trim();
        if t.starts_with("[VIDEO_DURATION_CONSTRAINTS")
            || t.starts_with("[OUTPUT_LANGUAGE")
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
pub fn clip_duration_secs(target_total: Option<u32>, shot_count: usize) -> u32 {
    let n = shot_count.max(1) as u32;
    let target = normalize_target_duration_secs(target_total);
    (target / n).clamp(MIN_CLIP_DURATION_SECS, MAX_CLIP_DURATION_SECS)
}

/// Allocate per-shot durations that sum as close as possible to the scene budget
/// while respecting Seedance `[MIN, MAX]` clip bounds.
///
/// Prefer this over repeating [`clip_duration_secs`] when shot lengths should vary
/// slightly so the last clip absorbs remainder seconds.
///
/// Soft-landing ([`SHOT_SPLICE_TAIL_PADDING_SECS`]) is reserved from `target` first
/// so the final sum stays near the user budget instead of overshooting by
/// `padding × shot_count`.
pub fn allocate_clip_durations(target_total: Option<u32>, shot_count: usize) -> Vec<u32> {
    let n = shot_count.max(1);
    let target = normalize_target_duration_secs(target_total);
    let (fit_budget, will_pad) = fit_budget_reserving_splice_padding(target, n);
    let base = clip_duration_secs(Some(fit_budget), n);
    let mut durs = vec![base; n];
    // Cap total near fit budget when base*n would overshoot (min-clip floor).
    let planned: u32 = base.saturating_mul(n as u32);
    if planned <= fit_budget {
        let mut rem = fit_budget.saturating_sub(planned);
        for d in durs.iter_mut().rev() {
            if rem == 0 {
                break;
            }
            let room = MAX_CLIP_DURATION_SECS.saturating_sub(*d);
            let add = rem.min(room);
            *d += add;
            rem -= add;
        }
    }
    reapply_splice_tail_padding(&mut durs, fit_budget, target, will_pad);
    durs
}

/// Carve soft-landing room out of the advertised target when there is enough
/// headroom above `MIN_CLIP × shots`.
fn fit_budget_reserving_splice_padding(target: u32, shot_count: usize) -> (u32, bool) {
    let n = shot_count.max(1) as u32;
    let pad_total = SHOT_SPLICE_TAIL_PADDING_SECS.saturating_mul(n);
    let min_content = MIN_CLIP_DURATION_SECS.saturating_mul(n);
    if target >= pad_total.saturating_add(min_content) {
        (target - pad_total, true)
    } else {
        (target, false)
    }
}

/// Add splice-tail padding to finalized per-shot durations (≤ Seedance max).
fn apply_shot_splice_tail_padding(durs: &mut [u32]) {
    for d in durs.iter_mut() {
        *d = d
            .saturating_add(SHOT_SPLICE_TAIL_PADDING_SECS)
            .clamp(MIN_CLIP_DURATION_SECS, MAX_CLIP_DURATION_SECS);
    }
}

/// Re-apply reserved soft-landing only when content still fits the pre-pad budget
/// (or fill leftover seconds toward `target` without exceeding it).
fn reapply_splice_tail_padding(
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
        apply_shot_splice_tail_padding(durs);
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
        let room = MAX_CLIP_DURATION_SECS
            .saturating_sub(*d)
            .min(SHOT_SPLICE_TAIL_PADDING_SECS)
            .min(rem);
        *d += room;
        rem -= room;
    }
}

/// Estimate spoken seconds from `audio_desc` (dialogue + SFX text).
///
/// Prefers quoted / braced dialogue payloads when present so stage directions
/// do not dominate the estimate. Uses conservative CJK char / English word
/// rates so Seedance clips are not time-compressed into rushed speech.
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

/// Prefer dialogue inside 「」 / “” / "" / `{…}`; otherwise the full text.
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
    if chunks.is_empty() {
        audio_desc.to_string()
    } else {
        chunks.join(" ")
    }
}

fn is_cjk_speech_char(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified
        | '\u{3400}'..='\u{4DBF}' // Extension A
        | '\u{F900}'..='\u{FAFF}' // Compatibility
        | '\u{3007}'              // Ideographic number zero
    )
}

/// Content-aware lower bound for one shot (Seedance-clamped).
///
/// Honors spoken audio first, then adds motion/variation headroom so dialogue
/// is not cut off or time-compressed when the clip ends.
pub fn estimate_shot_need_secs(
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
            .max(MIN_DIALOGUE_CLIP_SECS)
    };
    let variation_boost: u32 = match variation_type.trim().to_ascii_lowercase().as_str() {
        "large" => 3,
        "medium" => 2,
        _ => 1,
    };
    let motion_len = motion_desc.chars().count();
    let motion_extra = if motion_len > 220 {
        2
    } else if motion_len > 110 {
        1
    } else {
        0
    };
    let visual_need = MIN_CLIP_DURATION_SECS
        .saturating_add(variation_boost.saturating_sub(1))
        .saturating_add(motion_extra);
    speech_need
        .max(visual_need)
        .clamp(MIN_CLIP_DURATION_SECS, MAX_CLIP_DURATION_SECS)
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
/// but **never below** [`MIN_DIALOGUE_CLIP_SECS`] for dialogue-heavy shots
/// (`needs[i] >= MIN_DIALOGUE_CLIP_SECS`) or [`MIN_CLIP_DURATION_SECS`] otherwise.
/// That can make the rendered sum slightly exceed `target` — preferred over
/// cutting spoken lines mid-sentence.
///
/// Soft-landing ([`SHOT_SPLICE_TAIL_PADDING_SECS`]) is reserved from `target`
/// before fitting and re-applied only when content still fits, so happy-path
/// totals stay near the user budget (not `target + 2×shots`).
pub fn allocate_clip_durations_for_content(
    target_total: Option<u32>,
    needs: &[u32],
) -> Vec<u32> {
    if needs.is_empty() {
        return allocate_clip_durations(target_total, 1);
    }
    let target = normalize_target_duration_secs(target_total);
    let (fit_budget, will_pad) = fit_budget_reserving_splice_padding(target, needs.len());
    let mut durs: Vec<u32> = needs
        .iter()
        .map(|&n| n.clamp(MIN_CLIP_DURATION_SECS, MAX_CLIP_DURATION_SECS))
        .collect();
    let sum: u32 = durs.iter().sum();

    if sum < fit_budget {
        // Give spare seconds to the neediest shots first (usually dialogue-heavy).
        let mut rem = fit_budget - sum;
        let mut order: Vec<usize> = (0..durs.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(needs[i]));
        while rem > 0 {
            let mut progressed = false;
            for &i in &order {
                if rem == 0 {
                    break;
                }
                let room = MAX_CLIP_DURATION_SECS.saturating_sub(durs[i]);
                if room == 0 {
                    continue;
                }
                durs[i] += 1;
                rem -= 1;
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
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
                let floor = needs[i].clamp(MIN_CLIP_DURATION_SECS, MAX_CLIP_DURATION_SECS);
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
        // Phase 2: still over → compress toward dialogue-safe floors (not bare Seedance min).
        // Prefer cutting longer shots first, but never drop dialogue below MIN_DIALOGUE_CLIP_SECS.
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
                    let floor = content_compress_floor(needs[i]);
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
    reapply_splice_tail_padding(&mut durs, fit_budget, target, will_pad);
    durs
}

/// Floor used when Phase-2 budget compression must still leave room for speech.
fn content_compress_floor(need: u32) -> u32 {
    if need >= MIN_DIALOGUE_CLIP_SECS {
        MIN_DIALOGUE_CLIP_SECS
    } else {
        MIN_CLIP_DURATION_SECS
    }
}

/// Hard max shots for a budget (for post-LLM truncation).
pub fn max_shots_for_budget(budget_secs: u32) -> usize {
    suggested_shot_count(budget_secs).1 as usize
}

/// Hard max scenes for a film-level budget (for post-LLM truncation).
pub fn max_scenes_for_budget(total_secs: u32) -> usize {
    suggested_scene_count(total_secs).1 as usize
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            lower.contains("not a real-person")
                || lower.contains("fictional")
                || s.contains("非真人")
                || s.contains("无明星")
        );
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
    fn portrait_image_clause_is_compact_and_keeps_theme_room() {
        let s = portrait_image_style_clause("cinematic wuxia ink");
        assert!(s.chars().count() < 280, "too long for image budget: {}", s.chars().count());
        assert!(s.contains("非真人") || s.to_ascii_lowercase().contains("fictional"));
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
    }

    #[test]
    fn enrich_scene_uses_budget_not_film_total_as_shot_target() {
        let s = enrich_requirement_for_scene("funny", 10, 1, 3, 30);
        assert!(s.contains("funny"));
        assert!(s.contains("10"));
        assert!(s.contains("scene 2/3"));
        assert!(s.contains("HARD UPPER BOUND"));
        // Should not claim THIS SCENE is 30s.
        assert!(s.contains("30"));
        assert!(s.contains("THIS SCENE budget"));
        assert!(s.contains("SHOT CONTINUITY"));
        assert!(s.contains("Do NOT require continuity from the previous scene"));
    }

    #[test]
    fn short_budget_allows_only_one_or_two_shots() {
        let (ideal, max) = suggested_shot_count(8);
        assert!(ideal <= 2);
        assert!(max <= 2);
    }

    #[test]
    fn sixty_second_budget_allows_enough_shots_to_fill() {
        let (ideal, max) = suggested_shot_count(60);
        // 60s / 15s max per clip → need ≥4 shots; ~13s aim → ideal≈5.
        assert!(ideal >= 4, "ideal={ideal}");
        assert!(max >= 4, "max={max}");
        assert!(ideal as u32 * MAX_CLIP_DURATION_SECS >= 60);
        assert!(max as u32 * MAX_CLIP_DURATION_SECS >= 60);
    }

    #[test]
    fn forty_second_budget_prefers_three_shots_not_four() {
        let (ideal, max) = suggested_shot_count(40);
        assert_eq!(ideal, 3, "ideal={ideal}");
        assert!(max >= 3);
        assert!(ideal as u32 * MAX_CLIP_DURATION_SECS >= 40);
    }

    #[test]
    fn allocate_scene_budgets_sum_near_total() {
        let budgets = allocate_scene_budgets(30, 3);
        assert_eq!(budgets.len(), 3);
        assert!(budgets.iter().sum::<u32>() >= 30);
        assert!(budgets.iter().all(|&b| b >= MIN_CLIP_DURATION_SECS));
    }

    #[test]
    fn clip_duration_never_below_min() {
        assert_eq!(clip_duration_secs(Some(20), 10), MIN_CLIP_DURATION_SECS);
        assert!(clip_duration_secs(Some(60), 3) >= MIN_CLIP_DURATION_SECS);
    }

    #[test]
    fn allocate_clip_durations_respects_bounds_and_absorbs_remainder() {
        let durs = allocate_clip_durations(Some(30), 3);
        assert_eq!(durs.len(), 3);
        assert!(durs.iter().all(|&d| (MIN_CLIP_DURATION_SECS..=MAX_CLIP_DURATION_SECS).contains(&d)));
        // Soft-landing is reserved from target then re-applied → sum stays ≈ 30.
        assert_eq!(durs.iter().sum::<u32>(), 30);
        assert!(durs.iter().all(|&d| d == 10));
    }

    #[test]
    fn max_scenes_scales_with_budget() {
        assert!(max_scenes_for_budget(15) <= 3);
        assert!(max_scenes_for_budget(60) >= 3);
    }

    #[test]
    fn estimate_speech_secs_cjk_and_english() {
        // ~17 CJK chars @ 1.7/s → 10s
        let cjk: String = "他看着窗外轻声说道今天的风很温柔对吗".chars().cycle().take(17).collect();
        assert_eq!(estimate_speech_secs(&cjk), 10);
        // Quoted payload only (ignore stage directions outside 「」)
        assert_eq!(
            estimate_speech_secs("环境底噪。李薇：「今晚别等我」"),
            estimate_speech_secs("今晚别等我")
        );
        // ~14 English words @ 1.35/wps → ceil(14/1.35)=11s
        let en = "one two three four five six seven eight nine ten \
eleven twelve thirteen fourteen";
        assert_eq!(estimate_speech_secs(en), 11);
        assert_eq!(estimate_speech_secs(""), 0);
        assert_eq!(estimate_speech_secs("   "), 0);
    }

    #[test]
    fn estimate_shot_need_includes_speech_tail() {
        // 17 CJK → ceil(17/1.7)=10s speech + 1s lead + 5s tail = 16 → clamp 15
        let line: String = "中".chars().cycle().take(17).collect();
        assert_eq!(line.chars().count(), 17);
        let need = estimate_shot_need_secs(Some(&line), "slow pan", "small");
        assert_eq!(need, MAX_CLIP_DURATION_SECS);
        // Shorter line: 9 CJK → ceil(9/1.7)=6 + 1 + 5 = 12s
        let mid: String = "中".chars().cycle().take(9).collect();
        let mid_need = estimate_shot_need_secs(Some(&mid), "slow pan", "small");
        assert_eq!(mid_need, 12);
        // No dialogue → visual floor (min + small boost)
        let silent = estimate_shot_need_secs(None, "hold", "small");
        assert_eq!(silent, MIN_CLIP_DURATION_SECS);
        // Long dialogue clamped to Seedance max
        let long: String = "中".chars().cycle().take(80).collect();
        let capped = estimate_shot_need_secs(Some(&long), "walk across room", "large");
        assert_eq!(capped, MAX_CLIP_DURATION_SECS);
        // Brief dialogue still gets dialogue floor
        let brief = estimate_shot_need_secs(Some("你好"), "nod", "small");
        assert!(brief >= MIN_DIALOGUE_CLIP_SECS);
    }

    #[test]
    fn allocate_for_content_protects_dialogue_floors() {
        // Dialogue-heavy shot needs ~12s; silent needs 5s; budget 20s.
        let needs = vec![12, 5];
        let durs = allocate_clip_durations_for_content(Some(20), &needs);
        assert_eq!(durs.len(), 2);
        assert!(durs[0] >= 12);
        assert!(durs[1] >= MIN_CLIP_DURATION_SECS);
        assert!(durs.iter().all(|&d| (MIN_CLIP_DURATION_SECS..=MAX_CLIP_DURATION_SECS).contains(&d)));
        assert_eq!(durs.iter().sum::<u32>(), 20);
        // Spare seconds go to the needier (dialogue) shot first.
        assert!(durs[0] >= durs[1]);
    }

    #[test]
    fn allocate_for_content_fits_target_when_floors_overshoot() {
        // Dialogue floors that exceed the reserved fit budget still land near
        // the advertised target after soft-landing is re-applied only when safe.
        let needs = vec![12, 12];
        let durs = allocate_clip_durations_for_content(Some(18), &needs);
        assert_eq!(durs.iter().sum::<u32>(), 18);
        assert!(durs.iter().all(|&d| d >= MIN_DIALOGUE_CLIP_SECS));
        assert!(durs.iter().all(|&d| (MIN_CLIP_DURATION_SECS..=MAX_CLIP_DURATION_SECS).contains(&d)));
    }

    #[test]
    fn allocate_for_content_caps_four_max_clips_to_forty() {
        let needs = vec![15, 15, 15, 15];
        let durs = allocate_clip_durations_for_content(Some(40), &needs);
        // Budget + reserved soft-landing re-applied → sum stays at the user target.
        assert_eq!(durs.iter().sum::<u32>(), 40);
        assert!(durs.iter().all(|&d| d >= MIN_DIALOGUE_CLIP_SECS));
        assert!(durs.iter().all(|&d| d <= MAX_CLIP_DURATION_SECS));
    }

    #[test]
    fn allocate_for_content_never_cuts_dialogue_below_floor() {
        // Extreme under-budget: prefer exceeding target over 5s dialogue clips.
        let needs = vec![12, 12];
        let durs = allocate_clip_durations_for_content(Some(12), &needs);
        assert!(durs.iter().all(|&d| d >= MIN_DIALOGUE_CLIP_SECS));
        // Too tight to reserve soft-landing; dialogue floors alone sum to 18.
        assert_eq!(durs.iter().sum::<u32>(), MIN_DIALOGUE_CLIP_SECS * 2);
    }

    #[test]
    fn fifty_five_second_target_does_not_gain_two_secs_per_shot() {
        // Regression: max-shot packing at 55s used to become 55 + 2*11 = 77.
        let needs = vec![5; 11];
        let durs = allocate_clip_durations_for_content(Some(55), &needs);
        assert_eq!(durs.len(), 11);
        assert_eq!(durs.iter().sum::<u32>(), 55);
    }

    #[test]
    fn splice_tail_padding_respects_seedance_max() {
        let mut durs = vec![5, 13, 14, 15];
        apply_shot_splice_tail_padding(&mut durs);
        assert_eq!(durs, vec![7, 15, 15, 15]);
    }
}
