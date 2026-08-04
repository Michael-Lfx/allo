use crate::provider::http_extract::FetchedResource;
use crate::provider::document_kind::{DeferredDocumentKind, classify_document};
use crate::types::{ExtractedPage, WebError};

pub use crate::provider::url_safety::{
    CanonicalRequestedUrl, PreparedRemoteUrl, RemoteForbiddenReason, canonical_requested_url,
    prepare_remote_url,
};

#[derive(Debug)]
pub struct LocalExtractOutcome {
    pub requested_url: String,
    pub result: Result<ExtractedPage, LocalExtractFailure>,
    pub diagnostics: LocalExtractDiagnostics,
}

#[derive(Debug, Clone)]
pub struct LocalExtractFailure {
    pub kind: LocalExtractFailureKind,
    pub error: WebError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalExtractFailureKind {
    Pdf,
    UnsupportedDocument,
    JavascriptShell,
    EmptyContent,
    AccessChallenge,
    LoginRequired,
    Paywall,
    Dns,
    Tls,
    Network,
    Parse,
    Timeout,
    HttpStatus(u16),
    InvalidUrl,
    BlockedAddress,
    Cancelled,
}

#[derive(Debug, Clone, Default)]
pub struct LocalExtractDiagnostics {
    pub content_type: Option<String>,
    pub http_status: Option<u16>,
    pub body_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteFallbackDecision {
    NotNeeded,
    Eligible {
        reason: RemoteFallbackReason,
    },
    /// The Local failure is safe to classify, but the active rollout profile
    /// deliberately does not send this category to the remote provider yet.
    Deferred {
        reason: RemoteDeferredReason,
    },
    Forbidden {
        reason: RemoteForbiddenReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RemoteFallbackReason {
    Pdf,
    UnsupportedDocument,
    JavascriptShell,
    EmptyContent,
    TransientNetwork,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RemoteDeferredReason {
    ProfileNotEnabled(RemoteFallbackReason),
    ProviderUnsupported(RemoteFallbackReason),
    BudgetInsufficient(RemoteFallbackReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemoteExtractCapabilities {
    supported: &'static [RemoteFallbackReason],
}

impl RemoteExtractCapabilities {
    pub const fn evidence_backed() -> Self {
        Self {
            supported: &[
                RemoteFallbackReason::Pdf,
                RemoteFallbackReason::JavascriptShell,
                RemoteFallbackReason::EmptyContent,
            ],
        }
    }

    #[allow(dead_code)]
    #[cfg(any(test, feature = "fetch-eval"))]
    pub const fn all_eligible() -> Self {
        Self {
            supported: &[
                RemoteFallbackReason::Pdf,
                RemoteFallbackReason::UnsupportedDocument,
                RemoteFallbackReason::JavascriptShell,
                RemoteFallbackReason::EmptyContent,
                RemoteFallbackReason::TransientNetwork,
                RemoteFallbackReason::Timeout,
            ],
        }
    }

    pub fn supports(self, reason: RemoteFallbackReason) -> bool {
        self.supported.contains(&reason)
    }
}

/// Remote fallback profile owned by the extraction policy module.
///
/// `AllEligible` preserves the broad policy used by evaluation and internal
/// tests. Production Desktop uses `EvidenceBacked`, whose small allow-list is
/// based on the completed PDF and JavaScript Provider evidence.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteFallbackProfile {
    EvidenceBacked,
    #[cfg(any(test, feature = "fetch-eval"))]
    AllEligible,
}

impl RemoteFallbackProfile {
    #[allow(dead_code)]
    #[cfg(any(test, feature = "fetch-eval"))]
    pub const fn all_eligible() -> Self {
        Self::AllEligible
    }

    pub const fn evidence_backed() -> Self {
        Self::EvidenceBacked
    }

    pub const fn allows(self, reason: RemoteFallbackReason) -> bool {
        match self {
            Self::EvidenceBacked => matches!(
                reason,
                RemoteFallbackReason::Pdf
                    | RemoteFallbackReason::JavascriptShell
                    | RemoteFallbackReason::EmptyContent
            ),
            #[cfg(any(test, feature = "fetch-eval"))]
            Self::AllEligible => true,
        }
    }

    #[cfg(test)]
    pub fn decide(
        self,
        outcome: &LocalExtractOutcome,
        allow_private: bool,
    ) -> RemoteFallbackDecision {
        self.decide_with_capabilities(
            outcome,
            allow_private,
            RemoteExtractCapabilities::evidence_backed(),
        )
    }

    pub fn decide_with_capabilities(
        self,
        outcome: &LocalExtractOutcome,
        allow_private: bool,
        capabilities: RemoteExtractCapabilities,
    ) -> RemoteFallbackDecision {
        match decide_remote_fallback_with_private(outcome, allow_private) {
            RemoteFallbackDecision::Eligible { reason } if !self.allows(reason) => {
                RemoteFallbackDecision::Deferred {
                    reason: RemoteDeferredReason::ProfileNotEnabled(reason),
                }
            }
            RemoteFallbackDecision::Eligible { reason } if !capabilities.supports(reason) => {
                RemoteFallbackDecision::Deferred {
                    reason: RemoteDeferredReason::ProviderUnsupported(reason),
                }
            }
            decision => decision,
        }
    }
}

pub(crate) type RemoteFallbackPolicy = RemoteFallbackProfile;

pub fn successful_outcome(
    requested_url: String,
    resource: &FetchedResource,
    page: ExtractedPage,
) -> LocalExtractOutcome {
    let diagnostics = LocalExtractDiagnostics {
        content_type: resource.content_type.clone(),
        http_status: Some(resource.status.as_u16()),
        body_truncated: resource.body_truncated,
    };
    let result = classify_success(resource, &page).map_or_else(
        || Ok(page),
        |kind| {
            Err(LocalExtractFailure {
                kind,
                error: WebError::Provider(format!(
                    "local extract could not produce usable content for {requested_url}"
                )),
            })
        },
    );
    LocalExtractOutcome {
        requested_url,
        result,
        diagnostics,
    }
}

pub fn failed_outcome(
    requested_url: String,
    error: WebError,
    diagnostics: LocalExtractDiagnostics,
) -> LocalExtractOutcome {
    let kind = diagnostics
        .http_status
        .map(LocalExtractFailureKind::HttpStatus)
        .unwrap_or_else(|| classify_web_error(&error));
    LocalExtractOutcome {
        requested_url,
        result: Err(LocalExtractFailure { kind, error }),
        diagnostics,
    }
}

pub fn decide_remote_fallback(
    outcome: &LocalExtractOutcome,
) -> RemoteFallbackDecision {
    decide_remote_fallback_with_private(outcome, false)
}

pub fn decide_remote_fallback_with_private(
    outcome: &LocalExtractOutcome,
    allow_private: bool,
) -> RemoteFallbackDecision {
    if outcome.result.is_ok() {
        return RemoteFallbackDecision::NotNeeded;
    }
    if let Some(reason) = forbidden_url_reason_with_private(&outcome.requested_url, allow_private) {
        return RemoteFallbackDecision::Forbidden { reason };
    }
    let failure = outcome
        .result
        .as_ref()
        .expect_err("non-ok result has a local failure");
    match failure.kind {
        LocalExtractFailureKind::Pdf => RemoteFallbackDecision::Eligible {
            reason: RemoteFallbackReason::Pdf,
        },
        LocalExtractFailureKind::UnsupportedDocument => {
            RemoteFallbackDecision::Eligible {
                reason: RemoteFallbackReason::UnsupportedDocument,
            }
        }
        LocalExtractFailureKind::JavascriptShell => RemoteFallbackDecision::Eligible {
            reason: RemoteFallbackReason::JavascriptShell,
        },
        LocalExtractFailureKind::EmptyContent => RemoteFallbackDecision::Eligible {
            reason: RemoteFallbackReason::EmptyContent,
        },
        LocalExtractFailureKind::AccessChallenge => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::CaptchaOrWaf,
        },
        LocalExtractFailureKind::LoginRequired => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::LoginRequired,
        },
        LocalExtractFailureKind::Paywall => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Paywall,
        },
        LocalExtractFailureKind::Dns
        | LocalExtractFailureKind::Tls
        | LocalExtractFailureKind::Network => RemoteFallbackDecision::Eligible {
            reason: RemoteFallbackReason::TransientNetwork,
        },
        LocalExtractFailureKind::Parse => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Forbidden,
        },
        LocalExtractFailureKind::Timeout => RemoteFallbackDecision::Eligible {
            reason: RemoteFallbackReason::Timeout,
        },
        LocalExtractFailureKind::HttpStatus(401) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Unauthorized,
        },
        LocalExtractFailureKind::HttpStatus(403) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Forbidden,
        },
        LocalExtractFailureKind::HttpStatus(404) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::NotFound,
        },
        LocalExtractFailureKind::HttpStatus(410) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Gone,
        },
        LocalExtractFailureKind::HttpStatus(429) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::RateLimited,
        },
        LocalExtractFailureKind::HttpStatus(400) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Forbidden,
        },
        LocalExtractFailureKind::HttpStatus(_) => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::Forbidden,
        },
        LocalExtractFailureKind::InvalidUrl => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::UnsupportedScheme,
        },
        LocalExtractFailureKind::BlockedAddress => RemoteFallbackDecision::Forbidden {
            reason: RemoteForbiddenReason::PrivateOrLocalAddress,
        },
        LocalExtractFailureKind::Cancelled => RemoteFallbackDecision::NotNeeded,
    }
}

