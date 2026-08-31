//! Agent runtime lifecycle, per-conversation runtime registration, and skill management.
pub(crate) mod runtime_state;
pub mod agent_eval;
pub mod agent_trace;
pub mod artifact_store;
pub mod boot_process_reaper;
pub mod runtime_handle;
// Rendering page-fetch adapter for knowledge URL sources. The implementation
// consumes the application-owned Browser Session Hub and keeps the knowledge
// crate browser-platform-free.
#[cfg(feature = "browser-use")]
pub mod browser_fetcher;
pub mod capability;
pub mod cc_switch;
pub mod concept_graph_loop;
pub mod course_outline_loop;
pub mod factory;
pub mod goal_bridge;
pub(crate) mod goal_probe;
pub(crate) mod idle_scanner;
pub mod auxiliary_provider;
pub mod extraction_scanner;
pub mod knowledge_completer;
pub mod knowledge_retrieval;
pub mod knowledge_writeback;
pub mod learning_completer;
pub mod learning_course;
pub mod lesson_content_loop;
pub(crate) mod loop_core;
pub mod meeting_sink;
#[cfg(feature = "managed-search")]
pub mod managed_search;
#[cfg(feature = "managed-search")]
pub mod managed_web;
pub mod manager;
pub mod nomi_session_persistence;
pub mod one_shot;
pub(crate) mod persistence;
pub mod protocol;
pub mod registry;
pub mod routes;
pub(crate) mod services;
pub mod session;
pub mod runtime_registry;
pub mod conversation_title_completer;
pub mod terminal_title_completer;
pub mod types;

// ── Agent-layer re-exports (the seam) ──────────────────────────────────────
// Backend crates reach the agent (nomi-*) layer ONLY through nomifun-ai-agent.
// When the agent layer is later extracted into its own repo, these re-exports
// become the single integration surface.
pub use nomi_agent::companion_tools::CompanionMemorySink;
pub use nomi_agent::companion_tools::{CompanionSkillSink, CreateCompanionSkillTool, SkillListing};
pub use nomi_agent::summon_tools::{SummonContextSink, SummonProposalSink};
pub use nomi_agent::cron_tools::{CronJobSummary, CronSink};
pub use nomi_agent::meeting_tools::{
    MeetingListenContextSink, MeetingListenContributor, MeetingSessionSummary, MeetingSink,
    MeetingTranscriptHit, MEETING_TOOL_NAMES,
};
pub use nomi_agent::ssh_backend::{
    RemoteCommandOutput, RemoteFileStat, SshBackend, SshBackendProvider, SshLeaseRelease,
    SshSessionBinding, SshSessionLease,
};
pub use nomi_agent::requirement_tools::RequirementSink;
pub use nomi_agent::SearchProviderBinding;
pub use nomi_agent::ExtractCoordinatorBinding;

/// Host-selected mode for the private managed extraction fallback.
///
/// This type lives on the backend-to-agent seam so non-managed-search builds
/// can still construct the host capability profile without depending on the
/// optional `flowy-web` implementation crate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ManagedExtractMode {
    #[default]
    Disabled,
    EvidenceBacked,
}
pub use nomi_config;
pub use nomi_types;
pub use nomi_providers::{current_flowy_billing_turn_id, with_flowy_billing_turn_id};

