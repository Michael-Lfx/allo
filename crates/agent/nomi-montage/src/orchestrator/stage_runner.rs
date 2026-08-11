//! Runs the LLM↔tool loop for a single stage (see `orchestrator::prompts` for
//! why this simulates multi-turn conversation over a single-shot chat API).

use serde::Deserialize;
use serde_json::Value;

use nomi_media_backends::MediaChat;

use crate::checkpoint::canonical_artifact_for_stage;
use crate::error::{MontageError, MontageResult};
use crate::events::EventKind;
use crate::pipeline::{PipelineManifest, StageSpec};
use crate::tools::{ToolContext, ToolRegistry};

use super::prompts::{build_stage_system_prompt, build_turn_user_message};

/// Bookkeeping tools every stage can call regardless of its YAML `tools_available`
/// — these are the mechanism (not creative capability), so gating them per
/// pipeline would just force every author to repeat the same list.
const ALWAYS_AVAILABLE_TOOLS: &[&str] = &[
    "write_artifact",
    "read_artifact",
    "checkpoint_note",
    "decision_log_append",
    "cost_estimate",
    "cost_reconcile",
];

/// Cap on transcript size fed back to the model each turn (chars, not tokens —
/// a conservative proxy that keeps prompts bounded without a tokenizer dep).
const MAX_TRANSCRIPT_CHARS: usize = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageOutcome {
    Completed { summary: String },
    Failed { reason: String },
}

