use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{Value, json};

use nomi_types::{JsonSchema, ToolError, ToolHandler, ToolSchema, tool_schema};

use crate::delivery::workflow_prompt_json;
use crate::long_video_active::find_resumable_long_video_run;
use crate::long_video_plan::resolve_target_duration;
use crate::video_segment::route_long_video_template;
use crate::workflows::WorkflowPlan;
use crate::workflows::definition::parse_workflow_plan;
use crate::workflows::runner::WorkflowRunner;
use crate::workflows::store::WorkflowRunStatus;
use crate::workflows::templates::{builtin_template, default_template_inputs, suggest_template_id};

pub struct MediaWorkflowRunHandler {
    runner: Arc<WorkflowRunner>,
}

impl MediaWorkflowRunHandler {
    pub fn new(runner: Arc<WorkflowRunner>) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl ToolHandler for MediaWorkflowRunHandler {
    async fn execute(&self, params: Value) -> Result<String, ToolError> {
        if let Some(run_id) = params
            .get("resume_run_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let record = self.runner.resume_run_sync(run_id).await?;
            return Ok(serialize_run_result(&record));
        }

        let plan: WorkflowPlan = if let Some(plan_val) = params.get("plan").filter(|v| {
            v.as_object().is_some_and(|obj| !obj.is_empty())
        }) {
            parse_workflow_plan(plan_val.clone())
                .map_err(|e| ToolError::InvalidParams(e))?
        } else {
            let prompt = params
                .get("prompt")
                .or_else(|| params.get("objective"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let explicit_workflow_id = params
                .get("workflow_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let prompt = prompt.ok_or_else(|| {
                ToolError::InvalidParams(
                    "provide 'plan' (from media_workflow_plan), or 'workflow_id' + 'prompt'/'objective'. \
                     Example: {\"workflow_id\":\"long_txt2video\",\"prompt\":\"...\",\"duration\":20}"
                        .into(),
                )
            })?;

            let has_image = params
                .get("image_url")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            let media_cfg = &self.runner.executor().services.media;
            let mut workflow_id = explicit_workflow_id
                .map(str::to_string)
                .unwrap_or_else(|| {
                    suggest_template_id(prompt, has_image, &media_cfg.workflows.default_templates)
                });
            let default_duration = media_cfg.video.default_duration;
            let target_duration = params
                .get("duration")
                .and_then(|v| v.as_u64())
                .map(|d| d as u32)
                .unwrap_or_else(|| resolve_target_duration(None, prompt, default_duration));
            let model = media_cfg.video.model.clone();
            workflow_id = route_long_video_template(&workflow_id, target_duration, &model);
            let def = builtin_template(&workflow_id).ok_or_else(|| {
                ToolError::InvalidParams(format!("unknown workflow_id: {workflow_id}"))
            })?;
            let mut inputs = default_template_inputs(&workflow_id, prompt, None);
            inputs["duration"] = json!(target_duration);
            if let Some(url) = params.get("image_url") {
                inputs["image_url"] = url.clone();
            }
            if let Some(duration) = params.get("duration") {
                inputs["duration"] = duration.clone();
            }
            WorkflowPlan::from_definition(&def, inputs)
        };

        let force_new = params
            .get("force_new")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let target_duration = plan
            .inputs
            .get("duration")
            .and_then(|v| v.as_u64())
            .map(|d| d as u32)
            .unwrap_or(self.runner.executor().services.media.video.default_duration);

        if !force_new
            && plan.workflow_id.starts_with("long_")
            && let Some(prior) = find_resumable_long_video_run(
                self.runner.store().as_ref(),
                Some(target_duration),
                |run_id| self.runner.control().contains(run_id),
            )
        {
            nomi_types::report_tool_progress(format!(
                "检测到未完成的长视频任务（run_id={}），正在续传已保存的分段…",
                prior.run_id
            ));
            let wait = params
                .get("wait")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if wait {
                let record = self.runner.resume_run_sync(&prior.run_id).await?;
                return Ok(serialize_run_result(&record));
            }
            let run_id = self.runner.spawn_resume(&prior.run_id)?;
            return Ok(json!({
                "success": true,
                "run_id": run_id,
                "status": "running",
                "async": true,
                "resumed": true,
                "hint": "Call media_workflow_status ONCE with this run_id (wait defaults to true). Do NOT poll in a loop."
            })
            .to_string());
        }

        let wait = params
            .get("wait")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if wait {
            let record = self.runner.run_plan_sync(&plan).await?;
            return Ok(serialize_run_result(&record));
        }

        let workflow_id = plan.workflow_id.clone();
        let run_id = self.runner.spawn_plan(plan)?;
        Ok(json!({
            "success": true,
            "run_id": run_id,
            "status": "running",
            "async": true,
            "workflow_id": workflow_id,
            "hint": "Call media_workflow_status ONCE with this run_id (wait defaults to true). Do NOT poll in a loop."
        })
        .to_string())
    }

    fn schema(&self) -> ToolSchema {
        let mut props = IndexMap::new();
        props.insert(
            "force_new".into(),
            json!({
                "type": "boolean",
                "description": "When true, start a fresh long-video run even if an incomplete run exists for the same duration."
            }),
        );
        props.insert(
            "resume_run_id".into(),
            json!({
                "type": "string",
                "description": "Resume a failed media workflow run (e.g. long video after credit top-up). Use run_id from the earlier failure; do NOT start a new 10s clip."
            }),
        );
        props.insert(
            "plan".into(),
            json!({"type":"object","description":"Plan object from media_workflow_plan (preferred)"}),
        );
        props.insert(
            "workflow_id".into(),
            json!({"type":"string","description":"Builtin template id when plan is omitted (e.g. long_txt2video). Optional if prompt clearly implies the workflow."}),
        );
        props.insert(
            "prompt".into(),
            json!({"type":"string","description":"User objective when plan is omitted. Required unless plan or resume_run_id is provided."}),
        );
        props.insert(
            "objective".into(),
            json!({"type":"string","description":"Alias for prompt when plan is omitted."}),
        );
        props.insert(
            "duration".into(),
            json!({"type":"integer","description":"Video duration in seconds when plan is omitted (e.g. 20 for long video)."}),
        );
        props.insert(
            "image_url".into(),
            json!({"type":"string","description":"Optional reference image URL when plan is omitted (img2video)."}),
        );
        props.insert(
            "wait".into(),
            json!({
                "type": "boolean",
                "description": "When true (default), block server-side until the workflow finishes — do NOT poll in a loop. Set false only for manual background runs."
            }),
        );
        tool_schema(
            "media_workflow_run",
            "Execute a media workflow. Prefer plan from media_workflow_plan. Or pass workflow_id+prompt (or prompt alone). Empty {} is invalid. Default wait=true blocks until complete.",
            JsonSchema::object(props, vec![]),
        )
    }
}

fn serialize_run_result(record: &crate::workflows::store::WorkflowRunRecord) -> String {
    let media_tags: Vec<String> = record
        .artifacts
        .iter()
        .filter_map(|a| a.get("local_path").and_then(|p| p.as_str()))
        .map(|p| format!("MEDIA:{p}"))
        .collect();

    let prompt_payload = workflow_prompt_json(record);
    let user_prompt_block = prompt_payload
        .get("user_prompt_block")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let mut hint_parts = Vec::new();
    if let Some(block) = &user_prompt_block {
        hint_parts.push(format!(
            "Include user_prompt_block in your reply so the user sees the final API prompts:\n{block}"
        ));
    }
    if !media_tags.is_empty() {
        hint_parts.push(format!(
            "Include {} for native media delivery",
            media_tags.join(" ")
        ));
    }

    let mut body = json!({
        "success": record.status == WorkflowRunStatus::Succeeded,
        "run_id": record.run_id,
        "workflow_id": record.workflow_id,
        "status": record.status,
        "error": record.error,
        "artifacts": record.artifacts,
        "step_outputs": record.step_outputs,
        "media_tags": media_tags,
        "manifest_path": format!("~/.nomifun/media/workflows/{}/manifest.json", record.run_id),
        "hint": if hint_parts.is_empty() { Value::Null } else { json!(hint_parts.join("\n\n")) },
    });
    if let (Some(obj), Some(prompts)) = (body.as_object_mut(), prompt_payload.as_object()) {
        for (key, value) in prompts {
            obj.insert(key.clone(), value.clone());
        }
    }
    body.to_string()
}
