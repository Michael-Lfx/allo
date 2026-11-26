//! Offline, opt-in measurements for the agent boundary.
//!
//! Run with `cargo test -p flowy-agent offline_agent_baseline -- --ignored --nocapture`.
//! These numbers do not include GPUI layout, rendering, or system idle wakeups.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::index::{SessionIndex, SessionRecord};
use crate::{AgentHandle, ChatLine, LlmConfig, TurnStatus};

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(size: usize, sample: usize) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "flowy-agent-perf-{}-{size}-{sample}-{stamp}",
            std::process::id()
        )))
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn mock_config() -> LlmConfig {
    LlmConfig {
        api_key: None,
        base_url: String::new(),
        model: String::new(),
        provider: String::new(),
        ..LlmConfig::default()
    }
}

fn report(label: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    let percentile = |percent: usize| {
        let index = (samples.len() * percent).div_ceil(100) - 1;
        samples[index].as_secs_f64() * 1000.0
    };
    println!(
        "{label}: n={} p50={:.3}ms p95={:.3}ms max={:.3}ms",
        samples.len(),
        percentile(50),
        percentile(95),
        samples.last().expect("nonempty samples").as_secs_f64() * 1000.0,
    );
}

#[test]
#[ignore = "manual offline performance baseline"]
fn offline_agent_baseline() {
    // Three independent boots per scale are intentionally few: this is a
    // diagnostic baseline, not a statistically rigorous benchmark.
    const BOOTS: usize = 3;
    const SNAPSHOTS: usize = 20;
    const TEXT: &str = "A representative offline message with enough text to make cloning visible.";

    println!(
        "offline Agent baseline; OS={}, arch={}, text={} bytes, boots={BOOTS}, snapshots per boot={SNAPSHOTS}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        TEXT.len(),
    );
    println!(
        "Construct time excludes fixture setup; first load includes boot and replay. No GPUI rendering is measured."
    );

    for size in [0, 100, 1_000, 10_000] {
        let mut constructs = Vec::new();
        let mut loads = Vec::new();
        let mut snapshots = Vec::new();
        for sample in 0..BOOTS {
            let root = ScratchRoot::new(size, sample);
            if size != 0 {
                let lines = (0..size)
                    .map(|index| {
                        if index % 2 == 0 {
                            ChatLine::User { text: TEXT.into() }
                        } else {
                            ChatLine::Assistant { text: TEXT.into() }
                        }
                    })
                    .collect();
                SessionIndex {
                    active: Some("perf-session".into()),
                    sessions: vec![SessionRecord {
                        execution: flowy_agent_core::ExecutionMetadata::default(
                        ),
                        id: "perf-session".into(),
                        title: "Offline performance fixture".into(),
                        updated_at: 1,
                        lines,
                        status: TurnStatus::Completed,
                        titled: true,
                        workspace: None,
                        origin: flowy_agent_core::SessionOrigin::default(),
                        session_workspace: None,
                        pinned: false,
                        archived: false,
                        ..SessionRecord::default()
                    }],
                }
                .save(&root.0)
                .expect("write fixture");
            }

            let start = Instant::now();
            let handle =
                AgentHandle::with_config(root.0.clone(), mock_config());
            constructs.push(start.elapsed());

            let start = Instant::now();
            let lines = handle.lines();
            loads.push(start.elapsed());
            assert_eq!(lines.len(), size, "fixture gained or lost messages");
            for (index, line) in lines.iter().take(size).enumerate() {
                let expected = if index % 2 == 0 {
                    ChatLine::User { text: TEXT.into() }
                } else {
                    ChatLine::Assistant { text: TEXT.into() }
                };
                assert_eq!(*line, expected, "fixture changed at line {index}");
            }
            drop(lines);

            // One warm-up call before taking repeated snapshot samples.
            std::hint::black_box(handle.lines());
            for _ in 0..SNAPSHOTS {
                let start = Instant::now();
                let lines = handle.lines();
                snapshots.push(start.elapsed());
                std::hint::black_box(lines);
            }
            drop(handle);
        }
        report(&format!("lines={size} construct"), &mut constructs);
        report(&format!("lines={size} first load"), &mut loads);
        report(&format!("lines={size} snapshot"), &mut snapshots);
    }
}
