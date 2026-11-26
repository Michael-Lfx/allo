//! Persistent automation contracts and object-safe host ports.
//!
//! Persistence owns its SQLite connection on a dedicated thread. This crate
//! does not own an execution runtime, engine, or UI. An automation handle needs
//! an explicit service implementation; no permissive default backend exists.

mod command;
pub mod delivery;
mod executor;
mod identity;
mod model;
mod ports;
pub mod recovery;
pub mod retention;
pub mod schedule;
pub mod schema_contract;
mod service;
pub mod store;
pub mod worker;
pub use worker::{AutomationWorker, PreparedRun, RunPreparer, WorkerCycle};

pub use command::*;
pub use executor::*;
pub use identity::{
    AccountGeneration, AttemptId, AutomationId, DeletionPlanId,
    GenerationEpoch, GrantId, IdentityError, PrincipalId, RequestKey, RunId,
    SessionRef, SubmissionRef,
};
pub use model::*;
pub use ports::*;
pub use service::CommandService;
