//! Maps [`AgentStreamEvent`] into a [`TurnTraceBuilder`] and persists on terminal.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{debug, warn};

use nomi_agent_trace::{
    SpanStatus, TokenCounts, TraceArtifactMeta, TurnTraceBuilder, TurnTraceMeta,
    classify_session_kind, is_session_dialogue, redact_json_value, redact_preview,
};

use crate::artifact_store::{ArtifactKind, PersistedArtifact};
use crate::protocol::events::{
    AgentStreamEvent, ToolCallEventData, ToolCallStatus, TurnCompletedEventData, TurnStopReason,
};

use super::hub::AgentTraceHub;

/// Identity of the turn being recorded.
#[derive(Debug, Clone)]
pub struct TurnTraceContext {
    pub conversation_id: String,
    pub msg_id: String,
    pub root_turn_id: String,
    pub origin: Option<String>,
    pub companion: bool,
    pub channel_platform: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// When true, only `session_dialogue` turns are recorded (default for
    /// the Session module agent conversation surface).
    pub session_dialogue_only: bool,
}

/// Consumes a broadcast subscription until a terminal event, then persists.
pub struct TurnTraceCollector;

impl TurnTraceCollector {
    /// Spawn a fire-and-forget collector. Safe to call when developer mode is
    /// off — this returns immediately without spawning.
    pub fn spawn(hub: Arc<AgentTraceHub>, rx: broadcast::Receiver<AgentStreamEvent>, ctx: TurnTraceContext) {
        tokio::spawn(async move {
            if let Err(error) = Self::run(hub, rx, ctx).await {
                warn!(error = %error, "agent trace collector stopped with error");
            }
        });
    }

