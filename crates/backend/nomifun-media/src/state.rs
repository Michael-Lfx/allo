use std::path::PathBuf;
use std::sync::Arc;

use crate::service::MediaApiService;

#[derive(Clone)]
pub struct MediaRouterState {
    pub service: Arc<MediaApiService>,
    pub data_dir: PathBuf,
}

impl MediaRouterState {
    pub fn new(service: Arc<MediaApiService>, data_dir: PathBuf) -> Self {
        Self { service, data_dir }
    }
}
