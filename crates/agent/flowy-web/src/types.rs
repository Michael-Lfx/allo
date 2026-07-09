use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_SEARCH_COUNT: u32 = 5;
pub const MAX_SEARCH_COUNT: u32 = 10;
pub const MAX_EXTRACT_URLS: usize = 3;
pub const EXTRACT_CHAR_LIMIT: usize = 15_000;

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
