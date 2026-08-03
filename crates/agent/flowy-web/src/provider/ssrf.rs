use std::net::{IpAddr, SocketAddr};
use url::{Host, Url};

use crate::provider::url_safety::{is_forbidden_domain, is_forbidden_ip};
use crate::types::WebError;

/// Parse, scheme-check, and DNS-validate a URL before HTTP extract.
///
/// Thin wrapper over [`resolve_extract_url`] that discards the pinned addrs
/// (kept for callers that only need the validated [`Url`]).
pub async fn validate_extract_url(raw: &str, allow_private: bool) -> Result<Url, WebError> {
    let (url, _addrs) = resolve_extract_url(raw, allow_private).await?;
    Ok(url)
}

/// Parse + SSRF-validate a URL and return the resolved socket addresses for
/// connection pinning (`ClientBuilder::resolve_to_addrs`).
pub async fn resolve_extract_url(
    raw: &str,
    allow_private: bool,
) -> Result<(Url, Vec<SocketAddr>), WebError> {
    let url = parse_extract_url(raw)?;
    let addrs = resolve_validated(&url, allow_private).await?;
    Ok((url, addrs))
}

fn parse_extract_url(raw: &str) -> Result<Url, WebError> {
    let url = Url::parse(raw.trim())
        .map_err(|e| WebError::InvalidArgument(format!("invalid URL: {e}")))?;
    check_scheme(url)
}

pub(crate) fn check_scheme(url: Url) -> Result<Url, WebError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(WebError::InvalidArgument(format!(
            "only http(s) URLs are supported (got scheme: {})",
            url.scheme()
        )));
    }
    if url.host_str().is_none() {
        return Err(WebError::InvalidArgument("URL has no host".into()));
    }
    Ok(url)
}

pub(crate) async fn resolve_validated(
    url: &Url,
    allow_private: bool,
) -> Result<Vec<SocketAddr>, WebError> {
    let host = url
        .host_str()
        .ok_or_else(|| WebError::InvalidArgument("URL has no host".into()))?;

    if !allow_private && is_forbidden_domain(host) {
        return Err(WebError::BlockedUrl(format!("URL host {host} is blocked")));
    }

    let port = url.port_or_known_default().unwrap_or(443);

    if !allow_private
        && let Some(literal) = url.host().and_then(host_ip)
        && is_forbidden_ip(&literal)
    {
        return Err(WebError::BlockedUrl(format!(
            "URL host {host} is a private or local address; fetching it is blocked"
        )));
    }

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| WebError::Network(format!("DNS resolution failed for {host}: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(WebError::Network(format!(
            "DNS resolution returned no addresses for {host}"
        )));
    }
    if !allow_private && let Some(bad) = addrs.iter().find(|a| is_forbidden_ip(&a.ip())) {
        return Err(WebError::BlockedUrl(format!(
            "URL host {host} resolves to a private or local address ({}); fetching it is blocked",
            bad.ip()
        )));
    }
    Ok(addrs)
}

fn host_ip(host: Host<&str>) -> Option<IpAddr> {
    match host {
        Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        Host::Domain(_) => None,
    }
}

/// SSRF address policy: anything not unambiguously public is forbidden.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_localhost() {
        let err = validate_extract_url("http://localhost/x", false)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::types::WebError::BlockedUrl(_)));
    }

    #[tokio::test]
    async fn rejects_rfc1918_literal() {
        let err = validate_extract_url("http://192.168.1.1/", false)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::types::WebError::BlockedUrl(_)));
    }

    #[tokio::test]
    async fn rejects_non_http_scheme() {
        let err = validate_extract_url("file:///etc/passwd", false)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::types::WebError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn accepts_public_https_example() {
        match validate_extract_url("https://example.com/", false).await {
            Ok(u) => assert_eq!(u.scheme(), "https"),
            Err(crate::types::WebError::BlockedUrl(_)) => panic!("example.com must not be blocked"),
            Err(crate::types::WebError::Network(_)) => {
                // offline / DNS flake — acceptable
            }
            Err(e) => panic!("unexpected: {e}"),
        }
    }
}
