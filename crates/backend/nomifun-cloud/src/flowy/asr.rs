//! Claw ASR (speech-to-text) via `qwen3-asr-flash` and related models (`category=7`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use crate::error::ServerClientError;
use crate::flowy::media_types::MODEL_CATEGORY_ASR;
use crate::flowy::types::ClawModelEntry;
use crate::session::ServerSession;

use super::FlowyApiClient;

/// Reuse ASR catalog responses for this long to avoid listing models on every utterance.
const ASR_CATALOG_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
struct AsrCatalogCacheEntry {
    models: Vec<ClawModelEntry>,
    fetched_at: Instant,
}

static ASR_CATALOG_CACHE: LazyLock<Mutex<HashMap<String, AsrCatalogCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn asr_catalog_cache_key(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_ascii_lowercase()
}

#[cfg(test)]
fn clear_asr_catalog_cache_for_tests() {
    ASR_CATALOG_CACHE.lock().expect("asr catalog cache lock").clear();
}

/// Extract transcript text from an upstream ASR JSON body.
///
/// Matches FlowyClaw `openclaw-token-cloud-media` (`payload.text` or
/// `payload.output.choices[0].message.content[].text`) and OpenAI-style bodies.
pub fn extract_asr_text(body: &Value) -> Option<String> {
    if let Some(text) = body.get("text").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    for content_pointer in [
        "/output/choices/0/message/content",
        "/choices/0/message/content",
    ] {
        if let Some(text) = extract_text_from_message_content(body.pointer(content_pointer)) {
            return Some(text);
        }
    }

    None
}

fn extract_text_from_message_content(content: Option<&Value>) -> Option<String> {
    let content = content?;
    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

fn audio_format_from_mime(mime_type: &str) -> &'static str {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime.contains("wav") {
        return "wav";
    }
    if mime.contains("mp4") || mime.contains("m4a") {
        return "m4a";
    }
    if mime.contains("mpeg") || mime.contains("mp3") {
        return "mp3";
    }
    if mime.contains("ogg") {
        return "ogg";
    }
    "webm"
}

fn language_from_hint(language_hint: Option<&str>) -> Option<String> {
    language_hint.map(str::trim).filter(|s| !s.is_empty()).map(|hint| {
        hint.split(&['-', '_'][..])
            .next()
            .unwrap_or(hint)
            .to_ascii_lowercase()
    })
}

fn resolve_asr_model_id(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return "qwen3-asr-flash".to_owned();
    }
    trimmed
        .rsplit_once('/')
        .map(|(_, name)| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| trimmed.to_owned())
}

fn empty_asr_transcript_error() -> ServerClientError {
    ServerClientError::InvalidResponse("ASR returned empty transcript".into())
}

fn classify_transcription_result(
    result: Result<String, ServerClientError>,
) -> Result<Option<String>, ServerClientError> {
    match result {
        Ok(text) if !text.trim().is_empty() => Ok(Some(text)),
        Ok(_) => Ok(None),
        Err(err) => Err(err),
    }
}

