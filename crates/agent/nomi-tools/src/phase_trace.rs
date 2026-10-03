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

/// Value of a [`ToolAttr`]. Text is limited to `'static` labels so a tool
/// cannot attach user-controlled strings (paths, patterns, output) to telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrValue {
    Int(i64),
    Bool(bool),
    Label(&'static str),
}

impl AttrValue {
    pub fn count(value: usize) -> Self {
        Self::Int(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAttr {
    pub key: &'static str,
    pub value: AttrValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolTrace {
    pub phases: Vec<ToolPhase>,
    pub attrs: Vec<ToolAttr>,
}

#[derive(Debug, Default)]
struct PhaseSink {
    phases: Mutex<Vec<ToolPhase>>,
    attrs: Mutex<Vec<ToolAttr>>,
}

impl PhaseSink {
    fn push(&self, name: &'static str, elapsed: Duration) {
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.phases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(ToolPhase { name, micros });
    }

    fn set_attr(&self, key: &'static str, value: AttrValue) {
        let mut attrs = self
            .attrs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match attrs.iter_mut().find(|attr| attr.key == key) {
            Some(existing) => existing.value = value,
            None => attrs.push(ToolAttr { key, value }),
        }
    }

    fn take(&self) -> ToolTrace {
        ToolTrace {
            phases: std::mem::take(
                &mut *self
                    .phases
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
            attrs: std::mem::take(
                &mut *self
                    .attrs
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
        }
    }
}

tokio::task_local! {
    static CURRENT: Arc<PhaseSink>;
}

/// Runs `future` with a phase sink installed and returns the phases and
/// attributes recorded inside it, including by clocks moved into spawned tasks.
pub async fn collect_trace<F: Future>(future: F) -> (F::Output, ToolTrace) {
    let sink = Arc::new(PhaseSink::default());
    let output = CURRENT.scope(Arc::clone(&sink), future).await;
    (output, sink.take())
}

/// Like [`collect_trace`] for callers that only need the phases.
pub async fn collect<F: Future>(future: F) -> (F::Output, Vec<ToolPhase>) {
    let (output, trace) = collect_trace(future).await;
    (output, trace.phases)
}

/// Attaches a scalar fact about the running tool call (for example how many
/// files a search matched). A later write to the same key replaces the earlier
/// one. Without an installed sink this is a no-op.
pub fn record_attr(key: &'static str, value: AttrValue) {
    let _ = CURRENT.try_with(|sink| sink.set_attr(key, value));
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

    #[tokio::test]
    async fn attrs_are_collected_and_last_write_wins() {
        let ((), trace) = collect_trace(async {
            record_attr("matched", AttrValue::count(3));
            record_attr("stop", AttrValue::Label("completed"));
            record_attr("matched", AttrValue::count(7));
            record_attr("narrowed", AttrValue::Bool(true));
        })
        .await;

        assert_eq!(
            trace.attrs,
            vec![
                ToolAttr {
                    key: "matched",
                    value: AttrValue::Int(7)
                },
                ToolAttr {
                    key: "stop",
                    value: AttrValue::Label("completed")
                },
                ToolAttr {
                    key: "narrowed",
                    value: AttrValue::Bool(true)
                },
            ]
        );
        assert!(trace.phases.is_empty());
    }

    #[tokio::test]
    async fn attrs_recorded_from_a_spawned_task_are_dropped_not_misattributed() {
        let ((), trace) = collect_trace(async {
            tokio::spawn(async {
                record_attr("orphan", AttrValue::Bool(true));
            })
            .await
            .unwrap();
        })
        .await;

        assert!(trace.attrs.is_empty());
    }

    #[tokio::test]
    async fn concurrent_scopes_do_not_share_attrs() {
        let first = collect_trace(async {
            record_attr("who", AttrValue::Label("first"));
        });
        let second = collect_trace(async {
            record_attr("who", AttrValue::Label("second"));
        });
        let ((_, a), (_, b)) = tokio::join!(first, second);

        assert_eq!(a.attrs[0].value, AttrValue::Label("first"));
        assert_eq!(b.attrs[0].value, AttrValue::Label("second"));
    }

    #[test]
    fn without_a_sink_attrs_are_noops() {
        record_attr("ignored", AttrValue::Bool(true));
    }

    #[test]
    fn count_saturates_instead_of_wrapping() {
        assert_eq!(AttrValue::count(5), AttrValue::Int(5));
        assert_eq!(AttrValue::count(usize::MAX), AttrValue::Int(i64::MAX));
    }
}
