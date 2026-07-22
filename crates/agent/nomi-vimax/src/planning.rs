//! Planning helpers: Seedance clips are ≥5s — keep shot counts low and budgets real.

/// Minimum seconds the Flowy / Seedance video API accepts for I2V (and what we bill).
pub const MIN_CLIP_DURATION_SECS: u32 = 5;

/// Soft max per clip (Seedance allows up to 15; keep headroom).
pub const MAX_CLIP_DURATION_SECS: u32 = 12;

/// Default target total length when the user does not specify one.
pub const DEFAULT_TARGET_DURATION_SECS: u32 = 30;

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
