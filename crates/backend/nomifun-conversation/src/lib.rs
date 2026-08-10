//! Conversation and message CRUD with streaming relay and event emission.
mod acp_error_recovery;
pub mod boot;
mod agent_execution_port;
mod convert;
mod execution_conversation_boundary;
mod failover_seam;
mod message_persistence;
mod orphan_recovery;
pub mod model_failover;
pub mod relay_error_code;
pub mod response_middleware;
pub mod routes;
pub mod routes_aux;
pub mod routes_trace;
pub mod runtime_state;
mod turn_gate;
pub mod service;
mod service_ops;
pub mod skill_resolver;
pub mod skill_snapshot;
pub mod state;
pub mod stream_relay;
pub mod runtime_options;
pub mod effective_model;
pub mod terminal_proof;

pub use response_middleware::{
    CronCommand, CronCommandResult, CronCreateParams, CronUpdateParams, ICronService, MessageMiddleware,
    MiddlewareResult, detect_cron_commands, has_cron_commands, strip_cron_commands, strip_think_tags,
};
pub use effective_model::{EffectiveModelLayers, resolve_effective_model};
pub use boot::reconcile_running_conversations_on_boot;
pub use failover_seam::FailoverSwitch;
pub use agent_execution_port::AgentExecutionConversationPort;
pub use execution_conversation_boundary::{
    ConversationExecutionProjection, ExecutionConversationBoundary, NoExecutionConversationBoundary,
    RepositoryExecutionConversationBoundary,
};
pub use routes::conversation_routes;
pub use routes_aux::conversation_ops_routes;
pub use routes_trace::conversation_trace_routes;
pub use service::{
    ConversationService, ConversationSupervisionHook, DELIVERY_NOTIFY_ORIGIN,
    DeliveryNotifyRegistration, IdempotentMessageDelivery, IdmmTurnScope,
    EditResubmitDeliveryState, EditResubmitObservation, PublicTurnDeliveryState,
    TurnCompletionObserver,
};
pub use state::ConversationRouterState;

#[cfg(test)]
#[path = "service_test.rs"]
mod service_test;
