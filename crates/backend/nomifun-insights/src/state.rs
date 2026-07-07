//! Router state for insights contribution endpoints.

use std::path::PathBuf;
use std::sync::Arc;

use crate::service::InsightsService;

#[derive(Clone)]
pub struct InsightsRouterState {
    pub service: Arc<InsightsService>,
    pub data_dir: PathBuf,
}

impl InsightsRouterState {
    pub fn new(service: Arc<InsightsService>, data_dir: PathBuf) -> Self {
        Self { service, data_dir }
    }
}
