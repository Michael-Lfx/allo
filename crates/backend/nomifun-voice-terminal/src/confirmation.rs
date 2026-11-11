//! Single-use 8-second confirmation token manager.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.7.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationToken {
    pub token_id: String,
    pub task_id: String,
    pub created_at: Instant,
    pub ttl: Duration,
    pub consumed: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfirmationError {
    #[error("Token not found: {0}")]
    NotFound(String),
    #[error("Token expired (TTL exceeded 8s)")]
    Expired,
    #[error("Token has already been consumed (single-use constraint)")]
    AlreadyConsumed,
}

/// Manages 8-second single-use confirmation tokens for destructive actions.
pub struct ConfirmationManager {
    tokens: HashMap<String, ConfirmationToken>,
    next_id: u64,
}

impl ConfirmationManager {
    pub const DEFAULT_TTL: Duration = Duration::from_secs(8);

    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn issue_token(&mut self, task_id: impl Into<String>) -> ConfirmationToken {
        let token_id = format!("token_{}_{}", self.next_id, self.tokens.len());
        self.next_id += 1;

        let token = ConfirmationToken {
            token_id: token_id.clone(),
            task_id: task_id.into(),
            created_at: Instant::now(),
            ttl: Self::DEFAULT_TTL,
            consumed: false,
        };

        self.tokens.insert(token_id, token.clone());
        token
    }

    pub fn consume_token(&mut self, token_id: &str) -> Result<String, ConfirmationError> {
        let token = self
            .tokens
            .get_mut(token_id)
            .ok_or_else(|| ConfirmationError::NotFound(token_id.to_string()))?;

        if token.consumed {
            return Err(ConfirmationError::AlreadyConsumed);
        }

        if token.created_at.elapsed() > token.ttl {
            return Err(ConfirmationError::Expired);
        }

        token.consumed = true;
        Ok(token.task_id.clone())
    }

    pub fn is_expired(&self, token_id: &str) -> bool {
        if let Some(token) = self.tokens.get(token_id) {
            token.created_at.elapsed() > token.ttl
        } else {
            true
        }
    }

    pub fn revoke_token(&mut self, token_id: &str) -> bool {
        self.tokens.remove(token_id).is_some()
    }
}

impl Default for ConfirmationManager {
    fn default() -> Self {
        Self::new()
    }
}
