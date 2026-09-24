//! Bind live agent-eval cases to conversation shells that look like real sessions.

use std::collections::HashMap;

use async_trait::async_trait;
use nomi_agent_eval::EvalTrajectoryEvent;
use nomifun_ai_agent::protocol::events::tool_call::{ToolCallEventData, ToolCallStatus};
use nomifun_ai_agent::{EvalSessionBridge, OpenEvalCaseSession, RecordEvalCaseTurn};
use nomifun_api_types::CreateConversationRequest;
use nomifun_common::{
    generate_id, now_ms, AgentType, AppError, ConversationSource, DecisionPolicy, DelegationPolicy,
    ProviderWithModel,
};
use nomifun_db::models::MessageRow;
use serde_json::json;
use tracing::warn;

use crate::service::ConversationService;

/// Creates eval-tagged conversations and projects trajectories into chat messages.
pub struct ConversationEvalSessionBridge {
    service: ConversationService,
}

impl ConversationEvalSessionBridge {
    pub fn new(service: ConversationService) -> Self {
        Self { service }
    }
}

#[async_trait]
impl EvalSessionBridge for ConversationEvalSessionBridge {
    async fn open_case_session(&self, req: OpenEvalCaseSession) -> Result<String, AppError> {
        let creation_key = if req.trial <= 1 {
            format!("eval:{}:{}", req.run_id, req.case_id)
        } else {
            format!("eval:{}:{}:t{}", req.run_id, req.case_id, req.trial)
        };
        // Bind the conversation shell to the run's business-named parent
        // workspace so SessionList groups all cases under that workpath
        // (not 默认工作空间). Agent cwd / write_root still use the case subdir.
        let run_workspace = req.run_workspace.to_string_lossy().into_owned();
        let case_workspace = req.workspace.to_string_lossy().into_owned();
        let name = format!("{} · {}", req.case_id, req.case_category);
        let task_profile = req
            .task_profile
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("office");
        let create_req = CreateConversationRequest {
            r#type: AgentType::Nomi,
            name: Some(name),
            model: Some(ProviderWithModel {
                provider_id: req.provider_id,
                model: req.model,
                use_model: None,
            }),
            source: Some(ConversationSource::Nomifun),
            channel_chat_id: None,
            preset_id: None,
            preset_overrides: None,
            delegation_policy: DelegationPolicy::Disabled,
            execution_model_pool: None,
            decision_policy: DecisionPolicy::default(),
            execution_template_id: None,
            extra: json!({
                "workspace": run_workspace,
                "origin": "eval",
                "eval": true,
                "eval_run_id": req.run_id,
                "eval_case_id": req.case_id,
                "eval_suite": req.suite,
                "eval_case_workspace": case_workspace,
                "eval_run_workspace_label": req.run_workspace_label,
                "task_profile": task_profile,
                "session_mode": "yolo",
            }),
        };
        let response = self
            .service
            .create_idempotent(&req.user_id, create_req, &creation_key)
            .await?;
        Ok(response.conversation_id)
    }

    async fn record_case_turn(&self, req: RecordEvalCaseTurn) -> Result<(), AppError> {
        let _ = self.service.get(&req.user_id, &req.conversation_id).await?;
        let root_turn_id = req.root_turn_id.trim();
        if root_turn_id.is_empty() {
            return Err(AppError::BadRequest(
                "eval root_turn_id must not be empty".into(),
            ));
        }

        let rows = project_eval_turn_rows(
            &req.conversation_id,
            root_turn_id,
            &req.user_prompt,
            &req.assistant_text,
            &req.trajectory,
            now_ms(),
        );

        for row in &rows {
            persist_projected_row(&self.service, row).await?;
        }

        self.service.broadcast_list_changed(
            &req.user_id,
            &req.conversation_id,
            "updated",
            Some(&ConversationSource::Nomifun),
        );

        let total = req.usage.input_tokens.saturating_add(req.usage.output_tokens);
        let usage_patch = json!({
            "last_token_usage": {
                "total_tokens": total,
                "input_tokens": req.usage.input_tokens,
                "output_tokens": req.usage.output_tokens,
                "elapsed_ms": req.usage.elapsed_ms,
            },
            "eval_last_success": req.success,
            "eval_last_turns": req.usage.turns,
        });
        if let Err(error) = self
            .service
            .update_extra(&req.conversation_id, usage_patch)
            .await
        {
            warn!(
                conversation_id = %req.conversation_id,
                error = %error,
                "eval conversation usage persist failed"
            );
        }
        Ok(())
    }
}

