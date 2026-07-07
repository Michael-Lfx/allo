//! Router state for the POI domain.

use std::sync::Arc;

use crate::service::PoiService;

#[derive(Clone)]
pub struct PoiRouterState {
    pub service: Arc<PoiService>,
}

impl PoiRouterState {
    pub fn new(service: Arc<PoiService>) -> Self {
        Self { service }
    }
}
