//! Planning helpers: Seedance clips are ≥5s — keep shot counts low and budgets real.

/// Minimum seconds the Flowy / Seedance video API accepts for I2V (and what we bill).
pub const MIN_CLIP_DURATION_SECS: u32 = 5;

/// Soft max per clip (Seedance allows up to 15; keep headroom).
pub const MAX_CLIP_DURATION_SECS: u32 = 12;

/// Default target total length when the user does not specify one.
pub const DEFAULT_TARGET_DURATION_SECS: u32 = 30;

/// Default look when the user leaves style empty.
pub const DEFAULT_VISUAL_STYLE: &str = "cinematic film look, believable designed characters, natural wardrobe and lighting, gently softened facial skin with clear readable features";

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
        "pixar",
        "disney",
        "ghibli",
        "chibi",
    ];
    let positive_en = EN.iter().any(|n| positive_style_needle(&lower_raw, n));
    const ZH: &[&str] = &[
        "动画",
        "动漫",
        "二次元",
        "卡通",
        "漫画",
        "插画",
        "手绘",
        "水彩",
        "水墨",
        "日式",
        "赛璐璐",
        "绘本",
    ];
    let positive_zh = ZH.iter().any(|n| positive_style_needle_zh(raw, n));
    positive_en || positive_zh
}

/// True when `needle` appears as a positive style request (not after not/no/never/forbidden).
fn positive_style_needle(lower: &str, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    let mut start = 0;
    while let Some(rel) = lower[start..].find(&needle) {
        let abs = start + rel;
        let before = &lower[..abs];
        // Use the current clause (after last . ; ! ? or newline) so "FORBIDDEN: a, b, c"
        // still negates later list items.
        let clause_start = before
            .rfind(['.', ';', '!', '?', '\n'])
            .map(|i| i + 1)
            .unwrap_or(0);
        let clause = before[clause_start..].trim_start();
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
    while let Some(rel) = raw[start..].find(needle) {
        let abs = start + rel;
        let before = &raw[..abs];
        let clause_start = before
            .rfind(['。', '；', '！', '？', '.', ';', '\n'])
            .map(|i| i + 1)
            .unwrap_or(0);
        let clause = before[clause_start..].trim_start();
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
            "Visual style: {clipped}. Faces: gently softened skin, clear sharp features (no melt/blur)."
        )
    }
}

/// Face finish for character bible sheets — soft skin, sharp features (avoid collapse).
pub const PORTRAIT_FACE_GUIDANCE: &str = "\
Face finish: gently soften facial skin and beauty lighting only. Keep eyes, brows, nose, mouth sharp and anatomically correct. \
Do NOT melt, warp, or heavy-blur the face. Do NOT make a plastic doll or cheap cartoon unless Style asks for animation.";

/// Face finish when the user requested animation / illustration.
pub const PORTRAIT_FACE_GUIDANCE_STYLIZED: &str = "\
Face finish: premium animated-film character design — clear VOLUME under soft light, sharp readable eyes/brows/nose/mouth, hair strand detail. \
Do NOT flatten into a paper cutout or blank sticker face. Do NOT render photoreal live-action skin or celebrity likeness.";

/// Not a real-person / celebrity likeness (Seedance privacy + originality).
pub const PORTRAIT_NON_REAL_PERSON: &str = "\
IDENTITY SAFETY: fictional designed character only — NOT a real-person portrait, NOT photoreal ID-photo, NOT a celebrity/star likeness, NOT a recognizable famous face. Original character design. \
非真人肖像，无明星样貌，虚构角色造型，禁止做成可辨认的真人/明星脸。";

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
    "非真人肖像，无明星样貌; fictional designed character, not celebrity likeness.";

pub const PORTRAIT_FACE_SHORT: &str =
    "Soft skin, sharp eyes/brows/nose/mouth; no melt, no plastic doll.";

