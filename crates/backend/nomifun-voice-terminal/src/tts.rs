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

#[async_trait]
pub trait TtsProvider: Send + Sync {
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
        "wss://api.msedgeservices.com/tts/cognitiveservices/websocket/v1";
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
        let url = if self.endpoint == Self::DEFAULT_ENDPOINT {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|err| TtsError::Network(err.to_string()))?
                .as_secs();
            format!(
                "{}?ConnectionId={req_id}&Ocp-Apim-Subscription-Key={}&Sec-MS-GEC={}&Sec-MS-GEC-Version=2-{}",
                self.endpoint,
                crate::edge_protocol::CLIENT_TOKEN,
                crate::edge_protocol::gec(seconds),
                crate::edge_protocol::CLIENT_VERSION
            )
        } else {
            self.endpoint.clone()
        };
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
                .map_err(|e| {
                    if let tokio_tungstenite::tungstenite::Error::Http(response) = &e {
                        let status = response.status().as_u16();
                        tracing::warn!(status, "edge_tts_handshake_rejected");
                        if status == 429 {
                            let retry_after = parse_retry_after(response.headers());
                            return TtsError::RateLimited {
                                status,
                                retry_after,
                            };
                        }
                    } else {
                        tracing::warn!("edge_tts_connection_failed");
                    }
                    TtsError::Network(format!("Failed to connect to Edge-TTS: {e}"))
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
