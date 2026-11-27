//! The one seam between the worker and the agent.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::watch;

use crate::model::{Automation, RunId};

pub type RunFuture<'a> = Pin<Box<dyn Future<Output = RunReport> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Succeeded,
    /// The workspace was locked by another turn; the run is skipped.
    Busy,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// Kept on failure too, so the user can open the session and see why.
    pub session_id: Option<String>,
    pub outcome: RunOutcome,
}

/// Runs one `AgentTask` automation as a background agent turn. The executor
/// watches `cancel` and stops the turn when it fires.
pub trait RunExecutor: Send + Sync {
    fn run<'a>(
        &'a self,
        automation: &'a Automation,
        run_id: RunId,
        cancel: CancelToken,
    ) -> RunFuture<'a>;
}

#[derive(Clone, Debug)]
pub struct CancelToken(Arc<watch::Sender<bool>>);

impl Default for CancelToken {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(false)))
    }
}

impl CancelToken {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    /// Resolves once `cancel` has been called.
    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        // The sender lives in `self`, so this only returns after a cancel.
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}
