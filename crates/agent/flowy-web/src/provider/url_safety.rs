use std::net::IpAddr;

use url::{Host, Url};

/// Reasons for which a URL must not be sent to a remote extract provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RemoteForbiddenReason {
    Unauthorized,
    Forbidden,
    NotFound,
    Gone,
    RateLimited,
    CaptchaOrWaf,
    LoginRequired,
    Paywall,
    SensitiveQuery,
    SensitiveFragment,
    CredentialsInUrl,
    PrivateOrLocalAddress,
    UnsupportedScheme,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalRequestedUrl(String);

impl CanonicalRequestedUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRemoteUrl {
    pub requested_url: String,
    pub outbound_url: String,
    pub canonical_url: CanonicalRequestedUrl,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RemoteUrlSafetyPolicy;

pub fn canonical_requested_url(value: &str) -> CanonicalRequestedUrl {
    let value = value.trim();
    let canonical = Url::parse(value).ok().map(|mut parsed| {
        parsed.set_fragment(None);
        let mut text = parsed.to_string();
        if parsed.path() == "/" && parsed.query().is_none() {
            text = text.trim_end_matches('/').to_owned();
        }
        text
    });
    CanonicalRequestedUrl(canonical.unwrap_or_else(|| {
        value
            .split('#')
            .next()
            .unwrap_or(value)
            .trim_end_matches('/')
            .to_owned()
    }))
}

/// Pure URL admission shared by Local SSRF validation and Remote egress.
/// DNS resolution remains owned by the Local HTTP provider; this function is
/// the repeatable synchronous boundary immediately before `tools/call`.
pub fn prepare_remote_url(
    raw: &str,
    allow_private: bool,
) -> Result<PreparedRemoteUrl, RemoteForbiddenReason> {
    RemoteUrlSafetyPolicy::prepare(raw, allow_private)
}

impl RemoteUrlSafetyPolicy {
    pub(crate) fn prepare(
        raw: &str,
        allow_private: bool,
    ) -> Result<PreparedRemoteUrl, RemoteForbiddenReason> {
        let raw = raw.trim();
        let url = Url::parse(raw).map_err(|_| RemoteForbiddenReason::UnsupportedScheme)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(RemoteForbiddenReason::UnsupportedScheme);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(RemoteForbiddenReason::CredentialsInUrl);
        }
        if !allow_private && is_forbidden_host(url.host()) {
            return Err(RemoteForbiddenReason::PrivateOrLocalAddress);
        }
        if has_sensitive_query(&url) {
            return Err(RemoteForbiddenReason::SensitiveQuery);
        }
        if has_sensitive_fragment(&url) {
            return Err(RemoteForbiddenReason::SensitiveFragment);
        }
        let mut outbound = url;
        outbound.set_fragment(None);
        Ok(PreparedRemoteUrl {
            requested_url: raw.to_owned(),
            outbound_url: outbound.to_string(),
            canonical_url: canonical_requested_url(raw),
        })
    }
}

pub(crate) fn is_forbidden_host(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Ipv4(address)) => is_forbidden_ip(&IpAddr::V4(address)),
        Some(Host::Ipv6(address)) => is_forbidden_ip(&IpAddr::V6(address)),
        Some(Host::Domain(domain)) => is_forbidden_domain(domain),
        None => true,
    }
}

pub(crate) fn is_forbidden_domain(domain: &str) -> bool {
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    matches!(
        domain.as_str(),
        "localhost"
            | "localhost.localdomain"
            | "local"
            | "internal"
            | "home.arpa"
            | "test"
            | "invalid"
            | "example"
    )
        || domain.ends_with(".localhost")
        || domain.ends_with(".localhost.localdomain")
        || domain.ends_with(".local")
        || domain.ends_with(".internal")
        || domain.ends_with(".home.arpa")
        || domain.ends_with(".test")
        || domain.ends_with(".invalid")
        || domain.ends_with(".example")
        || domain == "metadata.google.internal"
        || domain == "metadata.internal"
        || !domain.contains('.')
}

pub(crate) fn is_forbidden_ip(ip: &IpAddr) -> bool {
    nomifun_net::ssrf::is_blocked_target(ip)
}

fn has_sensitive_query(url: &Url) -> bool {
    url.query_pairs()
        .any(|(key, _)| is_sensitive_parameter_name(&key))
}

fn has_sensitive_fragment(url: &Url) -> bool {
    let Some(fragment) = url.fragment() else {
        return false;
    };
    url::form_urlencoded::parse(fragment.as_bytes())
        .any(|(key, _)| is_sensitive_parameter_name(&key))
}

