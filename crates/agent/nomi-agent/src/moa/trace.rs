//! MoA JSONL trace assembly.
//!
//! When `moa.trace_enabled` is on, every REAL fan-out (cache-hit rounds
//! produce nothing) is condensed into one single-line JSON record and handed
//! to the output sink via `emit_moa_trace`. The record intentionally carries
//! the UNREDACTED advisory inputs/outputs — it is a local audit side-channel
//! owned by the host, not a user display surface; the privacy filter applies
//! to display/guidance paths only (see [`super::redact`]).

use std::time::{SystemTime, UNIX_EPOCH};

use nomi_types::message::{ContentBlock, Message, Role};
use serde_json::json;

use super::MoaState;
use super::runner::MoaOutcome;

/// Build the trace record for one fan-out opportunity, if one is due.
///
/// Returns `None` when tracing is disabled or the round was a cache hit
/// (nothing ran — a trace line would only duplicate the previous one).
pub fn build_trace_json(msg_id: &str, state: &MoaState, outcome: &MoaOutcome) -> Option<String> {
    if !state.config.trace_enabled || outcome.from_cache {
        return None;
    }
    Some(render_trace_json(msg_id, state, outcome))
}

/// Render the single-line JSON trace record for one real fan-out.
///
/// Shape:
/// `{"ts":<unix_ms>,"msg_id":…,"fanout":…,"from_cache":false,
///   "slots":[{"label","model","temperature"}],
///   "advisory_input":[{"role","text"}…],"outputs":[{"label","text","input_tokens",
///   "output_tokens","failed"}…],"total":{"input_tokens","output_tokens"}}`
fn render_trace_json(msg_id: &str, state: &MoaState, outcome: &MoaOutcome) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let slots: Vec<_> = state
        .slots
        .iter()
        .map(|slot| {
            json!({
                "label": slot.label,
                "model": slot.config.model,
                "temperature": slot.temperature,
            })
        })
        .collect();

    let advisory_input: Vec<_> = outcome
        .advisory_input
        .iter()
        .map(|msg| {
            json!({
                "role": role_name(msg.role),
                "text": text_of(msg),
            })
        })
        .collect();

    // Per-slot usage rides in slot order alongside the advices; a missing
    // entry (defensive only — real fan-outs fill both) reads as zeros.
    let outputs: Vec<_> = outcome
        .advices
        .iter()
        .enumerate()
        .map(|(idx, advice)| {
            let (input_tokens, output_tokens) = outcome
                .slot_usage
                .get(idx)
                .map(|u| (u.input_tokens, u.output_tokens))
                .unwrap_or((0, 0));
            json!({
                "label": advice.label,
                "text": advice.text,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "failed": advice.text.starts_with("[failed:"),
            })
        })
        .collect();

    json!({
        "ts": ts,
        "msg_id": msg_id,
        "fanout": state.config.fanout,
        // Only real fan-outs are traced (cache hits return early above), so
        // this is always false — kept explicit to match the planned Spec shape.
        "from_cache": false,
        "slots": slots,
        "advisory_input": advisory_input,
        "outputs": outputs,
        "total": {
            "input_tokens": outcome.usage.input_tokens,
            "output_tokens": outcome.usage.output_tokens,
        },
    })
    .to_string()
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
        Role::Tool => "tool",
    }
}

