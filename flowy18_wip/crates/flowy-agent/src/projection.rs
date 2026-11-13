use dsh_rs::types::{SessionEvent, SessionEventData, StreamChunk, TurnEndReason};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatLine {
    User { text: String },
    Assistant { text: String },
    Tool { text: String },
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
}

#[derive(Clone, Debug, Default)]
pub struct ChatProjection {
    pub lines: Vec<ChatLine>,
    pending: String,
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
            SessionEventData::AssistantChunk { chunk, .. } => {
                if let StreamChunk::TextDelta { text, .. } = chunk {
                    self.pending.push_str(text);
                    true
                } else {
                    false
                }
            }
            SessionEventData::AssistantMessage { message, .. } => {
                let text = message.text();
                self.pending.clear();
                if text.is_empty() {
                    return false;
                }
                self.lines.push(ChatLine::Assistant { text });
                true
            }
            SessionEventData::ToolCall { name, .. } => {
                self.flush_pending();
                self.lines.push(ChatLine::Tool {
                    text: format!("→ {name}"),
                });
                true
            }
            SessionEventData::ToolResult { message, .. } => {
                let text = message.text();
                if text.is_empty() {
                    return false;
                }
                self.lines.push(ChatLine::Tool { text });
                true
            }
            SessionEventData::TurnEnd { reason, .. } => match reason {
                TurnEndReason::Completed => false,
                TurnEndReason::Aborted { .. } => {
                    self.flush_pending();
                    self.lines.push(ChatLine::Status {
                        text: "chat.status_stopped".into(),
                    });
                    true
                }
                TurnEndReason::Error { message, .. } => {
                    self.flush_pending();
                    self.lines.push(ChatLine::Status {
                        text: message.clone(),
                    });
                    true
                }
                other => {
                    self.flush_pending();
                    self.lines.push(ChatLine::Status {
                        text: format!("{other:?}"),
                    });
                    true
                }
            },
            _ => false,
        }
    }

    pub fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            self.lines.push(ChatLine::Assistant {
                text: std::mem::take(&mut self.pending),
            });
        }
    }

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

    pub fn from_lines(lines: Vec<ChatLine>) -> Self {
        Self {
            lines,
            pending: String::new(),
        }
    }
}