fn is_sensitive_parameter_name(name: &str) -> bool {
    let chars = name.chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(name.len());
    for (index, ch) in chars.iter().copied().enumerate() {
        let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(index + 1).copied();
        if ch.is_ascii_uppercase()
            && previous.is_some_and(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
            || ch.is_ascii_uppercase()
                && previous.is_some_and(|value| value.is_ascii_uppercase())
                && next.is_some_and(|value| value.is_ascii_lowercase())
        {
            normalized.push('_');
        }
        normalized.push(if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        });
    }
    let compact = normalized.replace('_', "");
    const HIGH_SIGNAL: &[&str] = &[
        "token",
        "secret",
        "password",
        "passwd",
        "pwd",
        "credential",
        "authorization",
        "signature",
        "apikey",
        "jwt",
        "session",
        "assertion",
        "samlresponse",
        "samlrequest",
    ];
    if HIGH_SIGNAL.iter().any(|needle| compact.contains(needle)) {
        return true;
    }
    if compact.starts_with("xamz") || compact.starts_with("xgoog") {
        return true;
    }
    let components = normalized.split('_').collect::<Vec<_>>();
    components.iter().any(|component| {
        matches!(
            *component,
            "key" | "auth" | "code" | "ticket" | "sig" | "expires"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_special_use_domains_and_trailing_dots() {
        for host in [
            "localhost.",
            "foo.localhost",
            "foo.localhost.localdomain",
            "localhost.localdomain",
            "local",
            "service.local",
            "internal",
            "service.internal",
            "home.arpa",
            "router.home.arpa",
            "test",
            "invalid",
            "example",
            "service.test",
            "service.invalid",
            "service.example",
            "printer",
            "metadata.google.internal",
        ] {
            assert_eq!(
                prepare_remote_url(&format!("https://{host}/file.pdf"), false),
                Err(RemoteForbiddenReason::PrivateOrLocalAddress),
                "{host} must remain local-only"
            );
        }
    }

    #[test]
    fn rejects_private_ip_ranges_and_mapped_ipv4() {
        for url in [
            "https://127.0.0.1/file.pdf",
            "https://100.64.0.1/file.pdf",
            "https://192.0.0.1/file.pdf",
            "https://[::ffff:127.0.0.1]/file.pdf",
            "https://[2001:db8::1]/file.pdf",
            "https://[2001:2::1]/file.pdf",
            "https://[::192.0.2.1]/file.pdf",
        ] {
            assert_eq!(
                prepare_remote_url(url, false),
                Err(RemoteForbiddenReason::PrivateOrLocalAddress),
                "{url} must remain local-only"
            );
        }
    }

    #[test]
    fn accepts_tunnel_fake_ip_literals() {
        // TUN-mode VPN clients map DNS answers into these ranges; blocking them
        // makes every fetch fail on a machine behind such a tunnel.
        for url in ["https://198.18.0.24/file.pdf", "https://240.0.0.1/file.pdf"] {
            assert!(
                prepare_remote_url(url, false).is_ok(),
                "{url} must remain reachable"
            );
        }
    }

    #[test]
    fn rejects_sensitive_query_name_variants() {
        for key in [
            "code",
            "OAuth_Code",
            "%63ode",
            "download-token",
            "clientSecret",
            "oauthCode",
            "loginTicket",
            "clientKey",
            "AWSAccessKeyId",
            "SAMLRequest",
            "password",
            "refresh_token",
            "jwt",
            "ticket",
            "x-amz-signature",
        ] {
            let url = format!("https://example.com/file.pdf?{key}=value");
            assert_eq!(
                prepare_remote_url(&url, false),
                Err(RemoteForbiddenReason::SensitiveQuery),
                "{key} must remain local-only"
            );
        }
    }

    #[test]
    fn allows_ordinary_query_and_strips_plain_fragment() {
        let prepared = prepare_remote_url(
            "https://example.com/file.pdf?filename=tryhtml_default#section",
            false,
        )
        .expect("ordinary query should remain supported");
        assert_eq!(
            prepared.outbound_url,
            "https://example.com/file.pdf?filename=tryhtml_default"
        );
    }

    #[test]
    fn rejects_sensitive_fragment_name_variants() {
        for fragment in [
            "#access_token=value",
            "#oauth_code=value",
            "#clientSecret=value",
            "#access%5Ftoken=value",
            "#client%53ecret=value",
        ] {
            assert_eq!(
                prepare_remote_url(&format!("https://example.com/file.pdf{fragment}"), false),
                Err(RemoteForbiddenReason::SensitiveFragment),
                "{fragment} must remain local-only"
            );
        }
    }
}
