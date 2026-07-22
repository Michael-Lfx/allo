//! Flowy image generation → local file (with multi-tier safety rewrite).

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

use nomifun_cloud::{
    ImageGenerationRequest, MODEL_CATEGORY_IMAGE, resolve_model_in_catalog,
};

use super::{FlowyVimaxServices, VimaxImage, map_model_err, map_server_err};
use crate::error::{VimaxError, VimaxResult};
use crate::prompt_safety::{
    finalize_llm_rewrite, is_image_content_inspection_err, llm_rewrite_system_message,
    llm_rewrite_user_message, sanitize_image_prompt, sanitize_image_prompt_strict,
    ultra_safe_fallback_prompt,
};

pub struct FlowyImage {
    services: FlowyVimaxServices,
    model_override: Option<String>,
}

impl FlowyImage {
    pub fn new(services: FlowyVimaxServices, model_override: Option<String>) -> Self {
        Self {
            services,
            model_override: model_override.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }),
        }
    }

    async fn resolve_model(&self) -> VimaxResult<String> {
        self.services.require_token().await?;
        let configured = self
            .model_override
            .as_deref()
            .unwrap_or_else(|| self.services.media.image.model.trim());
        let catalog = self
            .services
            .api
            .get_available_models_claw(&self.services.session, Some(MODEL_CATEGORY_IMAGE))
            .await
            .map_err(map_server_err)?;
        if !configured.is_empty() {
            if let Some(id) = resolve_model_in_catalog(configured, &catalog.cloud) {
                return Ok(id);
            }
            if self.model_override.is_some() {
                return Ok(configured.to_string());
            }
        }
        catalog
            .cloud
            .first()
            .map(|m| m.id.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| VimaxError::Image("no Flowy image model in catalog".into()))
    }

    async fn generate_once(
        &self,
        model: &str,
        prompt: &str,
        image_url: Option<String>,
        out_path: &Path,
    ) -> Result<(), nomifun_cloud::ServerClientError> {
        let req = ImageGenerationRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            image_url,
            extra: Value::Null,
        };
        let upstream = self
            .services
            .api
            .generate_image(&self.services.session, &req)
            .await?;
        let url = extract_first_image_url(&upstream).ok_or_else(|| {
            nomifun_cloud::ServerClientError::InvalidResponse("image API returned no URL".into())
        })?;
        download_to_path(&url, out_path)
            .await
            .map_err(|e| nomifun_cloud::ServerClientError::Http(e.to_string()))?;
        Ok(())
    }

    /// LLM rewrite so semantic violence / sensitive framing is removed (not just keywords).
    async fn rewrite_prompt_with_llm(&self, original: &str) -> Option<String> {
        let system = llm_rewrite_system_message();
        let user = llm_rewrite_user_message(original);
        match self
            .services
            .api
            .chat_completions_text(
                &self.services.session,
                system,
                &user,
                1024,
                0.3,
                None,
            )
            .await
        {
            Ok(raw) => {
                let out = finalize_llm_rewrite(&raw, original);
                tracing::info!(
                    original_len = original.chars().count(),
                    rewritten_len = out.chars().count(),
                    "llm image-prompt safety rewrite ok"
                );
                Some(out)
            }
            Err(err) => {
                tracing::warn!(error = %err, "llm image-prompt safety rewrite failed");
                None
            }
        }
    }
}