pub use agent_eval::{
    eval_run_workspace_label, suite_business_label, EvalCaseTurnUsage, EvalLab, EvalSessionBridge,
    OpenEvalCaseSession, RecordEvalCaseTurn,
};
pub use agent_trace::{
    AgentTraceHub, ObservationIds, ObservationRecorder, ProjectedTurn, SessionObservationList,
    TraceApiError, session_observation_call_dto, session_observation_list_dto,
    session_observation_turn_dto, DEFAULT_SESSION_OBSERVATION_LIST_LIMIT,
    DEVELOPER_MODE_PREF_KEY, MAX_SESSION_OBSERVATION_LIST_LIMIT, classify_session_kind,
};
pub use runtime_state::AgentRuntimeState;
pub use boot_process_reaper::{
    AgentProcessReapReport, ConversationProcessReapVerdict, reap_orphan_agent_processes,
};
#[cfg(any(test, feature = "test-support"))]
pub use runtime_handle::MockAgentRuntime;
pub use runtime_handle::{
    AgentRuntimeControl, AgentRuntimeHandle, SystemResourceNoticeDelivery,
};
pub use capability::skill_manager::{
    AcpSkillManager, SkillDefinition, SkillIndex, build_skills_index_text, prepare_first_message_with_skills_index,
};
pub use capability::{
    ExtractionTrigger, LightweightTurnReviewer, PostTurnHook, PostTurnReviewHook, PreTurnHook,
    ProactiveSessionExtractor, SessionEndContext, SessionEndHook, SessionEndReason,
    SessionLifecycleCoordinator, TurnContext, WorkSessionEndHook,
};
pub use auxiliary_provider::AuxiliaryClientFactory;
pub use extraction_scanner::start_session_extraction_scanner;
pub use manager::acp::PoiPrefetchHook;
pub use manager::nomi::agent::goal_slash_commands;
pub use factory::provider_config::{
    one_shot_completion, resolve_provider_config, streaming_completion,
    streaming_completion_text_or_reasoning, user_message, DeltaKind,
};
pub use one_shot::{OneShotDeps, OneShotTool, OneShotTurnRequest, one_shot_handler, run_one_shot_turn};
pub use factory::{
    AgentFactoryDeps, CompanionPromptProvider, CompanionSummonProvider,
    build_agent_factory,
};
#[cfg(feature = "browser-use")]
pub use factory::browser_lane::{
    BrowserLaneBinding, BrowserLaneClientProvider, BrowserLaneClientProviderSlot,
    BrowserOwnerLeaseGuard, TrustedBrowserRuntimeContext,
};
pub use idle_scanner::start_idle_scanner;
#[cfg(feature = "browser-use")]
pub use browser_fetcher::BrowserFetcher;
pub use knowledge_completer::LiveKnowledgeCompleter;
pub use knowledge_completer::{resolve_default_model, resolve_flowy_cloud_model};
pub use learning_completer::LiveLearningCompleter;
pub use concept_graph_loop::LiveConceptGraphAgentEngine;
pub use course_outline_loop::LiveCourseOutlineAgentEngine;
pub use lesson_content_loop::LiveLessonContentAgentEngine;
pub use knowledge_retrieval::LiveKnowledgeRetrievalSink;
pub use knowledge_writeback::LiveKnowledgeWritebackSink;
pub use learning_course::LiveLearningCourseSink;
pub use meeting_sink::{LiveMeetingListenContextSink, LiveMeetingSink};
#[cfg(feature = "managed-search")]
pub use managed_search::ManagedSearchHandle;
#[cfg(feature = "managed-search")]
pub use managed_web::ManagedWebHandle;
pub use conversation_title_completer::{ConversationTitleCompleter, LiveConversationTitleCompleter};
pub use nomi_session_persistence::{
    NomiSessionPersistence, NomiSessionResetOutcome, NomiSessionRewindOutcome,
};
pub use terminal_title_completer::LiveTerminalTitleCompleter;
pub use nomifun_api_types::{
    AcpBuildExtra, AcpModelInfo, NomiBuildExtra, OpenClawBuildExtra, OpenClawGatewayConfig, RemoteBuildExtra,
    SlashCommandItem,
};
pub use persistence::AcpSessionSyncService;
pub use protocol::events::{
    AcpPermissionEventData, AcpPermissionOptionKind, AcpToolCallKind, AgentStreamEvent, FinishEventData, TurnStopReason,
};
pub use protocol::send_error::AgentSendError;
pub use registry::{AgentRegistry, UnavailableReason};
pub use routes::{
    AgentRouterState, RemoteAgentRouterState, agent_routes, eval_routes, remote_agent_routes,
};
pub use services::AgentService;
pub use services::RemoteAgentService;
pub use runtime_registry::{AgentRuntimeRegistry, InMemoryAgentRuntimeRegistry};
