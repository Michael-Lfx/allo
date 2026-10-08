//! Router state for the appearance domain.

use std::sync::Arc;

use crate::service::AppearanceService;

#[derive(Clone)]
pub struct AppearanceRouterState {
    pub service: Arc<AppearanceService>,
}

impl AppearanceRouterState {
    pub fn new(service: Arc<AppearanceService>) -> Self {
        Self { service }
    }
}
