//! Todo / plan continuation at natural EndTurn (Vetta-inspired).
//!
//! `update_plan` is Allo's checklist tool. At a natural stop we may inject an
//! ephemeral nudge so the model finishes remaining steps — with different
//! strictness for locked (host-prefilled) vs unlocked (model-owned) plans.

use serde::{Deserialize, Serialize};

/// How strictly incomplete plans force another turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoContinuationMode {
    /// Model-owned plan: nudge once per pending signature, then allow stop.
    #[default]
    Unlocked,
    /// Host-locked plan: keep continuing until all steps are completed (still
    /// subject to the global system-continuation budget).
    Locked,
    /// Never inject todo continuations.
    Off,
}

/// One plan step as observed from an `update_plan` tool result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStepView {
    pub content: String,
    pub status: String,
}

/// Snapshot of the latest plan used for continuation decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanSnapshot {
    pub steps: Vec<PlanStepView>,
}

impl PlanSnapshot {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn pending(&self) -> Vec<&PlanStepView> {
        self.steps
            .iter()
            .filter(|s| !is_completed_status(&s.status))
            .collect()
    }

    pub fn all_completed(&self) -> bool {
        !self.steps.is_empty() && self.pending().is_empty()
    }

    /// Stable signature of incomplete work (content + status).
    pub fn pending_signature(&self) -> String {
        self.pending()
            .iter()
            .map(|s| format!("{}:{}", s.content, s.status))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn is_completed_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "done" | "complete"
    )
}

