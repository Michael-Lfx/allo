use dsh_rs::types::{
    ContentBlock, SessionEvent, SessionEventData, StreamChunk, TurnEndReason,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatLine {
    User { text: String },
    Assistant { text: String },
    Reasoning {
        text: String,
        #[serde(default)]
        duration_secs: u64,
        #[serde(default)]
        live: bool,
    },
    /// Legacy flat tool row (persisted sessions).
    Tool { text: String },
    ToolCall {
        name: String,
        #[serde(default)]
        target: String,
        #[serde(default)]
        call_id: String,
        #[serde(default)]
        running: bool,
    },
    ToolResult {
        text: String,
        #[serde(default)]
        is_error: bool,
    },
    Status { text: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnStatus {
    #[default]
    Idle,
    Running,
    Completed,
    Stopped,
    Error { message: String },
}

impl TurnStatus {
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Idle => "chat.status_idle",
            Self::Running => "chat.status_running",
            Self::Completed => "chat.status_completed",
            Self::Stopped => "chat.status_stopped",
            Self::Error { .. } => "chat.status_error",
        }
    }

    #[must_use]
    pub const fn is_busy(&self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChatProjection {
    pub lines: Vec<ChatLine>,
    pending: String,
    pending_reasoning: String,
    reasoning_started_ms: Option<u64>,
}

impl ChatProjection {
    pub fn apply(&mut self, event: &SessionEvent) -> bool {
        match &event.data {
            SessionEventData::UserMessage { message } => {
                let text = message.text();
                if text.is_empty() {
                    return false;
                }
                self.flush_pending();
                self.lines.push(ChatLine::User { text });
                true
            }
            SessionEventData::AssistantChunk { chunk, .. } => self.apply_chunk(chunk, event.time),
            SessionEventData::AssistantMessage { message, .. } => {
                // Drop in-flight deltas; the assembled message is authoritative.
                let had_pending =
                    !self.pending.is_empty() || !self.pending_reasoning.is_empty();
                self.pending.clear();
                self.pending_reasoning.clear();
                self.reasoning_started_ms = None;
                let mut changed = had_pending;
                for block in &message.content {
                    match block {
                        ContentBlock::Reasoning { text } if !text.is_empty() => {
                            if !self.lines.iter().rev().take(8).any(|line| {
                                matches!(
                                    line,
                                    ChatLine::Reasoning { text: existing, .. } if existing == text
                                )
                            }) {
                                self.lines.push(ChatLine::Reasoning {
                                    text: text.clone(),
                                    duration_secs: 1,
                                    live: false,
                                });
                                changed = true;
                            }
                        }
                        ContentBlock::Text { text } if !text.is_empty() => {
                            if !self.lines.iter().rev().take(4).any(|line| {
                                matches!(
                                    line,
                                    ChatLine::Assistant { text: existing } if existing == text
                                )
                            }) {
                                self.lines.push(ChatLine::Assistant {
                                    text: text.clone(),
                                });
                                changed = true;
                            }
                        }
                        _ => {}
                    }
                }
                changed
            }
            SessionEventData::ToolCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                self.flush_pending();
                let target = tool_target(arguments);
                if let Some(ChatLine::ToolCall {
                    call_id: existing_id,
                    running,
                    target: existing_target,
                    ..
                }) = self.lines.iter_mut().rev().find(|line| {
                    matches!(line, ChatLine::ToolCall { call_id: id, .. } if id == call_id)
                }) {
                    *running = true;
                    if existing_target.is_empty() && !target.is_empty() {
                        *existing_target = target;
                    }
                    let _ = existing_id;
                    return true;
                }
                self.lines.push(ChatLine::ToolCall {
                    name: name.clone(),
                    target,
                    call_id: call_id.clone(),
                    running: true,
                });
                true
            }
            SessionEventData::ToolResult { message, .. } => {
                let text = tool_result_text(message);
                let is_error = message_is_error(message);
                if let Some(ChatLine::ToolCall { running, .. }) = self
                    .lines
                    .iter_mut()
                    .rev()
                    .find(|line| matches!(line, ChatLine::ToolCall { running: true, .. }))
                {
                    *running = false;
                }
                if text.is_empty() {
                    return true;
                }
                self.lines.push(ChatLine::ToolResult { text, is_error });
                true
            }
            SessionEventData::TurnEnd { reason, .. } => {
                self.flush_pending();
                self.settle_running_tools();
                match reason {
                    TurnEndReason::Completed => false,
                    TurnEndReason::Aborted { .. } => {
                        self.lines.push(ChatLine::Status {
                            text: "chat.status_stopped".into(),
                        });
                        true
                    }
                    TurnEndReason::Error { message, .. } => {
                        self.lines.push(ChatLine::Status {
                            text: message.clone(),
                        });
                        true
                    }
                    other => {
                        self.lines.push(ChatLine::Status {
                            text: format!("{other:?}"),
                        });
                        true
                    }
                }
            }
            _ => false,
        }
    }

    fn apply_chunk(&mut self, chunk: &StreamChunk, time_ms: u64) -> bool {
        match chunk {
            StreamChunk::TextDelta { text, .. } => {
                self.flush_reasoning(time_ms, false);
                self.pending.push_str(text);
                true
            }
            StreamChunk::ReasoningDelta { text, .. } => {
                if self.pending_reasoning.is_empty() {
                    self.reasoning_started_ms = Some(time_ms);
                }
                self.pending_reasoning.push_str(text);
                true
            }
            StreamChunk::BlockEnd {
                block: ContentBlock::Reasoning { text },
                ..
            } => {
                if !text.is_empty() && self.pending_reasoning.is_empty() {
                    self.pending_reasoning = text.clone();
                    if self.reasoning_started_ms.is_none() {
                        self.reasoning_started_ms = Some(time_ms);
                    }
                }
                self.flush_reasoning(time_ms, false);
                true
            }
            StreamChunk::BlockEnd {
                block: ContentBlock::Text { text },
                ..
            } => {
                self.flush_reasoning(time_ms, false);
                if !text.is_empty() && self.pending.is_empty() {
                    self.pending = text.clone();
                }
                self.flush_text();
                true
            }
            StreamChunk::ToolCallDelta {
                name,
                arguments_delta,
                id,
                ..
            } => {
                self.flush_pending();
                let call_id = id.clone();
                if let Some(ChatLine::ToolCall {
                    name: existing_name,
                    target,
                    running,
                    ..
                }) = self.lines.iter_mut().rev().find(|line| {
                    matches!(line, ChatLine::ToolCall { call_id: existing, .. } if existing == &call_id)
                }) {
                    if let Some(name) = name {
                        *existing_name = name.clone();
                    }
                    if !arguments_delta.is_empty() {
                        let next = tool_target(arguments_delta);
                        if !next.is_empty() {
                            *target = next;
                        }
                    }
                    *running = true;
                } else {
                    self.lines.push(ChatLine::ToolCall {
                        name: name.clone().unwrap_or_else(|| "tool".into()),
                        target: tool_target(arguments_delta),
                        call_id,
                        running: true,
                    });
                }
                true
            }
            _ => false,
        }
    }

    pub fn flush_pending(&mut self) {
        let now = now_ms();
        self.flush_reasoning(now, false);
        self.flush_text();
    }

    fn flush_text(&mut self) {
        if !self.pending.is_empty() {
            self.lines.push(ChatLine::Assistant {
                text: std::mem::take(&mut self.pending),
            });
        }
    }

    fn flush_reasoning(&mut self, ended_ms: u64, live: bool) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        let started = self.reasoning_started_ms.unwrap_or(ended_ms);
        let duration_secs = ended_ms.saturating_sub(started).div_ceil(1000).max(1);
        self.lines.push(ChatLine::Reasoning {
            text: std::mem::take(&mut self.pending_reasoning),
            duration_secs,
            live,
        });
        self.reasoning_started_ms = None;
    }

    fn settle_running_tools(&mut self) {
        for line in &mut self.lines {
            if let ChatLine::ToolCall { running, .. } = line {
                *running = false;
            }
        }
    }

    #[must_use]
    pub fn visible_lines(&self) -> Vec<ChatLine> {
        let mut lines = self.lines.clone();
        if !self.pending_reasoning.is_empty() {
            let started = self.reasoning_started_ms.unwrap_or_else(now_ms);
            let duration_secs = now_ms().saturating_sub(started).div_ceil(1000).max(1);
            lines.push(ChatLine::Reasoning {
                text: self.pending_reasoning.clone(),
                duration_secs,
                live: true,
            });
        }
        if !self.pending.is_empty() {
            lines.push(ChatLine::Assistant {
                text: self.pending.clone(),
            });
        }
        lines
    }

    pub fn from_lines(lines: Vec<ChatLine>) -> Self {
        Self {
            lines,
            pending: String::new(),
            pending_reasoning: String::new(),
            reasoning_started_ms: None,
        }
    }
}

fn tool_target(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in [
            "url",
            "uri",
            "path",
            "file_path",
            "file",
            "query",
            "command",
            "cmd",
            "target",
            "pattern",
            "name",
        ] {
            if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
                let text = text.trim();
                if !text.is_empty() {
                    return truncate(text, 120);
                }
            }
        }
        return truncate(&value.to_string(), 120);
    }
    truncate(trimmed, 120)
}

