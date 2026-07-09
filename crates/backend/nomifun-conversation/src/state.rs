use std::sync::Arc;

use crate::service::ConversationService;
use nomifun_ai_agent::{IWorkerTaskManager, SessionLifecycleCoordinator};

/// Shared state for conversation route handlers.
#[derive(Clone)]
pub struct ConversationRouterState {
    pub service: ConversationService,
    pub task_manager: Arc<dyn IWorkerTaskManager>,
    pub session_lifecycle: Option<Arc<SessionLifecycleCoordinator>>,
}