/// Parse `update_plan` tool result JSON (`kind=plan_update`).
pub fn parse_plan_update_content(content: &str) -> Option<PlanSnapshot> {
    // Tool may prefix soft notes before the JSON payload.
    let json_start = content.find('{')?;
    let value: serde_json::Value = serde_json::from_str(content[json_start..].trim()).ok()?;
    if value.get("kind").and_then(|k| k.as_str()) != Some("plan_update") {
        return None;
    }
    let entries = value.get("entries")?.as_array()?;
    let mut steps = Vec::with_capacity(entries.len());
    for e in entries {
        let content = e
            .get("content")
            .or_else(|| e.get("step"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = e
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending")
            .to_string();
        if content.is_empty() {
            continue;
        }
        steps.push(PlanStepView { content, status });
    }
    if steps.is_empty() {
        None
    } else {
        Some(PlanSnapshot { steps })
    }
}

/// Mutable tracker owned by [`crate::CodingHarness`].
#[derive(Debug, Default)]
pub struct TodoContinuationTracker {
    mode: TodoContinuationMode,
    latest: PlanSnapshot,
    /// Last signature we already nudged for (unlocked mode).
    last_nudge_signature: Option<String>,
}

impl TodoContinuationTracker {
    pub fn new(mode: TodoContinuationMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        self.latest = PlanSnapshot::default();
        self.last_nudge_signature = None;
    }

    pub fn set_mode(&mut self, mode: TodoContinuationMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> TodoContinuationMode {
        self.mode
    }

    pub fn observe_plan(&mut self, snapshot: PlanSnapshot) {
        // Progress (signature change) clears the unlocked one-shot latch.
        if let Some(prev) = self.last_nudge_signature.as_ref() {
            if prev != &snapshot.pending_signature() {
                self.last_nudge_signature = None;
            }
        }
        self.latest = snapshot;
    }

    pub fn clear_plan(&mut self) {
        self.latest = PlanSnapshot::default();
        self.last_nudge_signature = None;
    }

    pub fn latest(&self) -> &PlanSnapshot {
        &self.latest
    }

    /// Build a continuation nudge, or `None` if the agent may stop.
    pub fn continuation_nudge(&mut self) -> Option<String> {
        if matches!(self.mode, TodoContinuationMode::Off) {
            return None;
        }
        if self.latest.is_empty() || self.latest.all_completed() {
            return None;
        }
        let pending = self.latest.pending();
        if pending.is_empty() {
            return None;
        }
        let signature = self.latest.pending_signature();
        let done = self.latest.steps.len() - pending.len();
        let total = self.latest.steps.len();
        let next = pending[0];
        let list = pending
            .iter()
            .enumerate()
            .map(|(i, s)| format!("  {}. [{}] {}", i + 1, s.status, s.content))
            .collect::<Vec<_>>()
            .join("\n");

        match self.mode {
            TodoContinuationMode::Off => None,
            TodoContinuationMode::Locked => Some(format!(
                "[ephemeral:todo] You have {} uncompleted plan steps ({done}/{total} done). \
                 You MUST continue before stopping.\n\nRemaining:\n{list}\n\n\
                 Work on the first pending step next: \"{}\". Update `update_plan` when the \
                 milestone changes. Do not stop with incomplete plan steps.",
                pending.len(),
                next.content
            )),
            TodoContinuationMode::Unlocked => {
                if self.last_nudge_signature.as_deref() == Some(signature.as_str()) {
                    // Same pending set after a prior nudge — release so a stale
                    // plan cannot trap the user after a redirect.
                    self.last_nudge_signature = None;
                    return None;
                }
                self.last_nudge_signature = Some(signature);
                Some(format!(
                    "[ephemeral:todo] You still have {} uncompleted plan steps ({done}/{total} done):\n{list}\n\n\
                     If this plan still applies, keep going — work on \"{}\" next and call \
                     `update_plan` when status changes. If the user's latest request superseded \
                     this plan, submit an `update_plan` with a completed/abandoned snapshot \
                     that matches what they actually want, then proceed.",
                    pending.len(),
                    next.content
                ))
            }
        }
    }
}

pub const TODO_CONTINUATION_BUDGET_EXHAUSTED: &str = "\
[ephemeral:todo] System continuation budget exhausted while plan steps remain incomplete. \
Stop now: summarize what is done, what is blocked, and the remaining plan steps. \
Do not start a new open-ended tool tour.";

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(steps: &[(&str, &str)]) -> PlanSnapshot {
        PlanSnapshot {
            steps: steps
                .iter()
                .map(|(c, s)| PlanStepView {
                    content: (*c).into(),
                    status: (*s).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn parse_plan_update_strips_prefix() {
        let raw = "[progress] note\n{\"kind\":\"plan_update\",\"entries\":[{\"content\":\"A\",\"status\":\"pending\"}]}";
        let p = parse_plan_update_content(raw).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].content, "A");
    }

    #[test]
    fn unlocked_nudges_once_then_releases() {
        let mut t = TodoContinuationTracker::new(TodoContinuationMode::Unlocked);
        t.observe_plan(snap(&[("A", "pending"), ("B", "pending")]));
        assert!(t.continuation_nudge().is_some());
        assert!(t.continuation_nudge().is_none());
    }

    #[test]
    fn locked_always_nudges() {
        let mut t = TodoContinuationTracker::new(TodoContinuationMode::Locked);
        t.observe_plan(snap(&[("A", "pending")]));
        assert!(t.continuation_nudge().is_some());
        assert!(t.continuation_nudge().is_some());
    }

    #[test]
    fn completed_plan_allows_stop() {
        let mut t = TodoContinuationTracker::new(TodoContinuationMode::Locked);
        t.observe_plan(snap(&[("A", "completed")]));
        assert!(t.continuation_nudge().is_none());
    }

    #[test]
    fn progress_resets_unlocked_latch() {
        let mut t = TodoContinuationTracker::new(TodoContinuationMode::Unlocked);
        t.observe_plan(snap(&[("A", "pending"), ("B", "pending")]));
        assert!(t.continuation_nudge().is_some());
        t.observe_plan(snap(&[("A", "completed"), ("B", "pending")]));
        assert!(t.continuation_nudge().is_some());
    }
}
