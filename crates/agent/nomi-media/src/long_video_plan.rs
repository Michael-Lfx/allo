//! Build long-video workflow plans and map workflow results back to video_generate shape.

use serde_json::json;

use nomi_types::ToolError;
use nomi_tools::tools::video::VideoGenerateRequest;

use crate::video_segment::{
    needs_long_video_pipeline, parse_duration_secs_from_text, route_long_video_template,
};
use crate::workflows::WorkflowPlan;
use crate::workflows::store::{WorkflowRunRecord, WorkflowRunStatus};
use crate::workflows::templates::{builtin_template, default_template_inputs};

/// Resolve target seconds from explicit param, prompt text, or config default.
pub fn resolve_target_duration(duration: Option<u32>, prompt: &str, default_duration: u32) -> u32 {
    duration
        .filter(|d| *d > 0)
        .or_else(|| parse_duration_secs_from_text(prompt))
        .unwrap_or(default_duration.max(1))
}

/// True when this video request should use segmented long-video workflow.
pub fn video_request_needs_long_pipeline(
    request: &VideoGenerateRequest,
    model: &str,
    default_duration: u32,
) -> bool {
    let target = resolve_target_duration(request.duration, &request.prompt, default_duration);
    needs_long_video_pipeline(
        target,
        crate::video_segment::max_clip_duration_for_model(model),
    )
}

/// Build a long-video workflow plan from a single-shot `video_generate` request.
pub fn build_long_video_plan_from_request(
    request: &VideoGenerateRequest,
    target_duration: u32,
    model: &str,
) -> Result<WorkflowPlan, ToolError> {
    let has_image = request
        .image_url
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let base = if has_image {
        "img2video_direct"
    } else {
        "prompt_refine_txt2video"
    };
    let template_id = route_long_video_template(base, target_duration, model);
    let def = builtin_template(&template_id).ok_or_else(|| {
        ToolError::ExecutionFailed(format!("long video template missing: {template_id}"))
    })?;

    let mut inputs = default_template_inputs(&template_id, &request.prompt, None);
    inputs["duration"] = json!(target_duration);
    inputs["aspect_ratio"] = json!(request.aspect_ratio);
    inputs["resolution"] = json!(request.resolution);
    if let Some(url) = &request.image_url {
        inputs["image_url"] = json!(url);
    }
    if let Some(model) = &request.model {
        inputs["model"] = json!(model);
    }

    Ok(WorkflowPlan::from_definition(&def, inputs))
}

/// Extract a `video_generate`-compatible JSON string from a completed workflow run.
pub fn video_tool_response_from_workflow(record: &WorkflowRunRecord) -> Result<String, ToolError> {
    if record.status != WorkflowRunStatus::Succeeded {
        return Err(ToolError::ExecutionFailed(format!(
            "long video workflow failed: {}",
            record.error.as_deref().unwrap_or("unknown error")
        )));
    }

    for output in record.step_outputs.values() {
        if let Some(raw) = output.get("output").or_else(|| output.get("raw"))
            && (raw.get("video").is_some() || raw.get("assets").is_some())
        {
            let mut body = raw.clone();
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "long_video".into(),
                    json!({
                        "workflow_id": record.workflow_id,
                        "run_id": record.run_id,
                        "segment_plan": output.get("segment_plan"),
                    }),
                );
                obj.insert(
                    "delivery_note".into(),
                    json!("Long video assembled from multiple Seedance clips. Deliver with MEDIA: when local_path is available."),
                );
            }
            return Ok(body.to_string());
        }
    }

    Err(ToolError::ExecutionFailed(
        "long video workflow succeeded but no video output found in step_outputs".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_tools::tools::video::VideoGenerateRequest;

    fn sample_request(duration: Option<u32>, prompt: &str) -> VideoGenerateRequest {
        VideoGenerateRequest {
            prompt: prompt.to_string(),
            model: None,
            model_explicit: false,
            image_url: None,
            reference_image_urls: vec![],
            duration,
            aspect_ratio: "16:9".into(),
            resolution: "720p".into(),
            negative_prompt: None,
            audio: None,
            seed: None,
            last_frame_url: None,
            reference_video_url: None,
            reference_audio_url: None,
            generate_audio: None,
        }
    }

    #[test]
    fn detects_long_pipeline_from_duration_param() {
        let req = sample_request(Some(20), "a cat running");
        assert!(video_request_needs_long_pipeline(&req, "seedance", 5));
    }

    #[test]
    fn detects_long_pipeline_from_prompt_text() {
        let req = sample_request(None, "生成一段20秒的产品宣传视频");
        assert!(video_request_needs_long_pipeline(&req, "seedance", 5));
    }

    #[test]
    fn short_request_stays_single_clip() {
        let req = sample_request(Some(8), "short clip");
        assert!(!video_request_needs_long_pipeline(&req, "seedance", 5));
    }

    #[test]
    fn build_plan_routes_to_long_txt2video() {
        let req = sample_request(Some(20), "promo");
        let plan = build_long_video_plan_from_request(&req, 20, "seedance").expect("plan");
        assert_eq!(plan.workflow_id, "long_txt2video");
        assert_eq!(
            plan.inputs.get("duration").and_then(|v| v.as_u64()),
            Some(20)
        );
    }
}
