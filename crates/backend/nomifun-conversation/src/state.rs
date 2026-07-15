use std::sync::Arc;

use crate::service::ConversationService;
use nomifun_ai_agent::{AgentRuntimeRegistry, SessionLifecycleCoordinator};

/// Shared state for conversation route handlers.
#[derive(Clone)]
pub struct ConversationRouterState {
    pub service: ConversationService,
    pub runtime_registry: Arc<dyn AgentRuntimeRegistry>,
    pub session_lifecycle: Option<Arc<SessionLifecycleCoordinator>>,
}
