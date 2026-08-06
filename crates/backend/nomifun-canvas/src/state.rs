use std::sync::Arc;

use crate::service::CanvasService;

#[derive(Clone)]
pub struct CanvasRouterState {
    pub service: Arc<CanvasService>,
}

impl CanvasRouterState {
    pub fn new(service: Arc<CanvasService>) -> Self {
        Self { service }
    }
}
