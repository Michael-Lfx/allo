//! Flowy chat backend (text + vision via multimodal content parts).

use async_trait::async_trait;
use base64::Engine;
use serde_json::json;
use std::path::Path;

use super::{FlowyVimaxServices, VimaxChat, map_model_err};
use crate::error::VimaxResult;

pub struct FlowyChat {
    services: FlowyVimaxServices,
    /// Session override; empty / None → Flowy server default LLM.
    model: Option<String>,
}

impl FlowyChat {
    pub fn new(services: FlowyVimaxServices, model: Option<String>) -> Self {
        Self {
            services,
            model: nonempty(model),
        }
    }

    fn model_arg(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[async_trait]
impl VimaxChat for FlowyChat {
    async fn complete_text(&self, system: &str, user: &str) -> VimaxResult<String> {
        self.services.require_token().await?;
        self.services
            .api
            .chat_completions_text(
                &self.services.session,
                system,
                user,
                8192,
                0.7,
                self.model_arg(),
            )
            .await
            .map_err(|e| {
                map_model_err(
                    "llm",
                    self.model_arg(),
                    "chat_completions",
                    e,
                )
            })
    }

    async fn complete_vision(
        &self,
        system: &str,
        user_text: &str,
        image_paths: &[&Path],
    ) -> VimaxResult<String> {
        self.services.require_token().await?;

        let mut user_parts = vec![json!({"type": "text", "text": user_text})];
        for path in image_paths {
            let bytes = tokio::fs::read(path).await?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let mime = mime_for_path(path);
            user_parts.push(json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{mime};base64,{b64}") }
            }));
        }

        self.services
            .api
            .chat_completions_multimodal(
                &self.services.session,
                system,
                json!(user_parts),
                4096,
                0.3,
                self.model_arg(),
            )
            .await
            .map_err(|e| {
                map_model_err(
                    "llm",
                    self.model_arg(),
                    "chat_completions_vision",
                    e,
                )
            })
    }
}

fn nonempty(model: Option<String>) -> Option<String> {
    model.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    }
}
