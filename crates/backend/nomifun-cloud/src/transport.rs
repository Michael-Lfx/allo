//! HTTP transport with auth injection, tracing headers, and retry policy.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use nomi_config::{FLOWY_SERVER_HOST, ServerConfig};
use nomifun_net::sni;
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use reqwest::{Client, Method, Response};
use tracing::{debug, info, warn};

use crate::error::ServerClientError;
use crate::session::ServerSession;

const REQUEST_ID_HEADER: &str = "x-request-id";
const CLIENT_VERSION_HEADER: &str = "x-client-version";
const LEGACY_TOKEN_HEADER: &str = "token";
const FLOWY_TURN_ID_HEADER: &str = "x-flowy-turn-id";

/// Process-wide `reqwest::Client` so every transport shares one connection pool.
/// `HttpTransport` instances are rebuilt per request by callers, and a fresh
/// `Client` would force a new TCP+TLS handshake every time.
static SHARED_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static HTTP1_FALLBACK_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
/// Mainland networks reset TLS handshakes whose SNI names the Flowy server,
/// while the same IP answers when SNI is omitted. The server's default
/// certificate is still verified against the request host.
static NO_SNI_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();

fn build_flowy_client(http1_only: bool) -> Result<Client, String> {
    let builder = Client::builder();
    let builder = if http1_only {
        builder.http1_only()
    } else {
        builder
    };
    builder.build().map_err(|e| e.to_string())
}

fn shared_no_sni_client() -> Result<Client, ServerClientError> {
    NO_SNI_CLIENT
        .get_or_init(|| Client::builder().tls_sni(false).build().map_err(|e| e.to_string()))
        .clone()
        .map_err(|e| ServerClientError::Http(format!("build SNI-less client: {e}")))
}

fn is_flowy_server_url(url: &str) -> bool {
    sni::url_host_is(url, FLOWY_SERVER_HOST)
}

fn shared_client() -> Result<Client, ServerClientError> {
    SHARED_CLIENT
        .get_or_init(|| build_flowy_client(false))
        .clone()
        .map_err(|e| ServerClientError::Http(format!("build client: {e}")))
}

fn shared_http1_client() -> Result<Client, ServerClientError> {
    HTTP1_FALLBACK_CLIENT
        .get_or_init(|| build_flowy_client(true))
        .clone()
        .map_err(|e| ServerClientError::Http(format!("build HTTP/1.1 client: {e}")))
}

fn is_retryable_transport(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn describe_transport_error(err: &reqwest::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        let text = cause.to_string();
        if parts.last() != Some(&text) {
            parts.push(text);
        }
        source = cause.source();
    }
    let kind = if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "connect"
    } else if err.is_request() {
        "request"
    } else {
        "other"
    };
    format!("{} [kind={kind}]", parts.join(": "))
}

/// Dedicated client for OSS presigned PUT.
///
/// Cover + package uploads must not reuse the shared Flowy connection pool:
/// a stalled idle connection after the small cover PUT can make the large
/// package PUT hang with no response until the 10-minute timeout.
static OSS_PUT_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();

fn oss_put_client() -> Result<Client, ServerClientError> {
    OSS_PUT_CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .tcp_nodelay(true)
                .pool_max_idle_per_host(0)
                .build()
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(|e| ServerClientError::Http(format!("build OSS client: {e}")))
}

fn request_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .map(|parsed| {
            let host = parsed.host_str().unwrap_or("unknown");
            match parsed.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            }
        })
        .unwrap_or_else(|| "invalid-url".into())
}

/// Shared HTTP client for server auth and LLM calls.
#[derive(Clone)]
pub struct HttpTransport {
    client: Client,
    base_url: String,
    timeout: Duration,
    user_agent: String,
}

impl HttpTransport {
    pub fn new(config: &ServerConfig) -> Result<Self, ServerClientError> {
        if config.enabled && config.base_url.trim().is_empty() {
            return Err(ServerClientError::MissingBaseUrl);
        }
        Self::from_base_url(&config.base_url, config.llm.request_timeout_seconds)
    }

