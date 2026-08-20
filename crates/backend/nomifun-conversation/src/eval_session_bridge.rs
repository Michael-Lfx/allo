//! Bind live agent-eval cases to conversation shells that look like real sessions.

use std::collections::HashMap;

use async_trait::async_trait;
use nomifun_ai_agent::protocol::events::tool_call::{ToolCallEventData, ToolCallStatus};
use nomifun_ai_agent::{
    EvalSessionBridge, OpenEvalCaseSession, RecordEvalCaseTurn,
};
use nomifun_api_types::CreateConversationRequest;
use nomifun_common::{
    generate_id, now_ms, AgentType, AppError, ConversationSource, DecisionPolicy, DelegationPolicy,
    ProviderWithModel,
};
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
        let creation_key = format!("eval:{}:{}", req.run_id, req.case_id);
        // Bind the conversation shell to the run's business-named parent
        // workspace so SessionList groups all cases under that workpath
        // (not 默认工作空间). Agent cwd / write_root still use the case subdir.
        let run_workspace = req.run_workspace.to_string_lossy().into_owned();
        let case_workspace = req.workspace.to_string_lossy().into_owned();
        let name = format!("{} · {}", req.case_id, req.case_category);
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
                "task_profile": req.task_profile,
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

        let mut cursor = now_ms();
        let user_row = nomifun_db::models::MessageRow {
            id: 0,
            message_id: root_turn_id.to_owned(),
            conversation_id: req.conversation_id.clone(),
            msg_id: Some(root_turn_id.to_owned()),
            r#type: "text".into(),
            content: json!({ "content": req.user_prompt }).to_string(),
            position: Some("right".into()),
            status: Some("finish".into()),
            hidden: false,
            created_at: cursor,
        };
        self.service
            .conversation_repo()
            .insert_message(&user_row)
            .await
            .map_err(AppError::from)?;

        let mut thinking_chunks: Vec<String> = Vec::new();
        let mut tool_message_ids: HashMap<String, String> = HashMap::new();
        let mut pending_tools: HashMap<String, (String, Option<serde_json::Value>)> = HashMap::new();

        for event in &req.trajectory {
            cursor = cursor.saturating_add(1);
            match event.kind.as_str() {
                "thinking" => {
                    if let Some(chunk) =
                        event.content.as_deref().map(str::trim).filter(|s| !s.is_empty())
                    {
                        thinking_chunks.push(chunk.to_owned());
                    }
                }
                "tool_call" => {
                    flush_thinking(
                        &self.service,
                        &req.conversation_id,
                        root_turn_id,
                        &mut thinking_chunks,
                        &mut cursor,
                    )
                    .await?;
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
                    persist_tool_call(
                        &self.service,
                        &req.conversation_id,
                        root_turn_id,
                        &message_id,
                        &call_id,
                        &name,
                        Some(args),
                        None,
                        ToolCallStatus::Running,
                        cursor,
                    )
                    .await?;
                }
                "tool_result" => {
                    flush_thinking(
                        &self.service,
                        &req.conversation_id,
                        root_turn_id,
                        &mut thinking_chunks,
                        &mut cursor,
                    )
                    .await?;
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
                    persist_tool_call(
                        &self.service,
                        &req.conversation_id,
                        root_turn_id,
                        &message_id,
                        &call_id,
                        &name,
                        args,
                        event.content.clone(),
                        status,
                        cursor,
                    )
                    .await?;
                }
                _ => {}
            }
        }
        flush_thinking(
            &self.service,
            &req.conversation_id,
            root_turn_id,
            &mut thinking_chunks,
            &mut cursor,
        )
        .await?;

        cursor = cursor.saturating_add(1);
        let assistant_msg_id = generate_id();
        let assistant_row = nomifun_db::models::MessageRow {
            id: 0,
            message_id: assistant_msg_id.clone(),
            conversation_id: req.conversation_id.clone(),
            msg_id: Some(assistant_msg_id),
            r#type: "text".into(),
            content: json!({
                "content": req.assistant_text,
                "turn_id": root_turn_id,
            })
            .to_string(),
            position: Some("left".into()),
            status: Some("finish".into()),
            hidden: false,
            created_at: cursor,
        };
        self.service
            .conversation_repo()
            .insert_message(&assistant_row)
            .await
            .map_err(AppError::from)?;

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

async fn flush_thinking(
    service: &ConversationService,
    conversation_id: &str,
    root_turn_id: &str,
    chunks: &mut Vec<String>,
    cursor: &mut i64,
) -> Result<(), AppError> {
    if chunks.is_empty() {
        return Ok(());
    }
    let content = chunks.join("\n");
    chunks.clear();
    *cursor = cursor.saturating_add(1);
    let thinking_id = generate_id();
    let row = nomifun_db::models::MessageRow {
        id: 0,
        message_id: thinking_id.clone(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(thinking_id),
        r#type: "thinking".into(),
        content: json!({
            "content": content,
            "status": "done",
            "duration_ms": 0,
            "turn_id": root_turn_id,
        })
        .to_string(),
        position: Some("left".into()),
        status: Some("finish".into()),
        hidden: false,
        created_at: *cursor,
    };
    service
        .conversation_repo()
        .insert_message(&row)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn persist_tool_call(
    service: &ConversationService,
    conversation_id: &str,
    root_turn_id: &str,
    message_id: &str,
    call_id: &str,
    name: &str,
    args: Option<serde_json::Value>,
    output: Option<String>,
    status: ToolCallStatus,
    created_at: i64,
) -> Result<(), AppError> {
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
    // MessageRow.status must stay in the renderer allow-list (finish/pending/error/work).
    let status_str = match status {
        ToolCallStatus::Running => "work",
        ToolCallStatus::Completed | ToolCallStatus::Canceled => "finish",
        ToolCallStatus::Error => "error",
    };

    if let Ok(Some(_)) = service
        .conversation_repo()
        .get_message(conversation_id, message_id)
        .await
    {
        let update = nomifun_db::MessageRowUpdate {
            content: Some(content_value.to_string()),
            status: Some(Some(status_str.to_owned())),
            hidden: None,
        };
        service
            .conversation_repo()
            .update_message(message_id, &update)
            .await
            .map_err(AppError::from)?;
        return Ok(());
    }

    let row = nomifun_db::models::MessageRow {
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
    };
    service
        .conversation_repo()
        .insert_message(&row)
        .await
        .map_err(AppError::from)?;
    Ok(())
}
