//! Flowy media request normalization (resolution, duration).

use nomifun_cloud::{is_minimax_h3_model, normalize_minimax_h3_resolution};

/// True when `model` looks like a Flowy server model id (`AIPC-...` or `flowy/...`).
pub fn is_flowy_model_id(model: &str) -> bool {
    let m = model.trim();
    if m.is_empty() {
        return false;
    }
    let lower = m.to_ascii_lowercase();
    lower.starts_with("aipc-") || lower.starts_with("flowy/")
}

/// Normalize resolution for the active video model.
/// - Seedance fast/mini: clamp 1080p+ → 720p
/// - MiniMax-H3: map onto `768P` / `2K`
pub fn normalize_video_resolution(model: &str, resolution: &str) -> Option<String> {
    let r = resolution.trim();
    if r.is_empty() {
        return None;
    }
    if is_minimax_h3_model(model) {
        return Some(normalize_minimax_h3_resolution(r));
    }
    let lower = r.to_ascii_lowercase();
    let model_lower = model.to_ascii_lowercase();
    let seedance_capped = model_lower.contains("seedance")
        && (model_lower.contains("fast") || model_lower.contains("mini"));
    if seedance_capped && (lower == "1080p" || lower == "4k") {
        tracing::warn!(
            model,
            requested = %lower,
            "clamping video resolution -> 720p for Seedance fast/mini"
        );
        return Some("720p".to_string());
    }
    Some(lower)
}

/// Cap duration for a single upstream clip (model-specific).
pub fn normalize_video_duration(model: &str, duration: u32) -> u32 {
    let max_clip = crate::video_segment::max_clip_duration_for_model(model);
    if duration > max_clip {
        tracing::warn!(
            model,
            duration,
            max_clip,
            "single video_generate request exceeds max clip; use long video workflow for longer targets"
        );
        max_clip
    } else {
        duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowy_model_id_detection() {
        assert!(is_flowy_model_id("AIPC-Z-Image-Turbo"));
        assert!(is_flowy_model_id("flowy/doubao-seedance-1-0-pro"));
        assert!(!is_flowy_model_id("seedance-2.0"));
        assert!(!is_flowy_model_id("pixverse-v6"));
    }

    #[test]
    fn resolution_clamp_for_seedance_fast_only() {
        assert_eq!(
            normalize_video_resolution("flowy/doubao-seedance-fast", "1080p").as_deref(),
            Some("720p")
        );
        assert_eq!(
            normalize_video_resolution("AIPC-Doubao-Seedance-2.0", "1080p").as_deref(),
            Some("1080p")
        );
        assert_eq!(
            normalize_video_resolution("other-model", "1080p").as_deref(),
            Some("1080p")
        );
    }

    #[test]
    fn resolution_maps_for_minimax_h3() {
        assert_eq!(
            normalize_video_resolution("flowy/MiniMax-H3", "720p").as_deref(),
            Some("768P")
        );
        assert_eq!(
            normalize_video_resolution("AIPC-MiniMax-H3", "1080p").as_deref(),
            Some("2K")
        );
    }
}