    pub fn from_base_url(base_url: &str, timeout_secs: u64) -> Result<Self, ServerClientError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(ServerClientError::MissingBaseUrl);
        }

        Ok(Self {
            client: shared_client()?,
            base_url,
            timeout: Duration::from_secs(timeout_secs.max(1)),
            user_agent: format!("nomifun/{}", env!("CARGO_PKG_VERSION")),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn is_base_url_configured(&self) -> bool {
        !self.base_url.is_empty()
    }

    fn build_headers(
        &self,
        bearer_token: Option<&str>,
        request_id: Option<&str>,
        include_json_content_type: bool,
    ) -> Result<HeaderMap, ServerClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|e| ServerClientError::Http(format!("user-agent header: {e}")))?,
        );
        headers.insert(
            HeaderName::from_static(CLIENT_VERSION_HEADER),
            HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        if include_json_content_type {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }

        if let Some(id) = request_id {
            headers.insert(
                HeaderName::from_static(REQUEST_ID_HEADER),
                HeaderValue::from_str(id)
                    .map_err(|e| ServerClientError::Http(format!("request-id header: {e}")))?,
            );
        }

        if let Some(token) = bearer_token {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|e| ServerClientError::Http(format!("authorization header: {e}")))?,
            );
            headers.insert(
                HeaderName::from_static(LEGACY_TOKEN_HEADER),
                HeaderValue::from_str(token)
                    .map_err(|e| ServerClientError::Http(format!("token header: {e}")))?,
            );
        }

        // Propagate the current agent-run turn id when media/embeddings/etc.
        // run inside a scoped Flowy billing turn (same task-local as LLM).
        if let Some(turn_id) = nomi_providers::current_flowy_billing_turn_id() {
            headers.insert(
                HeaderName::from_static(FLOWY_TURN_ID_HEADER),
                HeaderValue::from_str(&turn_id)
                    .map_err(|e| ServerClientError::Http(format!("turn-id header: {e}")))?,
            );
        }

        Ok(headers)
    }

    fn resolve_url(&self, path: &str) -> String {
        let path = path.trim();
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        let path = path.strip_prefix('/').unwrap_or(path);
        format!("{}/{}", self.base_url, path)
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        session: Option<&ServerSession>,
        body: Option<serde_json::Value>,
    ) -> Result<Response, ServerClientError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let url = self.resolve_url(path);
        let bearer = match session {
            Some(s) => s.access_token().await?,
            None => None,
        };
        let headers = self.build_headers(bearer.as_deref(), Some(&request_id), body.is_some())?;

        debug!(%method, %url, request_id = %request_id, "server http request");

        let mut attempt = 0u32;
        let mut fallback_http1 = false;
        let flowy_server = is_flowy_server_url(&url);
        let mut no_sni = flowy_server && sni::prefers_no_sni(&url);
        loop {
            attempt += 1;
            // First try the pooled client (HTTP/2 when the peer offers it). A
            // transport error retries over HTTP/1.1: mainland middleboxes and
            // some local proxies stall or reset h2 after ALPN.
            let client = if no_sni {
                shared_no_sni_client()?
            } else if fallback_http1 {
                shared_http1_client()?
            } else {
                self.client.clone()
            };
            let mut builder = client
                .request(method.clone(), &url)
                .timeout(self.timeout)
                .headers(headers.clone());
            if let Some(ref json) = body {
                builder = builder.json(json);
            }

            match builder.send().await {
                Ok(resp) => {
                    if no_sni && sni::set_prefers_no_sni(&url, true) {
                        info!(
                            host = %request_host(&url),
                            "TLS without SNI succeeded after handshake reset; preferring it for this session"
                        );
                    }
                    let status = resp.status();
                    if (status.as_u16() == 429 || status.as_u16() == 502 || status.as_u16() == 503)
                        && attempt < 3
                    {
                        let delay = Duration::from_millis(250 * 2u64.pow(attempt - 1));
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) if is_retryable_transport(&err) => {
                    let detail = describe_transport_error(&err);
                    warn!(
                        host = %request_host(&url),
                        attempt,
                        http1_fallback = fallback_http1,
                        no_sni,
                        error = %detail,
                        "server http transport failure"
                    );
                    if attempt < 3 {
                        if no_sni {
                            no_sni = false;
                            sni::set_prefers_no_sni(&url, false);
                        } else if flowy_server && sni::is_connection_reset(&err) {
                            no_sni = true;
                        }
                        fallback_http1 = true;
                        let delay = Duration::from_millis(250 * 2u64.pow(attempt - 1));
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(ServerClientError::Http(detail));
                }
                Err(err) => return Err(ServerClientError::Http(describe_transport_error(&err))),
            }
        }
    }

    pub async fn get(
        &self,
        path: &str,
        session: Option<&ServerSession>,
    ) -> Result<Response, ServerClientError> {
        self.request(Method::GET, path, session, None).await
    }

    pub async fn post_json(
        &self,
        path: &str,
        session: Option<&ServerSession>,
        body: serde_json::Value,
    ) -> Result<Response, ServerClientError> {
        self.request(Method::POST, path, session, Some(body)).await
    }

    pub async fn put_json(
        &self,
        path: &str,
        session: Option<&ServerSession>,
        body: serde_json::Value,
    ) -> Result<Response, ServerClientError> {
        self.request(Method::PUT, path, session, Some(body)).await
    }

    pub async fn delete(
        &self,
        path: &str,
        session: Option<&ServerSession>,
    ) -> Result<Response, ServerClientError> {
        self.request(Method::DELETE, path, session, None).await
    }

    pub async fn post_multipart(
        &self,
        path: &str,
        session: Option<&ServerSession>,
        form: reqwest::multipart::Form,
    ) -> Result<Response, ServerClientError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let url = self.resolve_url(path);
        let bearer = match session {
            Some(s) => s.access_token().await?,
            None => None,
        };
        let headers = self.build_headers(bearer.as_deref(), Some(&request_id), false)?;

        debug!(method = "POST", %url, request_id = %request_id, "server http multipart request");

        let client = if is_flowy_server_url(&url) && sni::prefers_no_sni(&url) {
            shared_no_sni_client()?
        } else {
            self.client.clone()
        };
        let response = client
            .post(&url)
            .timeout(self.timeout)
            .headers(headers.clone())
            .multipart(form)
            .send()
            .await
            .map_err(|e| ServerClientError::Http(e.to_string()))?;

        Ok(response)
    }

    /// PUT raw bytes to an absolute URL (e.g. OSS presigned PUT).
    ///
    /// Does **not** inject Flowy `Authorization` / `token` headers — only the
    /// caller-supplied headers (typically `requiredHeaders` from presign).
    ///
    /// Uses a dedicated long timeout (not the LLM `request_timeout_seconds`,
    /// default 120s): TV Show `.nomivimax` packages routinely need several
    /// minutes on consumer uplinks.
    pub async fn put_bytes_absolute(
        &self,
        url: &str,
        required_headers: &std::collections::HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<Response, ServerClientError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(ServerClientError::Http("empty OSS put url".into()));
        }

        let mut headers = HeaderMap::new();
        for (key, value) in required_headers {
            let name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| ServerClientError::Http(format!("OSS header name `{key}`: {e}")))?;
            let val = HeaderValue::from_str(value)
                .map_err(|e| ServerClientError::Http(format!("OSS header value `{key}`: {e}")))?;
            headers.insert(name, val);
        }

        // At least 10 minutes, and never shorter than the transport's general timeout.
        const OSS_PUT_TIMEOUT_SECS: u64 = 600;
        let timeout = Duration::from_secs(OSS_PUT_TIMEOUT_SECS).max(self.timeout);
        let bytes = body.len();
        let host = request_host(url);
        info!(
            host,
            bytes,
            timeout_secs = timeout.as_secs(),
            "OSS presigned PUT starting"
        );
        let started = Instant::now();

        let response = oss_put_client()?
            .put(url)
            .timeout(timeout)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| {
                let elapsed_ms = started.elapsed().as_millis();
                if e.is_timeout() {
                    ServerClientError::Http(format!(
                        "OSS PUT timed out after {}s ({elapsed_ms}ms elapsed) while uploading {bytes} bytes to {host}",
                        timeout.as_secs()
                    ))
                } else if e.is_connect() {
                    ServerClientError::Http(format!(
                        "OSS PUT connect failed after {elapsed_ms}ms while uploading {bytes} bytes to {host}: {e}"
                    ))
                } else {
                    ServerClientError::Http(format!(
                        "OSS PUT failed after {elapsed_ms}ms while uploading {bytes} bytes to {host}: {e}"
                    ))
                }
            })?;
        info!(
            host,
            bytes,
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "OSS presigned PUT finished"
        );
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use nomi_config::DEFAULT_WECHAT_FLOWY_SERVER_BASE;
    use super::*;

    #[test]
    fn resolve_url_joins_base_and_path() {
        let transport = HttpTransport {
            client: Client::new(),
            base_url: DEFAULT_WECHAT_FLOWY_SERVER_BASE.to_string(),
            timeout: Duration::from_secs(30),
            user_agent: "test".to_string(),
        };
        assert_eq!(
            transport.resolve_url("/user/me"),
            format!("{DEFAULT_WECHAT_FLOWY_SERVER_BASE}/user/me")
        );
    }

    #[tokio::test]
    async fn transport_error_description_includes_cause_chain_and_kind() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let err = Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap_err();
        let text = describe_transport_error(&err);
        assert!(text.contains("[kind=connect]"), "{text}");
        assert!(text.matches(": ").count() >= 1, "{text}");
        assert!(is_retryable_transport(&err));
    }

    #[test]
    fn flowy_clients_build_for_default_and_http1_fallback() {
        assert!(build_flowy_client(false).is_ok());
        assert!(build_flowy_client(true).is_ok());
    }

    #[test]
    fn no_sni_fallback_is_scoped_to_flowy_server_host() {
        assert!(is_flowy_server_url(&format!("{DEFAULT_WECHAT_FLOWY_SERVER_BASE}/user/me")));
        assert!(is_flowy_server_url("https://SERVER.FLOWYAIPC.COM/claw"));
        assert!(!is_flowy_server_url("https://oss.example.com/upload"));
        assert!(!is_flowy_server_url("not a url"));
    }

    #[tokio::test]
    async fn tls_handshake_reset_is_classified_as_connection_reset() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 512];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            #[allow(deprecated)]
            stream.set_linger(Some(Duration::ZERO)).unwrap();
            drop(stream);
        });
        let err = Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("https://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap_err();
        assert!(sni::is_connection_reset(&err), "{}", describe_transport_error(&err));
        assert!(is_retryable_transport(&err));
    }

    #[tokio::test]
    #[ignore = "requires network access to the Flowy server"]
    async fn no_sni_client_verifies_flowy_server_certificate() {
        let resp = shared_no_sni_client()
            .unwrap()
            .get(format!("{DEFAULT_WECHAT_FLOWY_SERVER_BASE}/health"))
            .send()
            .await
            .map_err(|err| describe_transport_error(&err))
            .unwrap();
        assert!(resp.status().is_success(), "{}", resp.status());
    }

    #[tokio::test]
    #[ignore = "requires network access to the Flowy server"]
    async fn bundled_roots_trust_flowy_server_without_os_root_store() {
        let client = Client::builder()
            .tls_built_in_native_certs(false)
            .build()
            .unwrap();
        let resp = client
            .get(format!("{DEFAULT_WECHAT_FLOWY_SERVER_BASE}/health"))
            .send()
            .await
            .map_err(|err| describe_transport_error(&err))
            .unwrap();
        assert!(resp.status().is_success(), "{}", resp.status());
    }
}
