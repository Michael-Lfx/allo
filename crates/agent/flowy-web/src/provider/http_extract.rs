//! HTTP extract provider — fetch URL, convert HTML to markdown.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use url::Url;

use crate::provider::article::{ArticleExtractor, DomSmoothieExtractor};
use crate::provider::extract_policy::{
    LocalExtractDiagnostics, LocalExtractOutcome, failed_outcome, successful_outcome,
};
use crate::provider::html_md::{html_to_markdown, truncate_chars};
use crate::provider::ssrf::{check_scheme, resolve_extract_url, resolve_validated};
use crate::provider::ExtractProvider;
use crate::types::{
    ExtractRequest, ExtractedPage, WebError, EXTRACT_CHAR_LIMIT, EXTRACTOR_FULLPAGE,
    EXTRACTOR_READABILITY, MIN_ARTICLE_CHARS,
};

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(20);
const EXTRACT_MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const USER_AGENT: &str = "FlowyWeb/0.1 (+https://github.com/flowy)";

#[derive(Debug, Clone)]
pub struct FetchedResource {
    pub requested_url: Url,
    pub final_url: Url,
    pub status: reqwest::StatusCode,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub body_truncated: bool,
}

pub struct HttpExtractProvider {
    timeout: Duration,
    max_bytes: usize,
    allow_private: bool,
    article: Arc<dyn ArticleExtractor>,
}