/// Flatten a message to its text payload. The advisory view only carries
/// text blocks; anything else is skipped.
fn text_of(msg: &Message) -> String {
    let mut out = String::new();
    for block in &msg.content {
        if let ContentBlock::Text { text } = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use nomi_config::config::{Config, MoaConfig, ProviderType};
    use nomi_types::message::TokenUsage;

    use super::super::{MoaAdvice, MoaResolvedSlot, MoaSlotTurnUsage};
    use super::*;

    fn test_config(model: &str) -> Config {
        Config {
            provider_label: "mock".into(),
            provider: ProviderType::OpenAI,
            api_key: "test-key".into(),
            base_url: String::new(),
            model: model.into(),
            output_max_tokens: Some(1024),
            max_turns: None,
            system_prompt: None,
            project_instructions: Default::default(),
            thinking: None,
            prompt_caching: false,
            compat: Default::default(),
            tools: Default::default(),
            session: Default::default(),
            compact: Default::default(),
            plan: Default::default(),
            file_cache: Default::default(),
            hooks: Default::default(),
            bedrock: None,
            vertex: None,
            mcp: Default::default(),
            logging: Default::default(),
            memory: Default::default(),
            moa: Default::default(),
        }
    }

    fn traced_state() -> MoaState {
        MoaState::new(
            MoaConfig {
                enabled: true,
                fanout: "user_turn".into(),
                trace_enabled: true,
                ..MoaConfig::default()
            },
            vec![MoaResolvedSlot {
                config: test_config("alpha"),
                label: "mock/alpha".into(),
                max_tokens: None,
                temperature: Some(0.5),
                context_window_tokens: None,
            }],
        )
    }

    fn real_outcome() -> MoaOutcome {
        MoaOutcome {
            advices: vec![MoaAdvice {
                label: "mock/alpha".into(),
                text: "raw advice with bob@example.com kept verbatim".into(),
            }],
            slot_usage: vec![MoaSlotTurnUsage {
                label: "mock/alpha".into(),
                input_tokens: 10,
                output_tokens: 7,
            }],
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 7,
                ..TokenUsage::default()
            },
            advisory_input: vec![Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: "do the thing".into(),
                }],
            )],
            from_cache: false,
        }
    }

    #[test]
    fn trace_json_has_expected_shape_and_keeps_raw_text() {
        let state = traced_state();
        let outcome = real_outcome();

        let line = build_trace_json("msg-1", &state, &outcome).expect("trace line");
        assert!(!line.contains('\n'), "trace must be a single line");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();

        assert!(value["ts"].as_u64().unwrap() > 0);
        assert_eq!(value["msg_id"], "msg-1");
        assert_eq!(value["fanout"], "user_turn");
        assert_eq!(value["from_cache"], false);
        assert_eq!(value["slots"][0]["label"], "mock/alpha");
        assert_eq!(value["slots"][0]["model"], "alpha");
        assert_eq!(value["slots"][0]["temperature"], 0.5);
        assert_eq!(value["advisory_input"][0]["role"], "user");
        assert_eq!(value["advisory_input"][0]["text"], "do the thing");
        // Trace stores the UNREDACTED advisor output.
        assert_eq!(
            value["outputs"][0]["text"],
            "raw advice with bob@example.com kept verbatim"
        );
        assert_eq!(value["outputs"][0]["input_tokens"], 10);
        assert_eq!(value["outputs"][0]["output_tokens"], 7);
        assert_eq!(value["outputs"][0]["failed"], false);
        assert_eq!(value["total"]["input_tokens"], 10);
        assert_eq!(value["total"]["output_tokens"], 7);
    }

    #[test]
    fn failed_slots_are_flagged() {
        let state = traced_state();
        let mut outcome = real_outcome();
        outcome.advices[0].text = "[failed: timeout after 5s]".into();
        outcome.slot_usage[0] = MoaSlotTurnUsage {
            label: "mock/alpha".into(),
            input_tokens: 0,
            output_tokens: 0,
        };

        let line = build_trace_json("msg-2", &state, &outcome).expect("trace line");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["outputs"][0]["failed"], true);
        assert_eq!(value["outputs"][0]["input_tokens"], 0);
    }

    #[test]
    fn cache_hit_rounds_produce_no_trace() {
        let state = traced_state();
        let mut outcome = real_outcome();
        outcome.from_cache = true;
        assert!(build_trace_json("msg-3", &state, &outcome).is_none());
    }

    #[test]
    fn disabled_tracing_produces_no_trace() {
        let mut state = traced_state();
        state.config.trace_enabled = false;
        assert!(build_trace_json("msg-4", &state, &real_outcome()).is_none());
    }
}
