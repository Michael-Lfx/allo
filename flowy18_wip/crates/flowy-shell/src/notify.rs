use gpui::{App, SharedString, SystemNotification, SystemNotificationAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    Available,
    Ready,
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