#[async_trait]
impl VimaxImage for FlowyImage {
    async fn generate(
        &self,
        prompt: &str,
        ref_image_paths: &[&Path],
        out_path: &Path,
    ) -> VimaxResult<()> {
        self.services.require_token().await?;
        let model = self.resolve_model().await?;

        let image_url = if let Some(first) = ref_image_paths.first() {
            Some(path_to_data_url(first).await?)
        } else {
            None
        };

        // Tier 1: lexical soften + positive safety prefix (keep refs).
        let tier1 = sanitize_image_prompt(prompt);
        let err1 = match self
            .generate_once(&model, &tier1, image_url.clone(), out_path)
            .await
        {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        let raw1 = err1.to_string();
        if !is_image_content_inspection_err(&raw1) {
            return Err(map_model_err(
                "image",
                Some(model.as_str()),
                "image_generate",
                err1,
            ));
        }

        // Tier 2: stricter lexical rewrite, drop reference images.
        tracing::warn!(
            model = %model,
            error = %raw1,
            "image content inspection failed; tier2 strict lexical rewrite"
        );
        let tier2 = sanitize_image_prompt_strict(prompt);
        if let Err(err2) = self.generate_once(&model, &tier2, None, out_path).await {
            let raw2 = err2.to_string();
            if !is_image_content_inspection_err(&raw2) {
                return Err(map_model_err(
                    "image",
                    Some(model.as_str()),
                    "image_generate_safe_retry",
                    err2,
                ));
            }

            // Tier 3: LLM semantic rewrite.
            tracing::warn!(
                model = %model,
                error = %raw2,
                "image content inspection failed again; tier3 LLM safety rewrite"
            );
            let tier3 = match self.rewrite_prompt_with_llm(prompt).await {
                Some(p) => p,
                None => sanitize_image_prompt_strict(&tier2),
            };
            if let Err(err3) = self.generate_once(&model, &tier3, None, out_path).await {
                let raw3 = err3.to_string();
                if !is_image_content_inspection_err(&raw3) {
                    return Err(map_model_err(
                        "image",
                        Some(model.as_str()),
                        "image_generate_llm_rewrite",
                        err3,
                    ));
                }

                // Tier 4: ultra-safe fallback (may lose beat fidelity, keeps pipeline moving).
                tracing::warn!(
                    model = %model,
                    error = %raw3,
                    "image content inspection failed after LLM rewrite; tier4 ultra-safe fallback"
                );
                let tier4 = ultra_safe_fallback_prompt(prompt);
                self.generate_once(&model, &tier4, None, out_path)
                    .await
                    .map_err(|err4| {
                        map_model_err(
                            "image",
                            Some(model.as_str()),
                            "image_generate_ultra_safe_fallback",
                            nomifun_cloud::ServerClientError::Api {
                                code: 400,
                                msg: format!(
                                    "content inspection persisted after lexical+LLM+ultra-safe retries. first={raw1}; strict={raw2}; llm={raw3}; final={err4}"
                                ),
                            },
                        )
                    })?;
            }
        }

        Ok(())
    }
}

async fn path_to_data_url(path: &Path) -> VimaxResult<String> {
    let bytes = tokio::fs::read(path).await?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Ok(format!("data:{mime};base64,{b64}"))
}

fn extract_first_image_url(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if s.starts_with("http") || s.starts_with("data:image/") => Some(s.clone()),
        Value::Array(arr) => arr.iter().find_map(extract_first_image_url),
        Value::Object(map) => {
            for key in ["url", "image_url", "image", "b64_json"] {
                if let Some(v) = map.get(key) {
                    if key == "b64_json" {
                        if let Some(b64) = v.as_str() {
                            return Some(format!("data:image/png;base64,{b64}"));
                        }
                    } else if let Some(s) = v.as_str() {
                        if !s.is_empty() {
                            return Some(s.to_string());
                        }
                    }
                }
            }
            map.values().find_map(extract_first_image_url)
        }
        _ => None,
    }
}

async fn download_to_path(url: &str, out_path: &Path) -> VimaxResult<()> {
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Some(b64) = url.strip_prefix("data:image/") {
        let data = b64.split_once(',').map(|(_, d)| d).unwrap_or(b64);
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
            .map_err(|e| VimaxError::Image(format!("bad data URL: {e}")))?;
        tokio::fs::write(out_path, bytes).await?;
        return Ok(());
    }
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| VimaxError::Image(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(VimaxError::Image(format!(
            "download failed: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| VimaxError::Image(e.to_string()))?;
    tokio::fs::write(out_path, &bytes).await?;
    Ok(())
}