#[derive(Debug, Deserialize)]
struct ToolCallReply {
    tool: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct StageCompleteReply {
    stage_complete: bool,
    #[serde(default)]
    summary: String,
}

enum ParsedReply {
    ToolCall(ToolCallReply),
    Complete(StageCompleteReply),
    Unparseable(String),
}

fn parse_reply(raw: &str) -> ParsedReply {
    match extract_json_object(raw) {
        Some(value) => {
            if value.get("tool").and_then(|v| v.as_str()).is_some() {
                match serde_json::from_value::<ToolCallReply>(value) {
                    Ok(call) => ParsedReply::ToolCall(call),
                    Err(e) => ParsedReply::Unparseable(format!("malformed tool call: {e}")),
                }
            } else if value.get("stage_complete").is_some() {
                match serde_json::from_value::<StageCompleteReply>(value) {
                    Ok(done) if done.stage_complete => ParsedReply::Complete(done),
                    Ok(_) => ParsedReply::Unparseable(
                        "stage_complete must be `true` (or omit the field entirely and call a tool instead)".into(),
                    ),
                    Err(e) => ParsedReply::Unparseable(format!("malformed stage_complete reply: {e}")),
                }
            } else {
                ParsedReply::Unparseable(
                    "JSON object must contain either \"tool\" or \"stage_complete\"".into(),
                )
            }
        }
        None => ParsedReply::Unparseable("reply did not contain a parseable JSON object".into()),
    }
}

/// Best-effort extraction of the first JSON object in `raw`, tolerant of
/// markdown code fences and leading/trailing prose.
fn extract_json_object(raw: &str) -> Option<Value> {
    let stripped = raw
        .replace("```json", "```")
        .replace("```JSON", "```");
    let candidate = stripped.trim();
    let start = candidate.find('{')?;
    let slice = &candidate[start..];
    let mut de = serde_json::Deserializer::from_str(slice).into_iter::<Value>();
    de.next()?.ok()
}

fn append_transcript(transcript: &mut String, entry: &str) {
    transcript.push_str(entry);
    transcript.push('\n');
    if transcript.len() > MAX_TRANSCRIPT_CHARS {
        let drop = transcript.len() - MAX_TRANSCRIPT_CHARS;
        // Drop from the front but stay on a char boundary.
        let mut cut = drop;
        while !transcript.is_char_boundary(cut) {
            cut += 1;
        }
        transcript.replace_range(0..cut, "[…earlier turns truncated…]\n");
    }
}

/// Run one stage to completion (or failure), driving the LLM↔tool loop.
///
/// Returns `Ok(StageOutcome::Completed)` once the model declares
/// `stage_complete` *and* the canonical artifact (if any) validates. Returns
/// `Ok(StageOutcome::Failed)` when the turn budget is exhausted or an
/// unrecoverable tool error occurs; never fabricates success.
pub async fn run_stage(
    manifest: &PipelineManifest,
    stage: &StageSpec,
    chat: &dyn MediaChat,
    registry: &ToolRegistry,
    ctx: &ToolContext,
    stage_brief: &str,
    max_turns: u32,
) -> MontageResult<StageOutcome> {
    let mut allowed: Vec<String> = stage.tools_available.clone();
    for name in ALWAYS_AVAILABLE_TOOLS {
        if !allowed.iter().any(|n| n == name) {
            allowed.push((*name).to_string());
        }
    }
    let tools = registry.allowlist(&allowed);
    let specs: Vec<_> = tools.iter().map(|t| t.spec().clone()).collect();
    let system = build_stage_system_prompt(manifest, stage, &specs)?;

    let mut transcript = String::new();
    let canonical = canonical_artifact_for_stage(&stage.name).or_else(|| stage.canonical_artifact());

    for turn in 1..=max_turns {
        if ctx.is_cancelled() {
            return Err(MontageError::Cancelled);
        }
        let user = build_turn_user_message(stage_brief, &transcript, turn, max_turns);
        let reply = chat
            .complete_text(&system, &user)
            .await
            .map_err(|e| MontageError::Llm(e.to_string()))?;
        append_transcript(&mut transcript, &format!("assistant: {reply}"));

        match parse_reply(&reply) {
            ParsedReply::ToolCall(call) => {
                let tool = tools.iter().find(|t| t.spec().name == call.tool);
                let result = match tool {
                    Some(t) => {
                        ctx.emit(
                            EventKind::ToolCalled,
                            format!("{} calling {}", stage.name, call.tool),
                            Some(call.arguments.clone()),
                        );
                        t.execute(ctx, call.arguments).await
                    }
                    None => Err(MontageError::ToolUnavailable(
                        call.tool.clone(),
                        format!(
                            "'{}' is not in this stage's tools_available list: {}",
                            call.tool,
                            allowed.join(", ")
                        ),
                    )),
                };
                let observation = match result {
                    Ok(r) => serde_json::to_string(&r).unwrap_or_else(|_| "{}".into()),
                    Err(e) => serde_json::json!({"ok": false, "message": e.to_string()}).to_string(),
                };
                append_transcript(
                    &mut transcript,
                    &format!("tool_result[{}]: {observation}", call.tool),
                );
            }
            ParsedReply::Complete(done) => {
                if let Some(artifact_name) = canonical {
                    match read_artifact_value(ctx, artifact_name) {
                        Some(value) => {
                            if let Err(e) = ctx.artifact_registry.validate(artifact_name, &value) {
                                append_transcript(
                                    &mut transcript,
                                    &format!(
                                        "system: stage_complete rejected — canonical artifact '{artifact_name}' \
                                         failed validation: {e}. Fix it with write_artifact and try again."
                                    ),
                                );
                                continue;
                            }
                        }
                        None => {
                            append_transcript(
                                &mut transcript,
                                &format!(
                                    "system: stage_complete rejected — canonical artifact '{artifact_name}' \
                                     was never written with write_artifact."
                                ),
                            );
                            continue;
                        }
                    }
                }
                return Ok(StageOutcome::Completed { summary: done.summary });
            }
            ParsedReply::Unparseable(reason) => {
                append_transcript(
                    &mut transcript,
                    &format!("system: {reason}. Reply with exactly one JSON object as specified."),
                );
            }
        }
    }

    Ok(StageOutcome::Failed {
        reason: format!("stage '{}' did not complete within {max_turns} turns", stage.name),
    })
}

fn read_artifact_value(ctx: &ToolContext, name: &str) -> Option<Value> {
    let path = ctx.paths.artifact_path(name);
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_json() {
        let v = extract_json_object(r#"{"tool": "x", "arguments": {}}"#).unwrap();
        assert_eq!(v["tool"], "x");
    }

    #[test]
    fn extracts_json_from_fenced_block_with_prose() {
        let raw = "Sure, here you go:\n```json\n{\"stage_complete\": true, \"summary\": \"done\"}\n```\nThanks.";
        let v = extract_json_object(raw).unwrap();
        assert_eq!(v["stage_complete"], true);
    }

    #[test]
    fn unparseable_when_no_braces() {
        assert!(extract_json_object("no json here").is_none());
    }

    #[test]
    fn parse_reply_routes_tool_vs_complete() {
        match parse_reply(r#"{"tool": "write_artifact", "arguments": {"name": "script"}}"#) {
            ParsedReply::ToolCall(c) => assert_eq!(c.tool, "write_artifact"),
            _ => panic!("expected tool call"),
        }
        match parse_reply(r#"{"stage_complete": true, "summary": "ok"}"#) {
            ParsedReply::Complete(c) => assert_eq!(c.summary, "ok"),
            _ => panic!("expected complete"),
        }
    }
}
