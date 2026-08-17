//! Converge repeated Edit failures on the same path — soft then hard.

use crate::edit_hints::EditFailureKind;

/// Soft nudge after the same path fails edit repeatedly.
pub const CODING_EDIT_CONVERGE_NUDGE: &str = "Coding edit converge: the same file failed \
Edit several times in a row. Do not keep retrying near-identical hunks. \
Either (1) re-Read once and use fresh `line:hash` anchors (or a smaller unique old_string), \
or (2) stop and report the blocker. Claiming success without a successful edit is not allowed.";

/// Hard stop after soft converge was ignored.
pub const CODING_EDIT_HARD_STOP: &str = "Coding edit hard-stop: repeated Edit failures on the \
same path after a converge warning. Stop now. Summarize the blocker and the last error. \
Do not retry the same Edit again on this request.";

#[derive(Debug, Default)]
pub struct EditFailureTracker {
    path: Option<String>,
    streak: usize,
    last_kind: Option<EditFailureKind>,
    soft_nudge_sent: bool,
    hard_stop_sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditConvergeAction {
    None,
    SoftNudge,
    HardStop,
}

impl EditFailureTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Observe one completed Edit / Write call.
    ///
    /// `soft_threshold`: fire soft nudge. `hard_extra`: additional failures after
    /// soft nudge before hard stop (total failures ≈ soft + hard_extra).
    pub fn observe(
        &mut self,
        path: Option<&str>,
        success: bool,
        kind: Option<EditFailureKind>,
        soft_threshold: usize,
        hard_extra: usize,
    ) -> EditConvergeAction {
        let soft_threshold = soft_threshold.max(1);
        let hard_at = soft_threshold.saturating_add(hard_extra.max(1));
        if success {
            self.reset();
            return EditConvergeAction::None;
        }
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            self.path = None;
            self.streak = self.streak.saturating_add(1);
            self.last_kind = kind.or(self.last_kind);
            return self.action_for_streak(soft_threshold, hard_at);
        };
        match self.path.as_deref() {
            Some(prev) if prev == path => {
                self.streak = self.streak.saturating_add(1);
            }
            _ => {
                self.path = Some(path.to_string());
                self.streak = 1;
                self.soft_nudge_sent = false;
                self.hard_stop_sent = false;
            }
        }
        self.last_kind = kind.or(self.last_kind);
        self.action_for_streak(soft_threshold, hard_at)
    }

    fn action_for_streak(&mut self, soft_threshold: usize, hard_at: usize) -> EditConvergeAction {
        if self.streak >= hard_at && !self.hard_stop_sent {
            self.hard_stop_sent = true;
            return EditConvergeAction::HardStop;
        }
        if self.streak >= soft_threshold && !self.soft_nudge_sent {
            self.soft_nudge_sent = true;
            return EditConvergeAction::SoftNudge;
        }
        EditConvergeAction::None
    }

    pub fn streak(&self) -> usize {
        self.streak
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn last_kind(&self) -> Option<EditFailureKind> {
        self.last_kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_then_hard() {
        let mut t = EditFailureTracker::default();
        assert_eq!(
            t.observe(Some("a.rs"), false, None, 2, 1),
            EditConvergeAction::None
        );
        assert_eq!(
            t.observe(Some("a.rs"), false, None, 2, 1),
            EditConvergeAction::SoftNudge
        );
        assert_eq!(
            t.observe(Some("a.rs"), false, None, 2, 1),
            EditConvergeAction::HardStop
        );
    }

    #[test]
    fn success_clears() {
        let mut t = EditFailureTracker::default();
        let _ = t.observe(Some("a.rs"), false, None, 2, 1);
        assert_eq!(
            t.observe(Some("a.rs"), true, None, 2, 1),
            EditConvergeAction::None
        );
        assert_eq!(t.streak(), 0);
    }
}
