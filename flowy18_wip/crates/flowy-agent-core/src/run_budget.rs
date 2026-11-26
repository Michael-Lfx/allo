//! Durable reservations for one submission's model and tool dispatches.

use std::fmt;
use std::io;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::types::CancelToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudget {
    pub max_execution_ms: NonZeroU64,
    pub max_model_calls: NonZeroU32,
    pub max_tool_calls: NonZeroU32,
}

impl RunBudget {
    #[must_use]
    pub const fn new(
        execution_ms: u64,
        model_calls: u32,
        tool_calls: u32,
    ) -> Option<Self> {
        match (
            NonZeroU64::new(execution_ms),
            NonZeroU32::new(model_calls),
            NonZeroU32::new(tool_calls),
        ) {
            (
                Some(max_execution_ms),
                Some(max_model_calls),
                Some(max_tool_calls),
            ) => Some(Self {
                max_execution_ms,
                max_model_calls,
                max_tool_calls,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStopReason {
    Deadline,
    ModelCallBudget,
    ToolCallBudget,
}

/// The durable wall-clock record.
///
/// Live enforcement uses the separate monotonic deadline. Counts describe reserved dispatches, including an
/// already reserved request that is later interrupted before it can send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudgetCheckpoint {
    pub started_at_ms: i64,
    pub deadline_at_ms: i64,
    pub limits: RunBudget,
    pub model_calls: u32,
    pub tool_calls: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<BudgetStopReason>,
}

impl RunBudgetCheckpoint {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let duration = i64::try_from(self.limits.max_execution_ms.get()).ok();
        if duration.and_then(|ms| self.started_at_ms.checked_add(ms))
            != Some(self.deadline_at_ms)
            || self.model_calls > self.limits.max_model_calls.get()
            || self.tool_calls > self.limits.max_tool_calls.get()
        {
            return false;
        }
        match self.stop_reason {
            Some(BudgetStopReason::ModelCallBudget) => {
                self.model_calls == self.limits.max_model_calls.get()
            }
            Some(BudgetStopReason::ToolCallBudget) => {
                self.tool_calls == self.limits.max_tool_calls.get()
            }
            Some(BudgetStopReason::Deadline) | None => true,
        }
    }
}

/// A cooperative stop request. A storage failure is separate from a limit
/// so recovery cannot report an uncommitted cancellation as durable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetStop {
    Limit(BudgetStopReason),
    Storage,
    Cancelled,
}

impl fmt::Display for BudgetStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Limit(BudgetStopReason::Deadline) => {
                "execution time limit reached"
            }
            Self::Limit(BudgetStopReason::ModelCallBudget) => {
                "model call limit reached"
            }
            Self::Limit(BudgetStopReason::ToolCallBudget) => {
                "tool call limit reached"
            }
            Self::Storage => "could not persist run budget",
            Self::Cancelled => "run is no longer current",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetCheckpointError {
    StaleSubmission,
    Storage,
}

impl From<BudgetCheckpointError> for BudgetStop {
    fn from(error: BudgetCheckpointError) -> Self {
        match error {
            BudgetCheckpointError::StaleSubmission => Self::Cancelled,
            BudgetCheckpointError::Storage => Self::Storage,
        }
    }
}

#[derive(Debug)]
pub struct RunBudgetStart {
    deadline: Instant,
    checkpoint: RunBudgetCheckpoint,
}

impl RunBudgetStart {
    pub(crate) fn new(
        limits: RunBudget,
        started_at_ms: i64,
    ) -> io::Result<Self> {
        Self::with_instant(limits, started_at_ms, Instant::now())
    }

    fn with_instant(
        limits: RunBudget,
        started_at_ms: i64,
        start: Instant,
    ) -> io::Result<Self> {
        let range_error = || {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "run budget deadline is outside the supported clock range",
            )
        };
        let execution_ms = limits.max_execution_ms.get();
        let wall_duration =
            i64::try_from(execution_ms).map_err(|_| range_error())?;
        let deadline_at_ms = started_at_ms
            .checked_add(wall_duration)
            .ok_or_else(range_error)?;
        let deadline = start
            .checked_add(Duration::from_millis(execution_ms))
            .ok_or_else(range_error)?;
        Ok(Self {
            deadline,
            checkpoint: RunBudgetCheckpoint {
                started_at_ms,
                deadline_at_ms,
                limits,
                model_calls: 0,
                tool_calls: 0,
                stop_reason: None,
            },
        })
    }

    pub(crate) const fn checkpoint(&self) -> RunBudgetCheckpoint {
        self.checkpoint
    }

    /// Call after saving the initial checkpoint with the submission. The
    /// callback validates this exact submission and saves each later update.
    /// It must not call into the budget state while saving.
    pub(crate) fn track(
        self,
        persist: impl Fn(RunBudgetCheckpoint) -> Result<(), BudgetCheckpointError>
        + Send
        + Sync
        + 'static,
    ) -> RunBudgetState {
        RunBudgetState {
            deadline: self.deadline,
            state: Mutex::new(Accounting {
                checkpoint: self.checkpoint,
                stop: None,
            }),
            stop_signal: CancelToken::new(),
            persist: Box::new(persist),
        }
    }
}

struct Accounting {
    checkpoint: RunBudgetCheckpoint,
    stop: Option<BudgetStop>,
}

pub struct RunBudgetState {
    deadline: Instant,
    state: Mutex<Accounting>,
    stop_signal: CancelToken,
    persist: Box<
        dyn Fn(RunBudgetCheckpoint) -> Result<(), BudgetCheckpointError>
            + Send
            + Sync,
    >,
}

impl fmt::Debug for RunBudgetState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunBudgetState")
            .field("deadline", &self.deadline)
            .field("stopping", &self.is_stopping())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum RequestKind {
    Model,
    Tool,
}

impl RunBudgetState {
    pub(crate) fn reserve_model(&self) -> Result<(), BudgetStop> {
        self.reserve(RequestKind::Model, Instant::now)
    }

