//! Single-flight fence for one session's turn with two-phase cancel reaping.
//!
//! A session may have at most one active run. Starting a second run is refused
//! rather than queued. Cancellation (`cancel_active`) is two-phase:
//! 1. Immediately cancels the run's [`CancelToken`] and invalidates its
//!    [`PublishToken`] / generation so no late stream output or terminal result
//!    can publish into the session.
//! 2. Keeps the fence closed (`is_active() == true`, `is_cancelling() == true`,
//!    `try_begin() == None`) until the cancelled turn finishes unwinding and
//!    reaping any child process tree, dropping (or calling [`RunTicket::finish`]
//!    on) its unique [`RunTicket`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::types::CancelToken;

struct ActiveRun {
    generation: u64,
    signal: CancelToken,
    cancelling: bool,
}

/// Lightweight handle allowing an observer/emitter to publish deltas only while
/// its originating run is still the current, non-cancelling run.
///
/// Cloning or holding a `PublishToken` never keeps the execution fence closed
/// after the owning [`RunTicket`] has reaped and dropped.
#[derive(Clone)]
pub struct PublishToken {
    generation: u64,
    fence: Arc<RunFence>,
}

impl std::fmt::Debug for PublishToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublishToken")
            .field("generation", &self.generation)
            .field("current", &self.is_current())
            .finish_non_exhaustive()
    }
}

impl PublishToken {
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.fence.current.load(Ordering::SeqCst) == self.generation
    }

    pub fn publish(&self, update: impl FnOnce()) {
        if !self.is_current() {
            return;
        }
        let active = self.fence.active.lock().unwrap();
        if active.as_ref().is_some_and(|slot| {
            slot.generation == self.generation && !slot.cancelling
        }) {
            update();
        }
    }
}

/// Unique RAII ownership token for one active turn.
///
/// The per-session fence remains closed until [`Self::finish`] is called or the
/// `RunTicket` is dropped after the turn (and its child tools) have finished
/// unwinding and reaping.
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
    #[must_use]
    pub fn publisher(&self) -> PublishToken {
        PublishToken {
            generation: self.generation,
            fence: Arc::clone(&self.fence),
        }
    }

    pub(crate) fn publish(&self, update: impl FnOnce()) {
        self.publisher().publish(update);
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn signal(&self) -> &CancelToken {
        &self.signal
    }

    /// True while this ticket is still the session's current, non-cancelled run.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.fence.current.load(Ordering::SeqCst) == self.generation
    }

    /// Release the active execution slot and notify waiters. Returns `true`
    /// only when this ticket completed without being cancelled or superseded,
    /// so the caller may publish terminal results.
    pub fn finish(&self) -> bool {
        let mut active = self.fence.active.lock().unwrap();
        let Some(slot) = active.as_ref() else {
            return false;
        };
        if slot.generation != self.generation {
            return false;
        }
        let clean = !slot.cancelling && self.is_current();
        *active = None;
        drop(active);
        self.fence.idle_notify.notify_all();
        clean
    }

    fn release_if_owned(&self) {
        let mut active = self.fence.active.lock().unwrap();
        let owned = active
            .as_ref()
            .is_some_and(|slot| slot.generation == self.generation);
        if owned {
            *active = None;
            drop(active);
            self.fence.idle_notify.notify_all();
        }
    }
}

impl Drop for RunTicket {
    fn drop(&mut self) {
        self.release_if_owned();
    }
}

/// Per-session run gate.
#[derive(Default)]
pub struct RunFence {
    next_generation: AtomicU64,
    current: AtomicU64,
    active: Mutex<Option<ActiveRun>>,
    idle_notify: Condvar,
}

impl std::fmt::Debug for RunFence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let active = self.active.lock().unwrap();
        f.debug_struct("RunFence")
            .field("current", &self.current.load(Ordering::SeqCst))
            .field("active", &active.is_some())
            .field(
                "cancelling",
                &active.as_ref().is_some_and(|slot| slot.cancelling),
            )
            .finish_non_exhaustive()
    }
}