impl Default for HttpExtractProvider {
    fn default() -> Self {
        Self {
            timeout: EXTRACT_TIMEOUT,
            max_bytes: EXTRACT_MAX_BYTES,
            allow_private: false,
            article: Arc::new(DomSmoothieExtractor::new()),
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

    pub fn with_article_extractor(mut self, article: Arc<dyn ArticleExtractor>) -> Self {
        self.article = article;
        self
    }

    async fn fetch_resource(&self, raw_url: &str) -> Result<FetchedResource, WebError> {
        let (mut url, mut addrs) = resolve_extract_url(raw_url, self.allow_private).await?;
        let requested_url = url.clone();
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
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(normalize_content_type);
            let (body, body_truncated) = self.read_capped(response).await?;
            return Ok(FetchedResource {
                requested_url,
                final_url: url,
                status,
                content_type,
                body,
                body_truncated,
            });
        }
        Err(WebError::Network(format!(
            "too many redirects fetching {raw_url}"
        )))
    }

    async fn extract_resource(
        &self,
        req: ExtractRequest,
    ) -> Result<(FetchedResource, ExtractedPage), WebError> {
        let resource = self.fetch_resource(&req.url).await?;
        if !resource.status.is_success() {
            return Err(WebError::Provider(format!(
                "fetch failed: HTTP {} for {}",
                resource.status, resource.final_url
            )));
        }
        let page = self.page_from_resource(&resource)?;
        Ok((resource, page))
    }

    pub async fn extract_with_metadata(&self, req: ExtractRequest) -> LocalExtractOutcome {
        let requested_url = req.url.clone();
        match self.fetch_resource(&req.url).await {
            Err(error) => {
                let diagnostics = diagnostics_from_web_error(&error);
                failed_outcome(requested_url, error, diagnostics)
            }
            Ok(resource) => {
                let diagnostics = LocalExtractDiagnostics {
                    content_type: resource.content_type.clone(),
                    http_status: Some(resource.status.as_u16()),
                    body_truncated: resource.body_truncated,
                };
                if !resource.status.is_success() {
                    let error = WebError::Provider(format!(
                        "fetch failed: HTTP {} for {}",
                        resource.status, resource.final_url
                    ));
                    return failed_outcome(requested_url, error, diagnostics);
                }
                match self.page_from_resource(&resource) {
                    Ok(page) => successful_outcome(requested_url, &resource, page),
                    Err(error) => failed_outcome(requested_url, error, diagnostics),
                }
            }
        }
    }

    fn page_from_resource(&self, resource: &FetchedResource) -> Result<ExtractedPage, WebError> {
        let html = String::from_utf8_lossy(&resource.body).into_owned();
        let final_url = resource.final_url.clone();
        let url_str = final_url.as_str();

        let article = self.article.extract_article(&html, Some(url_str));
        let (raw_html, extractor, title_hint) = match article {
            Some(a) => (a.html, EXTRACTOR_READABILITY, a.title),
            None => (html.clone(), EXTRACTOR_FULLPAGE, None),
        };
        let (title, markdown) = html_to_markdown(&raw_html);
        let title = title_hint.or(title);
        let page = if extractor == EXTRACTOR_READABILITY
            && markdown.chars().count() < MIN_ARTICLE_CHARS
        {
            let (title, markdown) = html_to_markdown(&html);
            let (markdown, truncated) = truncate_chars(&markdown, EXTRACT_CHAR_LIMIT);
            ExtractedPage {
                url: final_url.to_string(),
                title,
                markdown,
                truncated,
                provider: self.name().to_owned(),
                extractor: EXTRACTOR_FULLPAGE.to_owned(),
            }
        } else {
            let (markdown, truncated) = truncate_chars(&markdown, EXTRACT_CHAR_LIMIT);
            ExtractedPage {
                url: final_url.to_string(),
                title,
                markdown,
                truncated,
                provider: self.name().to_owned(),
                extractor: extractor.to_owned(),
            }
        };
        Ok(page)
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

fn normalize_content_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

fn diagnostics_from_web_error(error: &WebError) -> LocalExtractDiagnostics {
    if let WebError::Provider(message) = error {
        let http_status = message
            .split("HTTP ")
            .nth(1)
            .and_then(|part| part.split(' ').next())
            .and_then(|status| status.parse().ok());
        return LocalExtractDiagnostics {
            http_status,
            ..LocalExtractDiagnostics::default()
        };
    }
    LocalExtractDiagnostics::default()
}

#[async_trait]
impl ExtractProvider for HttpExtractProvider {
    fn name(&self) -> &str {
        "http"
    }

    async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError> {
        self.extract_resource(req).await.map(|(_, page)| page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        WebError, EXTRACT_CHAR_LIMIT, EXTRACTOR_FULLPAGE, EXTRACTOR_READABILITY,
    };

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
        assert!(
            page.extractor == EXTRACTOR_READABILITY || page.extractor == EXTRACTOR_FULLPAGE
        );
    }

    #[tokio::test]
    async fn extract_uses_readability_on_chrome_heavy_page() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(
                include_str!("../../tests/fixtures/article_with_chrome.html"),
                "text/html",
            ))
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let page = provider
            .extract(ExtractRequest { url: server.uri() })
            .await
            .unwrap();
        assert_eq!(page.extractor, EXTRACTOR_READABILITY);
        assert!(
            page.markdown.to_lowercase().contains("tail numbers")
                || page.markdown.to_lowercase().contains("fifth ring")
        );
        assert!(!page.markdown.to_lowercase().contains("buy insurance now"));
    }

    #[tokio::test]
    async fn extract_falls_back_to_fullpage_when_article_too_thin() {
        let server = wiremock::MockServer::start().await;
        // Tiny body: readability may return None or < 400 chars after md → fullpage
        let body = "<html><head><title>X</title></head><body><p>Hi</p></body></html>";
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/html"))
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let page = provider
            .extract(ExtractRequest { url: server.uri() })
            .await
            .unwrap();
        assert_eq!(page.extractor, EXTRACTOR_FULLPAGE);
        assert!(page.markdown.to_lowercase().contains("hi"));
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
        assert!(
            page.extractor == EXTRACTOR_READABILITY || page.extractor == EXTRACTOR_FULLPAGE
        );
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

    #[tokio::test]
    async fn fetch_resource_preserves_html_metadata() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html; charset=utf-8")
                    .set_body_bytes("<html><body>Hello</body></html>"),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert_eq!(resource.status.as_u16(), 200);
        assert_eq!(resource.content_type.as_deref(), Some("text/html"));
        assert_eq!(resource.body, b"<html><body>Hello</body></html>");
        assert!(!resource.body_truncated);
        assert_eq!(resource.requested_url.as_str(), format!("{}/", server.uri()));
        assert_eq!(resource.final_url.as_str(), format!("{}/", server.uri()));
    }

    #[tokio::test]
    async fn fetch_resource_allows_missing_content_type() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_bytes("<html><body>Hi</body></html>"),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert!(resource.content_type.is_none());
        assert_eq!(resource.body, b"<html><body>Hi</body></html>");
    }

