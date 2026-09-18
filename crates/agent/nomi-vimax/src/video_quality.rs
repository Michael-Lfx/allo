//! Per-model video resolution / fps capabilities for ViMax.
//!
//! Seedance catalogs do not expose structured capability metadata, so we use
//! model-id heuristics aligned with Ark / Seedance 2.0 docs:
//! - standard Seedance 2.0: 480p / 720p / 1080p, fixed 24fps
//! - fast / mini: 480p / 720p (no 1080p), fixed 24fps
//!
//! MiniMax-H3 (MiniMax V2): 768P / 2K — keep in sync with
//! `nomifun_cloud::normalize_minimax_h3_resolution` and FE `videoModelCapabilities.ts`.
//! Wan 3.0 (DashScope): 480P / 720P / 1080P, 2–30s.

use nomifun_cloud::{
    is_minimax_h3_model, is_wan3_model, normalize_minimax_h3_resolution, normalize_wan3_resolution,
    DEFAULT_MINIMAX_H3_RESOLUTION, DEFAULT_WAN3_RESOLUTION, MINIMAX_H3_DURATION_MAX,
    MINIMAX_H3_DURATION_MIN, MINIMAX_H3_RESOLUTIONS, WAN3_DURATION_MAX, WAN3_DURATION_MIN,
    WAN3_RESOLUTIONS,
};

use crate::clip_bounds::ClipBounds;

/// Resolutions offered in the Style & Model UI (subset filtered per model).
pub const VIDEO_RESOLUTIONS: &[&str] = &["480p", "720p", "1080p"];
pub const DEFAULT_VIDEO_RESOLUTION: &str = "720p";
pub const DEFAULT_VIDEO_FPS: u32 = 24;

/// Seedance 2.0 / 2.0-fast I2V accept 4–15s. ViMax bills from 5s so a short-drama
/// beat still has room for a lead-in, the action, and a landing.
const SEEDANCE_CLIP_BOUNDS: ClipBounds = ClipBounds::new(5, 15);

/// MiniMax-H3 window, mirroring `nomifun_cloud::clamp_minimax_h3_duration`.
const MINIMAX_H3_CLIP_BOUNDS: ClipBounds =
    ClipBounds::new(MINIMAX_H3_DURATION_MIN, MINIMAX_H3_DURATION_MAX);

/// Wan 3.0 window, mirroring `nomifun_cloud::clamp_wan3_duration`.
const WAN3_CLIP_BOUNDS: ClipBounds = ClipBounds::new(WAN3_DURATION_MIN, WAN3_DURATION_MAX);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoModelCapabilities {
    pub resolutions: Vec<&'static str>,
    pub fps_options: Vec<u32>,
    /// When true the UI should show fps but not allow changing it.
    pub fps_locked: bool,
    /// Accepted duration window for one generated clip.
    pub clip: ClipBounds,
}

fn model_blob(model: &str) -> String {
    model.to_ascii_lowercase().replace(['_', '.', ' '], "-")
}

fn is_seedance(model: &str) -> bool {
    model_blob(model).contains("seedance")
}

fn is_seedance_fast_or_mini(model: &str) -> bool {
    let b = model_blob(model);
    b.contains("seedance") && (b.contains("fast") || b.contains("mini"))
}

/// Seedance 2.0 / 2.0-fast R2V rejects each `reference_audio` shorter than 1.8s.
/// Submit 1.9s so encoder/probe rounding cannot fall under the floor.
pub const SEEDANCE_REF_AUDIO_MIN_SECS: f64 = 1.9;
/// Combined `reference_audio` ceiling documented for Seedance 2.0 (3 files).
pub const SEEDANCE_REF_AUDIO_TOTAL_BUDGET_SECS: f64 = 15.0;
/// Wan 3.0: keep Σ under 15s with a small mux margin.
pub const WAN3_REF_AUDIO_TOTAL_BUDGET_SECS: f64 = 14.5;
/// Wan 3.0 per-clip trim cap so five slots still fit the combined budget.
pub const WAN3_VOICE_REF_MAX_SECS: f64 = 4.0;

