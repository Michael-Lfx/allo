//! API prompt trail extraction and user-facing prompt blocks.

use serde::Serialize;
use serde_json::{Value, json};

use super::MediaProvenance;
use crate::workflows::store::WorkflowRunRecord;

#[derive(Debug, Clone, Serialize)]
pub struct ApiPromptEntry {
    pub step: String,
    pub kind: String,
    pub label: String,
    pub api_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_prompt: Option<String>,
}

/// Plain-text block for the assistant to quote to users (WeCom, CLI, Telegram, etc.).
pub fn format_user_prompt_block(provenance: &MediaProvenance) -> Option<String> {
    let api = provenance
        .api_prompt
        .as_deref()
        .or(provenance.refined_prompt.as_deref())
        .or(provenance.prompt.as_deref())?
        .trim();
    if api.is_empty() {
        return None;
    }
    let mut lines = vec![format!("【提交给生成 API 的提示词】\n{api}")];
    if let Some(neg) = non_empty_opt(provenance.negative_prompt.as_deref()) {
        lines.push(format!("【负面提示词】\n{neg}"));
    }
    if let Some(motion) = non_empty_opt(provenance.motion_prompt.as_deref()) {
        lines.push(format!("【运动/镜头描述】\n{motion}"));
    }
    if let Some(orig) = non_empty_opt(provenance.original_prompt.as_deref())
        && orig != api
    {
        lines.push(format!("【您的原始描述】\n{orig}"));
    }
    Some(lines.join("\n\n"))
}

pub fn format_workflow_user_prompt_block(
    original: Option<&str>,
    trail: &[ApiPromptEntry],
) -> Option<String> {
    if trail.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    if let Some(orig) = non_empty_opt(original) {
        lines.push(format!("【您的原始描述】\n{orig}"));
    }
    for entry in trail {
        let header = match entry.kind.as_str() {
            "image_generate" => format!("【{} · 生图提示词】", entry.label),
            "video_generate" => format!("【{} · 生视频提示词】", entry.label),
            "prompt_refine" => format!("【{} · 优化后描述】", entry.label),
            _ => format!("【{}】", entry.label),
        };
        lines.push(format!("{header}\n{}", entry.api_prompt.trim()));
        if let Some(neg) = non_empty_opt(entry.negative_prompt.as_deref()) {
            lines.push(format!("  负面提示词：{neg}"));
        }
        if let Some(motion) = non_empty_opt(entry.motion_prompt.as_deref()) {
            lines.push(format!("  运动描述：{motion}"));
        }
    }
    Some(lines.join("\n\n"))
}

pub fn extract_prompt_trail_from_workflow(record: &WorkflowRunRecord) -> Vec<ApiPromptEntry> {
    let mut trail = Vec::new();
    for (step_id, output) in &record.step_outputs {
        if output.get("shots").is_some() {
            if let Some(shots) = output.get("shots").and_then(Value::as_array) {
                for shot in shots {
                    let shot_no = shot.get("shot").and_then(|v| v.as_u64()).unwrap_or(0);
                    if let Some(img) = shot.get("image") {
                        push_step_api_prompt(
                            &mut trail,
                            format!("shot{shot_no}_image"),
                            "image_generate",
                            format!("分镜 {shot_no} 关键帧"),
                            img,
                        );
                    }
                    if let Some(vid) = shot.get("video") {
                        push_step_api_prompt(
                            &mut trail,
                            format!("shot{shot_no}_video"),
                            "video_generate",
                            format!("分镜 {shot_no} 视频"),
                            vid,
                        );
                    }
                }
            }
            continue;
        }
        if output.get("api_prompt").is_some()
            || output.get("video_url").is_some()
            || output.pointer("/raw/kind").is_some()
        {
            let kind = if output.pointer("/raw/kind").and_then(|v| v.as_str()) == Some("video")
                || output.get("video_url").is_some()
            {
                "video_generate"
            } else {
                "image_generate"
            };
            push_step_api_prompt(
                &mut trail,
                step_id.clone(),
                kind,
                step_label(step_id),
                output,
            );
            continue;
        }
        if let Some(text) = output
            .get("output")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            trail.push(ApiPromptEntry {
                step: step_id.clone(),
                kind: "prompt_refine".into(),
                label: step_label(step_id),
                api_prompt: text.to_string(),
                negative_prompt: output
                    .get("negative_prompt")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                motion_prompt: output
                    .get("motion_prompt")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string),
            });
        }
    }
    trail
}

