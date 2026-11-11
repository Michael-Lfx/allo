//! Fast-path latency instrumentation and metrics tracking.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.9.

use std::time::Duration;

/// Tracks fast-path voice pipeline latency distribution from VAD silence point to speaker output.
#[derive(Debug, Default, Clone)]
pub struct FastPathLatencyTracker {
    samples: Vec<Duration>,
}

impl FastPathLatencyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_sample(&mut self, sample: Duration) {
        self.samples.push(sample);
    }

    pub fn p50(&self) -> Duration {
        self.percentile(50.0)
    }

    pub fn p95(&self) -> Duration {
        self.percentile(95.0)
    }

    pub fn p99(&self) -> Duration {
        self.percentile(99.0)
    }

    pub fn percentile(&self, p: f64) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.samples.clone();
        sorted.sort();
        let idx = (((p / 100.0) * (sorted.len() as f64)).ceil() as usize).saturating_sub(1);
        let safe_idx = idx.min(sorted.len() - 1);
        sorted[safe_idx]
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}