/// How one video model wants `reference_audio` sized at submit time.
///
/// Planning uses [`Self::max_count`] as the unique-named-speaker cap per
/// generated file. Duration floors/ceilings are applied to **copies** at
/// submit so a Wan trim cannot poison a later Seedance run (and vice versa).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceAudioPolicy {
    pub max_count: usize,
    /// Combined duration ceiling. `None` = no sum cap.
    pub max_total_secs: Option<f64>,
    /// Per-clip floor (Seedance R2V ≥ 1.8s). `0` = do not pad.
    pub min_clip_secs: f64,
    /// Per-clip ceiling used when shrinking a copy to fit the total budget.
    pub max_clip_secs: f64,
}

/// Max `reference_audio` clips one generate call should bind for this model.
///
/// Seedance 2.0 documents 3 audio refs (Σ ≤ 15s). Wan 3.0 documents 5 files
/// (Σ ≤ 15s). MiniMax-H3 does not take voice-timbre refs — `0` means packing
/// must not split a row just to stay under a slot count.
///
/// Planning uses this as the unique-named-speaker cap per generated file so
/// every speaking character can keep a timbre bible. Extra speakers become
/// another row (story is kept; nothing is dropped).
pub fn max_reference_audio(model: &str) -> usize {
    reference_audio_policy(model).max_count
}

pub fn reference_audio_policy(model: &str) -> ReferenceAudioPolicy {
    if is_minimax_h3_model(model) {
        ReferenceAudioPolicy {
            max_count: 0,
            max_total_secs: None,
            min_clip_secs: 0.0,
            max_clip_secs: 0.0,
        }
    } else if is_wan3_model(model) {
        ReferenceAudioPolicy {
            max_count: 5,
            max_total_secs: Some(WAN3_REF_AUDIO_TOTAL_BUDGET_SECS),
            min_clip_secs: 0.0,
            max_clip_secs: WAN3_VOICE_REF_MAX_SECS,
        }
    } else if is_seedance(model) {
        ReferenceAudioPolicy {
            max_count: 3,
            max_total_secs: Some(SEEDANCE_REF_AUDIO_TOTAL_BUDGET_SECS),
            min_clip_secs: SEEDANCE_REF_AUDIO_MIN_SECS,
            max_clip_secs: SEEDANCE_REF_AUDIO_TOTAL_BUDGET_SECS,
        }
    } else {
        // Unknown ids: satisfy Seedance's floor and Wan's combined ceiling.
        ReferenceAudioPolicy {
            max_count: 3,
            max_total_secs: Some(WAN3_REF_AUDIO_TOTAL_BUDGET_SECS),
            min_clip_secs: SEEDANCE_REF_AUDIO_MIN_SECS,
            max_clip_secs: WAN3_VOICE_REF_MAX_SECS,
        }
    }
}

/// Per-clip target durations for the prefix of `durations` that fits `policy`.
///
/// Drops trailing clips when `min_clip * n` cannot fit the combined ceiling.
/// Callers write **sidecars** to these targets and leave the canonical wavs alone.
pub fn plan_reference_audio_targets(durations: &[f64], policy: ReferenceAudioPolicy) -> Vec<f64> {
    if policy.max_count == 0 {
        return Vec::new();
    }
    let min_c = policy.min_clip_secs.max(0.0);
    let max_c = if policy.max_clip_secs > 0.0 {
        policy.max_clip_secs
    } else {
        f64::INFINITY
    };
    let max_total = policy.max_total_secs.filter(|v| *v > 0.0).unwrap_or(f64::INFINITY);

    let mut n = durations.len().min(policy.max_count);
    while n > 0 {
        if min_c * n as f64 > max_total + 1e-6 {
            n -= 1;
            continue;
        }
        let per_cap = (max_total / n as f64).min(max_c);
        if per_cap + 1e-6 < min_c {
            n -= 1;
            continue;
        }
        return durations[..n]
            .iter()
            .map(|&d| {
                let d = if d.is_finite() && d > 0.0 { d } else { 0.0 };
                let mut t = d;
                if t + 1e-3 < min_c {
                    t = min_c;
                }
                t.min(per_cap).max(min_c)
            })
            .collect();
    }
    Vec::new()
}

