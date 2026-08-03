//! Backend-to-agent bridge for the desktop host's managed web runtime.

use std::sync::Arc;
use std::time::Duration;

use flowy_web::{
    ManagedExtractMode as FlowyManagedExtractMode, ManagedWebService, WebError,
    coordinator::ExtractCoordinator, provider::SearchProvider,
};

use crate::ManagedExtractMode;

/// Keeps the managed web runtime alive and exposes only the stable provider
/// traits to the rest of the backend.
#[derive(Clone)]
pub struct ManagedWebHandle {
    service: Arc<ManagedWebService>,
}

impl ManagedWebHandle {
    pub fn keyless_default(mode: ManagedExtractMode) -> Result<Self, WebError> {
        let flowy_mode = match mode {
            ManagedExtractMode::Disabled => FlowyManagedExtractMode::Disabled,
            ManagedExtractMode::EvidenceBacked => FlowyManagedExtractMode::EvidenceBacked,
        };
        Ok(Self {
            service: Arc::new(ManagedWebService::keyless_default(flowy_mode)?),
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
        match tokio::time::timeout(Duration::from_secs(3), self.service.shutdown()).await {
            Ok(()) => {}
            Err(_) => tracing::warn!(
                target: "managed_web",
                "managed web shutdown timed out after 3 seconds"
            ),
        }
    }
}