    async fn run(
        hub: Arc<AgentTraceHub>,
        mut rx: broadcast::Receiver<AgentStreamEvent>,
        ctx: TurnTraceContext,
    ) -> Result<(), String> {
        if !hub.developer_mode_enabled().await {
            debug!(
                conversation_id = %ctx.conversation_id,
                "agent trace: developer mode off; skip collector"
            );
            return Ok(());
        }

        if ctx.session_dialogue_only
            && !is_session_dialogue(
                ctx.origin.as_deref(),
                ctx.companion,
                ctx.channel_platform.as_deref(),
            )
        {
            debug!(
                conversation_id = %ctx.conversation_id,
                origin = ?ctx.origin,
                companion = ctx.companion,
                "agent trace: non-session-dialogue turn; skip"
            );
            return Ok(());
        }

        let session_kind = classify_session_kind(
            ctx.origin.as_deref(),
            ctx.companion,
            ctx.channel_platform.as_deref(),
        )
        .to_owned();

        let mut builder = TurnTraceBuilder::new(TurnTraceMeta {
            conversation_id: ctx.conversation_id.clone(),
            msg_id: ctx.msg_id.clone(),
            root_turn_id: ctx.root_turn_id.clone(),
            session_kind,
            origin: ctx.origin.clone(),
            companion: ctx.companion,
            channel_platform: ctx.channel_platform.clone(),
            provider: ctx.provider.clone(),
            model: ctx.model.clone(),
        });

        let trace_id = builder.trace_id().to_owned();
        debug!(
            %trace_id,
            conversation_id = %ctx.conversation_id,
            msg_id = %ctx.msg_id,
            "agent trace: collector started"
        );

        loop {
            match rx.recv().await {
                Ok(event) => {
                    let terminal = observe_event(&mut builder, &event);
                    if terminal {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        %trace_id,
                        skipped,
                        "agent trace: lagged on event bus; continuing"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    builder.apply_error(
                        "event_bus_closed",
                        "agent event bus closed before terminal event",
                    );
                    break;
                }
            }
        }

        let trace = builder.finalize();
        if let Err(error) = hub.store().persist(&trace) {
            warn!(
                trace_id = %trace.trace_id,
                error = %error,
                "agent trace: failed to persist turn"
            );
        } else {
            debug!(
                trace_id = %trace.trace_id,
                spans = trace.spans.len(),
                "agent trace: persisted"
            );
        }
        Ok(())
    }
}

fn observe_event(builder: &mut TurnTraceBuilder, event: &AgentStreamEvent) -> bool {
    match event {
        AgentStreamEvent::Start(_) => {
            builder.note_llm_round_start("provider");
            false
        }
        AgentStreamEvent::Text(data) => {
            builder.note_text_delta(&data.content);
            false
        }
        AgentStreamEvent::Thinking(data) => {
            builder.note_thinking(Some(data.content.as_str()));
            false
        }
        AgentStreamEvent::ToolCall(data) => {
            match data.status {
                ToolCallStatus::Running => {
                    let preview = redact_json_value(&data.args);
                    let preview_str = serde_json::to_string(&preview).ok();
                    builder.note_tool_start(&data.call_id, &data.name, preview_str.as_deref());
                }
                ToolCallStatus::Completed => {
                    let artifacts = artifact_metas(data);
                    builder.note_tool_end(
                        &data.call_id,
                        SpanStatus::Ok,
                        data.output.as_deref(),
                        &artifacts,
                    );
                }
                ToolCallStatus::Error => {
                    let artifacts = artifact_metas(data);
                    builder.note_tool_end(
                        &data.call_id,
                        SpanStatus::Error,
                        data.output.as_deref(),
                        &artifacts,
                    );
                }
                ToolCallStatus::Canceled => {
                    let artifacts = artifact_metas(data);
                    builder.note_tool_end(
                        &data.call_id,
                        SpanStatus::Cancelled,
                        data.output.as_deref(),
                        &artifacts,
                    );
                }
            }
            false
        }
        AgentStreamEvent::AcpToolCall(data) => {
            let projected = crate::protocol::events::project_acp_tool_call_to_tool_call(data);
            observe_event(builder, &AgentStreamEvent::ToolCall(projected))
        }
        AgentStreamEvent::MoaReference(data) => {
            let mut attrs = BTreeMap::new();
            attrs.insert("label".into(), serde_json::json!(data.label));
            attrs.insert("index".into(), serde_json::json!(data.index));
            attrs.insert("total".into(), serde_json::json!(data.total));
            let span_id = builder.start_span(nomi_agent_trace::SpanKind::Moa, data.label.clone());
            let preview = redact_preview(&data.text);
            builder.end_span(&span_id, SpanStatus::Ok, Some(preview.as_str()), attrs);
            false
        }
        AgentStreamEvent::MoaProgress(_) => false,
        AgentStreamEvent::TurnCompleted(metrics) => {
            apply_turn_completed(builder, metrics);
            false
        }
        AgentStreamEvent::RequestTrace(value) => {
            let mut attrs = BTreeMap::new();
            attrs.insert("payload".into(), redact_json_value(value));
            let span_id = builder.start_span(nomi_agent_trace::SpanKind::System, "request_trace");
            builder.end_span(&span_id, SpanStatus::Ok, None, attrs);
            false
        }
        AgentStreamEvent::Tips(data) => {
            let mut attrs = BTreeMap::new();
            attrs.insert(
                "tip_type".into(),
                serde_json::json!(format!("{:?}", data.tip_type)),
            );
            let span_id = builder.start_span(nomi_agent_trace::SpanKind::System, "tips");
            let preview = redact_preview(&data.content);
            builder.end_span(&span_id, SpanStatus::Ok, Some(preview.as_str()), attrs);
            false
        }
        AgentStreamEvent::Error(data) => {
            let code = data
                .code
                .map(|c| format!("{c:?}"))
                .unwrap_or_else(|| "unknown".to_owned());
            builder.apply_error(code, data.message.as_str());
            true
        }
        AgentStreamEvent::Finish(_) => true,
        AgentStreamEvent::Plan(_)
        | AgentStreamEvent::Permission(_)
        | AgentStreamEvent::AcpPermission(_)
        | AgentStreamEvent::SkillSuggest(_)
        | AgentStreamEvent::CronTrigger(_)
        | AgentStreamEvent::AcpModelInfo(_)
        | AgentStreamEvent::AcpModeInfo(_)
        | AgentStreamEvent::AcpConfigOption(_)
        | AgentStreamEvent::AcpSessionInfo(_)
        | AgentStreamEvent::AcpContextUsage(_)
        | AgentStreamEvent::SlashCommandsUpdated(_)
        | AgentStreamEvent::AvailableCommands(_)
        | AgentStreamEvent::AgentStatus(_)
        | AgentStreamEvent::ToolGroup(_)
        | AgentStreamEvent::System(_)
        | AgentStreamEvent::SessionAssigned(_) => false,
    }
}

fn apply_turn_completed(builder: &mut TurnTraceBuilder, metrics: &TurnCompletedEventData) {
    let stop = metrics
        .stop_reason
        .map(stop_reason_label)
        .map(str::to_owned);
    builder.apply_turn_completed(
        Some(metrics.elapsed_ms),
        TokenCounts {
            input_tokens: metrics.input_tokens,
            output_tokens: metrics.output_tokens,
            cache_creation_tokens: metrics.cache_creation_tokens,
            cache_read_tokens: metrics.cache_read_tokens,
            context_tokens: metrics.context_tokens,
            context_window: metrics.context_window,
        },
        stop,
    );
}

fn stop_reason_label(reason: TurnStopReason) -> &'static str {
    match reason {
        TurnStopReason::EndTurn => "end_turn",
        TurnStopReason::MaxTokens => "max_tokens",
        TurnStopReason::MaxTurnRequests => "max_turn_requests",
        TurnStopReason::Refusal => "refusal",
        TurnStopReason::Cancelled => "cancelled",
    }
}

