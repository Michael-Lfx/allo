//! Harness KPIs for coding/office turns. Record before changing budgets.
//!
//! These numbers exist so 32KB limits, keep-recent, and thinking budgets are
//! not tuned by feel. The engine logs [`HarnessKpi::summary_line`] at EndTurn.

use std::collections::HashSet;
use std::time::Instant;

/// Per-root-request harness telemetry.
#[derive(Debug, Clone)]
pub struct HarnessKpi {
    /// Tool calls in the most recent assistant message.
    pub tools_this_assistant_turn: usize,
    /// Assistant messages that contained at least one tool call.
    pub assistant_turns_with_tools: usize,
    /// Sum of tool calls this request.
    pub total_tool_calls: usize,
    /// Recon-only parent turns (Read/Grep/Glob/non-verify Bash).
    pub recon_only_turns: usize,
    /// Recon-only turns that issued exactly one parent tool.
    pub serial_recon_turns: usize,
    unique_read_keys: HashSet<String>,
    reread_keys: HashSet<String>,
    /// Elapsed from request start to first successful Edit/Write.
    pub time_to_first_edit_ms: Option<u64>,
    first_edit_recorded: bool,
    /// True when a verify-like command succeeded after a file mutation.
    pub verify_before_end: bool,
    pub contributor_ms: u64,
    pub checkpoint_ms: u64,
    pub ttft_ms: Option<u64>,
    pub tool_wall_ms: u64,
    request_started: Instant,
}

impl Default for HarnessKpi {
    fn default() -> Self {
        Self {
            tools_this_assistant_turn: 0,
            assistant_turns_with_tools: 0,
            total_tool_calls: 0,
            recon_only_turns: 0,
            serial_recon_turns: 0,
            unique_read_keys: HashSet::new(),
            reread_keys: HashSet::new(),
            time_to_first_edit_ms: None,
            first_edit_recorded: false,
            verify_before_end: false,
            contributor_ms: 0,
            checkpoint_ms: 0,
            ttft_ms: None,
            tool_wall_ms: 0,
            request_started: Instant::now(),
        }
    }
}

impl HarnessKpi {
    pub fn reset_for_user_request(&mut self) {
        *self = Self::default();
    }

    pub fn observe_assistant_tools(&mut self, n: usize) {
        self.tools_this_assistant_turn = n;
        if n > 0 {
            self.assistant_turns_with_tools = self.assistant_turns_with_tools.saturating_add(1);
            self.total_tool_calls = self.total_tool_calls.saturating_add(n);
        }
    }

    pub fn observe_recon_turn(&mut self, serial: bool) {
        self.recon_only_turns = self.recon_only_turns.saturating_add(1);
        if serial {
            self.serial_recon_turns = self.serial_recon_turns.saturating_add(1);
        }
    }

    pub fn observe_read_key(&mut self, key: &str) {
        if key.is_empty() {
            return;
        }
        if !self.unique_read_keys.insert(key.to_string()) {
            self.reread_keys.insert(key.to_string());
        }
    }

    pub fn unique_path_reread_rate(&self) -> f32 {
        let n = self.unique_read_keys.len();
        if n == 0 {
            0.0
        } else {
            self.reread_keys.len() as f32 / n as f32
        }
    }

    pub fn observe_edit(&mut self) {
        if self.first_edit_recorded {
            return;
        }
        self.first_edit_recorded = true;
        self.time_to_first_edit_ms = Some(self.request_started.elapsed().as_millis() as u64);
    }

    pub fn add_contributor_ms(&mut self, ms: u64) {
        self.contributor_ms = self.contributor_ms.saturating_add(ms);
    }

    pub fn add_checkpoint_ms(&mut self, ms: u64) {
        self.checkpoint_ms = self.checkpoint_ms.saturating_add(ms);
    }

    pub fn add_tool_wall_ms(&mut self, ms: u64) {
        self.tool_wall_ms = self.tool_wall_ms.saturating_add(ms);
    }

    pub fn note_ttft_ms(&mut self, ms: u64) {
        if self.ttft_ms.is_none() {
            self.ttft_ms = Some(ms);
        }
    }

    pub fn summary_line(&self) -> String {
        format!(
            "harness_kpi tools_per_turn={} turns_with_tools={} total_tools={} \
             recon_turns={} serial_recon={} reread_rate={:.2} ttfe_ms={} verify_before_end={} \
             contributor_ms={} checkpoint_ms={} ttft_ms={} tool_wall_ms={}",
            self.tools_this_assistant_turn,
            self.assistant_turns_with_tools,
            self.total_tool_calls,
            self.recon_only_turns,
            self.serial_recon_turns,
            self.unique_path_reread_rate(),
            self.time_to_first_edit_ms
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            self.verify_before_end,
            self.contributor_ms,
            self.checkpoint_ms,
            self.ttft_ms
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            self.tool_wall_ms,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reread_rate_counts_repeat_keys_only() {
        let mut kpi = HarnessKpi::default();
        kpi.observe_read_key("a.rs:0:500");
        kpi.observe_read_key("b.rs:0:500");
        kpi.observe_read_key("a.rs:0:500");
        assert_eq!(kpi.unique_read_keys.len(), 2);
        assert!((kpi.unique_path_reread_rate() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn first_edit_records_once() {
        let mut kpi = HarnessKpi::default();
        kpi.observe_edit();
        let first = kpi.time_to_first_edit_ms;
        kpi.observe_edit();
        assert_eq!(kpi.time_to_first_edit_ms, first);
        assert!(first.is_some());
    }
}
