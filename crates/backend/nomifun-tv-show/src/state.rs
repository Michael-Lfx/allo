use std::sync::Arc;

use nomi_montage::MontageService;

use crate::service::TvShowService;

#[derive(Clone)]
pub struct TvShowRouterState {
    pub service: Arc<TvShowService>,
    /// Optional Agent/Montage runtime used by publish-from-montage / import.
    pub montage: Option<Arc<MontageService>>,
}

impl TvShowRouterState {
    pub fn new(service: Arc<TvShowService>, montage: Option<Arc<MontageService>>) -> Self {
        Self { service, montage }
    }
}
