//! Per-model video resolution / fps capabilities for ViMax.
//!
//! Seedance catalogs do not expose structured capability metadata, so we use
//! model-id heuristics aligned with Ark / Seedance 2.0 docs:
//! - standard Seedance 2.0: 480p / 720p / 1080p, fixed 24fps
//! - fast / mini: 480p / 720p (no 1080p), fixed 24fps
//!
//! MiniMax-H3 (MiniMax V2): 768P / 2K — keep in sync with
//! `nomifun_cloud::normalize_minimax_h3_resolution` and FE `videoModelCapabilities.ts`.

use nomifun_cloud::{
    is_minimax_h3_model, normalize_minimax_h3_resolution, DEFAULT_MINIMAX_H3_RESOLUTION,
    MINIMAX_H3_DURATION_MAX, MINIMAX_H3_DURATION_MIN, MINIMAX_H3_RESOLUTIONS,
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

/// Accepted single-clip duration window for a Flowy video model id or display name.
///
/// Unknown ids fall back to [`ClipBounds::DEFAULT`], which every integrated model
/// accepts — an unrecognized model can never be asked for an out-of-range clip.
pub fn clip_bounds_for_model(model: &str) -> ClipBounds {
    if is_minimax_h3_model(model) {
        MINIMAX_H3_CLIP_BOUNDS
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

        assert_eq!(
            clip_bounds_for_model("some-unreleased-model"),
            ClipBounds::DEFAULT
        );
        assert_eq!(
            video_model_capabilities("AIPC-Doubao-Seedance-2.0").clip,
            seedance
        );
    }
}
