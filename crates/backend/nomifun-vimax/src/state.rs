use std::sync::Arc;

use crate::service::VimaxApiService;

#[derive(Clone)]
pub struct VimaxRouterState {
    pub service: Arc<VimaxApiService>,
}

impl VimaxRouterState {
    pub fn new(service: Arc<VimaxApiService>) -> Self {
        Self { service }
    }
}
