//! Router state for the local model runtime.

use std::sync::Arc;

use crate::service::LocalModelsService;

#[derive(Clone)]
pub struct LocalModelsRouterState {
    pub service: Arc<LocalModelsService>,
}

impl LocalModelsRouterState {
    pub fn new(service: Arc<LocalModelsService>) -> Self {
        Self { service }
    }
}
