//! Flowy server image generation backend.

use async_trait::async_trait;
use serde_json::Value;

use nomi_types::ToolError;
use nomifun_cloud::flowy::ImageGenerationRequest;
use nomi_tools::ImageGenBackend;
use nomi_tools::tools::image_gen::ImageGenRequest;

use super::{FlowyMediaServices, map_server_err};
use crate::assets::{
    extract_image_urls, persist_data_url, persist_from_url, remote_fallback_artifact,
};
use crate::delivery::{MediaProvenance, image_generation_response};
use crate::progress::{
    MediaProgressHeartbeat, image_credits_check, image_persisting, image_resolving_model,
    image_submitting, image_waiting_upstream, report_media_progress,
};

pub struct FlowyImageGenBackend {
    services: FlowyMediaServices,
}

impl FlowyImageGenBackend {
    pub fn new(services: FlowyMediaServices) -> Self {
        Self { services }
    }

    pub async fn is_configured(services: &FlowyMediaServices) -> bool {
        services.is_authenticated().await
    }
}

#[async_trait]
impl ImageGenBackend for FlowyImageGenBackend {
    async fn generate(&self, request: ImageGenRequest) -> Result<String, ToolError> {
        self.services.require_token().await?;
        report_media_progress(image_credits_check());
        self.services.ensure_image_credits().await?;

        report_media_progress(image_resolving_model());
        let model = self
            .services
            .resolve_image_model(request.model.as_deref())
            .await?;

        let flowy_req = ImageGenerationRequest {
            model: model.clone(),
            prompt: request.prompt.clone(),
            image_url: request.image_url.clone(),
            extra: request.extra.unwrap_or(Value::Null),
        };

        report_media_progress(image_submitting());
        let heartbeat = MediaProgressHeartbeat::start(5, image_waiting_upstream);
        let upstream = self
            .services
            .api
            .generate_image(&self.services.session, &flowy_req)
            .await
            .map_err(map_server_err);
        heartbeat.stop();
        let upstream = upstream?;

        report_media_progress(image_persisting());

        let mut artifacts = Vec::new();
        let urls = extract_image_urls(&upstream);
        if urls.is_empty() {
            if let Some(data_url) = find_data_url(&upstream) {
                let artifact = persist_data_url(&data_url, "flowy", &model).await?;
                artifacts.push(artifact);
            }
        } else {
            for url in urls {
                if self.services.media.image.save_locally {
                    match persist_from_url(&url, "flowy", &model).await {
                        Ok(artifact) => artifacts.push(artifact),
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                url = %url,
                                "image generated but local persist failed; keeping remote URL"
                            );
                            artifacts.push(remote_fallback_artifact(
                                url,
                                "image/png",
                                "flowy",
                                &model,
                            ));
                        }
                    }
                } else {
                    artifacts.push(remote_fallback_artifact(url, "image/png", "flowy", &model));
                }
            }
        }

        if artifacts.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "image API returned no downloadable URLs".into(),
            ));
        }

        Ok(image_generation_response(
            &model,
            &artifacts,
            &upstream,
            MediaProvenance::for_api_call(request.prompt, None, None, None),
        ))
    }
}

fn find_data_url(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if s.starts_with("data:image/") => Some(s.clone()),
        Value::Array(arr) => arr.iter().find_map(find_data_url),
        Value::Object(map) => map.values().find_map(find_data_url),
        _ => None,
    }
}
