//! TTS Provider abstraction and Edge-TTS implementation.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.8.

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum TtsError {
    #[error("Network connection error: {0}")]
    Network(String),
    #[error("Audio decode error: {0}")]
    Decode(String),
    #[error("TTS synthesis failed: {0}")]
    Synthesis(String),
    #[error("TTS rate limited (HTTP {status}): retry after {retry_after:?}")]
    RateLimited {
        status: u16,
        retry_after: Option<std::time::Duration>,
    },
    #[error("TTS request cancelled")]
    Cancelled,
}

pub fn parse_retry_after(
    headers: &tokio_tungstenite::tungstenite::http::HeaderMap,
) -> Option<std::time::Duration> {
    let header_val = headers.get("retry-after")?.to_str().ok()?;
    let trimmed = header_val.trim();
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(std::time::Duration::from_secs(seconds.min(3600)));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        let now = chrono::Utc::now();
        let diff = dt.signed_duration_since(now);
        if diff.num_milliseconds() > 0 {
            let secs = (diff.num_milliseconds() as u64) / 1000;
            return Some(std::time::Duration::from_secs(secs.min(3600)));
        } else {
            return Some(std::time::Duration::ZERO);
        }
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(trimmed, "%a, %d %b %Y %H:%M:%S GMT") {
        let dt_utc = dt.and_utc();
        let now = chrono::Utc::now();
        let diff = dt_utc.signed_duration_since(now);
        if diff.num_milliseconds() > 0 {
            let secs = (diff.num_milliseconds() as u64) / 1000;
            return Some(std::time::Duration::from_secs(secs.min(3600)));
        } else {
            return Some(std::time::Duration::ZERO);
        }
    }
    None
}

/// Builds the Edge Read Aloud handshake URL. The upstream endpoint expects the
/// public DRM `TrustedClientToken`; the old subscription-key parameter is only
/// accepted by the retired `msedgeservices` route.
fn edge_request_url(endpoint: &str, request_id: &str, unix_seconds: u64) -> String {
    // A bare `ws://host:port` has no path; HTTP request targets must begin with
    // `/` even when only a query follows.
    let endpoint = if endpoint
        .split_once("://")
        .is_some_and(|(_, authority_and_path)| !authority_and_path.contains('/'))
    {
        format!("{endpoint}/")
    } else {
        endpoint.to_string()
    };
    let separator = if endpoint.contains('?') { '&' } else { '?' };
    format!(
        "{endpoint}{separator}ConnectionId={request_id}&TrustedClientToken={}&Sec-MS-GEC={}&Sec-MS-GEC-Version=2-{}",
        crate::edge_protocol::CLIENT_TOKEN,
        crate::edge_protocol::gec(unix_seconds),
        crate::edge_protocol::CLIENT_VERSION
    )
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    fn diagnostic(&self, _voice: &str) -> Option<nomifun_api_types::voice::VoiceTtsDiagnostic> { None }
    /// Reports whether the provider has the required resources to attempt synthesis.
    fn is_available(&self) -> bool { true }
    /// Stream audio chunk synthesis. Returns an async Receiver for byte buffers.
    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError>;
}

#[derive(Default)]
struct EdgeTtsState {
    cooldown_until: Option<tokio::time::Instant>,
}

/// Microsoft Edge-TTS streaming provider via WebSocket.
/// Free of API keys, low-latency neural voices.
#[derive(Clone)]
pub struct EdgeTtsProvider {
    endpoint: String,
    default_voice: String,
    output_format: String,
    state: std::sync::Arc<tokio::sync::Mutex<EdgeTtsState>>,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
}

impl EdgeTtsProvider {
    pub const DEFAULT_ENDPOINT: &'static str =
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1";
    pub const DEFAULT_VOICE: &'static str = "zh-CN-XiaoxiaoNeural";
    pub const DEFAULT_FORMAT: &'static str = "audio-24khz-48kbitrate-mono-mp3";

    pub fn new() -> Self {
        Self {
            endpoint: Self::DEFAULT_ENDPOINT.to_string(),
            default_voice: Self::DEFAULT_VOICE.to_string(),
            output_format: Self::DEFAULT_FORMAT.to_string(),
            state: std::sync::Arc::new(tokio::sync::Mutex::new(EdgeTtsState::default())),
            semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.default_voice = voice.into();
        self
    }

    pub fn with_output_format(mut self, format: impl Into<String>) -> Self {
        self.output_format = format.into();
        self
    }
}

impl Default for EdgeTtsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsProvider for EdgeTtsProvider {
    fn diagnostic(&self, voice: &str) -> Option<nomifun_api_types::voice::VoiceTtsDiagnostic> {
        Some(nomifun_api_types::voice::VoiceTtsDiagnostic {
            provider: "edge".into(), voice: Some(if voice.is_empty() { self.default_voice.clone() } else { voice.into() }),
            online: true, fallback_reason: None,
        })
    }
    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        let budget = std::time::Duration::from_secs(30);
        let start = tokio::time::Instant::now();
        tokio::time::timeout(
            budget,
            self.synthesize_with_budget(text, voice, start, budget),
        )
        .await
        .map_err(|_| TtsError::Network("synthesis_timeout".into()))?
    }
}

impl EdgeTtsProvider {
    async fn synthesize_with_budget(
        &self,
        text: &str,
        voice: &str,
        start: tokio::time::Instant,
        budget: std::time::Duration,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        // 1. Fast check if cooldown is active before queueing for semaphore
        {
            let s = self.state.lock().await;
            if let Some(until) = s.cooldown_until {
                let now = tokio::time::Instant::now();
                if now < until {
                    let remaining = until.duration_since(now);
                    tracing::warn!(remaining_ms = remaining.as_millis(), "edge_tts_cooldown_active");
                    return Err(TtsError::RateLimited {
                        status: 429,
                        retry_after: Some(remaining),
                    });
                }
            }
        }

        // 2. Concurrency limit = 1 per provider
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| TtsError::Cancelled)?;

        // Re-check cooldown after acquiring lock
        {
            let s = self.state.lock().await;
            if let Some(until) = s.cooldown_until {
                let now = tokio::time::Instant::now();
                if now < until {
                    let remaining = until.duration_since(now);
                    tracing::warn!(remaining_ms = remaining.as_millis(), "edge_tts_cooldown_active");
                    return Err(TtsError::RateLimited {
                        status: 429,
                        retry_after: Some(remaining),
                    });
                }
            }
        }

