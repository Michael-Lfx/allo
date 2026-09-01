//! Host image-generation adapter for briefing atmosphere plates.

use std::sync::Arc;

use nomi_briefing::{GeneratedStill, ImageChoice, StillSynth};
use nomifun_model_invoke::{
    ImageGenRequest, ModelInvokeService, ModelRef, ProducedData, TaskOutcome, TaskRequest,
    TaskResult,
};

pub struct InvokeStillSynth {
    invoke: Arc<ModelInvokeService>,
}

impl InvokeStillSynth {
    pub fn new(invoke: Arc<ModelInvokeService>) -> Self {
        Self { invoke }
    }

    async fn generate_async(
        &self,
        prompt: &str,
        choice: &ImageChoice,
    ) -> Result<GeneratedStill, String> {
        let request = TaskRequest::ImageGeneration(ImageGenRequest {
            prompt: prompt.to_owned(),
            count: 1,
            size: Some("1920x1080".into()),
            quality: None,
            extra: serde_json::json!({}),
        });
        let outcome = self
            .invoke
            .invoke(
                &ModelRef {
                    provider_id: choice.provider_id.clone(),
                    model: choice.model.clone(),
                },
                request,
            )
            .await
            .map_err(|e| e.to_string())?;
        let TaskOutcome::Done(result) = outcome else {
            return Err("image generation returned an async job unexpectedly".into());
        };
        let TaskResult::Assets(assets) = result else {
            return Err("image generation returned a non-image result".into());
        };
        let asset = assets
            .into_iter()
            .next()
            .ok_or_else(|| "provider returned no image asset".to_string())?;
        let ProducedData::Bytes(bytes) = asset.data else {
            return Err("provider returned an image URL instead of inline bytes".into());
        };
        if bytes.len() < 64 {
            return Err("image generation returned an empty still".into());
        }
        Ok(GeneratedStill {
            bytes,
            mime: asset.mime.unwrap_or_else(|| "image/png".to_owned()),
        })
    }
}

impl StillSynth for InvokeStillSynth {
    fn generate_still(&self, prompt: &str, choice: &ImageChoice) -> Result<GeneratedStill, String> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "briefing stills require a tokio runtime".to_string())?;
        handle.block_on(self.generate_async(prompt, choice))
    }
}
