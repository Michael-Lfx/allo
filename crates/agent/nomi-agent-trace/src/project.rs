//! Project JSONL events into per-turn workflow views. Sort only by `event_seq`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{
    ids_from_payload, Integrity, ObservationEvent, ObservationScope, EVENT_LLM_REQUEST,
    EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED,
    EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedTurn {
    pub root_turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_attempt_id: Option<String>,
    pub integrity: Integrity,
    pub interrupted: bool,
    pub gap_count: u32,
    pub model_calls: Vec<ProjectedModelCall>,
    pub gaps: Vec<ProjectedGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedModelCall {
    pub model_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_scope: Option<ObservationScope>,
    pub interrupted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(default)]
    pub tools: Vec<ProjectedToolExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedToolExecution {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedGap {
    pub event_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_seq: Option<u64>,
}

/// Group by turn / execution boundary, then order each group by `event_seq` only.
pub fn project_turns(events: &[ObservationEvent]) -> Vec<ProjectedTurn> {
    project_event_refs(events.iter())
}

pub(crate) fn project_event_refs<'a, I>(events: I) -> Vec<ProjectedTurn>
where
    I: IntoIterator<Item = &'a ObservationEvent>,
{
    let mut grouped: Vec<(String, Vec<&ObservationEvent>)> = Vec::new();
    for event in events {
        let key = turn_key(event);
        if let Some((_, bucket)) = grouped.iter_mut().find(|(k, _)| k == &key) {
            bucket.push(event);
        } else {
            grouped.push((key, vec![event]));
        }
    }

    grouped
        .into_iter()
        .map(|(_, mut bucket)| {
            bucket.sort_by_key(|event| event.event_seq);
            project_one(&bucket)
        })
        .collect()
}

pub fn project_turn_by_id(events: &[ObservationEvent], root_turn_id: &str) -> Option<ProjectedTurn> {
    project_turns(events)
        .into_iter()
        .find(|turn| turn.root_turn_id == root_turn_id)
}

fn turn_key(event: &ObservationEvent) -> String {
    let ids = ids_from_payload(&event.payload);
    ids.root_turn_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| ids.boundary_id())
}

/// Whether this event belongs to the projected turn identified by `root_turn_id`.
pub fn event_belongs_to_turn(event: &ObservationEvent, root_turn_id: &str) -> bool {
    turn_key(event) == root_turn_id
}

/// Drop request/response/tool payloads so list APIs can return counts only.
pub fn strip_projected_turn_payloads(turn: &mut ProjectedTurn) {
    for call in &mut turn.model_calls {
        call.request = None;
        call.response = None;
        for tool in &mut call.tools {
            tool.started = None;
            tool.completed = None;
            tool.failed = None;
            tool.cancelled = None;
        }
    }
}

fn project_one(events: &[&ObservationEvent]) -> ProjectedTurn {
    let first_ids = events
        .first()
        .map(|event| ids_from_payload(&event.payload))
        .unwrap_or_default();
    let root_turn_id = first_ids
        .root_turn_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| first_ids.boundary_id());

    let mut calls: Vec<ProjectedModelCall> = Vec::new();
    let mut gaps = Vec::new();

    for event in events {
        let ids = ids_from_payload(&event.payload);
        match event.event_type.as_str() {
            EVENT_LLM_REQUEST => {
                let model_call_id = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let call = upsert_call(&mut calls, &model_call_id);
                call.call_kind = string_field(&event.payload, "call_kind");
                call.observation_scope = event
                    .payload
                    .get("observation_scope")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                call.request = Some(event.payload.clone());
            }
            EVENT_LLM_RESPONSE => {
                let model_call_id = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let call = upsert_call(&mut calls, &model_call_id);
                if call.call_kind.is_none() {
                    call.call_kind = string_field(&event.payload, "call_kind");
                }
                call.response = Some(event.payload.clone());
            }
            EVENT_TOOL_EXECUTION_STARTED
            | EVENT_TOOL_EXECUTION_COMPLETED
            | EVENT_TOOL_EXECUTION_FAILED
            | EVENT_TOOL_EXECUTION_CANCELLED => {
                let model_call_id = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let tool_call_id = event
                    .payload
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let call = upsert_call(&mut calls, &model_call_id);
                let tool = upsert_tool(&mut call.tools, &tool_call_id);
                if tool.name.is_none() {
                    tool.name = string_field(&event.payload, "name")
                        .or_else(|| string_field(&event.payload, "tool_name"));
                }
                match event.event_type.as_str() {
                    EVENT_TOOL_EXECUTION_STARTED => tool.started = Some(event.payload.clone()),
                    EVENT_TOOL_EXECUTION_COMPLETED => tool.completed = Some(event.payload.clone()),
                    EVENT_TOOL_EXECUTION_FAILED => tool.failed = Some(event.payload.clone()),
                    EVENT_TOOL_EXECUTION_CANCELLED => tool.cancelled = Some(event.payload.clone()),
                    _ => {}
                }
            }
            EVENT_OBSERVATION_GAP => {
                gaps.push(ProjectedGap {
                    event_seq: event.event_seq,
                    reason: string_field(&event.payload, "reason"),
                    from_seq: event.payload.get("from_seq").and_then(Value::as_u64),
                    to_seq: event.payload.get("to_seq").and_then(Value::as_u64),
                });
            }
            _ => {}
        }
    }

    for call in &mut calls {
        call.interrupted = call.request.is_some() && call.response.is_none();
    }

    let interrupted = calls.iter().any(|call| call.interrupted);
    let integrity = if interrupted || !gaps.is_empty() {
        Integrity::Degraded
    } else {
        Integrity::Complete
    };

    ProjectedTurn {
        root_turn_id,
        conversation_id: first_ids.conversation_id,
        msg_id: first_ids.msg_id,
        session_kind: first_ids.session_kind,
        execution_id: first_ids.execution_id,
        step_id: first_ids.step_id,
        execution_attempt_id: first_ids.execution_attempt_id,
        integrity,
        interrupted,
        gap_count: gaps.len() as u32,
        model_calls: calls,
        gaps,
    }
}

