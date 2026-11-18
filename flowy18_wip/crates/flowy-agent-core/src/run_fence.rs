//! Single-flight fence for one session's turn.
//!
//! A session may have at most one active run. Starting a second run is refused
//! rather than queued: the UI already disables send while running, so a second
//! run means a race. A finished or cancelled run can only publish results while
//! its ticket is still current, which drops late stream output after stop.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::types::CancelToken;

/// One attempt to run a turn.
pub struct RunTicket {
    generation: u64,
    signal: CancelToken,
    fence: Arc<RunFence>,
}

impl std::fmt::Debug for RunTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunTicket")
            .field("generation", &self.generation)
            .field("current", &self.is_current())
            .finish_non_exhaustive()
    }
}

impl RunTicket {
    pub(crate) fn publish(&self, update: impl FnOnce()) {
        let active = self.fence.active.lock().unwrap();
        if active
            .as_ref()
            .is_some_and(|(generation, _)| *generation == self.generation)
        {
            update();
        }
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn signal(&self) -> &CancelToken {
        &self.signal
    }

    /// True while this ticket is still the session's current run.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.fence.current.load(Ordering::SeqCst) == self.generation
    }

    /// Mark the run finished. Returns false when a newer run (or a cancel)
    /// already superseded this one, so the caller must not publish results.
    pub fn finish(&self) -> bool {
        if !self.is_current() {
            return false;
        }
        let mut active = self.fence.active.lock().unwrap();
        let matches = active
            .as_ref()
            .is_some_and(|(generation, _)| *generation == self.generation);
        if matches {
            *active = None;
        }
        matches
    }
}

/// Per-session run gate.
#[derive(Default)]
pub struct RunFence {
    next_generation: AtomicU64,
    current: AtomicU64,
    active: Mutex<Option<(u64, CancelToken)>>,
}

impl std::fmt::Debug for RunFence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunFence")
            .field("current", &self.current.load(Ordering::SeqCst))
            .field("active", &self.active.lock().unwrap().is_some())
            .finish_non_exhaustive()
    }
}

impl RunFence {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a run. `None` means another run is already active for this session.
    pub fn try_begin(self: &Arc<Self>) -> Option<RunTicket> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return None;
        }
        let generation =
            self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let signal = CancelToken::new();
        *active = Some((generation, signal.clone()));
        self.current.store(generation, Ordering::SeqCst);
        drop(active);
        Some(RunTicket {
            generation,
            signal,
            fence: Arc::clone(self),
        })
    }

    /// Cancel the active run, if any, and free the gate for the next one.
    pub fn cancel_active(&self) {
        let mut active = self.active.lock().unwrap();
        if let Some((_, signal)) = active.as_ref() {
            signal.cancel();
        }
        *active = None;
        self.current.fetch_add(1, Ordering::SeqCst);
        drop(active);
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::RunFence;
    use std::sync::Arc;

    #[test]
    fn only_one_run_can_be_active() {
        let fence = Arc::new(RunFence::new());
        let first = fence.try_begin().expect("first run starts");
        assert!(fence.try_begin().is_none(), "second run must be refused");
        assert!(first.finish());
        let second = fence.try_begin().expect("run starts after finish");
        assert!(second.generation() > first.generation());
        assert!(second.finish());
    }

    #[test]
    fn cancel_fires_the_token_and_invalidates_late_results() {
        let fence = Arc::new(RunFence::new());
        let ticket = fence.try_begin().unwrap();
        let signal = ticket.signal().clone();
        fence.cancel_active();
        assert!(signal.is_cancelled());
        assert!(!ticket.finish(), "cancelled run may not publish results");
        assert!(!ticket.is_current());
        assert!(!fence.is_active(), "cancel must free the gate");
    }

    #[test]
    fn a_new_run_after_cancel_is_allowed_and_old_ticket_is_stale() {
        let fence = Arc::new(RunFence::new());
        let old = fence.try_begin().unwrap();
        fence.cancel_active();
        let new = fence.try_begin().expect("new run after cancel");
        assert!(!old.finish(), "old ticket must never finish the new run");
        assert!(new.is_current());
        assert!(new.finish());
    }
}
