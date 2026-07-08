//! OpenAI-compatible chat completions for media prompt refinement.

use serde_json::{Value, json};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::FlowyApiClient;

impl FlowyApiClient {
    /// Minimal non-streaming chat completion for side tasks (prompt refine, storyboard).
    pub async fn chat_completions_text(
        &self,
        session: &ServerSession,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: f64,
        model: Option<&str>,
    ) -> Result<String, ServerClientError> {
        let model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config().effective_default_llm_model());

        let body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": max_tokens,
            "temperature": temperature,
            "stream": false,
        });

        let value: Value = self
            .post_upstream_json(&self.llm_transport, "/chat/completions", session, body)
            .await?;

        let content = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        if content.is_empty() {
            return Err(ServerClientError::InvalidResponse(
                "chat completion returned empty content".into(),
            ));
        }
        Ok(content)
    }
}
