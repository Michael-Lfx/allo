//! Maps [`AgentStreamEvent`] into a [`TurnTraceBuilder`] and persists on terminal.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{debug, warn};

use nomi_agent_trace::{
    SpanStatus, TokenCounts, TraceArtifactMeta, TurnTraceBuilder, TurnTraceMeta,
    classify_session_kind, is_session_dialogue, normalize_reported_path, redact_json_value,
    redact_preview, reported_artifacts_from_tool_call,
};

use crate::artifact_store::{ArtifactKind, PersistedArtifact};
use crate::protocol::events::{
    AcpToolCallContentItem, AcpToolCallEventData, AcpToolCallKind, AgentStreamEvent,
    ToolCallEventData, ToolCallStatus, TurnCompletedEventData, TurnStopReason,
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

        let mut saw_turn_completed = false;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let terminal = match catch_unwind(AssertUnwindSafe(|| {
                        observe_event(&mut builder, &event)
                    })) {
                        Ok(terminal) => terminal,
                        Err(_) => {
                            warn!(
                                %trace_id,
                                "agent trace: observe_event panicked; continuing"
                            );
                            false
                        }
                    };
                    if matches!(event, AgentStreamEvent::TurnCompleted(_)) {
                        saw_turn_completed = true;
                    }
                    if terminal {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        %trace_id,
                        skipped,
                        saw_turn_completed,
                        "agent trace: lagged on event bus"
                    );
                    // Finish is easy to lose on the tiny broadcast buffer after a
                    // heavy Write/Edit args redact. If metrics already landed,
                    // persist instead of waiting forever for a skipped Finish.
                    if saw_turn_completed {
                        break;
                    }
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
            observe_tool_call(builder, data);
            false
        }
        AgentStreamEvent::AcpToolCall(data) => {
            observe_acp_tool_call(builder, data);
            false
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
        // TurnCompleted is intentionally terminal for the collector: Finish may
        // arrive much later (e.g. after memory distillation) or be dropped when
        // the broadcast buffer lags. Metrics + tool spans are already complete.
        AgentStreamEvent::TurnCompleted(metrics) => {
            apply_turn_completed(builder, metrics);
            true
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

fn observe_tool_call(builder: &mut TurnTraceBuilder, data: &ToolCallEventData) {
    match data.status {
        ToolCallStatus::Running => {
            let preview_str = tool_args_preview(&data.args);
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
            let artifacts = receipt_artifact_metas(data);
            builder.note_tool_end(
                &data.call_id,
                SpanStatus::Error,
                data.output.as_deref(),
                &artifacts,
            );
        }
        ToolCallStatus::Canceled => {
            let artifacts = receipt_artifact_metas(data);
            builder.note_tool_end(
                &data.call_id,
                SpanStatus::Cancelled,
                data.output.as_deref(),
                &artifacts,
            );
        }
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

fn observe_acp_tool_call(builder: &mut TurnTraceBuilder, data: &AcpToolCallEventData) {
    let projected = crate::protocol::events::project_acp_tool_call_to_tool_call(data);
    match projected.status {
        ToolCallStatus::Running => {
            let preview_str = tool_args_preview(&projected.args);
            builder.note_tool_start(
                &projected.call_id,
                &projected.name,
                preview_str.as_deref(),
            );
        }
        ToolCallStatus::Completed => {
            let mut artifacts = artifact_metas(&projected);
            merge_reported(&mut artifacts, reported_from_acp(data, &projected));
            builder.note_tool_end(
                &projected.call_id,
                SpanStatus::Ok,
                projected.output.as_deref(),
                &artifacts,
            );
        }
        ToolCallStatus::Error => {
            let artifacts = receipt_artifact_metas(&projected);
            builder.note_tool_end(
                &projected.call_id,
                SpanStatus::Error,
                projected.output.as_deref(),
                &artifacts,
            );
        }
        ToolCallStatus::Canceled => {
            let artifacts = receipt_artifact_metas(&projected);
            builder.note_tool_end(
                &projected.call_id,
                SpanStatus::Cancelled,
                projected.output.as_deref(),
                &artifacts,
            );
        }
    }
}

fn tool_args_preview(args: &serde_json::Value) -> Option<String> {
    let preview = redact_json_value(args);
    serde_json::to_string(&preview).ok()
}

/// Verified receipts + reported file-mutation outputs for a completed tool call.
fn artifact_metas(data: &ToolCallEventData) -> Vec<TraceArtifactMeta> {
    let mut out = receipt_artifact_metas(data);
    if data.status == ToolCallStatus::Completed {
        let args = if data.args.is_null() {
            data.input.as_ref().unwrap_or(&data.args)
        } else {
            &data.args
        };
        let reported = reported_artifacts_from_tool_call(
            &data.call_id,
            &data.name,
            args,
            data.output.as_deref(),
        );
        merge_reported(&mut out, reported);
    }
    out
}

fn receipt_artifact_metas(data: &ToolCallEventData) -> Vec<TraceArtifactMeta> {
    data.artifacts
        .iter()
        .map(|artifact| {
            to_trace_artifact(artifact, Some(data.call_id.as_str()), Some(data.name.as_str()))
        })
        .collect()
}

fn reported_from_acp(
    data: &AcpToolCallEventData,
    projected: &ToolCallEventData,
) -> Vec<TraceArtifactMeta> {
    let mut paths = Vec::new();
    if let Some(content) = data.update.content.as_ref() {
        for item in content {
            if let AcpToolCallContentItem::Diff { path, .. } = item {
                paths.push(path.clone());
            }
        }
    }
    if matches!(data.update.kind, Some(AcpToolCallKind::Edit)) {
        if let Some(locations) = data.update.locations.as_ref() {
            for location in locations {
                paths.push(location.path.clone());
            }
        }
    }
    // Force a file-mutation tool name: ACP titles are free-form ("Edited foo.py")
    // and would otherwise fail the Write/Edit whitelist inside reported_artifacts.
    let tool_name = if nomi_agent_trace::is_file_mutation_tool_name(&projected.name) {
        projected.name.as_str()
    } else {
        "Edit"
    };
    paths
        .into_iter()
        .filter_map(|raw| normalize_reported_path(&raw))
        .take(16)
        .flat_map(|relative_path| {
            reported_artifacts_from_tool_call(
                &projected.call_id,
                tool_name,
                &serde_json::json!({ "file_path": relative_path }),
                None,
            )
        })
        .collect()
}

fn merge_reported(out: &mut Vec<TraceArtifactMeta>, reported: Vec<TraceArtifactMeta>) {
    let mut seen: std::collections::BTreeSet<String> =
        out.iter().map(|a| a.relative_path.clone()).collect();
    for artifact in reported {
        if seen.insert(artifact.relative_path.clone()) {
            out.push(artifact);
        }
    }
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
        source: Some("receipt".into()),
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
    use crate::protocol::events::{StartEventData, TurnCompletedEventData};

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
        assert_eq!(metas[0].source.as_deref(), Some("receipt"));
        let json = serde_json::to_string(&metas[0]).unwrap();
        assert!(!json.contains("C:/secret"));
        assert!(!json.contains("workspace"));
    }

    #[test]
    fn synthesizes_reported_artifacts_from_write_args() {
        let data = ToolCallEventData {
            call_id: "call-w".into(),
            name: "Write".into(),
            args: serde_json::json!({
                "file_path": "scripts/hello.py",
                "content": "print('hi')"
            }),
            status: ToolCallStatus::Completed,
            input: None,
            output: Some("Created scripts/hello.py (1 lines)".into()),
            description: None,
            retry: None,
            artifacts: vec![],
        };
        let metas = artifact_metas(&data);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].relative_path, "scripts/hello.py");
        assert_eq!(metas[0].kind, "text");
        assert_eq!(metas[0].source.as_deref(), Some("reported"));
        assert!(metas[0].sha256.is_empty());
    }

    #[test]
    fn write_args_preview_omits_file_body() {
        let args = serde_json::json!({
            "file_path": "scripts/hello.py",
            "content": "x".repeat(50_000)
        });
        let preview = tool_args_preview(&args).expect("preview");
        assert!(preview.contains("scripts/hello.py"));
        assert!(preview.contains("omitted"));
        assert!(!preview.contains(&"x".repeat(100)));
    }

    #[test]
    fn turn_completed_is_terminal_and_keeps_spans() {
        let mut builder = TurnTraceBuilder::new(TurnTraceMeta {
            conversation_id: "c".into(),
            msg_id: "m".into(),
            root_turn_id: "r".into(),
            session_kind: "session_dialogue".into(),
            origin: None,
            companion: false,
            channel_platform: None,
            provider: None,
            model: None,
        });
        assert!(!observe_event(
            &mut builder,
            &AgentStreamEvent::Start(StartEventData::default())
        ));
        observe_tool_call(
            &mut builder,
            &ToolCallEventData {
                call_id: "c1".into(),
                name: "Write".into(),
                args: serde_json::json!({"file_path": "a.py", "content": "print(1)"}),
                status: ToolCallStatus::Running,
                input: None,
                output: None,
                description: None,
                retry: None,
                artifacts: vec![],
            },
        );
        observe_tool_call(
            &mut builder,
            &ToolCallEventData {
                call_id: "c1".into(),
                name: "Write".into(),
                args: serde_json::json!({"file_path": "a.py", "content": "print(1)"}),
                status: ToolCallStatus::Completed,
                input: None,
                output: Some("Created a.py (1 lines)".into()),
                description: None,
                retry: None,
                artifacts: vec![],
            },
        );
        let terminal = observe_event(
            &mut builder,
            &AgentStreamEvent::TurnCompleted(TurnCompletedEventData {
                elapsed_ms: 12,
                input_tokens: 1,
                output_tokens: 2,
                ..Default::default()
            }),
        );
        assert!(terminal);
        let trace = builder.finalize();
        assert!(!trace.spans.is_empty());
        assert_eq!(trace.summary.artifact_count, 1);
        assert_eq!(trace.summary.success, Some(true));
    }
}