fn classify_success(
    resource: &FetchedResource,
    page: &ExtractedPage,
) -> Option<LocalExtractFailureKind> {
    if let Some(kind) = classify_document(resource.content_type.as_deref(), &resource.body) {
        return Some(match kind {
            DeferredDocumentKind::Pdf => LocalExtractFailureKind::Pdf,
            DeferredDocumentKind::UnsupportedDocument => {
                LocalExtractFailureKind::UnsupportedDocument
            }
        });
    }
    if let Some(kind) = classify_access_challenge(resource, page) {
        return Some(kind);
    }
    if is_javascript_shell(resource, page) {
        return Some(LocalExtractFailureKind::JavascriptShell);
    }
    if page.markdown.trim().is_empty() {
        return Some(LocalExtractFailureKind::EmptyContent);
    }
    None
}

pub fn classify_web_error(error: &WebError) -> LocalExtractFailureKind {
    match error {
        WebError::InvalidArgument(_) => LocalExtractFailureKind::InvalidUrl,
        WebError::BlockedUrl(_) => LocalExtractFailureKind::BlockedAddress,
        WebError::Timeout(_) => LocalExtractFailureKind::Timeout,
        WebError::Network(message) => {
            let message = message.to_ascii_lowercase();
            if message.contains("dns") {
                LocalExtractFailureKind::Dns
            } else if message.contains("tls") || message.contains("certificate") {
                LocalExtractFailureKind::Tls
            } else {
                LocalExtractFailureKind::Network
            }
        }
        WebError::Provider(message) => parse_http_status(message)
            .map(LocalExtractFailureKind::HttpStatus)
            .unwrap_or(LocalExtractFailureKind::Network),
        WebError::Parse(_) => LocalExtractFailureKind::Parse,
    }
}