fn truncate(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn tool_result_text(message: &dsh_rs::types::Message) -> String {
    let mut parts = Vec::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text } => parts.push(text.as_str()),
            ContentBlock::ToolResult { content, .. } => {
                for inner in content {
                    if let ContentBlock::Text { text } = inner {
                        parts.push(text.as_str());
                    }
                }
            }
            _ => {}
        }
    }
    let joined = parts.join("");
    truncate(&joined, 240)
}

fn message_is_error(message: &dsh_rs::types::Message) -> bool {
    message.content.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolResult {
                is_error: Some(true),
                ..
            }
        )
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsh_rs::types::{Message, MessageSource, Role, SessionEvent};

    fn event(data: SessionEventData) -> SessionEvent {
        SessionEvent::new(1, 1_000, data)
    }

    #[test]
    fn projects_reasoning_and_tool_call() {
        let mut proj = ChatProjection::default();
        assert!(proj.apply(&event(SessionEventData::AssistantChunk {
            turn: 1,
            step: 0,
            chunk: StreamChunk::ReasoningDelta {
                index: 0,
                text: "plan".into(),
            },
        })));
        assert!(proj.apply(&event(SessionEventData::AssistantChunk {
            turn: 1,
            step: 0,
            chunk: StreamChunk::TextDelta {
                index: 1,
                text: "hello".into(),
            },
        })));
        assert!(proj.apply(&event(SessionEventData::ToolCall {
            turn: 1,
            step: 0,
            call_id: "c1".into(),
            name: "webfetch".into(),
            arguments: r#"{"url":"https://example.com"}"#.into(),
        })));
        let lines = proj.visible_lines();
        assert!(
            lines.iter().any(|line| matches!(
                line,
                ChatLine::Reasoning { text, live: false, .. } if text == "plan"
            )),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| matches!(line, ChatLine::Assistant { text } if text == "hello")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| matches!(
                line,
                ChatLine::ToolCall {
                    name,
                    target,
                    running: true,
                    ..
                } if name == "webfetch" && target == "https://example.com"
            )),
            "{lines:?}"
        );
    }

    #[test]
    fn assistant_message_dedups_streamed_text() {
        let mut proj = ChatProjection::default();
        proj.pending = "hello".into();
        proj.flush_pending();
        let changed = proj.apply(&event(SessionEventData::AssistantMessage {
            turn: 1,
            step: 0,
            message: Message {
                id: "a1".into(),
                role: Role::Assistant,
                content: vec![ContentBlock::text("hello")],
                source: MessageSource::Model {
                    provider: "mock".into(),
                    model: "mock".into(),
                },
            },
            usage: None,
            interrupted: None,
        }));
        assert!(!changed);
        assert_eq!(
            proj.lines
                .iter()
                .filter(|line| matches!(line, ChatLine::Assistant { .. }))
                .count(),
            1
        );
    }
}
