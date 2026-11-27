//! Local automations: definitions, schedule, a two-table SQLite store, the
//! background worker and the service shared by the UI and the model tools.

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
    RunStatus, RunTrigger, now_ms,
};
pub use service::{AutomationService, ServiceError};
pub use store::{AutomationStore, StoreError};
pub use worker::{RunListener, Worker, WorkerConfig};
