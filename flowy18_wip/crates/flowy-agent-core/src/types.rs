//! Message and tool vocabulary owned by the client, not by a kernel crate.
//!
//! Shapes mirror the wire format previously used by `dsh-rs` so existing
//! projections and adapters can migrate without a second serialization dialect.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub type MessageId = String;
pub type CallId = String;

/// Cooperative cancellation for one turn, tool call, or provider request.
///
/// Thin wrapper over `tokio_util` so the loop can pass the same token to a
/// provider request; `race` keeps the tool-side helper shape.
#[derive(Clone, Default)]
pub struct CancelToken {
    inner: tokio_util::sync::CancellationToken,
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: tokio_util::sync::CancellationToken::new(),
        }
    }

    /// Fire the cancellation; idempotent.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Resolve as soon as the token is cancelled.
    pub async fn cancelled(&self) {
        self.inner.cancelled().await;
    }

    /// Run `fut` until the token fires; `None` means cancelled first.
    pub async fn race<T>(
        &self,
        fut: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        tokio::select! {
            () = self.cancelled() => None,
            value = fut => Some(value),
        }
    }

    /// The underlying token, for crates that speak `tokio_util` directly.
    #[must_use]
    pub fn inner(&self) -> tokio_util::sync::CancellationToken {
        self.inner.clone()
    }

    /// Adopt a token owned by another component (for example a loop signal).
    #[must_use]
    pub fn from_inner(inner: tokio_util::sync::CancellationToken) -> Self {
        Self { inner }
    }
}

/// One typed content block in a stored or model-facing message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ToolCall {
        id: CallId,
        name: String,
        /// Raw JSON string exactly as the model produced it.
        arguments: String,
    },
    ToolResult {
        tool_call_id: CallId,
        content: Vec<ContentBlock>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Where a message came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MessageSource {
    User,
    Model { provider: String, model: String },
    Tool { tool: String },
}

/// One immutable stored message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub source: MessageSource,
}

impl Message {
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(ContentBlock::as_text)
            .collect()
    }
}

/// Model-facing tool schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Parsed arguments for one tool call.
#[derive(Debug, Clone)]
pub struct ToolCallArgs {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
}

/// The model-facing outcome of one tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ToolExecutionResult {
    Success {
        content: Vec<ContentBlock>,
        value: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
    Error {
        message: String,
        code: String,
        content: Vec<ContentBlock>,
    },
}

impl ToolExecutionResult {
    pub fn success_text(text: impl Into<String>, value: Value) -> Self {
        ToolExecutionResult::Success {
            content: vec![ContentBlock::text(text)],
            value,
            meta: None,
        }
    }

    pub fn success_value(value: Value) -> Self {
        let text = serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| value.to_string());
        ToolExecutionResult::success_text(text, value)
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        let message: String = message.into();
        let code: String = code.into();
        ToolExecutionResult::Error {
            content: vec![ContentBlock::text(format!("Error: {message}"))],
            message,
            code,
        }
    }

    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, ToolExecutionResult::Error { .. })
    }

    #[must_use]
    pub fn content(&self) -> Vec<ContentBlock> {
        match self {
            ToolExecutionResult::Success { content, .. }
            | ToolExecutionResult::Error { content, .. } => content.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

/// Host-owned sink for the `todo_write` tool.
///
/// The core crate never touches a session service or event bus; the embedding
/// host decides where todos live.
pub trait TodoSink: Send + Sync {
    fn replace_todos(&self, agent_id: Option<&str>, todos: Vec<TodoItem>);
}

/// Runtime context handed to a tool body.
#[derive(Clone)]
pub struct ToolRunContext {
    pub signal: CancelToken,
    pub agent_id: Option<String>,
    pub cwd: Option<String>,
    pub todos: Option<Arc<dyn TodoSink>>,
}

impl std::fmt::Debug for ToolRunContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRunContext")
            .field("cancelled", &self.signal.is_cancelled())
            .field("agent_id", &self.agent_id)
            .field("cwd", &self.cwd)
            .field("has_todo_sink", &self.todos.is_some())
            .finish()
    }
}