pub fn workflow_prompt_json(record: &WorkflowRunRecord) -> Value {
    let trail = extract_prompt_trail_from_workflow(record);
    let original = record
        .inputs
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let user_prompt_block = format_workflow_user_prompt_block(original.as_deref(), &trail);
    json!({
        "original_prompt": original,
        "api_prompt_trail": trail,
        "user_prompt_block": user_prompt_block,
        "delivery_note": user_prompt_block.as_ref().map(|_| {
            "Include user_prompt_block verbatim when replying so the user sees the final API prompts. \
             Also include MEDIA: tags for generated files."
        }),
    })
}

pub fn enrich_tool_response_with_prompts(mut value: Value, provenance: &MediaProvenance) -> Value {
    if let Some(block) = format_user_prompt_block(provenance)
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("user_prompt_block".into(), json!(block));
        obj.insert(
            "prompts".into(),
            json!({
                "original_prompt": provenance.original_prompt,
                "api_prompt": provenance.api_prompt,
                "refined_prompt": provenance.refined_prompt,
                "negative_prompt": provenance.negative_prompt,
                "motion_prompt": provenance.motion_prompt,
            }),
        );
        obj.insert(
            "delivery_note".into(),
            json!("Share user_prompt_block with the user so they see the final prompt sent to the API. \
                   Include MEDIA: path when delivering the file."),
        );
    }
    value
}

fn push_step_api_prompt(
    trail: &mut Vec<ApiPromptEntry>,
    step: String,
    kind: &str,
    label: String,
    output: &Value,
) {
    let Some(api_prompt) = extract_api_prompt_from_step_output(output) else {
        return;
    };
    trail.push(ApiPromptEntry {
        step,
        kind: kind.to_string(),
        label,
        api_prompt,
        negative_prompt: extract_negative_from_step_output(output),
        motion_prompt: output
            .get("motion_prompt")
            .or_else(|| output.pointer("/raw/provenance/motion_prompt"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string),
    });
}

fn extract_api_prompt_from_step_output(output: &Value) -> Option<String> {
    output
        .get("api_prompt")
        .and_then(|v| v.as_str())
        .or_else(|| {
            output
                .pointer("/raw/provenance/api_prompt")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            output
                .pointer("/raw/provenance/refined_prompt")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            output
                .pointer("/raw/provenance/prompt")
                .and_then(|v| v.as_str())
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn extract_negative_from_step_output(output: &Value) -> Option<String> {
    output
        .get("negative_prompt")
        .or_else(|| output.pointer("/raw/provenance/negative_prompt"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

fn step_label(step_id: &str) -> String {
    match step_id {
        "refine_prompt" | "refine_scene" => "提示词优化".into(),
        "refine_motion" => "运动描述优化".into(),
        "generate" | "keyframe" => "关键帧生图".into(),
        "video" => "视频生成".into(),
        "qa" => "质量检查".into(),
        other => other.to_string(),
    }
}

fn non_empty_opt(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::store::{WorkflowRunRecord, WorkflowRunStatus};
    use std::collections::HashMap;

    #[test]
    fn user_prompt_block_from_provenance() {
        let block = format_user_prompt_block(&MediaProvenance {
            original_prompt: Some("a cat".into()),
            api_prompt: Some("A fluffy cat on walnut table, morning light...".into()),
            negative_prompt: Some("blurry".into()),
            ..Default::default()
        })
        .expect("block");
        assert!(block.contains("提交给生成 API"));
        assert!(block.contains("fluffy cat"));
        assert!(block.contains("负面提示词"));
    }

    #[test]
    fn workflow_trail_from_step_outputs() {
        let mut record = WorkflowRunRecord {
            run_id: "r1".into(),
            workflow_id: "txt2img".into(),
            status: WorkflowRunStatus::Succeeded,
            inputs: json!({"prompt": "画一只猫"}),
            current_step: None,
            step_outputs: HashMap::new(),
            artifacts: vec![],
            error: None,
        };
        record.step_outputs.insert(
            "refine_prompt".into(),
            json!({
                "output": "A detailed cat illustration...",
                "negative_prompt": "blur"
            }),
        );
        record.step_outputs.insert(
            "generate".into(),
            json!({
                "api_prompt": "A detailed cat illustration...",
                "raw": {"kind": "image", "provenance": {"api_prompt": "A detailed cat illustration..."}}
            }),
        );
        let trail = extract_prompt_trail_from_workflow(&record);
        assert!(trail.len() >= 2);
        let payload = workflow_prompt_json(&record);
        assert!(
            payload["user_prompt_block"]
                .as_str()
                .unwrap()
                .contains("画一只猫")
        );
    }
}
