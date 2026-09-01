//! Host TTS adapter: `tools.textToSpeech` preference, then first enabled
//! `speech_synthesis` catalog model. Invokes the same layer as `/api/tts`.

use std::sync::Arc;

use nomi_briefing::{SynthesizedClip, VoiceSynth};
use nomifun_api_types::{TEXT_TO_SPEECH_PREFERENCE_KEY, TextToSpeechConfig};
use nomifun_db::{IClientPreferenceRepository, IProviderModelRepository};
use nomifun_model_invoke::{
    ModelInvokeService, ModelRef, ProducedData, TaskOutcome, TaskRequest, TaskResult, TtsRequest,
};
use tracing::warn;

pub struct InvokeVoiceSynth {
    invoke: Arc<ModelInvokeService>,
    prefs: Arc<dyn IClientPreferenceRepository>,
    models: Arc<dyn IProviderModelRepository>,
}

impl InvokeVoiceSynth {
    pub fn new(
        invoke: Arc<ModelInvokeService>,
        prefs: Arc<dyn IClientPreferenceRepository>,
        models: Arc<dyn IProviderModelRepository>,
    ) -> Self {
        Self {
            invoke,
            prefs,
            models,
        }
    }

    async fn synthesize_async(
        &self,
        text: &str,
        choice: Option<&nomi_briefing::TtsChoice>,
    ) -> Result<SynthesizedClip, String> {
        let (provider_id, model, voice) = if let Some(choice) = choice {
            (
                choice.provider_id.clone(),
                choice.model.clone(),
                choice.voice.clone(),
            )
        } else {
            self.resolve_tts().await?
        };
        let request = TaskRequest::SpeechSynthesis(TtsRequest {
            text: text.to_owned(),
            voice,
            format: Some("wav".to_owned()),
            extra: serde_json::json!({}),
        });
        let outcome = self
            .invoke
            .invoke(&ModelRef { provider_id, model }, request)
            .await
            .map_err(|e| e.to_string())?;
        let TaskOutcome::Done(result) = outcome else {
            return Err("speech synthesis returned an async job unexpectedly".into());
        };
        let TaskResult::Assets(assets) = result else {
            return Err("speech synthesis returned a non-audio result".into());
        };
        let asset = assets
            .into_iter()
            .next()
            .ok_or_else(|| "provider returned no audio asset".to_string())?;
        let ProducedData::Bytes(bytes) = asset.data else {
            return Err("provider returned an audio URL instead of inline bytes".into());
        };
        if bytes.len() < 64 {
            return Err("speech synthesis returned an empty audio clip".into());
        }
        Ok(SynthesizedClip {
            bytes,
            mime: asset.mime.unwrap_or_else(|| "audio/wav".to_owned()),
        })
    }

    async fn resolve_tts(&self) -> Result<(String, String, Option<String>), String> {
        if let Ok(rows) = self.prefs.get_by_keys(&[TEXT_TO_SPEECH_PREFERENCE_KEY]).await {
            if let Some(row) = rows.first() {
                if let Some((provider_id, model, voice)) = parse_tts_pref(&row.value) {
                    return Ok((provider_id, model, voice));
                }
            }
        }
        let rows = self
            .models
            .list()
            .await
            .map_err(|e| format!("catalog: {e}"))?;
        if let Some((provider_id, model)) = first_enabled_tts(&rows) {
            warn!(
                provider_id = %provider_id,
                model = %model,
                "briefing TTS falling back to first enabled speech_synthesis model"
            );
            return Ok((provider_id, model, None));
        }
        Err("tts_unavailable: no speech-synthesis model configured; set tools.textToSpeech in Settings".into())
    }
}

impl VoiceSynth for InvokeVoiceSynth {
    fn synthesize(
        &self,
        text: &str,
        choice: Option<&nomi_briefing::TtsChoice>,
    ) -> Result<SynthesizedClip, String> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "briefing TTS requires a tokio runtime".to_string())?;
        handle.block_on(self.synthesize_async(text, choice))
    }
}

fn parse_tts_pref(raw: &str) -> Option<(String, String, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let prefs = nomifun_api_types::ClientPreferencesResponse::from([(
        TEXT_TO_SPEECH_PREFERENCE_KEY.to_owned(),
        value,
    )]);
    let config = TextToSpeechConfig::from_preferences(&prefs)?;
    if config.provider_id.trim().is_empty() || config.model.trim().is_empty() {
        return None;
    }
    Some((config.provider_id, config.model, config.voice))
}

fn first_enabled_tts(rows: &[nomifun_db::ProviderModelRow]) -> Option<(String, String)> {
    let mut ranked: Vec<_> = rows
        .iter()
        .filter(|row| row.enabled && tasks_include_tts(&row.tasks))
        .collect();
    ranked.sort_by_key(|row| row.sort_order);
    ranked
        .first()
        .map(|row| (row.provider_id.clone(), row.model.clone()))
}

fn tasks_include_tts(tasks_json: &str) -> bool {
    serde_json::from_str::<Vec<String>>(tasks_json)
        .ok()
        .map(|tasks| tasks.iter().any(|task| task == "speech_synthesis"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{parse_tts_pref, tasks_include_tts};

    #[test]
    fn preference_json_round_trips() {
        let provider = "0190f5fe-7c00-7a00-8000-0000000000aa";
        let parsed = parse_tts_pref(&format!(
            r#"{{"provider_id":"{provider}","model":"tts-1","voice":"alloy"}}"#
        ))
        .expect("pref");
        assert_eq!(parsed.0, provider);
        assert_eq!(parsed.1, "tts-1");
        assert_eq!(parsed.2.as_deref(), Some("alloy"));
    }

    #[test]
    fn speech_synthesis_task_is_detected() {
        assert!(tasks_include_tts(r#"["speech_synthesis"]"#));
        assert!(!tasks_include_tts(r#"["chat"]"#));
    }
}
