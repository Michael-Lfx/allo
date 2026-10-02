use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPhase {
    pub name: &'static str,
    pub micros: u64,
}

#[derive(Debug, Default)]
struct PhaseSink {
    phases: Mutex<Vec<ToolPhase>>,
}

impl PhaseSink {
    fn push(&self, name: &'static str, elapsed: Duration) {
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.phases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(ToolPhase { name, micros });
    }

    fn take(&self) -> Vec<ToolPhase> {
        std::mem::take(
            &mut *self
                .phases
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

tokio::task_local! {
    static CURRENT: Arc<PhaseSink>;
}

/// Runs `future` with a phase sink installed and returns the phases recorded by
/// every [`PhaseClock`] created inside it, including clocks moved into spawned
/// tasks.
pub async fn collect<F: Future>(future: F) -> (F::Output, Vec<ToolPhase>) {
    let sink = Arc::new(PhaseSink::default());
    let output = CURRENT.scope(Arc::clone(&sink), future).await;
    (output, sink.take())
}

/// Records the time between consecutive marks. Without an installed sink every
/// call is a no-op, so tools can mark phases unconditionally.
pub struct PhaseClock {
    sink: Option<Arc<PhaseSink>>,
    last: Instant,
}

impl PhaseClock {
    pub fn start() -> Self {
        Self {
            sink: CURRENT.try_with(Arc::clone).ok(),
            last: Instant::now(),
        }
    }

    pub fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        if let Some(sink) = &self.sink {
            sink.push(name, now.duration_since(self.last));
        }
        self.last = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn marks_record_consecutive_intervals() {
        let ((), phases) = collect(async {
            let mut clock = PhaseClock::start();
            tokio::time::sleep(Duration::from_millis(20)).await;
            clock.mark("first");
            clock.mark("second");
        })
        .await;

        let names: Vec<_> = phases.iter().map(|phase| phase.name).collect();
        assert_eq!(names, ["first", "second"]);
        assert!(phases[0].micros >= 15_000, "{phases:?}");
        assert!(phases[1].micros < phases[0].micros);
    }

    #[tokio::test]
    async fn clock_moved_into_spawned_task_still_records() {
        let ((), phases) = collect(async {
            let mut clock = PhaseClock::start();
            tokio::spawn(async move {
                clock.mark("spawned");
            })
            .await
            .unwrap();
        })
        .await;

        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].name, "spawned");
    }

    #[tokio::test]
    async fn concurrent_scopes_do_not_share_phases() {
        let first = collect(async {
            PhaseClock::start().mark("a");
        });
        let second = collect(async {
            let mut clock = PhaseClock::start();
            clock.mark("b");
            clock.mark("c");
        });
        let ((_, a), (_, b)) = tokio::join!(first, second);

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn without_a_sink_marks_are_noops() {
        let mut clock = PhaseClock::start();
        clock.mark("ignored");
    }
}