/// Accepted single-clip duration window for a Flowy video model id or display name.
///
/// Unknown ids fall back to [`ClipBounds::DEFAULT`], which every integrated model
/// accepts — an unrecognized model can never be asked for an out-of-range clip.
pub fn clip_bounds_for_model(model: &str) -> ClipBounds {
    if is_minimax_h3_model(model) {
        MINIMAX_H3_CLIP_BOUNDS
    } else if is_wan3_model(model) {
        WAN3_CLIP_BOUNDS
    } else if is_seedance(model) {
        SEEDANCE_CLIP_BOUNDS
    } else {
        ClipBounds::DEFAULT
    }
}

/// Capability set for a Flowy video model id or display name.
pub fn video_model_capabilities(model: &str) -> VideoModelCapabilities {
    let clip = clip_bounds_for_model(model);
    if is_minimax_h3_model(model) {
        return VideoModelCapabilities {
            resolutions: MINIMAX_H3_RESOLUTIONS.to_vec(),
            fps_options: vec![DEFAULT_VIDEO_FPS],
            fps_locked: true,
            clip,
        };
    }
    if is_wan3_model(model) {
        return VideoModelCapabilities {
            resolutions: WAN3_RESOLUTIONS.to_vec(),
            fps_options: vec![DEFAULT_VIDEO_FPS],
            fps_locked: true,
            clip,
        };
    }
    if is_seedance_fast_or_mini(model) {
        return VideoModelCapabilities {
            resolutions: vec!["480p", "720p"],
            fps_options: vec![DEFAULT_VIDEO_FPS],
            fps_locked: true,
            clip,
        };
    }
    if is_seedance(model) {
        return VideoModelCapabilities {
            resolutions: vec!["480p", "720p", "1080p"],
            fps_options: vec![DEFAULT_VIDEO_FPS],
            fps_locked: true,
            clip,
        };
    }
    // Unknown models: expose the common tier; fps stays cinematic 24 until a model
    // advertises more options in the catalog.
    VideoModelCapabilities {
        resolutions: VIDEO_RESOLUTIONS.to_vec(),
        fps_options: vec![DEFAULT_VIDEO_FPS],
        fps_locked: true,
        clip,
    }
}

/// Normalize bare heights / aliases onto Seedance-style tokens (`720p`).
/// Canvas UI historically stored `vquality` as `"720"` (no `p`); accept both.
fn canonicalize_seedance_resolution_token(resolution: &str) -> String {
    let lower = resolution.trim().to_ascii_lowercase().replace(['_', ' '], "");
    match lower.as_str() {
        "low" => "480p".into(),
        "auto" | "medium" | "high" => "720p".into(),
        "4k" | "2160" | "2160p" => "2160p".into(),
        other if other.ends_with('p') => other.to_string(),
        other if !other.is_empty() && other.chars().all(|c| c.is_ascii_digit()) => {
            format!("{other}p")
        }
        _ => lower,
    }
}

/// Normalize a user/config resolution string and clamp to the model's allow-list.
pub fn normalize_resolution_for_model(model: &str, resolution: &str) -> String {
    if is_minimax_h3_model(model) {
        return normalize_minimax_h3_resolution(resolution);
    }
    if is_wan3_model(model) {
        return normalize_wan3_resolution(resolution);
    }
    let raw = canonicalize_seedance_resolution_token(resolution);
    let caps = video_model_capabilities(model);
    if caps.resolutions.iter().any(|r| *r == raw) {
        return raw;
    }
    // Closest fallback: prefer default when allowed, else the model's max tier.
    if caps
        .resolutions
        .iter()
        .any(|r| *r == DEFAULT_VIDEO_RESOLUTION)
    {
        DEFAULT_VIDEO_RESOLUTION.to_string()
    } else {
        caps.resolutions
            .last()
            .copied()
            .unwrap_or(DEFAULT_VIDEO_RESOLUTION)
            .to_string()
    }
}

