//! Credit estimation for media workflow planning.

use nomi_config::MediaGenConfig;
use serde_json::{Value, json};

/// Rough credit estimate for a workflow template (no server round-trip).
pub fn estimate_workflow_credits(
    template_id: &str,
    inputs: &Value,
    config: &MediaGenConfig,
) -> CreditEstimate {
    let wf = &config.workflows;
    let duration = inputs
        .get("duration")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::from(config.video.default_duration.max(1))) as u32;
    let max_shots = inputs
        .get("max_shots")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::from(wf.storyboard_max_shots.max(1)))
        .clamp(1, 5) as u32;

    let (image_ops, video_seconds) = match template_id {
        "simple_txt2img" => (1, 0),
        "txt2img" | "img2img" => (1, 0),
        "prompt_refine_txt2video" => (0, duration),
        "long_txt2video" => (0, duration),
        "img2video_direct" => (0, duration),
        "long_img2video_direct" => (0, duration),
        "img2video" | "storyboard_to_video" => (1, duration),
        "long_img2video" => (1, duration),
        "storyboard_multi" => (max_shots, max_shots.saturating_mul(duration)),
        "image_variation" | "image_upscale" => (1, 0),
        "video_extend" => (0, duration),
        _ => (1, duration),
    };

    let image_credits = u64::from(image_ops).saturating_mul(wf.image_min_credits);
    let video_credits = u64::from(video_seconds).saturating_mul(wf.video_credits_per_second);
    let total = image_credits.saturating_add(video_credits);

    CreditEstimate {
        estimated_total: total,
        image_credits,
        video_credits,
        image_operations: image_ops,
        video_seconds,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditEstimate {
    pub estimated_total: u64,
    pub image_credits: u64,
    pub video_credits: u64,
    pub image_operations: u32,
    pub video_seconds: u32,
}

impl CreditEstimate {
    pub fn to_json(&self, balance: Option<i64>) -> Value {
        let sufficient = balance.is_none_or(|b| b >= self.estimated_total as i64);
        json!({
            "estimated_total": self.estimated_total,
            "image_credits": self.image_credits,
            "video_credits": self.video_credits,
            "image_operations": self.image_operations,
            "video_seconds": self.video_seconds,
            "balance": balance,
            "sufficient": sufficient,
            "user_decision_hint": if sufficient {
                "Credits appear sufficient — proceed with media_workflow_run."
            } else {
                "Insufficient credits — ask the user to top up, shorten duration, reduce shots, or pick a cheaper model before running."
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt2img_estimate_uses_image_min() {
        let cfg = MediaGenConfig::default();
        let est = estimate_workflow_credits("txt2img", &json!({}), &cfg);
        assert_eq!(est.image_operations, 1);
        assert_eq!(est.estimated_total, cfg.workflows.image_min_credits);
    }

    #[test]
    fn storyboard_scales_with_shots() {
        let cfg = MediaGenConfig::default();
        let est = estimate_workflow_credits(
            "storyboard_multi",
            &json!({"max_shots": 3, "duration": 5}),
            &cfg,
        );
        assert_eq!(est.image_operations, 3);
        assert_eq!(est.video_seconds, 15);
    }

    #[test]
    fn long_video_credits_scale_with_duration() {
        let cfg = MediaGenConfig::default();
        let est = estimate_workflow_credits("long_txt2video", &json!({"duration": 20}), &cfg);
        assert_eq!(est.video_seconds, 20);
        assert_eq!(
            est.video_credits,
            20u64 * cfg.workflows.video_credits_per_second
        );
    }
}