    #[tokio::test]
    async fn fetch_resource_keeps_pdf_bytes_and_content_type() {
        let server = wiremock::MockServer::start().await;
        let pdf = b"%PDF-1.4\n% test\n%%EOF";
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "application/pdf")
                    .set_body_bytes(pdf),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert_eq!(resource.content_type.as_deref(), Some("application/pdf"));
        assert_eq!(resource.body, pdf);
    }

    #[tokio::test]
    async fn fetch_resource_keeps_plain_text_metadata() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/plain; charset=utf-8")
                    .set_body_bytes("plain text"),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert_eq!(resource.content_type.as_deref(), Some("text/plain"));
        assert_eq!(resource.body, b"plain text");
    }

    #[tokio::test]
    async fn fetch_resource_marks_unknown_binary() {
        let server = wiremock::MockServer::start().await;
        let bytes = vec![0x00, 0x01, 0x02, 0xff];
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_bytes(bytes.clone()),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert_eq!(
            resource.content_type.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(resource.body, bytes);
    }

    #[tokio::test]
    async fn fetch_resource_reports_body_truncation() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/plain")
                    .set_body_bytes("abcdefghijklmnop"),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider {
            timeout: EXTRACT_TIMEOUT,
            max_bytes: 8,
            allow_private: true,
            article: Arc::new(DomSmoothieExtractor::new()),
        };
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert_eq!(resource.body, b"abcdefgh");
        assert!(resource.body_truncated);
    }

    #[tokio::test]
    async fn fetch_resource_follows_redirect_and_keeps_requested_url() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/start"))
            .respond_with(
                wiremock::ResponseTemplate::new(302)
                    .insert_header("location", "/final")
                    .set_body_bytes(""),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/final"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_bytes("<html>redirected</html>"),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let requested = format!("{}/start", server.uri());
        let resource = provider.fetch_resource(&requested).await.unwrap();
        assert_eq!(resource.requested_url.as_str(), requested);
        assert_eq!(resource.final_url.as_str(), format!("{}/final", server.uri()));
        assert_eq!(resource.content_type.as_deref(), Some("text/html"));
    }

    #[tokio::test]
    async fn fetch_resource_keeps_invalid_utf8_bytes() {
        let server = wiremock::MockServer::start().await;
        let bytes = vec![0xff, 0xfe, 0x00, 0x41];
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_bytes(bytes.clone()),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let resource = provider.fetch_resource(&server.uri()).await.unwrap();
        assert_eq!(resource.body, bytes);
    }

    #[test]
    fn normalizes_content_type_without_parameters() {
        assert_eq!(normalize_content_type("text/html; charset=utf-8"), "text/html");
        assert_eq!(normalize_content_type("  APPLICATION/PDF "), "application/pdf");
    }

    #[tokio::test]
    async fn extract_with_metadata_preserves_http_failure_diagnostics() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(404)
                    .insert_header("content-type", "text/html")
                    .set_body_bytes("<html>not found</html>"),
            )
            .mount(&server)
            .await;

        let provider = HttpExtractProvider::new().allow_private_for_tests();
        let outcome = provider
            .extract_with_metadata(ExtractRequest {
                url: server.uri(),
            })
            .await;
        assert!(outcome.result.is_err());
        assert_eq!(outcome.diagnostics.http_status, Some(404));
        assert_eq!(outcome.diagnostics.content_type.as_deref(), Some("text/html"));
    }
}
