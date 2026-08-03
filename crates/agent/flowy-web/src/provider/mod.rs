pub mod article;
pub mod duckduckgo;
pub mod extract;
pub mod extract_policy;
pub mod html_md;
pub mod http_extract;
pub mod search;
pub mod ssrf;

pub use article::{ArticleExtractor, ArticleHtml, DomSmoothieExtractor};
pub use duckduckgo::DuckDuckGoSearchProvider;
pub use extract::ExtractProvider;
pub use extract_policy::{
    CanonicalRequestedUrl, LocalExtractDiagnostics, LocalExtractFailure,
    LocalExtractFailureKind, LocalExtractOutcome, PreparedRemoteUrl, RemoteFallbackDecision,
    RemoteDeferredReason, RemoteFallbackReason, RemoteForbiddenReason, canonical_requested_url,
    classify_web_error, decide_remote_fallback, decide_remote_fallback_with_private, failed_outcome,
    prepare_remote_url, successful_outcome,
};
pub(crate) use extract_policy::{
    RemoteExtractCapabilities, RemoteFallbackPolicy,
};
pub use http_extract::HttpExtractProvider;
pub use search::SearchProvider;
