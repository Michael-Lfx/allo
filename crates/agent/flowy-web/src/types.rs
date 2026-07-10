use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_SEARCH_COUNT: u32 = 5;
pub const MAX_SEARCH_COUNT: u32 = 10;
pub const MAX_EXTRACT_URLS: usize = 3;
pub const EXTRACT_CHAR_LIMIT: usize = 3_000;
/// Quality gate: readability markdown shorter than this falls back to full page.
pub const MIN_ARTICLE_CHARS: usize = 400;

pub const EXTRACTOR_READABILITY: &str = "readability";
pub const EXTRACTOR_FULLPAGE: &str = "fullpage";

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub rank: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub provider: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone)]
pub struct ExtractRequest {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedPage {
    pub url: String,
    pub title: Option<String>,
    pub markdown: String,
    pub truncated: bool,
    pub provider: String,
    /// `"readability"` or `"fullpage"`
    pub extractor: String,
}

#[derive(Debug, Error)]
pub enum WebError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("blocked URL: {0}")]
    BlockedUrl(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("provider error: {0}")]
    Provider(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_char_limit_is_three_thousand() {
        assert_eq!(EXTRACT_CHAR_LIMIT, 3_000);
    }

    #[test]
    fn min_article_chars_gate_is_four_hundred() {
        assert_eq!(MIN_ARTICLE_CHARS, 400);
    }
}