        // 3. First attempt
        match self.synthesize_once(text, voice).await {
            Ok(rx) => {
                let mut s = self.state.lock().await;
                s.cooldown_until = None;
                Ok(rx)
            }
            Err(TtsError::RateLimited { status, retry_after }) => {
                let wait_duration = retry_after.unwrap_or_else(|| std::time::Duration::from_secs(2));
                let elapsed = start.elapsed();
                let remaining_budget = budget.saturating_sub(elapsed);
                let now = tokio::time::Instant::now();

                // Record cooldown
                {
                    let mut s = self.state.lock().await;
                    s.cooldown_until = Some(now + wait_duration);
                }

                if wait_duration >= remaining_budget {
                    tracing::warn!(
                        wait_ms = wait_duration.as_millis(),
                        remaining_budget_ms = remaining_budget.as_millis(),
                        "edge_tts_retry_exceeds_budget"
                    );
                    return Err(TtsError::RateLimited {
                        status,
                        retry_after: Some(wait_duration),
                    });
                }

                tracing::info!(
                    wait_ms = wait_duration.as_millis(),
                    "edge_tts_backing_off_before_retry"
                );
                tokio::time::sleep(wait_duration).await;

                // 4. Second attempt (retry once)
                match self.synthesize_once(text, voice).await {
                    Ok(rx) => {
                        let mut s = self.state.lock().await;
                        s.cooldown_until = None;
                        Ok(rx)
                    }
                    Err(TtsError::RateLimited {
                        status: s2,
                        retry_after: ra2,
                    }) => {
                        // Persistent 429: set 30s default cooldown if retry-after is 0 or absent
                        let cooldown = match ra2 {
                            Some(d) if d.as_secs() > 0 => d,
                            _ => std::time::Duration::from_secs(30),
                        };
                        let mut s = self.state.lock().await;
                        s.cooldown_until = Some(tokio::time::Instant::now() + cooldown);
                        tracing::warn!(
                            cooldown_secs = cooldown.as_secs(),
                            "edge_tts_persistent_rate_limited_cooldown_set"
                        );
                        Err(TtsError::RateLimited {
                            status: s2,
                            retry_after: Some(cooldown),
                        })
                    }
                    Err(other) => Err(other),
                }
            }
            Err(other) => Err(other),
        }
    }

    async fn synthesize_once(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        if text.trim().is_empty() {
            return Err(TtsError::Synthesis("empty_text".into()));
        }
        let req_id = uuid::Uuid::now_v7().simple().to_string();
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| TtsError::Network(err.to_string()))?
            .as_secs();
        let url = edge_request_url(&self.endpoint, &req_id, seconds);
        let mut request = url
            .into_client_request()
            .map_err(|err| TtsError::Network(err.to_string()))?;
        for (name, value) in [
            (
                "origin",
                "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold",
            ),
            (
                "user-agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0",
            ),
            ("pragma", "no-cache"),
            ("cache-control", "no-cache"),
        ] {
            request.headers_mut().insert(name, value.parse().unwrap());
        }
        request.headers_mut().insert(
            "cookie",
            format!(
                "muid={};",
                uuid::Uuid::now_v7().simple().to_string().to_uppercase()
            )
            .parse()
            .unwrap(),
        );
        let connector = if request.uri().scheme_str() == Some("wss") {
            let mut roots = rustls::RootCertStore::empty();
            roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
            if roots.is_empty() {
                return Err(TtsError::Network("tls_root_certificates_missing".into()));
            }
            let provider = rustls::crypto::CryptoProvider::get_default()
                .cloned()
                .unwrap_or_else(|| std::sync::Arc::new(rustls::crypto::ring::default_provider()));
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|err| TtsError::Network(format!("tls_configuration_failed: {err}")))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Some(tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(
                config,
            )))
        } else {
            None
        };
        let (ws_stream, _) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector)
                .await
                .map_err(|e| match &e {
                    tokio_tungstenite::tungstenite::Error::Http(response) => {
                        let status = response.status().as_u16();
                        tracing::warn!(status, "edge_tts_handshake_rejected");
                        if status == 429 {
                            let retry_after = parse_retry_after(response.headers());
                            TtsError::RateLimited {
                                status,
                                retry_after,
                            }
                        } else {
                            TtsError::Network(format!(
                                "Edge-TTS handshake rejected: HTTP {status}"
                            ))
                        }
                    }
                    _ => {
                        tracing::warn!("edge_tts_connection_failed");
                        TtsError::Network(format!("Failed to connect to Edge-TTS: {e}"))
                    }
                })?;

        let (mut write, mut read) = ws_stream.split();

        let timestamp = chrono::Utc::now()
            .format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)")
            .to_string();
        let config = serde_json::json!({"context":{"synthesis":{"audio":{"metadataoptions":{"sentenceBoundaryEnabled":false,"wordBoundaryEnabled":false},"outputFormat":self.output_format}}}});
        let speech_config = format!(
            "X-Timestamp:{timestamp}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{config}\r\n"
        );
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                speech_config.into(),
            ))
            .await
            .map_err(|e| TtsError::Network(format!("Failed to send config: {e}")))?;

        let voice_name = if voice.is_empty() {
            &self.default_voice
        } else {
            voice
        };
        let escaped = |value: &str| {
            value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        };
        let text = escaped(text);
        let voice_name = escaped(voice_name);
        let ssml = format!(
            "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='zh-CN'>\
            <voice name='{voice_name}'>\
                {text}\
            </voice>\
            </speak>"
        );
        let ssml_message = format!(
            "X-RequestId:{req_id}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{timestamp}Z\r\nPath:ssml\r\n\r\n{ssml}"
        );
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                ssml_message.into(),
            ))
            .await
            .map_err(|e| TtsError::Network(format!("Failed to send SSML: {e}")))?;

        let mut audio = Vec::new();
        let mut completed = false;
        while let Some(message) = read.next().await {
            use tokio_tungstenite::tungstenite::Message;
            match message.map_err(|err| TtsError::Network(err.to_string()))? {
                Message::Binary(bytes) => {
                    if bytes.len() < 2 {
                        return Err(TtsError::Decode("missing_binary_header".into()));
                    }
                    let header_len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
                    if bytes.len() < 2 + header_len {
                        return Err(TtsError::Decode("truncated_binary_header".into()));
                    }
                    let header = std::str::from_utf8(&bytes[2..2 + header_len])
                        .map_err(|err| TtsError::Decode(err.to_string()))?;
                    if !header.lines().any(|line| line.trim() == "Path:audio") {
                        return Err(TtsError::Decode("unexpected_binary_path".into()));
                    }
                    audio.extend_from_slice(&bytes[2 + header_len..]);
                    if audio.len() > 4 * 1024 * 1024 {
                        return Err(TtsError::Decode("audio_too_large".into()));
                    }
                }
                Message::Text(text) if text.lines().any(|line| line.trim() == "Path:turn.end") => {
                    completed = true;
                    break;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        if !completed || audio.is_empty() {
            return Err(TtsError::Synthesis("no_complete_audio".into()));
        }
        let wav = decode_audio_to_wav(&audio).map_err(TtsError::Decode)?;
        tracing::info!(
            encoded_bytes = audio.len(),
            wav_bytes = wav.len(),
            "edge_tts_audio_decoded"
        );
        let (tx, rx) = mpsc::channel(1);
        tx.send(wav)
            .await
            .map_err(|err| TtsError::Synthesis(err.to_string()))?;
        Ok(rx)
    }
}

/// Decodes MP3 / WAV audio bytes into mono f32 PCM samples using Symphonia.
pub fn decode_audio_to_pcm(audio_bytes: &[u8]) -> Result<Vec<f32>, String> {
    decode_audio_with_rate(audio_bytes).map(|(pcm, _)| pcm)
}

pub fn decode_audio_to_wav(audio_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let (pcm, rate) = decode_audio_with_rate(audio_bytes)?;
    if pcm.is_empty() {
        return Err("decoded_audio_empty".into());
    }
    Ok(nomifun_audio::pcm_to_wav(&pcm, rate))
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio_tungstenite::tungstenite::Message;

    async fn fixture(bytes: Option<Vec<u8>>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let config = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(config.contains("audio-24khz-48kbitrate-mono-mp3"));
            let ssml = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(ssml.contains("&amp;"));
            if let Some(bytes) = bytes {
                let headers = b"Path:audio\r\nContent-Type:audio/mpeg\r\n";
                let mut frame = (headers.len() as u16).to_be_bytes().to_vec();
                frame.extend_from_slice(headers);
                frame.extend_from_slice(&bytes);
                ws.send(Message::Binary(frame.into())).await.unwrap();
            }
            ws.send(Message::Text("Path:turn.end\r\n\r\n".into()))
                .await
                .unwrap();
        });
        (url, server)
    }

    async fn fixture_429(
        attempts: usize,
        retry_after: Option<&'static str>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            for _ in 0..attempts {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await.unwrap();
                let resp = match retry_after {
                    Some(ra) => format!("HTTP/1.1 429 Too Many Requests\r\nRetry-After: {ra}\r\nContent-Length: 0\r\n\r\n"),
                    None => "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n".to_string(),
                };
                socket.write_all(resp.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        (url, server)
    }

    async fn fixture_429_then_audio(
        bytes: Vec<u8>,
        retry_after: Option<&'static str>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            // 1st request -> 429
            {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await.unwrap();
                let resp = match retry_after {
                    Some(ra) => format!("HTTP/1.1 429 Too Many Requests\r\nRetry-After: {ra}\r\nContent-Length: 0\r\n\r\n"),
                    None => "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n".to_string(),
                };
                socket.write_all(resp.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
            // 2nd request -> success
            {
                let (socket, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
                let _config = ws.next().await.unwrap().unwrap().into_text().unwrap();
                let _ssml = ws.next().await.unwrap().unwrap().into_text().unwrap();
                let headers = b"Path:audio\r\nContent-Type:audio/mpeg\r\n";
                let mut frame = (headers.len() as u16).to_be_bytes().to_vec();
                frame.extend_from_slice(headers);
                frame.extend_from_slice(&bytes);
                ws.send(Message::Binary(frame.into())).await.unwrap();
                ws.send(Message::Text("Path:turn.end\r\n\r\n".into())).await.unwrap();
            }
        });
        (url, server)
    }

    #[test]
    fn default_endpoint_is_edge_readaloud_upstream() {
        assert_eq!(
            EdgeTtsProvider::DEFAULT_ENDPOINT,
            "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1",
            "the current msedgeservices endpoint rejects the handshake with HTTP 200; \
             the upstream readaloud endpoint is the verified Edge contract"
        );
    }

    #[tokio::test]
    async fn handshake_request_uses_readaloud_token_and_reports_http_200() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let request_line = String::from_utf8_lossy(&buffer[..read])
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            request_line
        });
        let result = EdgeTtsProvider::new()
            .with_endpoint(endpoint)
            .synthesize_stream("连接测试", "")
            .await;
        let request_line = server.await.unwrap();
        assert!(
            request_line.contains("TrustedClientToken=6A5AA1D4EAFF4E9FB37E23D68491D6F4"),
            "Edge readaloud expects TrustedClientToken, got: {request_line}"
        );
        assert!(
            request_line.contains("ConnectionId=") && request_line.contains("Sec-MS-GEC="),
            "handshake must keep a per-request id and the GEC checksum: {request_line}"
        );
        assert!(
            !request_line.contains("Ocp-Apim-Subscription-Key"),
            "the readaloud endpoint does not accept the subscription-key parameter: {request_line}"
        );
        match result {
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("200") && text.contains("handshake"),
                    "HTTP 200 must be reported as an explicit handshake rejection, got: {text}"
                );
            }
            Ok(_) => panic!("an HTTP 200 handshake must not be reported as success"),
        }
    }

    #[tokio::test]
    async fn empty_synthesis_is_an_error_instead_of_successful_playback() {
        let (endpoint, server) = fixture(None).await;
        let result = EdgeTtsProvider::new()
            .with_endpoint(endpoint)
            .synthesize_stream("A & B", "")
            .await;
        assert!(matches!(result, Err(TtsError::Synthesis(_))));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn audio_is_decoded_with_the_original_sample_rate() {
        let pcm: Vec<f32> = (0..2400)
            .map(|sample| 0.1 * (sample as f32 * 0.1).sin())
            .collect();
        let (endpoint, server) = fixture(Some(nomifun_audio::pcm_to_wav(&pcm, 24000))).await;
        let mut stream = EdgeTtsProvider::new()
            .with_endpoint(endpoint)
            .synthesize_stream("A & B", "")
            .await
            .unwrap();
        let frame = stream.recv().await.unwrap();
        let (decoded, rate) = nomifun_audio::parse_audio_frame_to_pcm(&frame);
        assert_eq!(rate, 24000);
        assert_eq!(decoded.len(), pcm.len());
        assert!(decoded.iter().any(|sample| sample.abs() > 0.05));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn handshake_429_exceeding_budget_returns_rate_limited() {
        let (endpoint, server) = fixture_429(1, Some("35")).await;
        let provider = EdgeTtsProvider::new().with_endpoint(endpoint);
        let result = provider.synthesize_stream("测试内容", "").await;
        match result {
            Err(TtsError::RateLimited { status, retry_after }) => {
                assert_eq!(status, 429);
                assert_eq!(retry_after, Some(std::time::Duration::from_secs(35)));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn handshake_429_retry_success_recovers() {
        let pcm: Vec<f32> = (0..2400)
            .map(|sample| 0.1 * (sample as f32 * 0.1).sin())
            .collect();
        let wav = nomifun_audio::pcm_to_wav(&pcm, 24000);
        let (endpoint, server) = fixture_429_then_audio(wav, Some("0")).await;
        let provider = EdgeTtsProvider::new().with_endpoint(endpoint);
        let mut stream = provider.synthesize_stream("重试测试", "").await.unwrap();
        let frame = stream.recv().await.unwrap();
        let (decoded, rate) = nomifun_audio::parse_audio_frame_to_pcm(&frame);
        assert_eq!(rate, 24000);
        assert_eq!(decoded.len(), pcm.len());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn persistent_429_activates_cooldown_and_skips_network() {
        // Server expects at most 2 connections (initial + 1 retry)
        let (endpoint, server) = fixture_429(2, Some("0")).await;
        let provider = EdgeTtsProvider::new().with_endpoint(endpoint);
        let first = provider.synthesize_stream("第一次失败", "").await;
        assert!(matches!(first, Err(TtsError::RateLimited { status: 429, .. })));
        server.await.unwrap();

        // Immediate next call must hit cooldown without making any TCP connection (listener is already finished)
        let second = provider.synthesize_stream("第二次跳过网络", "").await;
        assert!(matches!(second, Err(TtsError::RateLimited { status: 429, .. })));
    }

    #[test]
    fn parse_retry_after_unit_tests() {
        use tokio_tungstenite::tungstenite::http::HeaderMap;

        let mut h = HeaderMap::new();
        h.insert("retry-after", "12".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(std::time::Duration::from_secs(12)));

        let mut h2 = HeaderMap::new();
        h2.insert("retry-after", "invalid-value".parse().unwrap());
        assert_eq!(parse_retry_after(&h2), None);

        let mut h3 = HeaderMap::new();
        let future_time = chrono::Utc::now() + chrono::Duration::seconds(45);
        let date_str = future_time.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        h3.insert("retry-after", date_str.parse().unwrap());
        let parsed = parse_retry_after(&h3).unwrap();
        assert!(parsed.as_secs() >= 40 && parsed.as_secs() <= 46);
    }

    #[tokio::test]
    #[ignore = "explicit online probe; fixed English sentence, no microphone or playback"]
    async fn edge_online_english_synthesis_probe() {
        let mut audio = EdgeTtsProvider::new()
            .synthesize_stream("This is a voice connection test.", "en-US-JennyNeural")
            .await
            .unwrap();
        let wav = audio.recv().await.unwrap();
        let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
        assert_eq!(rate, 24000);
        assert!(pcm.len() > 2400);
        assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
        println!(
            "Edge returned decoded English speech: {} samples at {} Hz",
            pcm.len(),
            rate
        );
    }

    #[tokio::test]
    #[ignore = "explicit online probe; no microphone or physical playback"]
    async fn edge_online_synthesis_probe() {
        let mut audio = EdgeTtsProvider::new()
            .synthesize_stream("这是语音连接测试。", "")
            .await
            .unwrap();
        let wav = audio.recv().await.unwrap();
        let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
        assert_eq!(rate, 24000);
        assert!(pcm.len() > 2400);
        assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
        println!(
            "Edge returned decoded speech: {} samples at {} Hz",
            pcm.len(),
            rate
        );
    }
}

fn decode_audio_with_rate(audio_bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    let source = std::io::Cursor::new(audio_bytes.to_vec());
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let hint = Hint::new();

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("Probe error: {e}"))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "No default audio track found".to_string())?;

    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err("Default track carries no audio codec parameters".to_string());
    };

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("Audio decoder creation error: {e}"))?;

    let mut pcm: Vec<f32> = Vec::new();
    let mut interleaved: Vec<i16> = Vec::new();
    let mut sample_rate = 16_000;

    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Decode packet error: {e}"))?;
        let channels = decoded.spec().channels().count().max(1);
        sample_rate = decoded.spec().rate();

        decoded.copy_to_vec_interleaved(&mut interleaved);
        if channels == 1 {
            pcm.extend(interleaved.iter().map(|&s| s as f32 / 32768.0));
        } else {
            pcm.extend(interleaved.chunks(channels).map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / channels as i32) as f32 / 32768.0
            }));
        }
    }

    Ok((pcm, sample_rate))
}

