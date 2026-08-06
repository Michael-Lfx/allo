//! Flowy media request normalization (resolution, duration).

/// True when `model` looks like a Flowy server model id (`AIPC-...` or `flowy/...`).
pub fn is_flowy_model_id(model: &str) -> bool {
    let m = model.trim();
    if m.is_empty() {
        return false;
    }
    let lower = m.to_ascii_lowercase();
    lower.starts_with("aipc-") || lower.starts_with("flowy/")
}

/// Seedance fast/mini tiers reject 1080p(+); clamp those to 720p.
/// Standard Seedance 2.0 keeps 1080p.
pub fn normalize_video_resolution(model: &str, resolution: &str) -> Option<String> {
    let r = resolution.trim().to_ascii_lowercase();
    if r.is_empty() {
        return None;
    }
    let model_lower = model.to_ascii_lowercase();
    let seedance_capped = model_lower.contains("seedance")
        && (model_lower.contains("fast") || model_lower.contains("mini"));
    if seedance_capped && (r == "1080p" || r == "4k") {
        tracing::warn!(
            model,
            requested = %r,
            "clamping video resolution -> 720p for Seedance fast/mini"
        );
        return Some("720p".to_string());
    }
    Some(r)
}

/// Cap duration for Seedance (upstream max ~10s per task).
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
}
