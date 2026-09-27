//! Process-scoped hot-path performance counters.
//!
//! An agent turn is dominated by a handful of side-effects that sit inside the
//! async loop but are not themselves async: the workspace `git` probe, session
//! checkpoint IO, token estimation over the whole transcript, per-call tool gate
//! decisions, and compaction bookkeeping. Each is timed here under a stable name
//! so a host can report the numbers ([`snapshot`]) and so a regression shows up
//! as a counter whose shape changed, without threading a handle through every
//! call site in a 4k-line engine loop.
//!
//! The counters are process-global and monotonic. That is deliberate: a "did this
//! path run, and how long did it take" metric must survive the borrow patterns of
//! the engine loop, and the delta across one turn is exactly the data an
//! iteration report needs. [`reset`] exists for tests and for hosts that report
//! per-turn deltas instead of per-process totals.
//!
//! Nothing here allocates on the recording path and nothing here is
//! authoritative for correctness: a missed sample changes a report, never the
//! conversation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Stable hot-path names.
///
/// The mapping to [`HotPath::as_str`] is a reporting contract: a name is never
/// renamed or reused for different work, because historical reports are keyed by
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotPath {
    /// Blocking `git` probe of the workspace used by the Horizon ledger.
    WorkspaceFingerprint,
    /// Session serialization plus the atomic checkpoint write.
    SessionCheckpoint,
    /// Token estimation over the system prompt, tool table, and transcript.
    TokenEstimate,
    /// One invocation-gate decision inside tool execution.
    ToolGateDecision,
    /// Tool-call schema preparation and canonicalization.
    ToolCallPrepare,
    /// Compaction fold planning and summarizer prompt assembly.
    CompactionFold,
    /// Bounded observation text accumulation from a provider stream.
    ObservationAccumulate,
    /// Session index/filesystem lookup during resume.
    SessionLookup,
}

impl HotPath {
    /// Every path, in a stable order, for reporting loops.
    pub const ALL: [HotPath; 8] = [
        HotPath::WorkspaceFingerprint,
        HotPath::SessionCheckpoint,
        HotPath::TokenEstimate,
        HotPath::ToolGateDecision,
        HotPath::ToolCallPrepare,
        HotPath::CompactionFold,
        HotPath::ObservationAccumulate,
        HotPath::SessionLookup,
    ];

    /// The reporting key. Stable across releases.
    pub const fn as_str(self) -> &'static str {
        match self {
            HotPath::WorkspaceFingerprint => "workspace_fingerprint",
            HotPath::SessionCheckpoint => "session_checkpoint",
            HotPath::TokenEstimate => "token_estimate",
            HotPath::ToolGateDecision => "tool_gate_decision",
            HotPath::ToolCallPrepare => "tool_call_prepare",
            HotPath::CompactionFold => "compaction_fold",
            HotPath::ObservationAccumulate => "observation_accumulate",
            HotPath::SessionLookup => "session_lookup",
        }
    }
}

/// One path's counters, shaped for a reporting layer to serialize.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HotPathSample {
    pub path: &'static str,
    pub count: u64,
    pub total_ms: u64,
    pub max_ms: u64,
    pub last_ms: u64,
}

#[derive(Debug)]
struct SlotCounters {
    count: AtomicU64,
    total_nanos: AtomicU64,
    max_nanos: AtomicU64,
    last_nanos: AtomicU64,
}

impl SlotCounters {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            total_nanos: AtomicU64::new(0),
            max_nanos: AtomicU64::new(0),
            last_nanos: AtomicU64::new(0),
        }
    }
}

const SLOTS: usize = HotPath::ALL.len();

static COUNTERS: [SlotCounters; SLOTS] = [const { SlotCounters::new() }; SLOTS];

fn slot(path: HotPath) -> &'static SlotCounters {
    &COUNTERS[path as usize]
}

/// Record one observation of `path`.
///
/// Saturating rather than panicking on overflow: a report that clamps after
/// ~584 years of accumulated nanoseconds must never take a turn down.
pub fn record(path: HotPath, elapsed: Duration) {
    let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
    let slot = slot(path);
    slot.count.fetch_add(1, Ordering::Relaxed);
    slot.total_nanos.fetch_add(nanos, Ordering::Relaxed);
    slot.max_nanos.fetch_max(nanos, Ordering::Relaxed);
    slot.last_nanos.store(nanos, Ordering::Relaxed);
}

/// Time `body`, record it under `path`, and return its value.
pub fn timed<T>(path: HotPath, body: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = body();
    record(path, started.elapsed());
    value
}

