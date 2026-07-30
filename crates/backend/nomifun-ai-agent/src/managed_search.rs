//! Backend-to-agent bridge for the desktop host's managed search dependency.

use std::sync::Arc;

use flowy_web::{managed::ManagedSearchService, provider::SearchProvider, WebError};

/// Keeps the concrete service alive for graceful shutdown while exposing only
/// the stable provider trait to the rest of the backend.
#[derive(Clone)]
pub struct ManagedSearchHandle {
    service: Arc<ManagedSearchService>,
}

impl ManagedSearchHandle {
    pub fn keyless_default() -> Result<Self, WebError> {
        Ok(Self {
            service: Arc::new(ManagedSearchService::keyless_default()?),
        })
    }

    pub fn ddg_only() -> Result<Self, WebError> {
        Ok(Self {
            service: Arc::new(ManagedSearchService::ddg_only()?),
        })
    }

    pub fn provider(&self) -> Arc<dyn SearchProvider> {
        self.service.clone()
    }

    pub async fn shutdown(&self) {
        self.service.shutdown().await;
    }
}
