use async_trait::async_trait;

use crate::types::{ExtractRequest, ExtractedPage, WebError};

#[async_trait]
pub trait ExtractProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError>;
}
