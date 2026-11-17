//! Chat projection owned by the host.
//!
//! The durable index stores these rows. The projection itself is fed by the
//! engine observer (text deltas, tool lifecycle) and never interprets provider
//! wire types, so this crate no longer depends on any agent kernel.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatLine {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    Reasoning {
        text: String,
        #[serde(default)]
        duration_secs: u64,
        #[serde(default)]
        live: bool,
    },
    /// Legacy flat tool row (persisted sessions).
    Tool {
        text: String,
    },
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
    Status {
        text: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnStatus {
    #[default]
    Idle,
    Running,
    Completed,
    Stopped,
    Error {
        message: String,
    },
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
}

impl ChatProjection {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_lines(lines: Vec<ChatLine>) -> Self {
        Self {
            lines,
            pending: String::new(),
        }
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        self.lines.push(ChatLine::User { text: text.into() });
    }

    pub fn push_status(&mut self, text: impl Into<String>) {
        self.lines.push(ChatLine::Status { text: text.into() });
    }

    /// Accumulate streamed assistant text until the turn ends.
    pub fn apply_text(&mut self, delta: &str) {
        self.pending.push_str(delta);
    }

    /// Move accumulated text into a durable assistant row.
    pub fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.pending);
        self.lines.push(ChatLine::Assistant { text });
    }

    /// Rows the UI should render now, including the in-flight assistant text.
    #[must_use]
    pub fn visible_lines(&self) -> Vec<ChatLine> {
        let mut lines = self.lines.clone();
        if !self.pending.is_empty() {
            lines.push(ChatLine::Assistant {
                text: self.pending.clone(),
            });
        }
        lines
    }

    pub fn push_tool_call(
        &mut self,
        name: impl Into<String>,
        target: impl Into<String>,
        call_id: impl Into<String>,
    ) {
        self.lines.push(ChatLine::ToolCall {
            name: name.into(),
            target: target.into(),
            call_id: call_id.into(),
            running: true,
        });
    }

    /// Mark the call finished and append its result row.
    pub fn finish_tool_call(
        &mut self,
        call_id: &str,
        text: impl Into<String>,
        is_error: bool,
    ) {
        for line in self.lines.iter_mut().rev() {
            if let ChatLine::ToolCall { call_id: id, running, .. } = line {
                if id == call_id {
                    *running = false;
                    break;
                }
            }
        }
        self.lines.push(ChatLine::ToolResult {
            text: text.into(),
            is_error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatLine, ChatProjection, TurnStatus};

    #[test]
    fn text_is_visible_while_streaming_and_durable_after_flush() {
        let mut projection = ChatProjection::new();
        projection.push_user("hi");
        projection.apply_text("hel");
        projection.apply_text("lo");
        assert_eq!(
            projection.visible_lines(),
            vec![
                ChatLine::User { text: "hi".into() },
                ChatLine::Assistant {
                    text: "hello".into()
                }
            ]
        );
        assert_eq!(projection.lines.len(), 1);
        projection.flush();
        assert_eq!(projection.lines.len(), 2);
        assert_eq!(
            projection.lines[1],
            ChatLine::Assistant {
                text: "hello".into()
            }
        );
    }

    #[test]
    fn tool_result_marks_the_matching_call_finished() {
        let mut projection = ChatProjection::new();
        projection.push_tool_call("read_file", "a.txt", "call-1");
        projection.finish_tool_call("call-1", "contents", false);
        assert!(matches!(
            &projection.lines[0],
            ChatLine::ToolCall { running: false, .. }
        ));
        assert_eq!(
            projection.lines[1],
            ChatLine::ToolResult {
                text: "contents".into(),
                is_error: false
            }
        );
    }

    #[test]
    fn legacy_rows_keep_their_wire_shape() {
        let line: ChatLine = serde_json::from_str(r#"{"kind":"tool","text":"legacy"}"#).unwrap();
        assert_eq!(line, ChatLine::Tool { text: "legacy".into() });
        let status: TurnStatus =
            serde_json::from_str(r#"{"kind":"error","message":"boom"}"#).unwrap();
        assert_eq!(status, TurnStatus::Error { message: "boom".into() });
    }
}