/// Normalize fps and clamp to the model's allow-list.
pub fn normalize_fps_for_model(model: &str, fps: u32) -> u32 {
    let caps = video_model_capabilities(model);
    if caps.fps_options.contains(&fps) {
        return fps;
    }
    caps.fps_options
        .first()
        .copied()
        .unwrap_or(DEFAULT_VIDEO_FPS)
}

/// Default resolution label for a model when the user has not picked one.
pub fn default_resolution_for_model(model: &str) -> &'static str {
    if is_minimax_h3_model(model) {
        DEFAULT_MINIMAX_H3_RESOLUTION
    } else if is_wan3_model(model) {
        DEFAULT_WAN3_RESOLUTION
    } else {
        DEFAULT_VIDEO_RESOLUTION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seedance_fast_rejects_1080p() {
        let caps = video_model_capabilities("AIPC-Doubao-Seedance-2.0-fast");
        assert_eq!(caps.resolutions, vec!["480p", "720p"]);
        assert!(caps.fps_locked);
        assert_eq!(
            normalize_resolution_for_model("AIPC-Doubao-Seedance-2.0-fast", "1080p"),
            "720p"
        );
    }

    #[test]
    fn seedance_standard_allows_1080p() {
        let caps = video_model_capabilities("AIPC-Doubao-Seedance-2.0");
        assert!(caps.resolutions.contains(&"1080p"));
        assert_eq!(
            normalize_resolution_for_model("AIPC-Doubao-Seedance-2.0", "1080p"),
            "1080p"
        );
        assert_eq!(normalize_fps_for_model("AIPC-Doubao-Seedance-2.0", 60), 24);
    }

    #[test]
    fn seedance_accepts_bare_height_from_canvas_ui() {
        assert_eq!(
            normalize_resolution_for_model("AIPC-Doubao-Seedance-2.0", "1080"),
            "1080p"
        );
        assert_eq!(
            normalize_resolution_for_model("AIPC-Doubao-Seedance-2.0", "480"),
            "480p"
        );
        assert_eq!(
            normalize_resolution_for_model("AIPC-Doubao-Seedance-2.0-fast", "1080"),
            "720p"
        );
    }

    #[test]
    fn minimax_h3_resolutions() {
        let caps = video_model_capabilities("flowy/MiniMax-H3");
        assert_eq!(caps.resolutions, vec!["768P", "2K"]);
        assert_eq!(
            normalize_resolution_for_model("flowy/MiniMax-H3", "720p"),
            "768P"
        );
        assert_eq!(
            normalize_resolution_for_model("AIPC-MiniMax-H3", "1080p"),
            "2K"
        );
        assert_eq!(default_resolution_for_model("MiniMax-H3"), "768P");
    }

    #[test]
    fn wan3_resolutions() {
        let caps = video_model_capabilities("flowy/wan3.0-video");
        assert_eq!(caps.resolutions, vec!["480P", "720P", "1080P"]);
        assert_eq!(
            normalize_resolution_for_model("flowy/wan3.0-video", "720p"),
            "720P"
        );
        assert_eq!(
            normalize_resolution_for_model("AIPC-wan3.0-video-prime", "1080"),
            "1080P"
        );
        assert_eq!(
            normalize_resolution_for_model("flowy/wan3.0-video", "480"),
            "480P"
        );
        assert_eq!(default_resolution_for_model("flowy/wan3.0-video"), "720P");
    }

    #[test]
    fn clip_bounds_follow_the_selected_model() {
        let seedance = clip_bounds_for_model("AIPC-Doubao-Seedance-2.0");
        assert_eq!((seedance.min_secs(), seedance.max_secs()), (5, 15));
        assert_eq!(
            clip_bounds_for_model("AIPC-Doubao-Seedance-2.0-fast"),
            seedance,
            "fast/mini differ in resolution tiers, not in clip length"
        );

        let h3 = clip_bounds_for_model("flowy/MiniMax-H3");
        assert_eq!((h3.min_secs(), h3.max_secs()), (4, 15));

        let wan3 = clip_bounds_for_model("flowy/wan3.0-video");
        assert_eq!((wan3.min_secs(), wan3.max_secs()), (2, 30));
        assert_eq!(clip_bounds_for_model("AIPC-wan3.0-video-prime"), wan3);
        assert_eq!(max_reference_audio("flowy/wan3.0-video"), 5);
        assert_eq!(max_reference_audio("AIPC-Doubao-Seedance-2.0"), 3);
        assert_eq!(max_reference_audio("flowy/MiniMax-H3"), 0);

        assert_eq!(
            clip_bounds_for_model("some-unreleased-model"),
            ClipBounds::DEFAULT
        );
        assert_eq!(
            video_model_capabilities("AIPC-Doubao-Seedance-2.0").clip,
            seedance
        );
    }

    #[test]
    fn seedance_pads_short_reference_audio_clips() {
        let policy = reference_audio_policy("AIPC-Doubao-Seedance-2.0-fast");
        assert_eq!(policy.max_count, 3);
        assert!(policy.min_clip_secs >= 1.8);
        let targets = plan_reference_audio_targets(&[1.2, 1.5, 2.0], policy);
        assert_eq!(targets.len(), 3);
        assert!(targets[0] >= 1.8);
        assert!(targets[1] >= 1.8);
        assert!((targets[2] - 2.0).abs() < 1e-9);
        assert!(targets.iter().sum::<f64>() <= SEEDANCE_REF_AUDIO_TOTAL_BUDGET_SECS + 1e-6);
    }

    #[test]
    fn wan3_trims_copies_instead_of_dropping_speakers() {
        let policy = reference_audio_policy("flowy/wan3.0-video");
        let targets = plan_reference_audio_targets(&[5.2, 5.2, 5.2], policy);
        assert_eq!(targets.len(), 3);
        assert!(targets.iter().all(|&t| t <= WAN3_VOICE_REF_MAX_SECS + 1e-9));
        assert!(targets.iter().sum::<f64>() <= WAN3_REF_AUDIO_TOTAL_BUDGET_SECS + 1e-6);
    }

    #[test]
    fn wan3_five_long_clips_share_the_combined_budget() {
        let policy = reference_audio_policy("flowy/wan3.0-video");
        let targets = plan_reference_audio_targets(&[10.0; 5], policy);
        assert_eq!(targets.len(), 5);
        let expected = WAN3_REF_AUDIO_TOTAL_BUDGET_SECS / 5.0;
        assert!(targets.iter().all(|&t| (t - expected).abs() < 1e-9));
    }

    #[test]
    fn minimax_h3_binds_no_reference_audio() {
        assert!(
            plan_reference_audio_targets(&[3.0, 3.0], reference_audio_policy("flowy/MiniMax-H3"))
                .is_empty()
        );
    }

    #[test]
    fn unknown_model_satisfies_seedance_floor_and_wan_ceiling() {
        let policy = reference_audio_policy("some-unreleased-model");
        let targets = plan_reference_audio_targets(&[1.0, 8.0, 8.0], policy);
        assert_eq!(targets.len(), 3);
        assert!(targets[0] >= 1.8);
        assert!(targets.iter().all(|&t| t <= WAN3_VOICE_REF_MAX_SECS + 1e-9));
        assert!(targets.iter().sum::<f64>() <= WAN3_REF_AUDIO_TOTAL_BUDGET_SECS + 1e-6);
    }
}
