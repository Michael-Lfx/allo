use std::sync::Arc;

use crate::{AgentRegistry, AgentService, EvalLab, RemoteAgentService};

/// Router state for remote agent routes.
#[derive(Clone)]
pub struct RemoteAgentRouterState {
    pub service: Arc<RemoteAgentService>,
}

#[derive(Clone)]
pub struct AgentRouterState {
    pub agent_registry: Arc<AgentRegistry>,
    pub service: Arc<AgentService>,
    pub eval_lab: Arc<EvalLab>,
}