/// Null TTS stub implementation for tests and dev skeleton.
/// Red-line: Zero mock contamination in production runtime (AGENTS.md / interaction.zh.md P2).
#[cfg(any(test, feature = "voice-skeleton"))]
#[derive(Debug, Default, Clone)]
pub struct NullTts;

#[cfg(any(test, feature = "voice-skeleton"))]
pub struct FailingTts;
#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl TtsProvider for FailingTts {
    async fn synthesize_stream(
        &self,
        _: &str,
        _: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        Err(TtsError::Network("fixture synthesis failure".into()))
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
impl NullTts {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl TtsProvider for NullTts {
    async fn synthesize_stream(
        &self,
        text: &str,
        _voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        let (tx, rx) = mpsc::channel(8);
        let payload = if text.is_empty() {
            vec![0u8; 64]
        } else {
            text.as_bytes().to_vec()
        };

        tokio::spawn(async move {
            let _ = tx.send(payload).await;
        });

        Ok(rx)
    }
}

/// Configuration for offline Sherpa-ONNX TTS.
/// Baseline: docs/architecture/voice-agent/execution/R1-R5-execution-coaching-2026-10-21.zh.md §8.4
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SherpaOfflineTtsConfig {
    /// Path to VITS ONNX model file (e.g. model.onnx)
    pub model_path: Option<std::path::PathBuf>,
    /// Path to lexicon.txt (pinyin to phonemes mapping)
    pub lexicon_path: Option<std::path::PathBuf>,
    /// Path to tokens.txt
    pub tokens_path: Option<std::path::PathBuf>,
    /// Path to data directory (e.g. espeak-ng-data)
    pub data_dir: Option<std::path::PathBuf>,
    /// Path to dict directory (e.g. jieba dict)
    pub dict_dir: Option<std::path::PathBuf>,
    /// Path to rule.fst
    pub rule_fsts: Option<std::path::PathBuf>,
    /// Path to rule.far
    pub rule_fars: Option<std::path::PathBuf>,
    /// Number of threads for ONNX runtime
    pub num_threads: i32,
    /// Speech speed rate (default 1.0)
    pub speed: f32,
    /// Speaker ID (for multi-speaker models)
    pub sid: i32,
    /// Noise scale (default 0.667)
    pub noise_scale: f32,
    /// Noise scale w (default 0.8)
    pub noise_scale_w: f32,
    /// Length scale (default 1.0)
    pub length_scale: f32,
    /// Path to vocoder model (e.g. vocos-22khz-univ.onnx) for Matcha-TTS
    pub vocoder_path: Option<std::path::PathBuf>,
    /// Path to English acoustic model for bilingual Matcha-TTS
    pub en_model_path: Option<std::path::PathBuf>,
    /// Path to English tokens.txt
    pub en_tokens_path: Option<std::path::PathBuf>,
    /// Path to English espeak-ng-data directory
    pub en_data_dir: Option<std::path::PathBuf>,
}

impl Default for SherpaOfflineTtsConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            lexicon_path: None,
            tokens_path: None,
            data_dir: None,
            dict_dir: None,
            rule_fsts: None,
            rule_fars: None,
            num_threads: 2,
            speed: 1.0,
            sid: 0,
            noise_scale: 0.667,
            noise_scale_w: 0.8,
            length_scale: 1.0,
            vocoder_path: None,
            en_model_path: None,
            en_tokens_path: None,
            en_data_dir: None,
        }
    }
}

