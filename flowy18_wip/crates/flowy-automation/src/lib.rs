//! Local automations owned by conversations: definitions, schedule, a
//! two-table SQLite store, the background worker and the service shared by
//! the model tools and the automation page.

pub mod executor;
pub mod model;
pub mod schedule;
pub mod service;
pub mod store;
pub mod worker;

pub use executor::{
    CancelToken, RunExecutor, RunFuture, RunOutcome, RunReport,
};
pub use model::{
    Access, Action, Automation, AutomationId, AutomationInput, Run, RunId,
    RunStatus, now_ms,
};
pub use service::{AutomationService, ServiceError};
pub use store::{AutomationStore, StoreError};
pub use worker::{StartError, Worker, WorkerConfig};
