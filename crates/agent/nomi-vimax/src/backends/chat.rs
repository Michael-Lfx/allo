//! Flowy chat backend (text + vision via multimodal content parts).

use async_trait::async_trait;
use base64::Engine;
use nomifun_cloud::ClawModelEntry;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use super::{FlowyVimaxServices, VimaxChat, map_model_err};
use crate::error::{VimaxError, VimaxResult};

pub struct FlowyChat {
    services: FlowyVimaxServices,
    /// Session override; empty / None → Flowy server default LLM.
    model: Option<String>,
    /// Sticky multimodal model after a successful vision call in this pipeline.
    vision_model: Mutex<Option<String>>,
}

impl FlowyChat {
    pub fn new(services: FlowyVimaxServices, model: Option<String>) -> Self {
        Self {
            services,
            model: nonempty(model),
            vision_model: Mutex::new(None),
        }
    }

    fn model_arg(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Catalog chat models whose `extra.input` includes `image`.
    async fn vision_model_candidates(&self) -> VimaxResult<Vec<String>> {
        let catalog = self
            .services
            .api
            .get_available_models_claw(&self.services.session, None)
            .await
            .map_err(|e| {
                VimaxError::Llm(format!(
                    "failed to load chat model catalog for vision routing: {e}"
                ))
            })?;
        let sticky = self
            .vision_model
            .lock()
            .ok()
            .and_then(|g| g.clone());
        Ok(order_vision_models(
            sticky.as_deref(),
            self.model_arg(),
            &catalog.cloud,
        ))
    }

    /// Prefer OSS HTTPS `publicUrl` for vision; fall back to a small JPEG data-URL.
    ///
    /// Uploading avoids megabyte base64 chat bodies that stall `classify_references`.
    async fn vision_image_url(&self, path: &Path, index: usize) -> VimaxResult<String> {
        let raw = tokio::fs::read(path).await?;
        let thumb = crate::media_local::jpeg_thumb_bytes_for_vision(&raw).unwrap_or(raw);
        let tmp = std::env::temp_dir().join(format!(
            "vimax_vision_{}_{}_{}.jpg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            index
        ));
        tokio::fs::write(&tmp, &thumb).await?;
        let upload = self
            .services
            .upload_image_public_url(&tmp, &format!("vision_{index}"))
            .await;
        let _ = tokio::fs::remove_file(&tmp).await;
        match upload {
            Ok(url) => {
                tracing::debug!(
                    path = %path.display(),
                    url = %url,
                    "vision image uploaded to OSS"
                );
                Ok(url)
            }
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "OSS upload for vision image failed; using JPEG data-URL fallback"
                );
                let b64 = base64::engine::general_purpose::STANDARD.encode(&thumb);
                Ok(format!("data:image/jpeg;base64,{b64}"))
            }
        }
    }
}

#[async_trait]
impl VimaxChat for FlowyChat {
    async fn complete_text(&self, system: &str, user: &str) -> VimaxResult<String> {
        self.services.require_token().await?;
        // Planning prompts are English-templated; re-assert language from the user payload
        // so Chinese ideas don't get translated into English storyboards/scripts.
        let system = format!(
            "{system}\n\n{}",
            crate::planning::language_lock_for_text(user)
        );
        self.services
            .api
            .chat_completions_text(
                &self.services.session,
                &system,
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
        let system = format!(
            "{system}\n\n{}",
            crate::planning::language_lock_for_text(user_text)
        );

        let mut user_parts = vec![json!({"type": "text", "text": user_text})];
        for (i, path) in image_paths.iter().enumerate() {
            let url = self.vision_image_url(path, i).await?;
            user_parts.push(json!({
                "type": "image_url",
                "image_url": { "url": url }
            }));
        }
        let user_parts = json!(user_parts);

        let candidates = self.vision_model_candidates().await?;
        if candidates.is_empty() {
            return Err(VimaxError::Llm(
                "no multimodal chat model available (catalog extra.input must include \"image\") \
                 for vision steps such as people/face checks"
                    .into(),
            ));
        }

        // Cap fan-out: each failed candidate can burn a full LLM HTTP timeout (~120s).
        const MAX_VISION_CANDIDATES: usize = 3;
        let candidates: Vec<String> = candidates.into_iter().take(MAX_VISION_CANDIDATES).collect();

        if let Some(plan) = self.model_arg() {
            if !candidates
                .iter()
                .any(|id| id.eq_ignore_ascii_case(plan))
            {
                tracing::info!(
                    planning_model = %plan,
                    vision_first = %candidates[0],
                    candidates = candidates.len(),
                    "planning model is text-only; routing vision call to multimodal catalog models"
                );
            }
        }

        let mut last_err: Option<VimaxError> = None;
        for model in &candidates {
            match self
                .services
                .api
                .chat_completions_multimodal(
                    &self.services.session,
                    &system,
                    user_parts.clone(),
                    4096,
                    0.3,
                    Some(model.as_str()),
                )
                .await
            {
                Ok(text) => {
                    if let Ok(mut guard) = self.vision_model.lock() {
                        *guard = Some(model.clone());
                    }
                    if candidates.len() > 1 {
                        tracing::info!(%model, "vision chat succeeded");
                    }
                    return Ok(text);
                }
                Err(e) => {
                    tracing::warn!(
                        %model,
                        error = %e,
                        "vision chat failed; trying next multimodal model"
                    );
                    last_err = Some(map_model_err(
                        "llm",
                        Some(model.as_str()),
                        "chat_completions_vision",
                        e,
                    ));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            VimaxError::Llm("all multimodal vision model candidates failed".into())
        }))
    }
}

/// Ordered multimodal model ids for vision calls.
///
/// Preference: sticky success → planning model (if vision-capable) → remaining
/// catalog entries whose `extra.input` includes `image`.
pub(crate) fn order_vision_models(
    sticky: Option<&str>,
    preferred: Option<&str>,
    catalog: &[ClawModelEntry],
) -> Vec<String> {
    let vision: Vec<&ClawModelEntry> = catalog
        .iter()
        .filter(|e| e.model_extra().supports_vision())
        .collect();
    if vision.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let push_match = |candidate: &str, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        if let Some(entry) = vision
            .iter()
            .find(|e| e.matches_model_candidate(candidate))
        {
            let id = entry.api_model_id();
            if seen.insert(id.to_ascii_lowercase()) {
                out.push(id);
            }
        }
    };

