//! Allowlist-guarded HTTP client and size-capped body readers for the skill
//! market. All market fetches go through [`build_market_client`], whose
//! custom redirect policy only follows redirects onto known market hosts —
//! a redirect to anything else (cloud metadata endpoints, loopback, RFC1918
//! hosts, arbitrary third parties) is rejected outright.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nomifun_common::AppError;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, RETRY_AFTER};

/// Ranking/readme/detail bodies larger than this are rejected.
pub(crate) const MAX_MARKET_BODY_BYTES: u64 = 8 * 1024 * 1024;
/// Native Skill market archives larger than this are rejected.
pub(crate) const MAX_MARKET_SKILL_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;
/// Compatibility name used by the existing SkillHub expert-package installer.
pub(crate) const MAX_SKILLHUB_SKILL_ZIP_BYTES: u64 = MAX_MARKET_SKILL_ARCHIVE_BYTES;
/// Per-request timeout. The outer per-source budget
/// ([`super::MARKET_SOURCE_TIMEOUT`]) covers a primary + fallback pair.
pub(crate) const MARKET_REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// Redirect hop cap enforced by the custom policy.
const MAX_MARKET_REDIRECT_HOPS: usize = 5;

/// Hosts market requests may land on, including via redirects. Everything
/// else — notably internal addresses like `169.254.169.254`, `127.0.0.1`,
/// or RFC1918 hosts, which can never appear here — is refused.
const MARKET_ALLOWED_HOSTS: &[&str] = &[
    "clawhub.ai",
    "api.skillhub.cn",
    "skillhub.cn",
    "www.skills.sh",
    "skills.sh",
    "codeload.github.com",
    "api.cocoloop.cn",
    "hub.cocoloop.cn",
    "dl.cocoloop.cn",
    // SkillHub's official download endpoint redirects child-skill archives
    // to this exact Tencent COS acceleration host.
    "skillhub-1388575217.cos.accelerate.myqcloud.com",
    "wry-manatee-359.convex.cloud",
    "www.mcpworld.com",
];

/// SkillHub 302s `/api/v1/download` onto this dedicated Tencent COS bucket.
/// Only this bucket's COS hostnames are trusted — arbitrary `*.myqcloud.com`
/// buckets are not, because anyone can create one.
const SKILLHUB_COS_BUCKET_PREFIX: &str = "skillhub-1388575217.cos.";
const SKILLHUB_COS_HOST_SUFFIX: &str = ".myqcloud.com";

/// SSRF redirect guard predicate: exact allowlisted market host, or SkillHub's
/// dedicated COS download bucket (accelerate / regional).
fn is_allowed_market_host(host: &str) -> bool {
    MARKET_ALLOWED_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
        || is_skillhub_cos_download_host(host)
}

fn is_skillhub_cos_download_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let Some(rest) = host
        .strip_prefix(SKILLHUB_COS_BUCKET_PREFIX)
        .and_then(|h| h.strip_suffix(SKILLHUB_COS_HOST_SUFFIX))
    else {
        return false;
    };
    // "accelerate" or a region like "ap-guangzhou" — a single DNS label.
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Build the shared market HTTP client. Redirects are only followed when the
/// target host passes [`is_allowed_market_host`], capped at
/// [`MAX_MARKET_REDIRECT_HOPS`] hops; off-allowlist redirect targets fail the
/// request instead of being fetched.
pub(crate) fn build_market_client() -> Result<reqwest::Client, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"));

    let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_MARKET_REDIRECT_HOPS {
            return attempt.error("too many market redirects");
        }
        if attempt.url().scheme() == "https"
            && attempt.url().host_str().is_some_and(is_allowed_market_host)
        {
            attempt.follow()
        } else {
            attempt.error("market redirect target must use HTTPS and an allowlisted host")
        }
    });

    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(redirect_policy)
        .timeout(MARKET_REQUEST_TIMEOUT)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 NomiFun-SkillMarket/1.0",
        )
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// GET `url` and return its body as text, capped at [`MAX_MARKET_BODY_BYTES`].
pub(crate) async fn read_market_body(client: &reqwest::Client, url: &str) -> Result<String, AppError> {
    let mut response = client.get(url).send().await.map_err(map_market_fetch_error)?;
    read_market_response(&mut response).await
}

