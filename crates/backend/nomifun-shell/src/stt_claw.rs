//! Claw cloud ASR transcription (qwen3-asr-flash via category=7 catalog).

use std::path::Path;

use nomi_config::{config_yaml_path, load_user_config_file};
use nomifun_api_types::{SpeechToTextProvider, SpeechToTextResult};
use nomifun_cloud::{FlowyApiClient, ServerClientError, ServerSession};

use crate::error::SttError;

fn map_claw_error(err: ServerClientError) -> SttError {
    match err {
        ServerClientError::AuthRequired(_) | ServerClientError::MissingBaseUrl | ServerClientError::Disabled => {
            SttError::ClawNotConfigured
        }
        ServerClientError::InvalidResponse(msg) if msg.contains("no claw ASR models") => {
            SttError::ClawNotConfigured
        }
        other => SttError::RequestFailed(other.to_string()),
    }
}

/// Transcribe audio via claw ASR when the user has a valid server session.
pub async fn transcribe_via_claw(
    data_dir: &Path,
    audio_data: Vec<u8>,
    file_name: &str,
    mime_type: &str,
    language_hint: Option<&str>,
) -> Result<SpeechToTextResult, SttError> {
    let config = load_user_config_file(&config_yaml_path(Some(data_dir)))
        .map_err(|e| SttError::Unknown(format!("load config: {e}")))?;

    if !config.server.api_ready() {
        return Err(SttError::ClawNotConfigured);
    }

    let api = FlowyApiClient::new(&config.server).map_err(|e| SttError::Unknown(e.to_string()))?;
    let session = ServerSession::from_config(&config.server, data_dir);

    let token = session
        .access_token()
        .await
        .map_err(|e| SttError::Unknown(e.to_string()))?;
    if token.filter(|t| !t.trim().is_empty()).is_none() {
        return Err(SttError::ClawNotConfigured);
    }

    let text = api
        .transcribe_audio(
            &session,
            audio_data,
            file_name,
            mime_type,
            language_hint,
        )
        .await
        .map_err(map_claw_error)?;

    Ok(SpeechToTextResult {
        text,
        model: "qwen3-asr-flash".to_owned(),
        provider: SpeechToTextProvider::Claw,
        language: language_hint.map(str::to_owned),
    })
}