impl FlowyApiClient {
    /// List ASR models from `GET /model/availableListClaw?category=7`.
    ///
    /// Results are cached in-process by server `base_url` for [`ASR_CATALOG_CACHE_TTL`].
    pub async fn fetch_asr_models(
        &self,
        session: &ServerSession,
    ) -> Result<Vec<ClawModelEntry>, ServerClientError> {
        let cache_key = asr_catalog_cache_key(&self.config.base_url);

        if let Some(models) = {
            let cache = ASR_CATALOG_CACHE
                .lock()
                .map_err(|_| ServerClientError::Http("ASR catalog cache lock poisoned".into()))?;
            cache.get(&cache_key).and_then(|entry| {
                if entry.fetched_at.elapsed() < ASR_CATALOG_CACHE_TTL {
                    Some(entry.models.clone())
                } else {
                    None
                }
            })
        } {
            return Ok(models);
        }

        let models = self
            .get_available_models_claw(session, Some(MODEL_CATEGORY_ASR))
            .await?
            .cloud;

        {
            let mut cache = ASR_CATALOG_CACHE
                .lock()
                .map_err(|_| ServerClientError::Http("ASR catalog cache lock poisoned".into()))?;
            cache.insert(
                cache_key,
                AsrCatalogCacheEntry {
                    models: models.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        Ok(models)
    }

    /// Transcribe audio bytes via the first available claw ASR model.
    pub async fn transcribe_audio(
        &self,
        session: &ServerSession,
        audio_data: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        language_hint: Option<&str>,
    ) -> Result<String, ServerClientError> {
        let catalog = self.fetch_asr_models(session).await?;
        let entry = catalog
            .first()
            .ok_or_else(|| ServerClientError::InvalidResponse("no claw ASR models available".into()))?;

        self.transcribe_audio_with_entry(session, entry, audio_data, file_name, mime_type, language_hint)
            .await
    }

    async fn transcribe_audio_with_entry(
        &self,
        session: &ServerSession,
        entry: &ClawModelEntry,
        audio_data: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        language_hint: Option<&str>,
    ) -> Result<String, ServerClientError> {
        // LLM root is `{business}/v1`; path is `/audio/transcriptions` (not `/v1/...` again).
        let model_candidates = [
            resolve_asr_model_id(&entry.tb_model_name()),
            resolve_asr_model_id(&entry.api_model_id()),
        ];
        let mut last_error =
            ServerClientError::InvalidResponse("ASR transcription failed".into());

        for model in model_candidates {
            match self
                .transcribe_audio_with_model(
                    session,
                    &audio_data,
                    file_name,
                    mime_type,
                    language_hint,
                    &model,
                )
                .await
            {
                Ok(text) => return Ok(text),
                Err(err) => last_error = err,
            }
        }

        Err(last_error)
    }

    async fn transcribe_audio_with_model(
        &self,
        session: &ServerSession,
        audio_data: &[u8],
        file_name: &str,
        mime_type: &str,
        language_hint: Option<&str>,
        model: &str,
    ) -> Result<String, ServerClientError> {
        match classify_transcription_result(
            self.post_audio_transcription_json(session, audio_data, mime_type, language_hint, model)
                .await,
        ) {
            Ok(Some(text)) => return Ok(text),
            Ok(None) => {}
            Err(json_err) => {
                return match classify_transcription_result(
                    self.post_audio_transcription_multipart(
                        session,
                        audio_data.to_vec(),
                        file_name,
                        mime_type,
                        language_hint,
                        model,
                    )
                    .await,
                ) {
                    Ok(Some(text)) => Ok(text),
                    Ok(None) => Err(json_err),
                    Err(err) => Err(err),
                };
            }
        }

        match classify_transcription_result(
            self.post_audio_transcription_multipart(
                session,
                audio_data.to_vec(),
                file_name,
                mime_type,
                language_hint,
                model,
            )
            .await,
        ) {
            Ok(Some(text)) => Ok(text),
            Ok(None) => Err(empty_asr_transcript_error()),
            Err(err) => Err(err),
        }
    }

    async fn post_audio_transcription_json(
        &self,
        session: &ServerSession,
        audio_data: &[u8],
        mime_type: &str,
        language_hint: Option<&str>,
        model: &str,
    ) -> Result<String, ServerClientError> {
        let mut body = json!({
            "model": model,
            "input_audio": {
                "data": BASE64.encode(audio_data),
                "format": audio_format_from_mime(mime_type),
            },
        });
        if let Some(language) = language_from_hint(language_hint) {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("language".to_owned(), Value::String(language));
            }
        }

        self.parse_audio_transcription_response(
            self.llm_transport
                .post_json("/audio/transcriptions", Some(session), body)
                .await?,
        )
        .await
    }

    async fn post_audio_transcription_multipart(
        &self,
        session: &ServerSession,
        audio_data: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        language_hint: Option<&str>,
        model: &str,
    ) -> Result<String, ServerClientError> {
        let file_part = reqwest::multipart::Part::bytes(audio_data)
            .file_name(file_name.to_owned())
            .mime_str(mime_type)
            .map_err(|e| ServerClientError::Http(format!("invalid MIME type: {e}")))?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", model.to_owned());

        if let Some(language) = language_from_hint(language_hint) {
            form = form.text("language", language);
        }

        self.parse_audio_transcription_response(
            self.llm_transport
                .post_multipart("/audio/transcriptions", Some(session), form)
                .await?,
        )
        .await
    }

    async fn parse_audio_transcription_response(
        &self,
        resp: reqwest::Response,
    ) -> Result<String, ServerClientError> {
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| ServerClientError::Http(e.to_string()))?;

        if status == 200 {
            let body: Value = serde_json::from_str(&text).map_err(|e| {
                ServerClientError::InvalidResponse(format!("upstream ASR JSON: {e}"))
            })?;
            return extract_asr_text(&body).ok_or_else(|| {
                ServerClientError::InvalidResponse("ASR returned empty transcript".into())
            });
        }

        if let Ok(env) = super::response::FlowyEnvelope::parse_body(&text) {
            return Err(ServerClientError::Api {
                code: env.code,
                msg: env.msg,
            });
        }

        Err(ServerClientError::Http(format!("HTTP {status}: {text}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: &str) -> nomi_config::ServerConfig {
        nomi_config::ServerConfig {
            base_url: base_url.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn transcribe_audio_hits_llm_audio_transcriptions_and_parses_output() {
        clear_asr_catalog_cache_for_tests();
        let mock = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/model/availableListClaw"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-qwen3-asr-flash","name":"Qwen3 ASR Flash"}]}}"#,
            ))
            .mount(&mock)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"output":{"choices":[{"message":{"content":[{"text":"你好世界"}]}}]}}"#,
            ))
            .mount(&mock)
            .await;

        let config = test_config(&mock.uri());
        let api = FlowyApiClient::new(&config).expect("client");
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test-asr") };
        let session = ServerSession::from_config(&config, tmp.path());

        let text = api
            .transcribe_audio(
                &session,
                vec![1, 2, 3, 4],
                "speech-input.webm",
                "audio/webm",
                Some("zh-CN"),
            )
            .await
            .expect("transcribe");

        assert_eq!(text, "你好世界");
    }

    #[tokio::test]
    async fn transcribe_audio_reuses_cached_asr_catalog() {
        clear_asr_catalog_cache_for_tests();
        let mock = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/model/availableListClaw"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-qwen3-asr-flash","name":"Qwen3 ASR Flash"}]}}"#,
            ))
            .expect(1)
            .mount(&mock)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"output":{"choices":[{"message":{"content":[{"text":"你好世界"}]}}]}}"#,
            ))
            .expect(2)
            .mount(&mock)
            .await;

        let config = test_config(&mock.uri());
        let api = FlowyApiClient::new(&config).expect("client");
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test-asr") };
        let session = ServerSession::from_config(&config, tmp.path());

        for _ in 0..2 {
            let text = api
                .transcribe_audio(
                    &session,
                    vec![1, 2, 3, 4],
                    "speech-input.webm",
                    "audio/webm",
                    Some("zh-CN"),
                )
                .await
                .expect("transcribe");
            assert_eq!(text, "你好世界");
        }
    }

    #[test]
    fn parse_asr_transcript_extracts_message_content() {
        let body = serde_json::json!({
            "choices": [{
                "message": { "content": "你好世界" }
            }]
        });
        assert_eq!(extract_asr_text(&body).as_deref(), Some("你好世界"));
    }

    #[test]
    fn parse_asr_transcript_extracts_output_choices_content_array() {
        let body = serde_json::json!({
            "output": {
                "choices": [{
                    "message": {
                        "content": [{ "text": "你好" }, { "text": "世界" }]
                    }
                }]
            }
        });
        assert_eq!(extract_asr_text(&body).as_deref(), Some("你好\n世界"));
    }

    #[test]
    fn parse_asr_transcript_falls_back_to_text_field() {
        let body = serde_json::json!({ "text": "hello" });
        assert_eq!(extract_asr_text(&body).as_deref(), Some("hello"));
    }

    #[test]
    fn audio_format_from_mime_maps_common_types() {
        assert_eq!(audio_format_from_mime("audio/webm;codecs=opus"), "webm");
        assert_eq!(audio_format_from_mime("audio/wav"), "wav");
        assert_eq!(audio_format_from_mime("audio/mp4"), "m4a");
    }

    #[test]
    fn resolve_asr_model_id_strips_provider_prefix() {
        assert_eq!(resolve_asr_model_id("token-cloud/qwen3-asr-flash"), "qwen3-asr-flash");
        assert_eq!(resolve_asr_model_id("qwen3-asr-flash"), "qwen3-asr-flash");
    }

    #[test]
    fn language_from_hint_normalizes_locale() {
        assert_eq!(language_from_hint(Some("zh-CN")).as_deref(), Some("zh"));
        assert_eq!(language_from_hint(Some("en-US")).as_deref(), Some("en"));
    }
}