/// GET `url` with a caller-specific deadline. This is reserved for known
/// large market payloads; ordinary detail and archive requests keep the
/// tighter client-wide timeout.
pub(crate) async fn read_market_body_with_timeout(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<String, AppError> {
    let mut response = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(map_market_fetch_error)?;
    read_market_response(&mut response).await
}

/// GET a detail resource while preserving a real upstream 404 as
/// [`AppError::NotFound`]. Ranking pages use [`read_market_body`] because a
/// missing market page is an upstream integration failure, whereas a missing
/// detail resource means the selected entry was removed or became stale.
pub(crate) async fn read_market_detail_body(
    client: &reqwest::Client,
    url: &str,
    label: &str,
) -> Result<String, AppError> {
    let mut response = client.get(url).send().await.map_err(map_market_fetch_error)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::NotFound(format!("{label} not found")));
    }
    read_market_response(&mut response).await
}

/// POST a JSON `body` to `url` and return the response body as text, capped
/// at [`MAX_MARKET_BODY_BYTES`].
pub(crate) async fn read_market_json_post(
    client: &reqwest::Client,
    url: &str,
    body: serde_json::Value,
) -> Result<String, AppError> {
    let mut response = client.post(url).json(&body).send().await.map_err(map_market_fetch_error)?;
    read_market_response(&mut response).await
}

