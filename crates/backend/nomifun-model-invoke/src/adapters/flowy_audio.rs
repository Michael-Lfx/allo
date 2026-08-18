//! Flowy Cloud non-streaming TTS (`category=8`).
//!
//! Wire: `POST {claw}/v1/audio/speech` with JSON `{model, input, voice,
//! response_format, language_type}` (Apifox 「非流式tts」). The response may be
//! raw audio (OpenAI-compatible) or JSON wrapping base64 / a URL. Other
//! providers keep [`super::openai_audio::OpenAiAudioSpeechAdapter`].

use std::time::Duration;

use async_trait::async_trait;
use nomifun_api_types::ModelTask;
use serde_json::{Value, json};

use crate::adapter::ProtocolAdapter;
use crate::call::ResolvedCall;
use crate::error::{InvokeError, InvokeErrorKind};
use crate::transport::{
    MAX_ARTIFACT_BYTES, decode_b64, error_from_response, get_request, post_json, read_body_capped,
};
use crate::types::{ProducedAsset, ProducedData, TaskOutcome, TaskRequest, TaskResult};

const ADAPTER_ID: &str = "flowy.audio_speech";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_VOICE: &str = "Cherry";
const DEFAULT_FORMAT: &str = "wav";
const DEFAULT_LANGUAGE_TYPE: &str = "Chinese";

/// Flowy Cloud claw TTS (`/v1/audio/speech`).
pub struct FlowyAudioSpeechAdapter;

#[async_trait]
impl ProtocolAdapter for FlowyAudioSpeechAdapter {
    fn id(&self) -> &'static str {
        ADAPTER_ID
    }

    fn supports(&self, task: ModelTask) -> bool {
        task == ModelTask::SpeechSynthesis
    }

    async fn submit(&self, http: &reqwest::Client, call: &ResolvedCall) -> Result<TaskOutcome, InvokeError> {
        let TaskRequest::SpeechSynthesis(req) = &call.request else {
            return Err(InvokeError::new(
                InvokeErrorKind::UnsupportedTask,
                format!("{ADAPTER_ID} cannot serve task {:?}", call.request.task()),
            ));
        };

        let voice = req
            .voice
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_VOICE);
        let format = req
            .format
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_FORMAT);
        let language_type = req
            .extra
            .get("language_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                req.extra
                    .get("language")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or(DEFAULT_LANGUAGE_TYPE);

        let body = json!({
            "model": call.model,
            "input": req.text,
            "voice": voice,
            "response_format": format,
            "language_type": language_type,
        });

        let url = call.dispatch_target().url;
        let resp = post_json(http, &url, REQUEST_TIMEOUT, &call.connection.auth, &body).await?;
        if !resp.status().is_success() {
            return Err(error_from_response(resp).await);
        }

        let mime_hint = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase());
        let bytes = read_body_capped(resp, MAX_ARTIFACT_BYTES).await?;
        let (audio, mime) = decode_speech_payload(&bytes, mime_hint.as_deref(), format, http, call).await?;

        Ok(TaskOutcome::Done(TaskResult::Assets(vec![ProducedAsset {
            data: ProducedData::Bytes(audio),
            mime: Some(mime),
        }])))
    }
}

fn mime_for_speech_format(format: &str) -> &'static str {
    match format.trim().to_ascii_lowercase().as_str() {
        "wav" => "audio/wav",
        "opus" => "audio/ogg",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "pcm" | "pcm16" => "audio/pcm",
        _ => "audio/mpeg",
    }
}

async fn decode_speech_payload(
    bytes: &[u8],
    content_type: Option<&str>,
    format: &str,
    http: &reqwest::Client,
    call: &ResolvedCall,
) -> Result<(Vec<u8>, String), InvokeError> {
    let looks_json = content_type.is_some_and(|mime| mime.contains("json"))
        || bytes.first().is_some_and(|b| *b == b'{');
    if !looks_json {
        let mime = content_type
            .filter(|mime| mime.starts_with("audio/"))
            .unwrap_or_else(|| mime_for_speech_format(format))
            .to_owned();
        return Ok((bytes.to_vec(), mime));
    }

    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| InvokeError::parse(format!("invalid Flowy TTS JSON: {error}")))?;
    if let Some(data) = extract_audio_b64(&value) {
        let audio = decode_b64(data).ok_or_else(|| {
            InvokeError::parse("Flowy TTS returned invalid base64 audio".to_owned())
        })?;
        return Ok((audio, mime_for_speech_format(format).to_owned()));
    }
    if let Some(url) = extract_audio_url(&value) {
        let resp = get_request(http, url, REQUEST_TIMEOUT, &call.connection.auth).await?;
        if !resp.status().is_success() {
            return Err(error_from_response(resp).await);
        }
        let audio = read_body_capped(resp, MAX_ARTIFACT_BYTES).await?;
        return Ok((audio, mime_for_speech_format(format).to_owned()));
    }
    Err(InvokeError::parse(
        "Flowy TTS JSON response did not contain audio data".to_owned(),
    ))
}