fn classify_access_challenge(
    resource: &FetchedResource,
    page: &ExtractedPage,
) -> Option<LocalExtractFailureKind> {
    let visible = page.markdown.trim();
    let visible_chars = visible.chars().count();
    let body = String::from_utf8_lossy(&resource.body);
    let lower = body.to_ascii_lowercase();

    const STRONG_CAPTCHA_MARKERS: &[&str] = &[
        "cf-chl-",
        "cf-challenge",
        "challenge-platform",
        "cloudflare challenge",
        "hcaptcha iframe",
        "hcaptcha script",
        "hcaptcha.com",
        "recaptcha iframe",
        "recaptcha script",
        "recaptcha/api",
        "gstatic.com/recaptcha",
        "captcha challenge",
        "checking your browser",
        "verify you are human",
        "attention required",
    ];
    if STRONG_CAPTCHA_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return Some(LocalExtractFailureKind::AccessChallenge);
    }

    if visible_chars >= 400 {
        return None;
    }

    const LOGIN_PHRASES: &[&str] = &["sign in", "log in", "authenticate", "authentication"];
    let has_password_input = lower.contains("type=\"password\"")
        || lower.contains("type='password'")
        || lower.contains("name=\"password\"")
        || lower.contains("name='password'");
    if has_password_input && LOGIN_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return Some(LocalExtractFailureKind::LoginRequired);
    }

    const PAYWALL_OVERLAY_MARKERS: &[&str] = &[
        "class=\"paywall\"",
        "class='paywall'",
        "id=\"paywall\"",
        "id='paywall'",
        "paywall overlay",
        "subscription overlay",
    ];
    const PAYWALL_PHRASES: &[&str] = &[
        "subscribe to continue",
        "premium content",
    ];
    if PAYWALL_OVERLAY_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
        && PAYWALL_PHRASES
            .iter()
            .any(|phrase| lower.contains(phrase))
    {
        return Some(LocalExtractFailureKind::Paywall);
    }
    None
}

