use gpui::{App, SharedString, SystemNotification, SystemNotificationAction};

const AUTOMATION_TAG: &str = "automation-run:";

pub fn automation_response<'a>(
    tag: &'a str,
    action: Option<&str>,
) -> Option<&'a str> {
    let run_id = tag.strip_prefix(AUTOMATION_TAG)?;
    (matches!(action, None | Some("open"))
        && super::deep_link::valid_run_route(run_id))
    .then_some(run_id)
}

/// Submit an opaque run route. Show only generic product copy on the OS surface,
/// which can outlive the account that was active when it was submitted.
pub fn show_automation_notification(
    cx: &App,
    run_id: &str,
    title: &str,
    body: &str,
    open_label: &str,
) -> bool {
    if !super::deep_link::valid_run_route(run_id) {
        return false;
    }
    cx.show_system_notification(SystemNotification {
        tag: format!("{AUTOMATION_TAG}{run_id}").into(),
        title: title.to_owned().into(),
        body: body.to_owned().into(),
        actions: vec![SystemNotificationAction {
            id: "open".into(),
            label: open_label.to_owned().into(),
        }],
    });
    true
}

#[cfg(test)]
mod tests {
    use super::automation_response;

    #[test]
    fn automation_tag_handles_body_and_open_but_rejects_extra_route_data() {
        let id = "0191f3a8-fb01-73f0-8abe-fa324261fe20";
        let tag = format!("automation-run:{id}");
        assert_eq!(automation_response(&tag, None), Some(id));
        assert_eq!(automation_response(&tag, Some("open")), Some(id));
        assert_eq!(automation_response(&tag, Some("other")), None);
        assert_eq!(
            automation_response(&format!("{tag}/account/42"), None),
            None
        );
        assert_eq!(automation_response("ota-ready", Some("open")), None);
        assert!(matches!(
            crate::deep_link::parse_with_scheme(&format!("flowy://automation-run/{id}"), "flowy"),
            Some(crate::DeepLink::AutomationRun { run_id }) if run_id == id
        ));
    }
}

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
