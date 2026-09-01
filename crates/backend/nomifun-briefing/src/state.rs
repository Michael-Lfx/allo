use std::sync::Arc;

use crate::service::BriefingApiService;

#[derive(Clone)]
pub struct BriefingRouterState {
    pub service: Arc<BriefingApiService>,
}

impl BriefingRouterState {
    pub fn new(service: Arc<BriefingApiService>) -> Self {
        Self { service }
    }
}
