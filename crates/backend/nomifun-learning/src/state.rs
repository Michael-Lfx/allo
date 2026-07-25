use std::sync::Arc;

use crate::LearningService;

#[derive(Clone)]
pub struct LearningRouterState {
    pub service: Arc<LearningService>,
}

impl LearningRouterState {
    pub fn new(service: Arc<LearningService>) -> Self {
        Self { service }
    }
}