impl SherpaOfflineTtsConfig {
    /// Auto-discovers standard VITS / Matcha model layout from a directory.
    pub fn from_dir(dir: impl AsRef<std::path::Path>) -> Self {
        let dir = dir.as_ref();
        let find_file = |base: &std::path::Path, name: &str| {
            let p = base.join(name);
            if p.exists() { Some(p) } else { None }
        };

        // 1. Check for vocoder in dir, dir/shared, or dir/tts
        let vocoder = find_file(dir, "vocos-22khz-univ.onnx")
            .or_else(|| find_file(dir, "vocoder.onnx"))
            .or_else(|| find_file(&dir.join("shared"), "vocos-22khz-univ.onnx"))
            .or_else(|| find_file(&dir.join("shared"), "vocoder.onnx"))
            .or_else(|| find_file(&dir.join("tts"), "vocos-22khz-univ.onnx"));

        // 2. Check for Chinese Matcha sub-directory
        let zh_sub = if dir.join("matcha-icefall-zh-baker").is_dir() {
            dir.join("matcha-icefall-zh-baker")
        } else if dir.join("zh").is_dir() {
            dir.join("zh")
        } else if dir.join("tts").join("matcha-icefall-zh-baker").is_dir() {
            dir.join("tts").join("matcha-icefall-zh-baker")
        } else {
            dir.to_path_buf()
        };

        // 3. Check for English Matcha sub-directory
        let en_sub = if dir.join("matcha-icefall-en_US-ljspeech").is_dir() {
            Some(dir.join("matcha-icefall-en_US-ljspeech"))
        } else if dir.join("en").is_dir() {
            Some(dir.join("en"))
        } else if dir.join("tts").join("matcha-icefall-en_US-ljspeech").is_dir() {
            Some(dir.join("tts").join("matcha-icefall-en_US-ljspeech"))
        } else {
            None
        };

        let model = find_file(&zh_sub, "model-steps-3.onnx")
            .or_else(|| find_file(&zh_sub, "model.onnx"))
            .or_else(|| find_file(&zh_sub, "vits.onnx"))
            .or_else(|| find_file(&zh_sub, "model.int8.onnx"));
        let lexicon = find_file(&zh_sub, "lexicon.txt");
        let tokens = find_file(&zh_sub, "tokens.txt");
        let data_dir = find_file(&zh_sub, "espeak-ng-data");
        let dict_dir = find_file(&zh_sub, "dict");
        let rule_fsts = find_file(&zh_sub, "rule.fst")
            .or_else(|| find_file(&zh_sub, "date.fst"));
        let rule_fars = find_file(&zh_sub, "rule.far");

        let (en_model, en_tokens, en_data) = if let Some(ref ed) = en_sub {
            (
                find_file(ed, "model-steps-3.onnx").or_else(|| find_file(ed, "model.onnx")),
                find_file(ed, "tokens.txt"),
                find_file(ed, "espeak-ng-data"),
            )
        } else {
            (None, None, None)
        };

        Self {
            model_path: model,
            lexicon_path: lexicon,
            tokens_path: tokens,
            data_dir,
            dict_dir,
            rule_fsts,
            rule_fars,
            num_threads: 2,
            speed: 1.0,
            sid: 0,
            noise_scale: 0.667,
            noise_scale_w: 0.8,
            length_scale: 1.0,
            vocoder_path: vocoder,
            en_model_path: en_model,
            en_tokens_path: en_tokens,
            en_data_dir: en_data,
        }
    }

