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
    TaskReceipt {
        task_id: String,
        goal: String,
    },
    /// One file the agent changed, summarised for the chat card.
    FileChange {
        path: String,
        /// Named `change` because `kind` is the enum's own serde tag.
        change: FileChangeKind,
        added: u32,
        removed: u32,
        /// Non-text content: line counts carry no meaning, so the card says so
        /// instead of printing zeros.
        #[serde(default)]
        binary: bool,
    },
}

/// How a file changed during one settled tool call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Add,
    Update,
    Delete,
    Move,
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
    pending_reasoning: String,
    reasoning_started: Option<std::time::Instant>,
}

#[derive(Clone, Debug, Default)]
pub struct ChatHistory {
    pub rows: Vec<ChatLine>,
    pub streaming: Option<usize>,
    pub status: TurnStatus,
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
            ..Self::default()
        }
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        self.lines.push(ChatLine::User { text: text.into() });
    }

    /// Accumulate streamed assistant text; ends the reasoning clock on the
    /// first text delta so the reported thinking duration excludes generation.
    pub fn apply_text(&mut self, delta: &str) {
        self.flush_reasoning();
        self.pending.push_str(delta);
    }

    /// Accumulate streamed reasoning; it is finalized when the turn ends.
    pub fn apply_reasoning(&mut self, delta: &str) {
        if self.reasoning_started.is_none() {
            self.reasoning_started = Some(std::time::Instant::now());
        }
        self.pending_reasoning.push_str(delta);
    }

    /// Move accumulated reasoning into a durable row.
    pub fn flush_reasoning(&mut self) {
        if self.pending_reasoning.is_empty() {
            self.reasoning_started = None;
            return;
        }
        let duration_secs = self
            .reasoning_started
            .take()
            .map_or(0, |started| started.elapsed().as_secs());
        let text = std::mem::take(&mut self.pending_reasoning);
        self.lines.push(ChatLine::Reasoning {
            text,
            duration_secs,
            live: false,
        });
    }

    /// Move accumulated reasoning and text into durable rows.
    pub fn flush(&mut self) {
        self.flush_reasoning();
        if self.pending.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.pending);
        self.lines.push(ChatLine::Assistant { text });
    }

    /// Rows the UI should render now, including in-flight rows.
    #[must_use]
    pub fn visible_lines(&self) -> Vec<ChatLine> {
        let mut lines = self.lines.clone();
        if !self.pending_reasoning.is_empty() {
            let duration_secs = self
                .reasoning_started
                .map_or(0, |started| started.elapsed().as_secs());
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

    #[must_use]
    pub fn streaming_row(&self) -> Option<usize> {
        (!self.pending.is_empty()).then_some(
            self.lines.len() + usize::from(!self.pending_reasoning.is_empty()),
        )
    }

    pub fn push_tool_call(
        &mut self,
        name: impl Into<String>,
        target: impl Into<String>,
        call_id: impl Into<String>,
    ) {
        self.flush();
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
            if let ChatLine::ToolCall {
                call_id: id,
                running,
                ..
            } = line
                && id == call_id
            {
                *running = false;
                break;
            }
        }
        self.lines.push(ChatLine::ToolResult {
            text: text.into(),
            is_error,
        });
    }

    /// Append the file-change row for a tool call that already settled.
    /// Consecutive writes to the same path fold into one row so a turn that
    /// rewrites a file several times does not produce duplicate cards.
    pub fn push_file_change(
        &mut self,
        path: impl Into<String>,
        kind: FileChangeKind,
        added: u32,
        removed: u32,
        binary: bool,
    ) {
        let path = path.into();
        if let Some(ChatLine::FileChange {
            path: last_path,
            added: last_added,
            removed: last_removed,
            binary: last_binary,
            ..
        }) = self.lines.last_mut()
            && last_path.as_str() == path.as_str()
        {
            *last_added = last_added.saturating_add(added);
            *last_removed = last_removed.saturating_add(removed);
            *last_binary = *last_binary || binary;
            return;
        }
        self.lines.push(ChatLine::FileChange {
            path,
            change: kind,
            added,
            removed,
            binary,
        });
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::{ChatLine, ChatProjection};

    #[test]
    fn tools_settle_the_preceding_assistant_fragment_in_place() {
        let mut projection = ChatProjection::new();
        projection.push_user("question");
        projection.apply_text("before");
        assert_eq!(projection.streaming_row(), Some(1));
        projection.push_tool_call("tool", "", "call");
        assert_eq!(projection.streaming_row(), None);
        projection.finish_tool_call("call", "out", false);
        projection.apply_text("after");
        assert_eq!(projection.streaming_row(), Some(4));
        let rows = projection.visible_lines();
        assert!(
            matches!(&rows[1], ChatLine::Assistant { text } if text == "before")
        );
        assert!(matches!(&rows[2], ChatLine::ToolCall { .. }));
        assert!(
            matches!(&rows[4], ChatLine::Assistant { text } if text == "after")
        );
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
    fn reasoning_is_visible_live_and_finalized_on_flush() {
        let mut projection = ChatProjection::new();
        projection.apply_reasoning("think");
        projection.apply_reasoning("ing");
        let visible = projection.visible_lines();
        assert!(matches!(
            visible.last(),
            Some(ChatLine::Reasoning { text, live: true, .. }) if text == "thinking"
        ));
        projection.flush();
        assert!(matches!(
            projection.lines.last(),
            Some(ChatLine::Reasoning { text, live: false, .. }) if text == "thinking"
        ));
    }

    #[test]
    fn first_text_delta_ends_the_reasoning_row() {
        let mut projection = ChatProjection::new();
        projection.apply_reasoning("think");
        projection.apply_text("answer");
        assert!(matches!(
            projection.lines.first(),
            Some(ChatLine::Reasoning { text, live: false, .. }) if text == "think"
        ));
        assert_eq!(
            projection.visible_lines().last(),
            Some(&ChatLine::Assistant {
                text: "answer".into()
            })
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
        let line: ChatLine =
            serde_json::from_str(r#"{"kind":"tool","text":"legacy"}"#).unwrap();
        assert_eq!(
            line,
            ChatLine::Tool {
                text: "legacy".into()
            }
        );
        let status: TurnStatus =
            serde_json::from_str(r#"{"kind":"error","message":"boom"}"#)
                .unwrap();
        assert_eq!(
            status,
            TurnStatus::Error {
                message: "boom".into()
            }
        );
    }
}