fn is_javascript_shell(resource: &FetchedResource, page: &ExtractedPage) -> bool {
    let visible = page.markdown.trim();
    if visible.chars().count() >= 400 {
        return false;
    }
    let body = String::from_utf8_lossy(&resource.body);
    let lower = body.to_ascii_lowercase();
    let explicit = [
        "enable javascript",
        "javascript is required",
        "js is required",
        "please enable javascript",
        "your browser does not support javascript",
    ];
    if explicit.iter().any(|phrase| lower.contains(phrase)) {
        return true;
    }
    let root_container = lower.contains("<div id=\"root\"")
        || lower.contains("<div id=\"app\"")
        || lower.contains("<div id=\"__next\"");
    if root_container
        && visible.is_empty()
        && lower.contains("<script")
    {
        return true;
    }
    if root_container
        && visible.chars().count() < 80
        && (lower.contains("<script")
            || explicit.iter().any(|phrase| lower.contains(phrase)))
    {
        return true;
    }
    script_dominance(&lower) > 0.35
}

fn script_dominance(lower: &str) -> f64 {
    let mut script_bytes = 0usize;
    let mut inside_script = false;
    let mut index = 0usize;
    while index < lower.len() {
        if !inside_script {
            let Some(relative) = lower[index..].find("<script") else {
                break;
            };
            index += relative;
            inside_script = true;
        } else if let Some(relative) = lower[index..].find("</script") {
            script_bytes += relative + "</script>".len();
            index += relative + "</script>".len();
            inside_script = false;
        } else {
            script_bytes += lower.len() - index;
            index = lower.len();
        }
    }
    if lower.is_empty() {
        0.0
    } else {
        script_bytes as f64 / lower.len() as f64
    }
}

fn parse_http_status(message: &str) -> Option<u16> {
    message
        .split("HTTP ")
        .nth(1)?
        .split(' ')
        .next()?
        .parse()
        .ok()
}