pub const PORTRAIT_FACE_SHORT_STYLIZED: &str =
    "Premium animated-film faces with volume + hair detail; sharp features; no flat paper-doll/cutout; no photoreal.";

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
- Soft painted shading + gentle rim/fill light; sharp readable facial features.
- FORBIDDEN: flat paper cutout, sticker/chibi low-detail, empty blank faces, muddy blur."
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
- Detailed skin texture (gently softened, not plastic), individual hair strands, fabric weave, seams, wrinkles, accessories.
- Natural photographic lighting and shallow depth cues; sharp eyes, brows, nose, mouth.
- FORBIDDEN: anime, manga, cartoon, chibi, 2D model-sheet, cel shading, flat paper doll, illustration lineart, sticker look."
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
    if wants_stylized_non_photoreal(user_style) {
        " Young character: keep child proportions, but render in the SAME animation/illustration Style as adult cast — never switch only kids to a different look.".into()
    } else {
        " Young character: keep child proportions, but render in the SAME Style as adult cast — cinematic character design, not anime/cartoon/chibi.".into()
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
        .clamp(MIN_CLIP_DURATION_SECS, 180)
}

/// Suggested shot count for a **single scene budget** (not the whole film).
/// Each shot burns ≥5s of finished video — keep counts very low.
pub fn suggested_shot_count(budget_secs: u32) -> (u32, u32) {
    let budget = budget_secs.max(MIN_CLIP_DURATION_SECS);
    // Aim ~8–10s of story per clip so 5s API minimum is fully used.
    let ideal = ((budget + 9) / 10).clamp(1, 4);
    let max_shots = (budget / MIN_CLIP_DURATION_SECS).clamp(1, 5);
    (ideal.min(max_shots), max_shots)
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

/// Film-level constraints (develop story / write multi-scene script).
pub fn enrich_requirement_for_film(user_requirement: &str, target_secs: Option<u32>) -> String {
    let target = normalize_target_duration_secs(target_secs);
    let (ideal_scenes, max_scenes) = suggested_scene_count(target);
    let base = user_requirement.trim();
    let block = format!(
        "[VIDEO_DURATION_CONSTRAINTS — MUST FOLLOW]\n\
         - Target finished film length ≈ {target} seconds TOTAL.\n\
         - Prefer about {ideal_scenes} scenes (hard upper bound {max_scenes}).\n\
         - Each rendered shot clip is at least {MIN_CLIP_DURATION_SECS} seconds — never invent micro-beats.\n\
         - Keep the whole story compact so total scenes × shots × {MIN_CLIP_DURATION_SECS}s stays near {target}s."
    );
    if base.is_empty() {
        block
    } else {
        format!("{base}\n\n{block}")
    }
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
    let base = user_requirement.trim();
    // Strip a previous film-level block so we don't double-confuse the LLM with two totals.
    let base = strip_duration_constraint_blocks(base);
    let block = format!(
        "[VIDEO_DURATION_CONSTRAINTS — MUST FOLLOW]\n\
         - This is scene {scene_num}/{scene_count} of a film targeting ≈ {film_total_secs}s total.\n\
         - THIS SCENE budget ≈ {budget} seconds of finished video (NOT the whole film).\n\
         - Each shot clip is ≥{MIN_CLIP_DURATION_SECS}s. Prefer about {ideal} shots; HARD UPPER BOUND: {max_shots} shots for this scene.\n\
         - Pack multiple beats (action + dialogue + reaction + camera move) INTO each shot.\n\
         - Reuse cam_idx whenever possible. Prefer in-shot motion over cutting.\n\
         - If you would create more than {max_shots} shots, merge beats instead.",
        scene_num = scene_idx + 1,
    );
    if base.is_empty() {
        block
    } else {
        format!("{base}\n\n{block}")
    }
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
        if t.starts_with("[VIDEO_DURATION_CONSTRAINTS") {
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

/// Hard max shots for a budget (for post-LLM truncation).
pub fn max_shots_for_budget(budget_secs: u32) -> usize {
    suggested_shot_count(budget_secs).1 as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_visual_style_defaults_to_cinematic_soft_faces() {
        let s = resolve_visual_style("");
        let lower = s.to_ascii_lowercase();
        assert!(lower.contains("cinematic") || lower.contains("film"));
        assert!(lower.contains("soften") || lower.contains("softened") || lower.contains("clear"));
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
    fn portrait_style_asks_for_gentle_face_soften_not_melt() {
        let s = portrait_style_for_generation("cinematic");
        let lower = s.to_ascii_lowercase();
        assert!(lower.contains("soften") || lower.contains("softened"));
        assert!(lower.contains("sharp") || lower.contains("readable") || lower.contains("not melt"));
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
    }

    #[test]
    fn short_budget_allows_only_one_or_two_shots() {
        let (ideal, max) = suggested_shot_count(8);
        assert!(ideal <= 2);
        assert!(max <= 2);
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
}