/// Map verified receipts into trace metadata (no absolute paths / no bytes).
fn artifact_metas(data: &ToolCallEventData) -> Vec<TraceArtifactMeta> {
    data.artifacts
        .iter()
        .map(|artifact| to_trace_artifact(artifact, Some(data.call_id.as_str()), Some(data.name.as_str())))
        .collect()
}

fn to_trace_artifact(
    artifact: &PersistedArtifact,
    call_id: Option<&str>,
    tool_name: Option<&str>,
) -> TraceArtifactMeta {
    TraceArtifactMeta {
        id: artifact.id.clone(),
        kind: artifact_kind_label(artifact.kind).to_owned(),
        mime_type: artifact.mime_type.clone(),
        relative_path: artifact.relative_path.clone(),
        size_bytes: artifact.size_bytes,
        sha256: artifact.sha256.clone(),
        call_id: call_id.map(str::to_owned),
        tool_name: tool_name.map(str::to_owned),
    }
}

fn artifact_kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Image => "image",
        ArtifactKind::Audio => "audio",
        ArtifactKind::Video => "video",
        ArtifactKind::Text => "text",
        ArtifactKind::File => "file",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_store::ArtifactKind;

    #[test]
    fn maps_persisted_artifact_without_absolute_path() {
        let data = ToolCallEventData {
            call_id: "call-1".into(),
            name: "generate_image".into(),
            args: serde_json::json!({}),
            status: ToolCallStatus::Completed,
            input: None,
            output: Some("ok".into()),
            description: None,
            retry: None,
            artifacts: vec![PersistedArtifact {
                id: "art-1".into(),
                kind: ArtifactKind::Image,
                mime_type: "image/png".into(),
                path: "C:/secret/workspace/nomifun-artifacts/a.png".into(),
                relative_path: "nomifun-artifacts/a.png".into(),
                size_bytes: 42,
                sha256: "deadbeef".into(),
            }],
        };
        let metas = artifact_metas(&data);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].relative_path, "nomifun-artifacts/a.png");
        assert_eq!(metas[0].kind, "image");
        assert_eq!(metas[0].call_id.as_deref(), Some("call-1"));
        let json = serde_json::to_string(&metas[0]).unwrap();
        assert!(!json.contains("C:/secret"));
        assert!(!json.contains("workspace"));
    }
}