fn forbidden_url_reason_with_private(
    raw: &str,
    allow_private: bool,
) -> Option<RemoteForbiddenReason> {
    prepare_remote_url(raw, allow_private).err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EXTRACTOR_FULLPAGE;
    use url::Url;

    fn page(markdown: &str) -> ExtractedPage {
        ExtractedPage {
            url: "https://example.com/".to_owned(),
            title: Some("Title".to_owned()),
            markdown: markdown.to_owned(),
            truncated: false,
            provider: "http".to_owned(),
            extractor: EXTRACTOR_FULLPAGE.to_owned(),
        }
    }

    fn resource(content_type: Option<&str>, body: Vec<u8>) -> FetchedResource {
        FetchedResource {
            requested_url: Url::parse("https://example.com/").unwrap(),
            final_url: Url::parse("https://example.com/").unwrap(),
            status: reqwest::StatusCode::OK,
            content_type: content_type.map(ToOwned::to_owned),
            body,
            body_truncated: false,
        }
    }

    fn failure(requested_url: &str, kind: LocalExtractFailureKind) -> LocalExtractOutcome {
        LocalExtractOutcome {
            requested_url: requested_url.to_owned(),
            result: Err(LocalExtractFailure {
                kind,
                error: WebError::Provider("local".to_owned()),
            }),
            diagnostics: LocalExtractDiagnostics::default(),
        }
    }

    #[test]
    fn local_success_never_falls_back_even_with_sensitive_query() {
        let resource = resource(Some("text/html"), b"<html>ok</html>".to_vec());
        let outcome = successful_outcome(
            "https://example.com/path?token=abc".to_owned(),
            &resource,
            page("usable content"),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn pdf_is_remote_eligible() {
        let outcome = failure("https://example.com/a.pdf", LocalExtractFailureKind::Pdf);
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Eligible {
                reason: RemoteFallbackReason::Pdf
            }
        ));
    }

    #[test]
    fn javascript_shell_is_remote_eligible() {
        let resource = resource(
            Some("text/html"),
            b"<html><div id=\"root\"></div><script src=\"/app.js\"></script></html>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/".to_owned(),
            &resource,
            page("Loading"),
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Eligible {
                reason: RemoteFallbackReason::JavascriptShell
            }
        ));
    }

    #[test]
    fn empty_content_is_remote_eligible() {
        let outcome = failure(
            "https://example.com/",
            LocalExtractFailureKind::EmptyContent,
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Eligible {
                reason: RemoteFallbackReason::EmptyContent
            }
        ));
    }

    #[test]
    fn evidence_backed_policy_allows_only_qualified_categories() {
        let policy = RemoteFallbackPolicy::evidence_backed();
        for (url, kind, reason) in [
            (
                "https://example.com/a.pdf",
                LocalExtractFailureKind::Pdf,
                RemoteFallbackReason::Pdf,
            ),
            (
                "https://example.com/app",
                LocalExtractFailureKind::JavascriptShell,
                RemoteFallbackReason::JavascriptShell,
            ),
            (
                "https://example.com/empty",
                LocalExtractFailureKind::EmptyContent,
                RemoteFallbackReason::EmptyContent,
            ),
        ] {
            assert_eq!(
                policy.decide(&failure(url, kind), false),
                RemoteFallbackDecision::Eligible { reason }
            );
        }
    }

    #[test]
    fn evidence_backed_policy_defers_unqualified_transient_failures() {
        let policy = RemoteFallbackPolicy::evidence_backed();
        for kind in [
            LocalExtractFailureKind::Dns,
            LocalExtractFailureKind::Tls,
            LocalExtractFailureKind::Network,
            LocalExtractFailureKind::Timeout,
            LocalExtractFailureKind::UnsupportedDocument,
        ] {
            let reason = match kind {
                LocalExtractFailureKind::Dns
                | LocalExtractFailureKind::Tls
                | LocalExtractFailureKind::Network => RemoteFallbackReason::TransientNetwork,
                LocalExtractFailureKind::Timeout => RemoteFallbackReason::Timeout,
                LocalExtractFailureKind::UnsupportedDocument => {
                    RemoteFallbackReason::UnsupportedDocument
                }
                _ => unreachable!(),
            };
            assert_eq!(
                policy.decide(&failure("https://example.com/", kind), false),
                RemoteFallbackDecision::Deferred {
                    reason: RemoteDeferredReason::ProfileNotEnabled(reason),
                }
            );
        }
    }

    #[test]
    fn auth_and_missing_statuses_are_forbidden() {
        for (status, expected) in [
            (401, RemoteForbiddenReason::Unauthorized),
            (403, RemoteForbiddenReason::Forbidden),
            (404, RemoteForbiddenReason::NotFound),
            (410, RemoteForbiddenReason::Gone),
            (429, RemoteForbiddenReason::RateLimited),
        ] {
            let outcome = failure(
                "https://example.com/",
                LocalExtractFailureKind::HttpStatus(status),
            );
            assert_eq!(
                decide_remote_fallback(&outcome),
                RemoteFallbackDecision::Forbidden { reason: expected }
            );
        }
    }

    #[test]
    fn sensitive_urls_are_remote_forbidden_but_local_allowed() {
        for url in [
            "https://example.com/?token=abc",
            "https://example.com/?X-Amz-Signature=abc",
            "https://example.com/?X-Goog-Credential=abc",
            "https://user:pass@example.com/",
            "https://localhost/",
            "file:///etc/passwd",
        ] {
            let outcome = failure(url, LocalExtractFailureKind::Network);
            assert!(
                matches!(
                    decide_remote_fallback(&outcome),
                    RemoteFallbackDecision::Forbidden { .. }
                ),
                "{url} must be forbidden"
            );
        }
    }

    #[test]
    fn transient_network_and_timeout_are_remote_eligible() {
        for kind in [
            LocalExtractFailureKind::Dns,
            LocalExtractFailureKind::Tls,
            LocalExtractFailureKind::Network,
            LocalExtractFailureKind::Timeout,
        ] {
            let outcome = failure("https://example.com/", kind);
            assert!(
                matches!(
                    decide_remote_fallback(&outcome),
                    RemoteFallbackDecision::Eligible { .. }
                ),
                "{kind:?} must be eligible"
            );
        }
    }

    #[test]
    fn unsupported_document_is_eligible() {
        let resource = resource(
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            b"PK".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/report.docx".to_owned(),
            &resource,
            page(""),
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Eligible {
                reason: RemoteFallbackReason::UnsupportedDocument
            }
        ));
    }

    #[test]
    fn short_static_page_is_not_javascript_shell() {
        let resource = resource(
            Some("text/html"),
            b"<html><body><p>Hello world.</p></body></html>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/".to_owned(),
            &resource,
            page("Hello world."),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn short_static_page_with_root_container_is_not_javascript_shell() {
        let resource = resource(
            Some("text/html"),
            b"<html><body><div id=\"app\"><p>Hello world.</p></div></body></html>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/".to_owned(),
            &resource,
            page("Hello world."),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn http_status_is_extracted_from_provider_message() {
        assert_eq!(
            classify_web_error(&WebError::Provider(
                "fetch failed: HTTP 404 for https://example.com".to_owned()
            )),
            LocalExtractFailureKind::HttpStatus(404)
        );
    }

    #[test]
    fn failed_outcome_prefers_structured_http_status() {
        let outcome = failed_outcome(
            "https://example.com/".to_owned(),
            WebError::Parse("parse failed".to_owned()),
            LocalExtractDiagnostics {
                http_status: Some(404),
                ..LocalExtractDiagnostics::default()
            },
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::HttpStatus(404)
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden {
                reason: RemoteForbiddenReason::NotFound
            }
        ));
    }

    #[test]
    fn parse_failure_is_remote_forbidden() {
        let outcome = failed_outcome(
            "https://example.com/".to_owned(),
            WebError::Parse("invalid html".to_owned()),
            LocalExtractDiagnostics::default(),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::Parse
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden { .. }
        ));
    }

    #[test]
    fn http_400_is_forbidden_but_not_mechanically_captcha() {
        let outcome = failure("https://example.com/", LocalExtractFailureKind::HttpStatus(400));
        assert_eq!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden {
                reason: RemoteForbiddenReason::Forbidden
            }
        );
    }

    #[test]
    fn plain_fragment_is_stripped_and_sensitive_fragment_is_forbidden() {
        let prepared = prepare_remote_url("https://example.com/page#section-2", false)
            .expect("plain fragment should be allowed");
        assert_eq!(prepared.outbound_url, "https://example.com/page");
        assert_eq!(
            prepared.requested_url,
            "https://example.com/page#section-2"
        );
        assert!(matches!(
            prepare_remote_url("https://example.com/callback#access_token=secret", false),
            Err(RemoteForbiddenReason::SensitiveFragment)
        ));
        assert!(matches!(
            prepare_remote_url("https://example.com/callback#TOKEN=secret", false),
            Err(RemoteForbiddenReason::SensitiveFragment)
        ));
    }

    #[test]
    fn canonical_url_only_normalizes_root_slash() {
        assert_eq!(
            canonical_requested_url("https://example.com/").as_str(),
            "https://example.com"
        );
        assert_eq!(
            canonical_requested_url("https://example.com/a/").as_str(),
            "https://example.com/a/"
        );
    }

    #[test]
    fn captcha_waf_challenge_is_remote_forbidden() {
        let resource = resource(
            Some("text/html"),
            b"<html><body>Checking your browser before accessing...</body></html>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/".to_owned(),
            &resource,
            page("Checking your browser"),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::AccessChallenge
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden {
                reason: RemoteForbiddenReason::CaptchaOrWaf
            }
        ));
    }

    #[test]
    fn login_page_is_remote_forbidden() {
        let resource = resource(
            Some("text/html"),
            b"<form><input type=\"password\" name=\"password\" /></form>Sign in".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/login".to_owned(),
            &resource,
            page("Sign in"),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::LoginRequired
        );
    }

    #[test]
    fn paywall_page_is_remote_forbidden() {
        let resource = resource(
            Some("text/html"),
            b"<div class=\"paywall\">Subscribe to continue reading</div>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/article".to_owned(),
            &resource,
            page("Premium Content"),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::Paywall
        );
    }

    #[test]
    fn full_articles_with_access_control_links_are_not_blocked() {
        let resource = resource(
            Some("text/html"),
            b"<a href=\"/login\">Sign in</a><article>Article text</article>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/article".to_owned(),
            &resource,
            page(&"Article ".repeat(60)),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn captcha_technical_article_is_not_blocked() {
        let resource = resource(
            Some("text/html"),
            b"<article>How recaptcha works</article>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/docs".to_owned(),
            &resource,
            page(&"This article explains recaptcha ".repeat(10)),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn medium_cloudflare_challenge_is_remote_forbidden() {
        let resource = resource(
            Some("text/html"),
            b"<div class=\"cf-chl-container\">Checking your browser...</div>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/challenge".to_owned(),
            &resource,
            page(&"C".repeat(150)),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::AccessChallenge
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden {
                reason: RemoteForbiddenReason::CaptchaOrWaf
            }
        ));
    }

    #[test]
    fn medium_login_form_is_remote_forbidden() {
        let resource = resource(
            Some("text/html"),
            b"<form><input type=\"password\" /></form>Sign in to continue".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/login".to_owned(),
            &resource,
            page(&"L".repeat(250)),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::LoginRequired
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden {
                reason: RemoteForbiddenReason::LoginRequired
            }
        ));
    }

    #[test]
    fn medium_paywall_overlay_is_remote_forbidden() {
        let resource = resource(
            Some("text/html"),
            b"<div class=\"paywall\">Subscribe to continue reading</div>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/article".to_owned(),
            &resource,
            page(&"P".repeat(300)),
        );
        assert_eq!(
            outcome.result.as_ref().unwrap_err().kind,
            LocalExtractFailureKind::Paywall
        );
        assert!(matches!(
            decide_remote_fallback(&outcome),
            RemoteFallbackDecision::Forbidden {
                reason: RemoteForbiddenReason::Paywall
            }
        ));
    }

    #[test]
    fn medium_captcha_technical_article_is_not_blocked() {
        let resource = resource(
            Some("text/html"),
            b"<article>How recaptcha works and why it exists</article>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/docs".to_owned(),
            &resource,
            page(&"T".repeat(250)),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn medium_sign_in_navigation_is_not_blocked() {
        let resource = resource(
            Some("text/html"),
            b"<nav><a href=\"/sign-in\">Sign in</a></nav><article>Body</article>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/article".to_owned(),
            &resource,
            page(&"N".repeat(300)),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }

    #[test]
    fn medium_subscribe_button_blog_is_not_blocked() {
        let resource = resource(
            Some("text/html"),
            b"<button>Subscribe</button><article>Blog body</article>".to_vec(),
        );
        let outcome = successful_outcome(
            "https://example.com/blog".to_owned(),
            &resource,
            page(&"B".repeat(350)),
        );
        assert_eq!(decide_remote_fallback(&outcome), RemoteFallbackDecision::NotNeeded);
    }
}