/// [`timed`] for an awaitable body.
pub async fn timed_async<T>(path: HotPath, body: impl std::future::Future<Output = T>) -> T {
    let started = Instant::now();
    let value = body.await;
    record(path, started.elapsed());
    value
}

/// Every path's counters. Paths that never ran are present with zeros, so a
/// consumer can distinguish "did not run" from "not instrumented".
pub fn snapshot() -> Vec<HotPathSample> {
    HotPath::ALL
        .into_iter()
        .map(|path| {
            let slot = slot(path);
            HotPathSample {
                path: path.as_str(),
                count: slot.count.load(Ordering::Relaxed),
                total_ms: nanos_to_ms(slot.total_nanos.load(Ordering::Relaxed)),
                max_ms: nanos_to_ms(slot.max_nanos.load(Ordering::Relaxed)),
                last_ms: nanos_to_ms(slot.last_nanos.load(Ordering::Relaxed)),
            }
        })
        .collect()
}

/// Zero every counter. Hosts that report per-turn deltas call this at a turn
/// boundary; the engine itself never resets, so process totals stay intact.
pub fn reset() {
    for slot in &COUNTERS {
        slot.count.store(0, Ordering::Relaxed);
        slot.total_nanos.store(0, Ordering::Relaxed);
        slot.max_nanos.store(0, Ordering::Relaxed);
        slot.last_nanos.store(0, Ordering::Relaxed);
    }
}

fn nanos_to_ms(nanos: u64) -> u64 {
    nanos / 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deltas, never absolutes: the counters are process-global and the test
    /// binary runs cases in parallel.
    fn counts(path: HotPath) -> (u64, u64, u64) {
        let sample = snapshot()
            .into_iter()
            .find(|sample| sample.path == path.as_str())
            .expect("every declared path reports");
        (sample.count, sample.total_ms, sample.max_ms)
    }

    #[test]
    fn record_accumulates_count_total_and_max() {
        let before = counts(HotPath::ToolGateDecision);

        record(HotPath::ToolGateDecision, Duration::from_millis(3));
        record(HotPath::ToolGateDecision, Duration::from_millis(11));

        let after = counts(HotPath::ToolGateDecision);
        assert!(
            after.0 >= before.0 + 2,
            "both observations must be counted: {before:?} -> {after:?}"
        );
        assert!(after.1 >= before.1 + 14, "total must sum both samples");
        assert!(after.2 >= 11, "max must keep the largest sample seen");
        assert_eq!(
            after.2,
            after.1.min(after.2).max(11),
            "max is monotonic and never below the largest record"
        );
    }

    #[test]
    fn snapshot_reports_every_declared_path_with_a_stable_name() {
        let snapshot = snapshot();
        assert_eq!(snapshot.len(), HotPath::ALL.len());
        for (sample, path) in snapshot.iter().zip(HotPath::ALL) {
            assert_eq!(sample.path, path.as_str());
            assert!(!sample.path.is_empty());
            assert!(!sample.path.contains(char::is_whitespace));
        }
    }

    #[test]
    fn timed_records_once_and_returns_the_value() {
        let before = counts(HotPath::CompactionFold);

        let value = timed(HotPath::CompactionFold, || 21 * 2);

        assert_eq!(value, 42);
        assert!(counts(HotPath::CompactionFold).0 >= before.0 + 1);
    }

    #[tokio::test]
    async fn timed_async_records_once_and_returns_the_value() {
        let before = counts(HotPath::SessionLookup);

        let value = timed_async(HotPath::SessionLookup, async { "loaded" }).await;

        assert_eq!(value, "loaded");
        assert!(counts(HotPath::SessionLookup).0 >= before.0 + 1);
    }

    #[test]
    fn reset_zeroes_every_counter() {
        record(HotPath::ObservationAccumulate, Duration::from_millis(5));
        assert!(counts(HotPath::ObservationAccumulate).0 >= 1);

        reset();

        for sample in snapshot() {
            assert_eq!(sample.count, 0, "{} must be zeroed", sample.path);
            assert_eq!(sample.total_ms, 0);
            assert_eq!(sample.max_ms, 0);
            assert_eq!(sample.last_ms, 0);
        }
    }

    #[test]
    fn nanos_convert_to_whole_milliseconds() {
        assert_eq!(nanos_to_ms(0), 0);
        assert_eq!(nanos_to_ms(999_999), 0);
        assert_eq!(nanos_to_ms(1_000_000), 1);
        assert_eq!(nanos_to_ms(1_500_000), 1);
    }
}
