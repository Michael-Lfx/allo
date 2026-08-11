use std::sync::Arc;

use nomifun_canvas::CanvasService;

use crate::service::MontageApiService;

#[derive(Clone)]
pub struct MontageRouterState {
    pub service: Arc<MontageApiService>,
    pub canvas: Arc<CanvasService>,
}

impl MontageRouterState {
    pub fn new(service: Arc<MontageApiService>, canvas: Arc<CanvasService>) -> Self {
        Self { service, canvas }
    }
}