async fn persist_projected_row(
    service: &ConversationService,
    row: &MessageRow,
) -> Result<(), AppError> {
    if let Ok(Some(_)) = service
        .conversation_repo()
        .get_message(&row.conversation_id, &row.message_id)
        .await
    {
        let update = nomifun_db::MessageRowUpdate {
            content: Some(row.content.clone()),
            status: Some(row.status.clone()),
            hidden: Some(row.hidden),
        };
        service
            .conversation_repo()
            .update_message(&row.message_id, &update)
            .await
            .map_err(AppError::from)?;
        return Ok(());
    }

    service
        .conversation_repo()
        .insert_message(row)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

/// Project a captured eval trajectory into the same durable message shapes
/// ChatLayout uses for a normal Nomi turn (process rail + final answer).
pub(crate) fn project_eval_turn_rows(
    conversation_id: &str,
    root_turn_id: &str,
    user_prompt: &str,
    assistant_text: &str,
    trajectory: &[EvalTrajectoryEvent],
    started_at: i64,
) -> Vec<MessageRow> {
    let mut cursor = started_at;
    let mut rows = Vec::new();
    rows.push(text_row(
        conversation_id,
        root_turn_id,
        root_turn_id,
        user_prompt,
        "right",
        root_turn_id,
        cursor,
    ));

    let mut thinking_chunks: Vec<String> = Vec::new();
    let mut thinking_started_ms: Option<u64> = None;
    let mut thinking_ended_ms: Option<u64> = None;
    let mut pending_text: Vec<String> = Vec::new();
    let mut last_flushed_text: Option<String> = None;
    let mut tool_message_ids: HashMap<String, String> = HashMap::new();
    let mut pending_tools: HashMap<String, (String, Option<serde_json::Value>)> = HashMap::new();

    let flush_thinking = |chunks: &mut Vec<String>,
                          started: &mut Option<u64>,
                          ended: &mut Option<u64>,
                          cursor: &mut i64,
                          rows: &mut Vec<MessageRow>| {
        if chunks.is_empty() {
            *started = None;
            *ended = None;
            return;
        }
        let content = chunks.join("\n");
        chunks.clear();
        let duration_ms = match (*started, *ended) {
            (Some(start), Some(end)) if end >= start => end.saturating_sub(start),
            _ => 0,
        };
        *started = None;
        *ended = None;
        *cursor = cursor.saturating_add(1);
        rows.push(thinking_row(
            conversation_id,
            root_turn_id,
            &content,
            duration_ms,
            *cursor,
        ));
    };

    let flush_text = |chunks: &mut Vec<String>,
                      last_flushed: &mut Option<String>,
                      cursor: &mut i64,
                      rows: &mut Vec<MessageRow>| {
        if chunks.is_empty() {
            return;
        }
        let content = chunks.join("");
        chunks.clear();
        if content.trim().is_empty() {
            return;
        }
        *last_flushed = Some(content.clone());
        *cursor = cursor.saturating_add(1);
        rows.push(text_row(
            conversation_id,
            &generate_id(),
            root_turn_id,
            &content,
            "left",
            root_turn_id,
            *cursor,
        ));
    };

    for event in trajectory {
        cursor = cursor.saturating_add(1);
        match event.kind.as_str() {
            "thinking" => {
                if let Some(chunk) = event
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    if thinking_started_ms.is_none() {
                        thinking_started_ms = Some(event.ts_ms);
                    }
                    thinking_ended_ms = Some(event.ts_ms);
                    thinking_chunks.push(chunk.to_owned());
                }
            }
            "text" => {
                flush_thinking(
                    &mut thinking_chunks,
                    &mut thinking_started_ms,
                    &mut thinking_ended_ms,
                    &mut cursor,
                    &mut rows,
                );
                if let Some(chunk) = event
                    .content
                    .as_deref()
                    .filter(|value| !value.is_empty())
                {
                    pending_text.push(chunk.to_owned());
                }
            }
            "tool_call" => {
                flush_thinking(
                    &mut thinking_chunks,
                    &mut thinking_started_ms,
                    &mut thinking_ended_ms,
                    &mut cursor,
                    &mut rows,
                );
                flush_text(
                    &mut pending_text,
                    &mut last_flushed_text,
                    &mut cursor,
                    &mut rows,
                );
                let call_id = event
                    .tool_use_id
                    .clone()
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(generate_id);
                let name = event
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned());
                let args = event
                    .input
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .unwrap_or_else(|| json!(event.input.clone().unwrap_or_default()));
                pending_tools.insert(call_id.clone(), (name.clone(), Some(args.clone())));
                let message_id = generate_id();
                tool_message_ids.insert(call_id.clone(), message_id.clone());
                rows.push(tool_row(
                    conversation_id,
                    root_turn_id,
                    &message_id,
                    &call_id,
                    &name,
                    Some(args),
                    None,
                    ToolCallStatus::Running,
                    cursor,
                ));
            }
            "tool_result" => {
                flush_thinking(
                    &mut thinking_chunks,
                    &mut thinking_started_ms,
                    &mut thinking_ended_ms,
                    &mut cursor,
                    &mut rows,
                );
                flush_text(
                    &mut pending_text,
                    &mut last_flushed_text,
                    &mut cursor,
                    &mut rows,
                );
                let call_id = event
                    .tool_use_id
                    .clone()
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(generate_id);
                let (name, args) = pending_tools.remove(&call_id).unwrap_or_else(|| {
                    (
                        event.name.clone().unwrap_or_else(|| "unknown".to_owned()),
                        None,
                    )
                });
                let message_id = tool_message_ids
                    .get(&call_id)
                    .cloned()
                    .unwrap_or_else(generate_id);
                let status = if event.is_error == Some(true) {
                    ToolCallStatus::Error
                } else {
                    ToolCallStatus::Completed
                };
                rows.push(tool_row(
                    conversation_id,
                    root_turn_id,
                    &message_id,
                    &call_id,
                    &name,
                    args,
                    event.content.clone(),
                    status,
                    cursor,
                ));
            }
            "error" => {
                flush_thinking(
                    &mut thinking_chunks,
                    &mut thinking_started_ms,
                    &mut thinking_ended_ms,
                    &mut cursor,
                    &mut rows,
                );
                if let Some(content) = event
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    rows.push(tips_row(conversation_id, root_turn_id, content, cursor));
                }
            }
            "info" => {
                flush_thinking(
                    &mut thinking_chunks,
                    &mut thinking_started_ms,
                    &mut thinking_ended_ms,
                    &mut cursor,
                    &mut rows,
                );
                if let Some(content) = event
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    rows.push(info_row(conversation_id, root_turn_id, content, cursor));
                }
            }
            _ => {}
        }
    }

    flush_thinking(
        &mut thinking_chunks,
        &mut thinking_started_ms,
        &mut thinking_ended_ms,
        &mut cursor,
        &mut rows,
    );
    flush_text(
        &mut pending_text,
        &mut last_flushed_text,
        &mut cursor,
        &mut rows,
    );

    let final_text = assistant_text.trim();
    let already_flushed = last_flushed_text
        .as_deref()
        .is_some_and(|text| text.trim() == final_text);
    if !final_text.is_empty() && !already_flushed {
        cursor = cursor.saturating_add(1);
        rows.push(text_row(
            conversation_id,
            &generate_id(),
            root_turn_id,
            assistant_text,
            "left",
            root_turn_id,
            cursor,
        ));
    }

    rows
}

