//! HTTP extract provider — fetch URL, convert HTML to markdown.

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use url::Url;

use crate::provider::html_md::{html_to_markdown, truncate_chars};
use crate::provider::ssrf::{check_scheme, resolve_extract_url, resolve_validated};
use crate::provider::ExtractProvider;
use crate::types::{ExtractRequest, ExtractedPage, WebError, EXTRACT_CHAR_LIMIT, EXTRACTOR_FULLPAGE};

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(20);
const EXTRACT_MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const USER_AGENT: &str = "FlowyWeb/0.1 (+https://github.com/flowy)";

pub struct HttpExtractProvider {
    timeout: Duration,
    max_bytes: usize,
    allow_private: bool,
}

impl Default for HttpExtractProvider {
    fn default() -> Self {
        Self {
            timeout: EXTRACT_TIMEOUT,
            max_bytes: EXTRACT_MAX_BYTES,
            allow_private: false,
        }
    }
}

impl HttpExtractProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable the private/local address guard. ONLY for tests (mock HTTP
    /// servers bind to loopback).
    pub fn allow_private_for_tests(mut self) -> Self {
        self.allow_private = true;
        self
    }

    async fn fetch_html(&self, raw_url: &str) -> Result<(Url, String), WebError> {
        let (mut url, mut addrs) = resolve_extract_url(raw_url, self.allow_private).await?;
        for _hop in 0..=MAX_REDIRECTS {
            let response = self.send(&url, &addrs).await?;
            let status = response.status();

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        WebError::Network(format!("redirect without Location from {url}"))
                    })?;
                let next = url.join(location).map_err(|e| {
                    WebError::Network(format!("invalid redirect target {location}: {e}"))
                })?;
                url = check_scheme(next)?;
                addrs = resolve_validated(&url, self.allow_private).await?;
                continue;
            }
            if !status.is_success() {
                return Err(WebError::Provider(format!(
                    "fetch failed: HTTP {status} for {url}"
                )));
            }

            let (body, _body_truncated) = self.read_capped(response).await?;
            let text = String::from_utf8_lossy(&body).into_owned();
            return Ok((url, text));
        }
        Err(WebError::Network(format!(
            "too many redirects fetching {raw_url}"
        )))
    }

    async fn send(&self, url: &Url, addrs: &[SocketAddr]) -> Result<reqwest::Response, WebError> {
        // Fresh Client per hop: `resolve_to_addrs` pins one host's pre-validated
        // addresses; each redirect hop may need its own pinning.
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.timeout)
            .user_agent(USER_AGENT);
        if let Some(host) = url.host_str()
            && !addrs.is_empty()
        {
            builder = builder.resolve_to_addrs(host, addrs);
        }
        let client = builder
            .build()
            .map_err(|e| WebError::Network(format!("failed to build http client: {e}")))?;
        client.get(url.clone()).send().await.map_err(|e| {
            if e.is_timeout() {
                WebError::Timeout(format!("fetch timed out for {url}"))
            } else {
                WebError::Network(format!("fetch failed for {url}: {e}"))
            }
        })
    }

    /// Drain the body up to `max_bytes`; longer bodies are truncated.
    async fn read_capped(&self, response: reqwest::Response) -> Result<(Vec<u8>, bool), WebError> {
        let mut body: Vec<u8> = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) if e.is_timeout() => {
                    return Err(WebError::Timeout(format!("fetch body timed out: {e}")));
                }
                Err(e) => {
                    return Err(WebError::Network(format!("fetch body failed: {e}")));
                }
            };
            if body.len() + chunk.len() > self.max_bytes {
                let take = self.max_bytes.saturating_sub(body.len());
                body.extend_from_slice(&chunk[..take]);
                return Ok((body, true));
            }
            body.extend_from_slice(&chunk);
        }
        Ok((body, false))
    }
}

#[async_trait]
impl ExtractProvider for HttpExtractProvider {
    fn name(&self) -> &str {
        "http"
    }

    async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError> {
        let (final_url, html) = self.fetch_html(&req.url).await?;
        let (title, markdown) = html_to_markdown(&html);
        let (markdown, truncated) = truncate_chars(&markdown, EXTRACT_CHAR_LIMIT);
        Ok(ExtractedPage {
            url: final_url.to_string(),
            title,
            markdown,
            truncated,
            provider: self.name().to_owned(),
            extractor: EXTRACTOR_FULLPAGE.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WebError, EXTRACT_CHAR_LIMIT};

    #[tokio::test]
    async fn extracts_public_page_via_mock() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(
                include_str!("../../tests/fixtures/page_sample.html"),
                "text/html",
            ))
            .mount(&server)
            .await;

        // allow_private_for_tests so loopback mock works
        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let page = provider
            .extract(ExtractRequest {
                url: server.uri(),
            })
            .await
            .unwrap();
        assert_eq!(page.title.as_deref(), Some("Sample"));
        assert!(page.markdown.contains("Hello world"));
        assert!(!page.truncated);
        assert_eq!(page.provider, "http");
    }

    #[tokio::test]
    async fn truncates_long_markdown() {
        let server = wiremock::MockServer::start().await;
        let body = format!(
            "<html><head><title>T</title></head><body><p>{}</p></body></html>",
            "x".repeat(20_000)
        );
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/html"))
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let page = provider
            .extract(ExtractRequest { url: server.uri() })
            .await
            .unwrap();
        assert!(page.truncated);
        assert!(page.markdown.chars().count() <= EXTRACT_CHAR_LIMIT);
    }

    #[tokio::test]
    async fn blocks_private_by_default() {
        let provider = HttpExtractProvider::new();
        let err = provider
            .extract(ExtractRequest {
                url: "http://127.0.0.1/".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, WebError::BlockedUrl(_)));
    }
}
