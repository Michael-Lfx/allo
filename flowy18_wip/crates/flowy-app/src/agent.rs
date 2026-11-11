//! Agent host port. UI depends on this trait only — never on `dsh_rs::llm`.

use std::sync::{Arc, Mutex};

/// A turn the UI can render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub role: AgentRole,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    User,
    Assistant,
}

/// Port the desktop shell binds to. Production may wrap `dsh_rs::api`; tests
/// and early wiring use [`MockAgentRuntime`].
pub trait AgentRuntime: Send {
    fn send(&mut self, text: &str) -> Result<(), &'static str>;
    fn events(&self) -> Vec<AgentEvent>;
    fn clear(&mut self);
}

/// In-memory runtime used until the dsh bundle is embedded.
#[derive(Debug, Default)]
pub struct MockAgentRuntime {
    events: Vec<AgentEvent>,
}

impl AgentRuntime for MockAgentRuntime {
    fn send(&mut self, text: &str) -> Result<(), &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        self.events.push(AgentEvent {
            role: AgentRole::User,
            text: text.to_string(),
        });
        self.events.push(AgentEvent {
            role: AgentRole::Assistant,
            text: format!("(mock) received: {text}"),
        });
        Ok(())
    }

    fn events(&self) -> Vec<AgentEvent> {
        self.events.clone()
    }

    fn clear(&mut self) {
        self.events.clear();
    }
}

/// Shared handle for the shell.
#[derive(Clone, Debug, Default)]
pub struct AgentHandle {
    inner: Arc<Mutex<MockAgentRuntime>>,
}

impl AgentHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn send(&self, text: &str) -> Result<(), &'static str> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(text)
    }

    #[must_use]
    pub fn events(&self) -> Vec<AgentEvent> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .events()
    }

    pub fn clear(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_records_user_and_assistant() {
        let mut runtime = MockAgentRuntime::default();
        runtime.send("hello").unwrap();
        let events = runtime.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].role, AgentRole::User);
        assert_eq!(events[1].role, AgentRole::Assistant);
    }

    #[test]
    fn empty_prompt_is_rejected() {
        let mut runtime = MockAgentRuntime::default();
        assert_eq!(runtime.send("  "), Err("notice.prompt_empty"));
    }
}