fn text_row(
    conversation_id: &str,
    message_id: &str,
    msg_id: &str,
    content: &str,
    position: &str,
    turn_id: &str,
    created_at: i64,
) -> MessageRow {
    MessageRow {
        id: 0,
        message_id: message_id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(msg_id.to_owned()),
        r#type: "text".into(),
        content: json!({
            "content": content,
            "turn_id": turn_id,
        })
        .to_string(),
        position: Some(position.into()),
        status: Some("finish".into()),
        hidden: false,
        created_at,
    }
}

fn thinking_row(
    conversation_id: &str,
    root_turn_id: &str,
    content: &str,
    duration_ms: u64,
    created_at: i64,
) -> MessageRow {
    // Match live stream_relay: thinking is self-rooted. ChatLayout groups it
    // onto the user turn via content.turn_id, not parent msg_id.
    let message_id = generate_id();
    MessageRow {
        id: 0,
        message_id: message_id.clone(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(message_id),
        r#type: "thinking".into(),
        content: json!({
            "content": content,
            "status": "done",
            "duration": duration_ms,
            "duration_ms": duration_ms,
            "turn_id": root_turn_id,
        })
        .to_string(),
        position: Some("left".into()),
        status: Some("finish".into()),
        hidden: false,
        created_at,
    }
}

fn info_row(
    conversation_id: &str,
    root_turn_id: &str,
    content: &str,
    created_at: i64,
) -> MessageRow {
    MessageRow {
        id: 0,
        message_id: generate_id(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(root_turn_id.to_owned()),
        r#type: "tips".into(),
        content: json!({
            "content": content,
            "type": "warning",
            "turn_id": root_turn_id,
        })
        .to_string(),
        position: Some("left".into()),
        status: Some("finish".into()),
        hidden: false,
        created_at,
    }
}

fn tips_row(
    conversation_id: &str,
    root_turn_id: &str,
    content: &str,
    created_at: i64,
) -> MessageRow {
    MessageRow {
        id: 0,
        message_id: generate_id(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(root_turn_id.to_owned()),
        r#type: "tips".into(),
        content: json!({
            "content": content,
            "type": "error",
            "turn_id": root_turn_id,
        })
        .to_string(),
        position: Some("left".into()),
        status: Some("error".into()),
        hidden: false,
        created_at,
    }
}

#[allow(clippy::too_many_arguments)]
fn tool_row(
    conversation_id: &str,
    root_turn_id: &str,
    message_id: &str,
    call_id: &str,
    name: &str,
    args: Option<serde_json::Value>,
    output: Option<String>,
    status: ToolCallStatus,
    created_at: i64,
) -> MessageRow {
    let data = ToolCallEventData {
        call_id: call_id.to_owned(),
        name: name.to_owned(),
        args: args.clone().unwrap_or(json!({})),
        status,
        input: args,
        output,
        description: None,
        retry: None,
        artifacts: Vec::new(),
    };
    let mut content_value = serde_json::to_value(&data).unwrap_or_else(|_| json!({}));
    if let Some(object) = content_value.as_object_mut() {
        object.insert("turn_id".to_owned(), json!(root_turn_id));
    }
    let status_str = match status {
        ToolCallStatus::Running => "work",
        ToolCallStatus::Completed | ToolCallStatus::Canceled => "finish",
        ToolCallStatus::Error => "error",
    };
    MessageRow {
        id: 0,
        message_id: message_id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(root_turn_id.to_owned()),
        r#type: "tool_call".into(),
        content: content_value.to_string(),
        position: Some("left".into()),
        status: Some(status_str.to_owned()),
        hidden: false,
        created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_common::generate_id;

    fn event(kind: &str, content: &str) -> EvalTrajectoryEvent {
        EvalTrajectoryEvent {
            kind: kind.into(),
            ts_ms: 10,
            content: Some(content.into()),
            ..EvalTrajectoryEvent::default()
        }
    }

    #[test]
    fn projects_user_thinking_tools_and_final_text_onto_one_turn() {
        let conversation_id = generate_id();
        let root_turn_id = generate_id();
        let rows = project_eval_turn_rows(
            &conversation_id,
            &root_turn_id,
            "写一份纪要",
            "已写好纪要。",
            &[
                event("thinking", "先读模板"),
                EvalTrajectoryEvent {
                    kind: "tool_call".into(),
                    ts_ms: 11,
                    tool_use_id: Some("call-1".into()),
                    name: Some("Write".into()),
                    input: Some(r#"{"path":"notes.md"}"#.into()),
                    ..EvalTrajectoryEvent::default()
                },
                EvalTrajectoryEvent {
                    kind: "tool_result".into(),
                    ts_ms: 12,
                    tool_use_id: Some("call-1".into()),
                    name: Some("Write".into()),
                    content: Some("ok".into()),
                    is_error: Some(false),
                    ..EvalTrajectoryEvent::default()
                },
                event("text", "已写好纪要。"),
            ],
            1_000,
        );

        assert_eq!(rows[0].message_id, root_turn_id);
        assert_eq!(rows[0].position.as_deref(), Some("right"));
        assert!(rows[0].content.contains("turn_id"));
        assert!(rows.iter().any(|row| row.r#type == "thinking"));
        assert!(rows.iter().any(|row| row.r#type == "tool_call"));
        let assistant = rows
            .iter()
            .filter(|row| row.r#type == "text" && row.position.as_deref() == Some("left"))
            .last()
            .expect("assistant text");
        assert_eq!(assistant.msg_id.as_deref(), Some(root_turn_id.as_str()));
        assert!(assistant.content.contains("已写好纪要。"));
        assert_eq!(
            rows.iter()
                .filter(|row| row.r#type == "text" && row.position.as_deref() == Some("left"))
                .count(),
            1,
            "final assistant text must not duplicate a matching text event"
        );
        let thinking = rows
            .iter()
            .find(|row| row.r#type == "thinking")
            .expect("thinking");
        assert_eq!(thinking.msg_id.as_deref(), Some(thinking.message_id.as_str()));
        assert!(thinking.content.contains("\"duration\""));
        assert!(thinking.content.contains("turn_id"));
        for row in &rows {
            if row.r#type == "thinking" {
                continue;
            }
            if row.r#type != "text" || row.position.as_deref() != Some("right") {
                assert_eq!(row.msg_id.as_deref(), Some(root_turn_id.as_str()));
            }
            assert!(
                row.content.contains("turn_id"),
                "{} must carry turn_id for the session process rail",
                row.r#type
            );
        }
    }

    #[test]
    fn keeps_intermediate_text_in_the_process_rail() {
        let rows = project_eval_turn_rows(
            &generate_id(),
            &generate_id(),
            "改预算",
            "最终答复",
            &[
                event("text", "先看表格"),
                EvalTrajectoryEvent {
                    kind: "tool_call".into(),
                    ts_ms: 20,
                    tool_use_id: Some("c".into()),
                    name: Some("Read".into()),
                    input: Some(r#"{"path":"a.csv"}"#.into()),
                    ..EvalTrajectoryEvent::default()
                },
                EvalTrajectoryEvent {
                    kind: "tool_result".into(),
                    ts_ms: 21,
                    tool_use_id: Some("c".into()),
                    name: Some("Read".into()),
                    content: Some("ok".into()),
                    ..EvalTrajectoryEvent::default()
                },
            ],
            50,
        );
        let left_text: Vec<_> = rows
            .iter()
            .filter(|row| row.r#type == "text" && row.position.as_deref() == Some("left"))
            .collect();
        assert_eq!(left_text.len(), 2);
        assert!(left_text[0].content.contains("先看表格"));
        assert!(left_text[1].content.contains("最终答复"));
    }

    #[test]
    fn projects_info_events_onto_the_process_rail() {
        let rows = project_eval_turn_rows(
            &generate_id(),
            &generate_id(),
            "提示",
            "完成",
            &[event("info", "已加载技能")],
            10,
        );
        assert!(rows.iter().any(|row| {
            row.r#type == "tips" && row.content.contains("已加载技能") && row.content.contains("warning")
        }));
    }
}