impl RunFence {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a run. `None` means another run is still active or reaping after
    /// cancellation for this session.
    pub fn try_begin(self: &Arc<Self>) -> Option<RunTicket> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return None;
        }
        let generation =
            self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let signal = CancelToken::new();
        *active = Some(ActiveRun {
            generation,
            signal: signal.clone(),
            cancelling: false,
        });
        self.current.store(generation, Ordering::SeqCst);
        drop(active);
        Some(RunTicket {
            generation,
            signal,
            fence: Arc::clone(self),
        })
    }

    /// Cancel the active run, if any.
    ///
    /// Immediately triggers its [`CancelToken`] and invalidates its generation
    /// so no late deltas or results can publish, while keeping the execution
    /// gate closed until the cancelled [`RunTicket`] finishes reaping and drops.
    pub fn cancel_active(&self) {
        let mut active = self.active.lock().unwrap();
        if let Some(slot) = active.as_mut() {
            slot.signal.cancel();
            slot.cancelling = true;
        }
        self.current.fetch_add(1, Ordering::SeqCst);
        drop(active);
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    #[must_use]
    pub fn is_cancelling(&self) -> bool {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|slot| slot.cancelling)
    }

    /// Block the calling thread until no run is active (including reaping after
    /// cancel) or `timeout` elapses. Returns `true` when the fence is idle.
    #[must_use]
    pub fn wait_until_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut active = self.active.lock().unwrap();
        while active.is_some() {
            let Some(remaining) =
                deadline.checked_duration_since(Instant::now())
            else {
                return false;
            };
            if remaining.is_zero() {
                return false;
            }
            let (guard, result) =
                self.idle_notify.wait_timeout(active, remaining).unwrap();
            active = guard;
            if result.timed_out() && active.is_some() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::RunFence;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

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
    fn cancel_fires_token_blocks_overlap_until_reaped_and_drops_late_publish() {
        let fence = Arc::new(RunFence::new());
        let ticket = fence.try_begin().unwrap();
        let publisher = ticket.publisher();
        let signal = ticket.signal().clone();
        fence.cancel_active();
        assert!(signal.is_cancelled());
        assert!(!ticket.is_current());
        assert!(fence.is_active(), "gate must stay closed while reaping");
        assert!(fence.is_cancelling());
        assert!(
            fence.try_begin().is_none(),
            "replacement run must wait until cancelled run finishes reaping"
        );
        let published = AtomicUsize::new(0);
        publisher.publish(|| {
            published.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(published.load(Ordering::SeqCst), 0);
        assert!(!ticket.finish(), "cancelled run may not publish results");
        assert!(!fence.is_active(), "finish after cancel releases the gate");
        assert!(!fence.is_cancelling());
    }

    #[test]
    fn a_new_run_after_reap_is_allowed_and_old_ticket_is_stale() {
        let fence = Arc::new(RunFence::new());
        let old = fence.try_begin().unwrap();
        let old_pub = old.publisher();
        fence.cancel_active();
        assert!(fence.try_begin().is_none(), "still reaping");
        assert!(!old.finish(), "reaped cancelled run does not publish");
        let new = fence.try_begin().expect("new run after reap");
        let published = AtomicUsize::new(0);
        old_pub.publish(|| {
            published.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(published.load(Ordering::SeqCst), 0);
        assert!(!old.finish(), "old ticket must never finish the new run");
        drop(old);
        assert!(new.is_current());
        assert!(new.finish());
    }

    #[test]
    fn dropping_cancelled_ticket_releases_gate_and_wakes_waiters() {
        let fence = Arc::new(RunFence::new());
        let ticket = fence.try_begin().unwrap();
        fence.cancel_active();
        assert!(!fence.wait_until_idle(Duration::from_millis(20)));
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(ticket);
        });
        assert!(fence.wait_until_idle(Duration::from_secs(2)));
        releaser.join().unwrap();
        let next = fence.try_begin().expect("fence idle after drop");
        assert!(next.finish());
    }
}
