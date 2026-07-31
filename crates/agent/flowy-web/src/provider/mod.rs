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
    LocalExtractDiagnostics, LocalExtractFailure, LocalExtractFailureKind, LocalExtractOutcome,
    RemoteFallbackDecision, RemoteFallbackReason, RemoteForbiddenReason, classify_web_error,
    decide_remote_fallback, decide_remote_fallback_with_private, failed_outcome,
    successful_outcome,
};
pub use http_extract::HttpExtractProvider;
pub use search::SearchProvider;