fn extract_audio_b64(value: &Value) -> Option<&str> {
    const POINTERS: &[&str] = &[
        "/output/audio/data",
        "/audio/data",
        "/data",
        "/choices/0/message/audio/data",
        "/output/choices/0/message/audio/data",
    ];
    POINTERS
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_audio_url(value: &Value) -> Option<&str> {
    const POINTERS: &[&str] = &[
        "/output/audio/url",
        "/audio/url",
        "/url",
        "/choices/0/message/audio/url",
        "/output/choices/0/message/audio/url",
    ];
    POINTERS
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::adapters::test_support::call;
    use crate::transport::encode_b64;
    use crate::types::TtsRequest;

    fn tts(text: &str, voice: Option<&str>, format: Option<&str>, extra: Value) -> TaskRequest {
        TaskRequest::SpeechSynthesis(TtsRequest {
            text: text.into(),
            voice: voice.map(str::to_string),
            format: format.map(str::to_string),
            extra,
        })
    }

    #[tokio::test]
    async fn posts_claw_json_with_default_voice_and_returns_binary_audio() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/speech"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(json!({
                "model": "AIPC-qwen3-tts",
                "input": "今天天气真不错",
                "voice": "Cherry",
                "response_format": "wav",
                "language_type": "Chinese",
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/wav")
                    .set_body_bytes(b"RIFFwav".to_vec()),
            )
            .expect(1)
            .mount(&server)
            .await;

        let call = call(
            &format!("{}/v1", server.uri()),
            "AIPC-qwen3-tts",
            tts("今天天气真不错", None, None, json!({})),
        );
        let out = FlowyAudioSpeechAdapter
            .submit(&reqwest::Client::new(), &call)
            .await
            .unwrap();
        let TaskOutcome::Done(TaskResult::Assets(assets)) = out else {
            panic!("expected Done(Assets)")
        };
        assert!(matches!(&assets[0].data, ProducedData::Bytes(b) if b == b"RIFFwav"));
        assert_eq!(assets[0].mime.as_deref(), Some("audio/wav"));
    }

    #[tokio::test]
    async fn posts_requested_voice_format_and_language() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/speech"))
            .and(body_partial_json(json!({
                "model": "AIPC-qwen3-tts",
                "input": "hello",
                "voice": "Serena",
                "response_format": "pcm",
                "language_type": "English",
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/pcm")
                    .set_body_bytes(b"pcm".to_vec()),
            )
            .expect(1)
            .mount(&server)
            .await;

        let call = call(
            &format!("{}/v1", server.uri()),
            "AIPC-qwen3-tts",
            tts("hello", Some("Serena"), Some("pcm"), json!({"language_type": "English"})),
        );
        FlowyAudioSpeechAdapter
            .submit(&reqwest::Client::new(), &call)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn decodes_json_base64_audio() {
        let server = MockServer::start().await;
        let audio = encode_b64(b"RIFFwav");
        Mock::given(method("POST"))
            .and(path("/v1/audio/speech"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "output": {"audio": {"data": audio}},
                "request_id": "tts-1",
            })))
            .mount(&server)
            .await;

        let call = call(
            &format!("{}/v1", server.uri()),
            "AIPC-qwen3-tts",
            tts("hi", None, Some("wav"), json!({})),
        );
        let out = FlowyAudioSpeechAdapter
            .submit(&reqwest::Client::new(), &call)
            .await
            .unwrap();
        let TaskOutcome::Done(TaskResult::Assets(assets)) = out else {
            panic!("expected Done(Assets)")
        };
        assert!(matches!(&assets[0].data, ProducedData::Bytes(b) if b == b"RIFFwav"));
        assert_eq!(assets[0].mime.as_deref(), Some("audio/wav"));
    }

    #[tokio::test]
    async fn json_without_audio_is_a_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/speech"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "output": {"choices": [{"message": {"content": [{"text": "no audio"}]}}]},
                "request_id": "tts-1",
            })))
            .mount(&server)
            .await;

        let call = call(
            &format!("{}/v1", server.uri()),
            "AIPC-qwen3-tts",
            tts("hi", None, None, json!({})),
        );
        let err = FlowyAudioSpeechAdapter
            .submit(&reqwest::Client::new(), &call)
            .await
            .unwrap_err();
        assert_eq!(err.kind, InvokeErrorKind::ParseError);
    }
}
