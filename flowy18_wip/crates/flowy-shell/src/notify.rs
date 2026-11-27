use gpui::{App, SharedString, SystemNotification, SystemNotificationAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    Available,
    Ready,
}

fn send_open_notification(
    cx: &App,
    tag: &str,
    title: &str,
    body: &str,
    open_label: &str,
) {
    cx.show_system_notification(SystemNotification {
        tag: SharedString::from(tag.to_owned()),
        title: SharedString::from(title.to_owned()),
        body: SharedString::from(body.to_owned()),
        actions: vec![SystemNotificationAction {
            id: SharedString::from("open"),
            label: SharedString::from(open_label.to_owned()),
        }],
    });
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
    send_open_notification(cx, tag, title, body, open_label);
}

/// Show a desktop toast with an "open" action when an asynchronous task event
/// settles while the window is backgrounded.
pub fn show_task_notification(
    cx: &App,
    tag: &str,
    title: &str,
    body: &str,
    open_label: &str,
) {
    send_open_notification(cx, tag, title, body, open_label);
}