    /// Validates whether the required model and vocabulary files exist on disk.
    pub fn is_valid(&self) -> bool {
        // Matcha validation: model + vocoder + tokens
        if let (Some(m), Some(v), Some(t)) = (&self.model_path, &self.vocoder_path, &self.tokens_path) {
            if m.exists() && v.exists() && t.exists() {
                return true;
            }
        }
        // Vits validation: model + tokens + lexicon
        let model_ok = self.model_path.as_ref().is_some_and(|p| p.exists());
        let tokens_ok = self.tokens_path.as_ref().is_some_and(|p| p.exists());
        let lexicon_ok = self.lexicon_path.as_ref().is_some_and(|p| p.exists());
        model_ok && tokens_ok && lexicon_ok
    }

    pub fn is_matcha(&self) -> bool {
        self.vocoder_path.as_ref().is_some_and(|p| p.exists())
            && self.model_path.as_ref().is_some_and(|p| p.exists())
            && self.tokens_path.as_ref().is_some_and(|p| p.exists())
    }
}

/// Strip Markdown formatting for clean TTS speech output.
pub fn strip_markdown_tts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_code_block = false;

    while let Some(ch) = chars.next() {
        if ch == '\0' {
            continue;
        }
        if ch == '`' {
            if chars.peek() == Some(&'`') {
                chars.next();
                if chars.peek() == Some(&'`') {
                    chars.next();
                    in_code_block = !in_code_block;
                    if in_code_block {
                        out.push_str("，代码片段，");
                    }
                    continue;
                }
            } else {
                continue;
            }
        }
        if in_code_block {
            continue;
        }
        if ch == '*' {
            continue; // strip bold/italic asterisks
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangSegment {
    Zh(String),
    En(String),
}

/// Segment text into language blocks for dual-engine synthesis.
pub fn segment_languages(text: &str) -> Vec<LangSegment> {
    segment_languages_with_locale(text, None)
}

/// Segment text into language blocks with a fallback locale when text has no alphabetic cues.
pub fn segment_languages_with_locale(text: &str, default_locale: Option<&str>) -> Vec<LangSegment> {
    let cleaned = strip_markdown_tts(text);
    let mut segments = Vec::new();
    let is_default_en = default_locale.map_or(false, |l| l.to_ascii_lowercase().starts_with("en"));
    let mut current_is_zh: Option<bool> = None;
    let mut buf = String::new();

    for ch in cleaned.chars() {
        let is_zh = matches!(ch, '\u{3000}'..='\u{303f}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}');
        let is_en = ch.is_ascii_alphabetic();

        let target_is_zh = if is_zh {
            Some(true)
        } else if is_en {
            Some(false)
        } else {
            current_is_zh.or(Some(!is_default_en))
        };

        if target_is_zh == current_is_zh {
            buf.push(ch);
        } else {
            let trimmed = buf.trim();
            if !trimmed.is_empty() {
                match current_is_zh {
                    Some(false) => segments.push(LangSegment::En(trimmed.to_string())),
                    _ => segments.push(LangSegment::Zh(trimmed.to_string())),
                }
            }
            buf.clear();
            current_is_zh = target_is_zh;
            buf.push(ch);
        }
    }
    let trimmed = buf.trim();
    if !trimmed.is_empty() {
        match current_is_zh {
            Some(false) => segments.push(LangSegment::En(trimmed.to_string())),
            _ => segments.push(LangSegment::Zh(trimmed.to_string())),
        }
    }
    segments
}

#[cfg(feature = "local-tts")]
struct ActiveTtsEngines {
    primary: sherpa_onnx::OfflineTts,
    english: Option<sherpa_onnx::OfflineTts>,
}

#[cfg(feature = "local-tts")]
#[derive(Clone)]
pub struct SherpaOfflineTtsProvider {
    config: SherpaOfflineTtsConfig,
    engines: std::sync::Arc<tokio::sync::Mutex<Option<ActiveTtsEngines>>>,
}

#[cfg(feature = "local-tts")]
impl SherpaOfflineTtsProvider {
    pub fn new(config: SherpaOfflineTtsConfig) -> Self {
        Self {
            config,
            engines: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn from_dir(dir: impl AsRef<std::path::Path>) -> Self {
        Self::new(SherpaOfflineTtsConfig::from_dir(dir))
    }

    pub fn is_available(&self) -> bool {
        self.config.is_valid()
    }

    fn init_engines_sync(config: &SherpaOfflineTtsConfig) -> Result<ActiveTtsEngines, TtsError> {
        if !config.is_valid() {
            return Err(TtsError::Synthesis(
                "offline_tts_resources_missing: required model, lexicon or tokens file not found".into(),
            ));
        }

        if config.is_matcha() {
            let acoustic_str = config.model_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let vocoder_str = config.vocoder_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let lexicon_str = config.lexicon_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let tokens_str = config.tokens_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let dict_dir_str = config.dict_dir.as_ref().map(|p| p.to_string_lossy().into_owned());
            let data_dir_str = config.data_dir.as_ref().map(|p| p.to_string_lossy().into_owned());

            // Check for fsts
            let rule_fsts = if let Some(ref m_path) = config.model_path {
                let parent = m_path.parent().unwrap_or(m_path);
                let mut fsts = Vec::new();
                for f in ["date.fst", "number.fst", "phone.fst", "rule.fst"] {
                    let fp = parent.join(f);
                    if fp.exists() {
                        fsts.push(fp.to_string_lossy().into_owned());
                    }
                }
                if fsts.is_empty() { None } else { Some(fsts.join(",")) }
            } else {
                None
            };

            let primary_config = sherpa_onnx::OfflineTtsConfig {
                model: sherpa_onnx::OfflineTtsModelConfig {
                    matcha: sherpa_onnx::OfflineTtsMatchaModelConfig {
                        acoustic_model: acoustic_str,
                        vocoder: vocoder_str.clone(),
                        lexicon: lexicon_str,
                        tokens: tokens_str,
                        data_dir: data_dir_str,
                        dict_dir: dict_dir_str,
                        noise_scale: config.noise_scale,
                        length_scale: config.length_scale,
                    },
                    num_threads: config.num_threads,
                    debug: false,
                    provider: Some("cpu".into()),
                    ..Default::default()
                },
                rule_fsts,
                rule_fars: None,
                max_num_sentences: 1,
                silence_scale: 0.2,
            };

            let primary = sherpa_onnx::OfflineTts::create(&primary_config).ok_or_else(|| {
                TtsError::Synthesis("failed_to_create_sherpa_offline_tts_matcha_engine".into())
            })?;

            let english = if let (Some(en_m), Some(en_t)) = (&config.en_model_path, &config.en_tokens_path) {
                if en_m.exists() && en_t.exists() {
                    let en_data_str = config.en_data_dir.as_ref().map(|p| p.to_string_lossy().into_owned());
                    let en_config = sherpa_onnx::OfflineTtsConfig {
                        model: sherpa_onnx::OfflineTtsModelConfig {
                            matcha: sherpa_onnx::OfflineTtsMatchaModelConfig {
                                acoustic_model: Some(en_m.to_string_lossy().into_owned()),
                                vocoder: vocoder_str,
                                lexicon: None,
                                tokens: Some(en_t.to_string_lossy().into_owned()),
                                data_dir: en_data_str,
                                dict_dir: None,
                                noise_scale: config.noise_scale,
                                length_scale: config.length_scale,
                            },
                            num_threads: config.num_threads,
                            debug: false,
                            provider: Some("cpu".into()),
                            ..Default::default()
                        },
                        rule_fsts: None,
                        rule_fars: None,
                        max_num_sentences: 1,
                        silence_scale: 0.2,
                    };
                    sherpa_onnx::OfflineTts::create(&en_config)
                } else {
                    None
                }
            } else {
                None
            };

            Ok(ActiveTtsEngines { primary, english })
        } else {
            // Legacy VITS
            let model_str = config.model_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let lexicon_str = config.lexicon_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let tokens_str = config.tokens_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let data_dir_str = config.data_dir.as_ref().map(|p| p.to_string_lossy().into_owned());
            let dict_dir_str = config.dict_dir.as_ref().map(|p| p.to_string_lossy().into_owned());
            let rule_fsts_str = config.rule_fsts.as_ref().map(|p| p.to_string_lossy().into_owned());
            let rule_fars_str = config.rule_fars.as_ref().map(|p| p.to_string_lossy().into_owned());

            let tts_config = sherpa_onnx::OfflineTtsConfig {
                model: sherpa_onnx::OfflineTtsModelConfig {
                    vits: sherpa_onnx::OfflineTtsVitsModelConfig {
                        model: model_str,
                        lexicon: lexicon_str,
                        tokens: tokens_str,
                        data_dir: data_dir_str,
                        dict_dir: dict_dir_str,
                        noise_scale: config.noise_scale,
                        noise_scale_w: config.noise_scale_w,
                        length_scale: config.length_scale,
                    },
                    num_threads: config.num_threads,
                    debug: false,
                    provider: Some("cpu".into()),
                    ..Default::default()
                },
                rule_fsts: rule_fsts_str,
                rule_fars: rule_fars_str,
                max_num_sentences: 1,
                silence_scale: 0.2,
            };

            let primary = sherpa_onnx::OfflineTts::create(&tts_config).ok_or_else(|| {
                TtsError::Synthesis("failed_to_create_sherpa_offline_tts_engine".into())
            })?;

            Ok(ActiveTtsEngines { primary, english: None })
        }
    }
}

#[cfg(feature = "local-tts")]
#[async_trait]
impl TtsProvider for SherpaOfflineTtsProvider {
    fn is_available(&self) -> bool {
        self.config.is_valid()
    }
    fn diagnostic(&self, voice: &str) -> Option<nomifun_api_types::voice::VoiceTtsDiagnostic> {
        Some(nomifun_api_types::voice::VoiceTtsDiagnostic {
            provider: "sherpa".into(), voice: Some(if voice.is_empty() { self.config.sid.to_string() } else { voice.into() }),
            online: false, fallback_reason: None,
        })
    }
    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        let text_clean = text.trim();
        if text_clean.is_empty() {
            return Err(TtsError::Synthesis("empty_text".into()));
        }
        if text_clean.contains('\0') {
            return Err(TtsError::Synthesis("null_byte_detected".into()));
        }

        let (actual_voice, custom_speed) = if let Some((v, s)) = voice.split_once(";speed=") {
            (v, s.parse::<f32>().ok())
        } else {
            (voice, None)
        };

        let sid = if actual_voice.is_empty() {
            self.config.sid
        } else {
            actual_voice.parse::<i32>().unwrap_or(self.config.sid)
        };

        let speed = custom_speed.unwrap_or(self.config.speed);

        let engines_arc = self.engines.clone();
        let config = self.config.clone();
        let text_owned = text_clean.to_string();
        let default_locale = if actual_voice.starts_with("en") || actual_voice.contains("en_") || actual_voice.contains("en-") {
            Some("en-US".to_string())
        } else {
            None
        };

        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled_worker = cancelled.clone();

        let blocking_task = tokio::task::spawn_blocking(move || {
            let mut lock = engines_arc.blocking_lock();
            if lock.is_none() {
                let eng = Self::init_engines_sync(&config)?;
                *lock = Some(eng);
            }
            let engines = lock.as_ref().expect("engines initialized");
            let gen_config = sherpa_onnx::GenerationConfig {
                speed,
                sid,
                ..Default::default()
            };

            let segments = segment_languages_with_locale(&text_owned, default_locale.as_deref());
            if segments.is_empty() {
                return Err(TtsError::Synthesis("empty_text".into()));
            }

            let mut all_samples = Vec::new();
            let mut sample_rate = engines.primary.sample_rate();

            for (i, seg) in segments.iter().enumerate() {
                if cancelled_worker.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(TtsError::Cancelled);
                }

                let (target_engine, chunk_text) = match seg {
                    LangSegment::Zh(s) => (&engines.primary, s.as_str()),
                    LangSegment::En(s) => {
                        if let Some(ref en) = engines.english {
                            (en, s.as_str())
                        } else if !config.is_matcha() {
                            // Single-model legacy VITS handles both
                            (&engines.primary, s.as_str())
                        } else {
                            return Err(TtsError::Synthesis("english_tts_engine_unavailable".into()));
                        }
                    }
                };

                let cancel_cb = cancelled_worker.clone();
                let callback = move |_samples: &[f32], _progress: f32| -> bool {
                    !cancel_cb.load(std::sync::atomic::Ordering::Relaxed)
                };

                let audio = target_engine
                    .generate_with_config(chunk_text, &gen_config, Some(callback))
                    .ok_or_else(|| TtsError::Synthesis("sherpa_offline_tts_generation_failed".into()))?;

                let samples = audio.samples();
                let seg_sr = audio.sample_rate();
                if samples.is_empty() || seg_sr <= 0 {
                    return Err(TtsError::Synthesis("tts_segment_empty_samples".into()));
                }
                if sample_rate == 0 {
                    sample_rate = seg_sr;
                } else if sample_rate != seg_sr {
                    return Err(TtsError::Synthesis("tts_inconsistent_sample_rates".into()));
                }

                if i > 0 && !all_samples.is_empty() {
                    let gap = vec![0.0f32; (sample_rate as f32 * 0.08) as usize];
                    all_samples.extend_from_slice(&gap);
                }
                all_samples.extend_from_slice(samples);
            }

            if all_samples.is_empty() {
                return Err(TtsError::Synthesis("sherpa_offline_tts_empty_samples".into()));
            }

            // Verify samples are finite
            if !all_samples.iter().all(|s| s.is_finite()) {
                return Err(TtsError::Synthesis("tts_non_finite_samples".into()));
            }

            let wav = nomifun_audio::pcm_to_wav(&all_samples, sample_rate as u32);
            tracing::info!(
                text_len = text_owned.len(),
                sample_count = all_samples.len(),
                sample_rate,
                wav_bytes = wav.len(),
                "sherpa_offline_tts_synthesized"
            );
            Ok(wav)
        });

        let wav = match tokio::time::timeout(std::time::Duration::from_secs(30), blocking_task).await {
            Ok(res) => res.map_err(|join_err| TtsError::Synthesis(format!("tts_task_panicked: {join_err}")))??,
            Err(_) => {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                return Err(TtsError::Synthesis("tts_generation_timeout".into()));
            }
        };

        let (tx, rx) = mpsc::channel(1);
        tx.send(wav)
            .await
            .map_err(|err| TtsError::Synthesis(err.to_string()))?;
        Ok(rx)
    }
}

#[cfg(not(feature = "local-tts"))]
#[derive(Clone, Default)]
pub struct SherpaOfflineTtsProvider {
    #[allow(dead_code)]
    config: SherpaOfflineTtsConfig,
}

#[cfg(not(feature = "local-tts"))]
impl SherpaOfflineTtsProvider {
    pub fn new(config: SherpaOfflineTtsConfig) -> Self {
        Self { config }
    }

    pub fn from_dir(dir: impl AsRef<std::path::Path>) -> Self {
        Self::new(SherpaOfflineTtsConfig::from_dir(dir))
    }

    pub fn is_available(&self) -> bool {
        false
    }
}

#[cfg(not(feature = "local-tts"))]
#[async_trait]
impl TtsProvider for SherpaOfflineTtsProvider {
    fn is_available(&self) -> bool {
        false
    }
    async fn synthesize_stream(
        &self,
        _text: &str,
        _voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        Err(TtsError::Synthesis("sherpa_offline_tts_not_compiled (enable feature = 'local-tts')".into()))
    }
}

/// Fallback TTS provider attempting primary provider first, and falling back
/// to secondary on failure.
#[derive(Clone)]
pub struct FallbackTtsProvider {
    primary: std::sync::Arc<dyn TtsProvider>,
    fallback: std::sync::Arc<dyn TtsProvider>,
    selected: std::sync::Arc<std::sync::Mutex<Option<nomifun_api_types::voice::VoiceTtsDiagnostic>>>,
}

impl FallbackTtsProvider {
    pub fn new(
        primary: std::sync::Arc<dyn TtsProvider>,
        fallback: std::sync::Arc<dyn TtsProvider>,
    ) -> Self {
        Self { primary, fallback, selected: Default::default() }
    }

    pub fn primary(&self) -> &std::sync::Arc<dyn TtsProvider> {
        &self.primary
    }

    pub fn fallback(&self) -> &std::sync::Arc<dyn TtsProvider> {
        &self.fallback
    }
}

#[async_trait]
impl TtsProvider for FallbackTtsProvider {
    fn diagnostic(&self, _voice: &str) -> Option<nomifun_api_types::voice::VoiceTtsDiagnostic> {
        self.selected.lock().unwrap_or_else(|e|e.into_inner()).clone()
    }
    fn is_available(&self) -> bool {
        self.primary.is_available() || self.fallback.is_available()
    }
    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        match self.primary.synthesize_stream(text, voice).await {
            Ok(rx) => {
                *self.selected.lock().unwrap_or_else(|e|e.into_inner()) = self.primary.diagnostic(voice);
                Ok(rx)
            },
            Err(primary_err) => {
                if !self.fallback.is_available() {
                    tracing::warn!(
                        error = %primary_err,
                        "primary_tts_failed_and_fallback_unavailable"
                    );
                    *self.selected.lock().unwrap_or_else(|e|e.into_inner()) = self.primary.diagnostic(voice);
                    return Err(primary_err);
                }
                tracing::warn!(
                    error = %primary_err,
                    "primary_tts_failed_attempting_fallback"
                );
                let result = self.fallback.synthesize_stream(text, voice).await;
                let diagnostic = result.as_ref().ok().and_then(|_|self.fallback.diagnostic(voice)).map(|mut diagnostic| {
                    diagnostic.fallback_reason = Some("primary_synthesis_failed".into()); diagnostic
                });
                *self.selected.lock().unwrap_or_else(|e|e.into_inner()) = diagnostic;
                result
            }
        }
    }
}

#[cfg(test)]
mod fallback_and_sherpa_tests {
    use super::*;

    struct MockSuccessTts(Vec<u8>);
    #[async_trait]
    impl TtsProvider for MockSuccessTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
            let (tx, rx) = mpsc::channel(1);
            let _ = tx.send(self.0.clone()).await;
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn fallback_tts_provider_uses_fallback_when_primary_fails() {
        let primary = std::sync::Arc::new(FailingTts);
        let fallback = std::sync::Arc::new(MockSuccessTts(vec![1, 2, 3, 4]));
        let provider = FallbackTtsProvider::new(primary, fallback);

        let mut rx = provider
            .synthesize_stream("hello world", "")
            .await
            .expect("must succeed via fallback");
        let data = rx.recv().await.expect("must receive frame");
        assert_eq!(data, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn fallback_tts_provider_uses_primary_when_primary_succeeds() {
        let primary = std::sync::Arc::new(MockSuccessTts(vec![9, 9, 9]));
        let fallback = std::sync::Arc::new(FailingTts);
        let provider = FallbackTtsProvider::new(primary, fallback);

        let mut rx = provider
            .synthesize_stream("hello world", "")
            .await
            .expect("must succeed via primary");
        let data = rx.recv().await.expect("must receive frame");
        assert_eq!(data, vec![9, 9, 9]);
    }

    #[tokio::test]
    async fn fallback_tts_provider_returns_primary_error_when_fallback_unavailable() {
        struct CustomErrorTts;
        #[async_trait]
        impl TtsProvider for CustomErrorTts {
            async fn synthesize_stream(&self, _: &str, _: &str) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
                Err(TtsError::Network("connection reset 10054".into()))
            }
        }

        let primary = std::sync::Arc::new(CustomErrorTts);
        let fallback = std::sync::Arc::new(SherpaOfflineTtsProvider::new(SherpaOfflineTtsConfig::default()));
        assert!(!fallback.is_available());

        let provider = FallbackTtsProvider::new(primary, fallback);
        let res = provider.synthesize_stream("hello world", "").await;
        assert!(res.is_err());
        let err = res.err().unwrap();
        // Crucial: Must return primary's real network error, NOT masked offline_tts_resources_missing
        assert_eq!(err, TtsError::Network("connection reset 10054".into()));
    }

    #[test]
    fn fallback_tts_provider_is_available_reflects_sub_providers() {
        struct UnavailableMock;
        #[async_trait]
        impl TtsProvider for UnavailableMock {
            fn is_available(&self) -> bool { false }
            async fn synthesize_stream(&self, _: &str, _: &str) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
                Err(TtsError::Synthesis("unavailable".into()))
            }
        }

        let unavail1 = std::sync::Arc::new(UnavailableMock);
        let unavail2 = std::sync::Arc::new(UnavailableMock);
        let provider_both_unavail = FallbackTtsProvider::new(unavail1, unavail2);
        assert!(!provider_both_unavail.is_available());

        let avail = std::sync::Arc::new(MockSuccessTts(vec![]));
        let unavail3 = std::sync::Arc::new(UnavailableMock);
        let provider_one_avail = FallbackTtsProvider::new(unavail3, avail);
        assert!(provider_one_avail.is_available());
    }

    #[tokio::test]
    async fn sherpa_offline_tts_reports_missing_resources_cleanly() {
        let provider = SherpaOfflineTtsProvider::new(SherpaOfflineTtsConfig::default());
        assert!(!provider.is_available());
        let res = provider.synthesize_stream("测试离线语音", "").await;
        assert!(res.is_err());
        let err_msg = format!("{}", res.err().unwrap());
        assert!(
            err_msg.contains("offline_tts_resources_missing")
                || err_msg.contains("sherpa_offline_tts_not_compiled")
        );
    }

    #[test]
    fn sherpa_offline_tts_auto_discovers_layout() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();

        let cfg = SherpaOfflineTtsConfig::from_dir(path);
        assert!(!cfg.is_valid());

        // Create standard layout files
        std::fs::write(path.join("model.onnx"), b"fake onnx").unwrap();
        std::fs::write(path.join("lexicon.txt"), b"fake lexicon").unwrap();
        std::fs::write(path.join("tokens.txt"), b"fake tokens").unwrap();

        let cfg2 = SherpaOfflineTtsConfig::from_dir(path);
        assert!(cfg2.is_valid());
        assert_eq!(cfg2.model_path, Some(path.join("model.onnx")));
        assert_eq!(cfg2.lexicon_path, Some(path.join("lexicon.txt")));
        assert_eq!(cfg2.tokens_path, Some(path.join("tokens.txt")));
    }

    #[test]
    fn sherpa_offline_tts_auto_discovers_matcha_layout() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();

        let zh_dir = path.join("matcha-icefall-zh-baker");
        std::fs::create_dir_all(&zh_dir).unwrap();
        std::fs::write(zh_dir.join("model-steps-3.onnx"), b"fake matcha").unwrap();
        std::fs::write(zh_dir.join("tokens.txt"), b"fake tokens").unwrap();
        std::fs::write(path.join("vocos-22khz-univ.onnx"), b"fake vocos").unwrap();

        let cfg = SherpaOfflineTtsConfig::from_dir(path);
        assert!(cfg.is_valid());
        assert!(cfg.is_matcha());
        assert_eq!(cfg.model_path, Some(zh_dir.join("model-steps-3.onnx")));
        assert_eq!(cfg.vocoder_path, Some(path.join("vocos-22khz-univ.onnx")));
        assert_eq!(cfg.tokens_path, Some(zh_dir.join("tokens.txt")));
    }

    #[test]
    fn test_tts_strip_markdown_and_language_segmentation() {
        let md = "好的，我正在使用 `Docker` 构建镜像：\n```bash\ndocker build -t app .\n```\n请稍候。";
        let cleaned = strip_markdown_tts(md);
        assert!(!cleaned.contains("```"));
        assert!(!cleaned.contains("`"));
        assert!(cleaned.contains("代码片段"));

        let mixed = "正在使用 Docker 部署应用。";
        let segs = segment_languages(mixed);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], LangSegment::Zh("正在使用".into()));
        assert_eq!(segs[1], LangSegment::En("Docker".into()));
        assert_eq!(segs[2], LangSegment::Zh("部署应用。".into()));
    }
}
