use gpui::{App, SharedString, SystemNotification, SystemNotificationAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    Available,
    Ready,
}

/// Priority tier for agent work notifications (`最终方案.md` §12.4 S3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyTier {
    /// Interactive approval or disambiguation required — MUST ALWAYS notify,
    /// even when the user enabled quiet / completion-only mode.
    ApprovalRequired,
    /// Routine batch/work completion summary — suppressed in quiet mode.
    BatchDelivered,
}

/// Pure policy deciding whether a work notification of `tier` should be
/// emitted given the user's `quiet_mode` setting.
#[must_use]
pub const fn should_emit_notification(
    tier: NotifyTier,
    quiet_mode: bool,
) -> bool {
    match tier {
        NotifyTier::ApprovalRequired => true,
        NotifyTier::BatchDelivered => !quiet_mode,
    }
}

pub fn show_update_notification(
    cx: &App,
    kind: NotifyKind,
    title: &str,
    body: &str,
    open_label: &str,
) {
    let tag = match kind {
        NotifyKind::Available => "ota-available",
        NotifyKind::Ready => "ota-ready",
    };
    cx.show_system_notification(SystemNotification {
        tag: SharedString::from(tag),
        title: SharedString::from(title.to_owned()),
        body: SharedString::from(body.to_owned()),
        actions: vec![SystemNotificationAction {
            id: SharedString::from("open"),
            label: SharedString::from(open_label.to_owned()),
        }],
    });
}

/// Emit a tiered work notification honoring [`should_emit_notification`].
/// Returns `true` when emitted and `false` when suppressed by `quiet_mode`.
pub fn show_work_notification(
    cx: &App,
    tier: NotifyTier,
    quiet_mode: bool,
    title: &str,
    body: &str,
    open_label: &str,
) -> bool {
    if !should_emit_notification(tier, quiet_mode) {
        return false;
    }
    let tag = match tier {
        NotifyTier::ApprovalRequired => "work-approval-required",
        NotifyTier::BatchDelivered => "work-batch-delivered",
    };
    cx.show_system_notification(SystemNotification {
        tag: SharedString::from(tag),
        title: SharedString::from(title.to_owned()),
        body: SharedString::from(body.to_owned()),
        actions: vec![SystemNotificationAction {
            id: SharedString::from("open"),
            label: SharedString::from(open_label.to_owned()),
        }],
    });
    true
}

#[cfg(test)]
mod tests {
    use super::{NotifyTier, should_emit_notification};

    #[test]
    fn approval_required_always_emits_even_in_quiet_mode() {
        assert!(should_emit_notification(
            NotifyTier::ApprovalRequired,
            false
        ));
        assert!(should_emit_notification(NotifyTier::ApprovalRequired, true));
    }

    #[test]
    fn batch_delivered_respects_quiet_mode() {
        assert!(should_emit_notification(NotifyTier::BatchDelivered, false));
        assert!(!should_emit_notification(NotifyTier::BatchDelivered, true));
    }
}