    if let Some(s) = sticky.map(str::trim).filter(|s| !s.is_empty()) {
        push_match(s, &mut out, &mut seen);
    }
    if let Some(p) = preferred.map(str::trim).filter(|s| !s.is_empty()) {
        push_match(p, &mut out, &mut seen);
    }
    for entry in &vision {
        let id = entry.api_model_id();
        if seen.insert(id.to_ascii_lowercase()) {
            out.push(id);
        }
    }
    out
}

fn nonempty(model: Option<String>) -> Option<String> {
    model.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

#[cfg(test)]
mod tests {
    use super::order_vision_models;
    use nomifun_cloud::ClawModelEntry;

    fn entry(id: &str, extra: &str) -> ClawModelEntry {
        ClawModelEntry {
            id: id.into(),
            name: id.into(),
            extra: extra.into(),
            endpoint: String::new(),
            anthropic_endpoint: String::new(),
            icon: String::new(),
            category: 1,
            catalog_family: None,
            catalog_auto_tier: None,
        }
    }

    #[test]
    fn skips_text_only_planning_model() {
        let catalog = vec![
            entry(
                "AIPC-deepseek-v4-pro",
                r#"{"input":["text"],"tools":true}"#,
            ),
            entry(
                "AIPC-gpt-4o",
                r#"{"input":["text","image"],"tools":true}"#,
            ),
            entry(
                "AIPC-claude-sonnet",
                r#"{"input":["text","image"]}"#,
            ),
        ];
        let ids = order_vision_models(None, Some("AIPC-deepseek-v4-pro"), &catalog);
        assert_eq!(
            ids,
            vec![
                "AIPC-gpt-4o".to_string(),
                "AIPC-claude-sonnet".to_string(),
            ]
        );
    }

    #[test]
    fn prefers_vision_capable_planning_model() {
        let catalog = vec![
            entry("AIPC-a", r#"{"input":["text","image"]}"#),
            entry("AIPC-b", r#"{"input":["text","image"]}"#),
        ];
        let ids = order_vision_models(None, Some("AIPC-b"), &catalog);
        assert_eq!(ids[0], "AIPC-b");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn sticky_model_wins() {
        let catalog = vec![
            entry("AIPC-a", r#"{"input":["text","image"]}"#),
            entry("AIPC-b", r#"{"input":["text","image"]}"#),
        ];
        let ids = order_vision_models(Some("AIPC-b"), Some("AIPC-a"), &catalog);
        assert_eq!(ids[0], "AIPC-b");
        assert_eq!(ids[1], "AIPC-a");
    }

    #[test]
    fn empty_when_no_vision_models() {
        let catalog = vec![entry(
            "AIPC-deepseek-v4-pro",
            r#"{"input":["text"]}"#,
        )];
        assert!(order_vision_models(None, Some("AIPC-deepseek-v4-pro"), &catalog).is_empty());
    }
}
