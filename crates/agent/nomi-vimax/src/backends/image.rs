//! Flowy image generation → local file (with multi-tier safety rewrite).

use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

use nomifun_cloud::{
    ImageGenerationRequest, MODEL_CATEGORY_IMAGE, resolve_model_in_catalog,
};

use super::{FlowyVimaxServices, ImageGenerateOpts, VimaxImage, map_model_err, map_server_err};
use crate::error::{VimaxError, VimaxResult};
use crate::prompt_safety::{
    finalize_llm_rewrite, is_image_content_inspection_err, llm_rewrite_system_message,
    llm_rewrite_user_message, sanitize_image_prompt, sanitize_image_prompt_strict,
    ultra_safe_fallback_prompt,
};

/// Prefer true multi-ref URL arrays for Seedream-class models; fall back to a
/// composed strip only when OSS upload fails or the model cannot take multi-image.
const MAX_MULTI_REF_IMAGES: usize = 8;

pub struct FlowyImage {
    services: FlowyVimaxServices,
    model_override: Option<String>,
    /// Session / plan aspect ratio for cover & sized image outputs.
    aspect_ratio: Option<String>,
}

impl FlowyImage {
    pub fn new(
        services: FlowyVimaxServices,
        model_override: Option<String>,
        aspect_ratio: Option<String>,
    ) -> Self {
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
            aspect_ratio: aspect_ratio.and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(crate::aspect::normalize_aspect_ratio(&t))
                }
            }),
        }
    }

    fn resolved_aspect(&self) -> String {
        self.aspect_ratio
            .clone()
            .unwrap_or_else(|| {
                crate::aspect::normalize_aspect_ratio(
                    &self.services.media.video.default_aspect_ratio,
                )
            })
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
        image_urls: &[String],
        out_path: &Path,
        opts: &ImageGenerateOpts,
    ) -> Result<(), nomifun_cloud::ServerClientError> {
        // Only poster/cover paths set `aspect_ratio`. Portraits / world plates must
        // keep the default Seedream `2K` canvas — forcing video aspect (e.g. 1280x720)
        // fails Seedream 5.0's ≥3.6M pixel floor and warps three-view sheets.
        let mut extra = if self.aspect_ratio.is_some() {
            crate::aspect::image_request_extra_for_aspect(&self.resolved_aspect())
        } else {
            Value::Null
        };
        extra = merge_image_edit_opts(extra, opts);
        let req = ImageGenerationRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            image_url: None,
            image_urls: image_urls.to_vec(),
            extra,
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

    /// Resolve local refs → HTTPS public URLs (preferred) or a single data-URL strip fallback.
    async fn resolve_reference_urls(
        &self,
        ref_image_paths: &[&Path],
        out_path: &Path,
    ) -> VimaxResult<Vec<String>> {
        if ref_image_paths.is_empty() {
            return Ok(Vec::new());
        }

        let mut urls = Vec::new();
        for (i, path) in ref_image_paths
            .iter()
            .take(MAX_MULTI_REF_IMAGES)
            .enumerate()
        {
            match self
                .services
                .upload_image_public_url(path, &format!("img_ref_{i}"))
                .await
            {
                Ok(url) => urls.push(url),
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "OSS upload for image ref failed; will try strip/data-url fallback"
                    );
                    urls.clear();
                    break;
                }
            }
        }
        if !urls.is_empty() {
            return Ok(urls);
        }

        // Fallback: single-slot APIs / OSS outage — compose a strip, send as data URL.
        if ref_image_paths.len() == 1 {
            return Ok(vec![path_to_data_url(ref_image_paths[0]).await?]);
        }
        let composed_path = out_path.with_extension("ref_strip.png");
        let paths_for_blocking: Vec<PathBuf> =
            ref_image_paths.iter().map(|p| (*p).to_path_buf()).collect();
        let dest = composed_path.clone();
        tokio::task::spawn_blocking(move || {
            let refs: Vec<&Path> = paths_for_blocking.iter().map(|p| p.as_path()).collect();
            crate::media_local::compose_reference_strip(&refs, &dest)
        })
        .await
        .map_err(|e| VimaxError::Image(format!("compose ref strip join: {e}")))??;
        Ok(vec![path_to_data_url(&composed_path).await?])
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
        self.generate_with_opts(prompt, ref_image_paths, out_path, ImageGenerateOpts::default())
            .await
    }

    async fn generate_with_opts(
        &self,
        prompt: &str,
        ref_image_paths: &[&Path],
        out_path: &Path,
        opts: ImageGenerateOpts,
    ) -> VimaxResult<()> {
        self.services.require_token().await?;
        let model = self.resolve_model().await?;
        let image_urls = self
            .resolve_reference_urls(ref_image_paths, out_path)
            .await?;

        // Tier 1: lexical soften + positive safety prefix (keep refs).
        let tier1 = sanitize_image_prompt(prompt);
        let err1 = match self
            .generate_once(&model, &tier1, &image_urls, out_path, &opts)
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

        // Tier 2+: rewrite prompt text but KEEP reference images when present.
        // Dropping refs on portrait side/back / three-view expand causes identity drift
        // (three different people stitched into one sheet).
        tracing::warn!(
            model = %model,
            error = %raw1,
            keep_refs = !image_urls.is_empty(),
            ref_count = image_urls.len(),
            "image content inspection failed; tier2 strict lexical rewrite"
        );
        let tier2 = sanitize_image_prompt_strict(prompt);
        if let Err(err2) = self
            .generate_once(&model, &tier2, &image_urls, out_path, &opts)
            .await
        {
            let raw2 = err2.to_string();
            if !is_image_content_inspection_err(&raw2) {
                return Err(map_model_err(
                    "image",
                    Some(model.as_str()),
                    "image_generate_safe_retry",
                    err2,
                ));
            }

            // Tier 3: LLM semantic rewrite (still keep refs).
            tracing::warn!(
                model = %model,
                error = %raw2,
                "image content inspection failed again; tier3 LLM safety rewrite"
            );
            let tier3 = match self.rewrite_prompt_with_llm(prompt).await {
                Some(p) => p,
                None => sanitize_image_prompt_strict(&tier2),
            };
            if let Err(err3) = self
                .generate_once(&model, &tier3, &image_urls, out_path, &opts)
                .await
            {
                let raw3 = err3.to_string();
                if !is_image_content_inspection_err(&raw3) {
                    return Err(map_model_err(
                        "image",
                        Some(model.as_str()),
                        "image_generate_llm_rewrite",
                        err3,
                    ));
                }

                // Tier 4: ultra-safe fallback — keep refs if we have them for identity.
                tracing::warn!(
                    model = %model,
                    error = %raw3,
                    "image content inspection failed after LLM rewrite; tier4 ultra-safe fallback"
                );
                let tier4 = ultra_safe_fallback_prompt(prompt);
                self.generate_once(&model, &tier4, &image_urls, out_path, &opts)
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

/// Merge mild img2img controls into the Flowy image `extra` object.
fn merge_image_edit_opts(base: Value, opts: &ImageGenerateOpts) -> Value {
    let mut map = match base {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    if let Some(neg) = opts
        .negative_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        map.insert("negative_prompt".into(), serde_json::json!(neg));
    }
    if let Some(strength) = opts.denoising_strength {
        let s = strength.clamp(0.0, 1.0);
        // Seedream / Flowy img2img only honors `denoising_strength` (not `strength`).
        map.insert("denoising_strength".into(), serde_json::json!(s));
    }
    Value::Object(map)
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

fn extract_first_image_url(v: &Value) -> Option<String> {
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(u) = item.get("url").and_then(|x| x.as_str()) {
                if !u.trim().is_empty() {
                    return Some(u.to_string());
                }
            }
            if let Some(u) = item.get("b64_json").and_then(|x| x.as_str()) {
                if !u.trim().is_empty() {
                    return Some(format!("data:image/png;base64,{u}"));
                }
            }
        }
    }
    if let Some(u) = v.pointer("/output/results/0/url").and_then(|x| x.as_str()) {
        return Some(u.to_string());
    }
    if let Some(u) = v.pointer("/output/choices/0/message/content/0/image").and_then(|x| x.as_str())
    {
        return Some(u.to_string());
    }
    None
}

async fn download_to_path(url: &str, out_path: &Path) -> Result<(), String> {
    if let Some(rest) = url.strip_prefix("data:") {
        let b64 = rest.split(',').nth(1).ok_or("invalid data url")?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .map_err(|e| e.to_string())?;
        if let Some(parent) = out_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        tokio::fs::write(out_path, bytes)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::write(out_path, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
