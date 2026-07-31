//! Backend-to-agent bridge for the desktop host's managed web runtime.

use std::sync::Arc;

use flowy_web::{ManagedWebService, WebError, coordinator::ExtractCoordinator, provider::SearchProvider};

/// Keeps the managed web runtime alive and exposes only the stable provider
/// traits to the rest of the backend.
#[derive(Clone)]
pub struct ManagedWebHandle {
    service: Arc<ManagedWebService>,
}

impl ManagedWebHandle {
    pub fn keyless_default(managed_extract: bool) -> Result<Self, WebError> {
        Ok(Self {
            service: Arc::new(ManagedWebService::keyless_default(managed_extract)?),
        })
    }

    pub fn ddg_only() -> Result<Self, WebError> {
        Ok(Self {
            service: Arc::new(ManagedWebService::ddg_only()?),
        })
    }

    pub fn search_provider(&self) -> Arc<dyn SearchProvider> {
        self.service.search_provider()
    }

    pub fn extract_coordinator(&self) -> Option<Arc<dyn ExtractCoordinator>> {
        self.service.extract_coordinator()
    }

    pub async fn shutdown(&self) {
        self.service.shutdown().await;
    }
}
