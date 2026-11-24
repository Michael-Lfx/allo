//! Host-owned agent core seams: messages, tool definitions, guarded execution.
//!
//! This crate intentionally has no agent kernel, plugin system, or transport
//! dependency. The loop and HTTP provider are added separately so the tool and
//! session layer can be tested without a running model.

pub mod acceptance;
pub mod engine;
pub mod exec_input;
pub mod loop_runner;
pub mod run_fence;
pub mod session;
pub mod tools;
pub mod transport;
pub mod types;

pub use acceptance::{AcceptanceQueue, AcceptedMessage, Lease};
pub use engine::{Engine, EngineConfig, RunError, TurnObserver};
pub use exec_input::{Decision, ExecAssignment, ExecContext, ExecInput};
pub use loop_runner::{
    LoopOutcome, ModelRoute, TurnRequest, TurnResult, complete_text,
    log_preview, run_turn,
};
pub use run_fence::{RunFence, RunTicket};
pub use session::{
    DeliveryCommit, DeliveryCommitter, SessionIndex, SessionOrigin,
    SessionRecord, SessionWorkspace, TurnState,
};
pub use transport::{echo_stream_fn, openai_stream_fn};

pub use types::{
    ArtifactReceipt, CancelToken, ContentBlock, Message, MessageId,
    MessageSource, Role, TodoItem, TodoSink, TodoStatus, ToolCallArgs,
    ToolExecutionResult, ToolRunContext, ToolSchema,
};
