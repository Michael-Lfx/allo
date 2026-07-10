use async_trait::async_trait;

use crate::types::{SearchQuery, SearchResult, WebError};

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, query: SearchQuery) -> Result<SearchResult, WebError>;
}
