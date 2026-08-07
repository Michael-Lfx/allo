use std::sync::Arc;

use nomifun_canvas::CanvasService;

use crate::service::VimaxApiService;

#[derive(Clone)]
pub struct VimaxRouterState {
    pub service: Arc<VimaxApiService>,
    pub canvas: Arc<CanvasService>,
}

impl VimaxRouterState {
    pub fn new(service: Arc<VimaxApiService>, canvas: Arc<CanvasService>) -> Self {
        Self { service, canvas }
    }
}