    pub(crate) fn reserve_tool(&self) -> Result<(), BudgetStop> {
        self.reserve(RequestKind::Tool, Instant::now)
    }

    pub(crate) fn is_stopping(&self) -> bool {
        self.stop_signal.is_cancelled()
    }

    pub(crate) fn stop_requested(&self) -> Option<BudgetStop> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(stop) = state.stop {
            return Some(stop);
        }
        let stop = (Instant::now() >= self.deadline).then(|| {
            self.stop_at_limit(&mut state, BudgetStopReason::Deadline)
        });
        drop(state);
        stop
    }

    /// Waits for a deadline or refused reservation. The engine still owns
    /// cancellation, tool reaping, and the durable commit decision.
    pub(crate) async fn wait_for_stop(&self) -> BudgetStop {
        tokio::select! {
            () = tokio::time::sleep_until(self.deadline) => {
                let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(stop) = state.stop {
                    return stop;
                }
                let stop = self.stop_at_limit(&mut state, BudgetStopReason::Deadline);
                drop(state);
                stop
            }
            () = self.stop_signal.cancelled() => {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                    .stop.expect("a stop reason is latched before waking the supervisor")
            }
        }
    }

    fn reserve(
        &self,
        request: RequestKind,
        now: impl Fn() -> Instant,
    ) -> Result<(), BudgetStop> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(stop) = state.stop {
            return Err(stop);
        }
        if now() >= self.deadline {
            return Err(
                self.stop_at_limit(&mut state, BudgetStopReason::Deadline)
            );
        }
        let mut next = state.checkpoint;
        let (count, limit, reason) = match request {
            RequestKind::Model => (
                &mut next.model_calls,
                next.limits.max_model_calls,
                BudgetStopReason::ModelCallBudget,
            ),
            RequestKind::Tool => (
                &mut next.tool_calls,
                next.limits.max_tool_calls,
                BudgetStopReason::ToolCallBudget,
            ),
        };
        if *count >= limit.get() {
            return Err(self.stop_at_limit(&mut state, reason));
        }
        *count += 1;
        if let Err(error) = (self.persist)(next) {
            return Err(self.latch_stop(&mut state, error.into()));
        }
        state.checkpoint = next;
        // A slow checkpoint must not let work through after the deadline.
        if now() >= self.deadline {
            return Err(
                self.stop_at_limit(&mut state, BudgetStopReason::Deadline)
            );
        }
        drop(state);
        Ok(())
    }

    fn stop_at_limit(
        &self,
        state: &mut Accounting,
        reason: BudgetStopReason,
    ) -> BudgetStop {
        let mut next = state.checkpoint;
        next.stop_reason = Some(reason);
        let stop = match (self.persist)(next) {
            Ok(()) => {
                state.checkpoint = next;
                BudgetStop::Limit(reason)
            }
            Err(error) => error.into(),
        };
        self.latch_stop(state, stop)
    }

    fn latch_stop(
        &self,
        state: &mut Accounting,
        stop: BudgetStop,
    ) -> BudgetStop {
        state.stop = Some(stop);
        self.stop_signal.cancel();
        stop
    }
}

#[cfg(test)]
mod tests;