pub(crate) async fn read_market_response(response: &mut reqwest::Response) -> Result<String, AppError> {
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!("market page returned {}", response.status())));
    }
    if response.content_length().unwrap_or(0) > MAX_MARKET_BODY_BYTES {
        return Err(AppError::BadGateway("market response is too large".into()));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_market_fetch_error)? {
        if bytes.len().saturating_add(chunk.len()) as u64 > MAX_MARKET_BODY_BYTES {
            return Err(AppError::BadGateway("market response is too large".into()));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Drain a binary response (e.g. a skill zip) up to `max_bytes`, mapping 404
/// to [`AppError::NotFound`] so callers can fall back to a search.
pub(crate) async fn read_market_bytes(
    response: &mut reqwest::Response,
    max_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, AppError> {
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::NotFound(format!("{label} not found")));
    }
    if !status.is_success() {
        return Err(AppError::BadGateway(format!("{label} returned {status}")));
    }
    if response.content_length().unwrap_or(0) > max_bytes {
        return Err(AppError::BadGateway(format!("{label} is too large")));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_market_fetch_error)? {
        if bytes.len().saturating_add(chunk.len()) as u64 > max_bytes {
            return Err(AppError::BadGateway(format!("{label} is too large")));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

/// Per-attempt timeout for SkillHub archive downloads. The shared client
/// timeout (12s) covers the whole body stream, which is too tight for a
/// 32 MiB zip on a slow link; detail/metadata calls keep the client default.
pub(crate) const SKILLHUB_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);

/// Maximum attempts for one SkillHub request (first try plus retries).
const SKILLHUB_MAX_ATTEMPTS: u32 = 3;
/// Cap applied to a server-provided `Retry-After` hint.
const SKILLHUB_MAX_RETRY_AFTER: Duration = Duration::from_secs(5);

/// GET `url` with limited retry for transient upstream conditions: HTTP 429,
/// 5xx, connection errors, and timeouts. Deterministic answers (404 and other
/// 4xx) are returned to the caller immediately — retrying them only adds
/// latency. Every attempt builds a fresh request; a caller that streams the
/// response body must restart its own consumption per attempt.
///
/// When attempts are exhausted, the last result is returned as-is: an
/// exhausted 429/5xx reaches the caller as a non-success `Response`, and an
/// exhausted transport error as the mapped [`AppError`]. The caller owns the
/// non-success mapping — this helper never converts an exhausted status into
/// an error itself, so a persistent 429 costs exactly [`SKILLHUB_MAX_ATTEMPTS`]
/// requests and no more.
pub(crate) async fn send_skillhub_get_with_retry(
    client: &reqwest::Client,
    url: reqwest::Url,
    accept: &str,
    per_attempt_timeout: Duration,
) -> Result<reqwest::Response, AppError> {
    let mut attempt = 0_u32;
    loop {
        attempt += 1;
        let result = client
            .get(url.clone())
            .header(ACCEPT, accept)
            .timeout(per_attempt_timeout)
            .send()
            .await;
        let retryable = match &result {
            Ok(response) => is_retryable_market_status(response.status()),
            Err(error) => is_retryable_reqwest_error(error),
        };
        if !retryable || attempt >= SKILLHUB_MAX_ATTEMPTS {
            return result.map_err(map_market_fetch_error);
        }
        let delay = match &result {
            Ok(response) => retry_after_hint(response).unwrap_or_else(|| retry_backoff(attempt)),
            Err(_) => retry_backoff(attempt),
        };
        tokio::time::sleep(delay).await;
    }
}

fn is_retryable_market_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Transport-level failures worth one more attempt. Redirect-policy
/// rejections (`is_redirect`) and request-construction errors are
/// configuration/contract problems, so they are deliberately excluded.
fn is_retryable_reqwest_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

/// Parse a `Retry-After: <seconds>` hint, capped at [`SKILLHUB_MAX_RETRY_AFTER`].
fn retry_after_hint(response: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = response.headers().get(RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(SKILLHUB_MAX_RETRY_AFTER))
}

/// Exponential backoff with a small time-derived jitter: ~0.5s, ~1s, ~2s.
fn retry_backoff(attempt: u32) -> Duration {
    let base_ms = 500_u64 << attempt.saturating_sub(1).min(3);
    let jitter_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64
        % 250;
    Duration::from_millis(base_ms + jitter_ms)
}

pub(crate) fn map_market_fetch_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::Timeout(format!("skill market fetch timed out: {error}"))
    } else {
        AppError::BadGateway(format!("skill market fetch failed: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn spawn_test_response(
        status: &'static str,
        body: &'static str,
        body_delay: Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(headers.as_bytes()).await;
            tokio::time::sleep(body_delay).await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
        (format!("http://{address}/"), server)
    }

    #[test]
    fn market_host_allowlist_accepts_known_hosts() {
        for host in MARKET_ALLOWED_HOSTS {
            assert!(is_allowed_market_host(host), "{host}");
        }
        // Case-insensitive.
        assert!(is_allowed_market_host("ClawHub.AI"));
    }

    /// SkillHub 302s skill zips to a dedicated Tencent COS bucket. The
    /// redirect policy must follow that bucket's COS hosts or expert-package
    /// install fails for every child skill.
    #[test]
    fn market_host_allowlist_accepts_skillhub_cos_download_hosts() {
        for host in [
            "skillhub-1388575217.cos.accelerate.myqcloud.com",
            "skillhub-1388575217.cos.ap-guangzhou.myqcloud.com",
            "SkillHub-1388575217.COS.accelerate.myqcloud.com",
        ] {
            assert!(is_allowed_market_host(host), "{host}");
        }
    }

    /// SSRF redirect guard: internal addresses and off-allowlist hosts can
    /// never satisfy the redirect policy's host predicate.
    #[test]
    fn market_host_allowlist_rejects_internal_and_foreign_redirect_targets() {
        // Redirect-to-internal pivots (cloud metadata, loopback, RFC1918).
        for url in [
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1:8080/",
            "http://10.0.0.5/",
            "http://192.168.1.1/admin",
            "http://[::1]/",
        ] {
            let parsed = reqwest::Url::parse(url).unwrap();
            let host = parsed.host_str().expect("test URL has a host");
            assert!(!is_allowed_market_host(host), "{url} must be rejected");
        }

        // Off-allowlist public hosts are rejected too.
        for host in [
            "evil.example.com",
            "clawhub.ai.evil.com",
            "sub.skillhub.cn",
            "example.com",
            // Other people's COS buckets, and suffix / prefix tricks around the
            // SkillHub bucket hostname, must stay rejected.
            "evil.cos.accelerate.myqcloud.com",
            "not-skillhub-1388575217.cos.accelerate.myqcloud.com",
            "evil.skillhub-1388575217.cos.accelerate.myqcloud.com",
            "skillhub-1388575217.cos.accelerate.myqcloud.com.evil.com",
            "skillhub-1388575217.cos.accelerate.evil.myqcloud.com",
            "skillhub-1388575217.cos..myqcloud.com",
        ] {
            assert!(!is_allowed_market_host(host), "{host} must be rejected");
        }
    }

    #[test]
    fn build_market_client_succeeds() {
        assert!(build_market_client().is_ok());
    }

    #[tokio::test]
    async fn request_timeout_override_covers_delayed_response_body() {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();

        let (default_url, default_server) =
            spawn_test_response("200 OK", "{}", Duration::from_millis(150)).await;
        let error = read_market_body(&client, &default_url).await.unwrap_err();
        assert!(matches!(&error, AppError::Timeout(_)), "{error}");
        default_server.await.unwrap();

        let (override_url, override_server) =
            spawn_test_response("200 OK", "{}", Duration::from_millis(150)).await;
        let body = read_market_body_with_timeout(&client, &override_url, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(body, "{}");
        override_server.await.unwrap();
    }

    #[tokio::test]
    async fn detail_reader_preserves_not_found_and_rejects_other_errors() {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let (not_found_url, not_found_server) =
            spawn_test_response("404 Not Found", "missing", Duration::ZERO).await;
        let error = read_market_detail_body(&client, &not_found_url, "test package")
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::NotFound(_)), "{error}");
        not_found_server.await.unwrap();

        let (server_error_url, server_error) =
            spawn_test_response("500 Internal Server Error", "failed", Duration::ZERO).await;
        let error = read_market_detail_body(&client, &server_error_url, "test package")
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::BadGateway(_)), "{error}");
        server_error.await.unwrap();
    }

    // -----------------------------------------------------------------------
    // send_skillhub_get_with_retry
    // -----------------------------------------------------------------------

    struct ScriptedResponse {
        status: &'static str,
        extra_headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
    }

    /// Serve one scripted response per accepted connection and count requests.
    async fn spawn_scripted_server(
        responses: Vec<ScriptedResponse>,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let server = tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).await;
                let mut headers = format!("HTTP/1.1 {}\r\n", response.status);
                for (name, value) in response.extra_headers {
                    headers.push_str(&format!("{name}: {value}\r\n"));
                }
                headers.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    response.body.len()
                ));
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(response.body.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}"), hits, server)
    }

    fn scripted_url(base: &str) -> reqwest::Url {
        reqwest::Url::parse(&format!("{base}/resource")).unwrap()
    }

    #[tokio::test]
    async fn skillhub_retry_recovers_from_429_and_honors_retry_after() {
        use std::sync::atomic::Ordering;

        let (base, hits, server) = spawn_scripted_server(vec![
            ScriptedResponse {
                status: "429 Too Many Requests",
                extra_headers: vec![("Retry-After", "0")],
                body: "{\"error\":\"too many requests\"}",
            },
            ScriptedResponse {
                status: "200 OK",
                extra_headers: vec![],
                body: "{\"ok\":true}",
            },
        ])
        .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let response = send_skillhub_get_with_retry(
            &client,
            scripted_url(&base),
            "application/json",
            Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn skillhub_retry_does_not_retry_404() {
        use std::sync::atomic::Ordering;

        // Extra identical responses so a buggy retry would be observable.
        let (base, hits, _server) = spawn_scripted_server(vec![
            ScriptedResponse {
                status: "404 Not Found",
                extra_headers: vec![],
                body: "{\"error\":\"Skill not found\"}",
            },
            ScriptedResponse {
                status: "404 Not Found",
                extra_headers: vec![],
                body: "{\"error\":\"Skill not found\"}",
            },
        ])
        .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let response = send_skillhub_get_with_retry(
            &client,
            scripted_url(&base),
            "application/json",
            Duration::from_secs(2),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn skillhub_retry_exhausts_attempts_on_persistent_429() {
        use std::sync::atomic::Ordering;

        let responses = (0..SKILLHUB_MAX_ATTEMPTS)
            .map(|_| ScriptedResponse {
                status: "429 Too Many Requests",
                extra_headers: vec![("Retry-After", "0")],
                body: "{\"error\":\"too many requests\"}",
            })
            .collect();
        let (base, hits, server) = spawn_scripted_server(responses).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let response = send_skillhub_get_with_retry(
            &client,
            scripted_url(&base),
            "application/json",
            Duration::from_secs(2),
        )
        .await
        .unwrap();

        // The exhausted last response is handed to the caller, which owns the
        // non-success mapping.
        assert_eq!(response.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), SKILLHUB_MAX_ATTEMPTS as usize);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn skillhub_retry_retries_timeout_then_fails_as_timeout() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let server = tokio::spawn(async move {
            for _ in 0..SKILLHUB_MAX_ATTEMPTS {
                let (mut socket, _) = listener.accept().await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                let mut request = [0_u8; 1024];
                let _ = socket.read(&mut request).await;
                // Never respond: force every attempt into a timeout.
                tokio::time::sleep(Duration::from_millis(300)).await;
                drop(socket);
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let error = send_skillhub_get_with_retry(
            &client,
            scripted_url(&format!("http://{address}")),
            "application/json",
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();

        assert!(matches!(&error, AppError::Timeout(_)), "{error}");
        assert_eq!(hits.load(Ordering::SeqCst), SKILLHUB_MAX_ATTEMPTS as usize);
        server.await.unwrap();
    }
}