fn upsert_call<'a>(calls: &'a mut Vec<ProjectedModelCall>, model_call_id: &str) -> &'a mut ProjectedModelCall {
    if let Some(index) = calls.iter().position(|call| call.model_call_id == model_call_id) {
        return &mut calls[index];
    }
    calls.push(ProjectedModelCall {
        model_call_id: model_call_id.to_owned(),
        call_kind: None,
        observation_scope: None,
        interrupted: false,
        request: None,
        response: None,
        tools: Vec::new(),
    });
    calls.last_mut().expect("just pushed")
}

fn upsert_tool<'a>(
    tools: &'a mut Vec<ProjectedToolExecution>,
    tool_call_id: &str,
) -> &'a mut ProjectedToolExecution {
    if let Some(index) = tools.iter().position(|tool| tool.tool_call_id == tool_call_id) {
        return &mut tools[index];
    }
    tools.push(ProjectedToolExecution {
        tool_call_id: tool_call_id.to_owned(),
        name: None,
        started: None,
        completed: None,
        failed: None,
        cancelled: None,
    });
    tools.last_mut().expect("just pushed")
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ObservationEvent, ObservationIds, EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE};

    fn event(event_type: &str, seq: u64, ids: ObservationIds, extra: Value) -> ObservationEvent {
        let mut payload = extra.as_object().cloned().unwrap_or_default();
        payload.insert("ids".into(), serde_json::to_value(ids).unwrap());
        ObservationEvent::new(
            event_type,
            seq,
            "2026-08-18T10:00:00Z",
            1_000 + seq,
            Value::Object(payload),
        )
    }

    fn turn_ids(turn: &str, model_call: &str) -> ObservationIds {
        ObservationIds {
            conversation_id: Some("c1".into()),
            root_turn_id: Some(turn.into()),
            model_call_id: Some(model_call.into()),
            ..ObservationIds::default()
        }
    }

    #[test]
    fn interrupted_without_response_degrades_boundary() {
        let events = vec![event(
            EVENT_LLM_REQUEST,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({ "call_kind": "agent_turn" }),
        )];
        let turns = project_turns(&events);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].interrupted);
        assert_eq!(turns[0].integrity, Integrity::Degraded);
        assert!(turns[0].model_calls[0].interrupted);
        assert!(turns[0].model_calls[0].response.is_none());
    }

    #[test]
    fn gap_degrades_without_marking_interrupted() {
        let events = vec![
            event(
                EVENT_LLM_REQUEST,
                1,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "call_kind": "agent_turn" }),
            ),
            event(
                EVENT_LLM_RESPONSE,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({}),
            ),
            event(
                EVENT_OBSERVATION_GAP,
                3,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "reason": "sink_failed", "from_seq": 2, "to_seq": 4 }),
            ),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].integrity, Integrity::Degraded);
        assert!(!turns[0].interrupted);
        assert_eq!(turns[0].gap_count, 1);
        assert_eq!(turns[0].gaps[0].reason.as_deref(), Some("sink_failed"));
    }

    #[test]
    fn sorts_by_event_seq_not_timestamp() {
        let late = ObservationEvent::new(
            EVENT_LLM_RESPONSE,
            2,
            "2026-08-18T09:00:00Z",
            1,
            serde_json::json!({
                "ids": turn_ids("t1", "mc1")
            }),
        );
        let early = ObservationEvent::new(
            EVENT_LLM_REQUEST,
            1,
            "2026-08-18T12:00:00Z",
            9_999,
            serde_json::json!({
                "ids": turn_ids("t1", "mc1"),
                "call_kind": "agent_turn"
            }),
        );
        let turns = project_turns(&[late, early]);
        assert!(!turns[0].interrupted);
        assert!(turns[0].model_calls[0].request.is_some());
        assert!(turns[0].model_calls[0].response.is_some());
    }

    #[test]
    fn complete_request_response_is_complete() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].integrity, Integrity::Complete);
        assert!(!turns[0].interrupted);
    }

    #[test]
    fn strip_projected_turn_payloads_keeps_counts() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
        ];
        let mut turns = project_turns(&events);
        assert!(turns[0].model_calls[0].request.is_some());
        strip_projected_turn_payloads(&mut turns[0]);
        assert!(turns[0].model_calls[0].request.is_none());
        assert!(turns[0].model_calls[0].response.is_none());
        assert_eq!(turns[0].model_calls.len(), 1);
    }
}
