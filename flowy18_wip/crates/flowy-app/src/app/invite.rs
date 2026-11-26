use super::FlowyApp;
use flowy_ui as theme;
use gpui::{ClipboardItem, Context, SharedString, div, prelude::*, px, rgb};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Share,
    Terms,
    History,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    pub code: String,
    pub emails: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    code: String,
    emails: Vec<String>,
}

const VERSION: u32 = 1;
const SOCIAL: &[(&str, &str)] = &[
    ("wa", "W"),
    ("fb", "f"),
    ("x", "X"),
    ("in", "in"),
    ("rd", "r"),
];

impl FlowyApp {
    pub(super) fn open_invite_with_status(
        &mut self,
        status: Option<&'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        self.activate_dialog(super::state::ActiveDialog::Invite, cx);
        self.invite.panel = Panel::Share;
        self.invite.status = status;
        self.invite.pending_open = false;
        self.user_menu = false;
        cx.notify();
    }

    pub(super) fn open_invite(&mut self, cx: &mut Context<'_, Self>) {
        self.open_invite_with_status(None, cx);
    }

    pub(super) fn close_invite(&mut self, cx: &mut Context<'_, Self>) {
        self.active_dialog = super::state::ActiveDialog::None;
        self.invite.status = None;
        self.invite.pending_open = false;
        cx.notify();
    }

    fn copy_invite(&mut self, cx: &mut Context<'_, Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.invite_link()));
        self.invite.status = Some("invite.copied");
        cx.notify();
    }

    fn share_invite(&mut self, cx: &mut Context<'_, Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.invite_link()));
        self.invite.status = Some("invite.share_local");
        cx.notify();
    }

    fn send_invite(&mut self, cx: &mut Context<'_, Self>) {
        let email = self.invite.email.read(cx).value();
        if !remember(&mut self.invite.emails, &email) {
            self.invite.status = Some("invite.bad_email");
            cx.notify();
            return;
        }
        save(self.store.root(), &self.invite.code, &self.invite.emails);
        // The saved address is already in `self.invite.emails`; clearing the
        // editor only resets the next entry, so no change event is needed.
        self.set_input_text(&self.invite.email, "", cx);
        self.invite.status = Some("invite.email_saved");
        cx.notify();
    }

    fn show_panel(&mut self, panel: Panel, cx: &mut Context<'_, Self>) {
        self.invite.panel = panel;
        cx.notify();
    }

    fn invite_link(&self) -> String {
        format!(
            "{}://invite/{}",
            crate::brand::url_scheme(),
            self.invite.code
        )
    }

    pub(super) fn invite_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::modal_layer(
            "invite-layer",
            &self.theme,
            &self.ui,
            self.invite_card(cx),
        )
    }

    fn invite_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::dialog(
            "invite-card",
            self.t("invite.title"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(440.))
        .max_h(px(640.))
        .overflow_y_scroll()
        .bg(self.theme.card())
        .rounded(self.theme.modal_radius())
        .shadow_md()
        .px_6()
        .pt_4()
        .pb_5()
        .flex()
        .flex_col()
        .gap_3()
        .on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
        .child(self.invite_close(cx))
        .child(self.invite_body(cx))
        .children(self.invite.status.map(|key| {
            let text = if key == "invite.deep_link_received" {
                if let Some(code) = &self.invite.inbound_code {
                    self.tf(key, &[("code", code)])
                } else {
                    self.t(key)
                }
            } else {
                self.t(key)
            };
            div()
                .text_size(self.theme.caption_size())
                .text_color(self.theme.muted())
                .child(text)
        }))
        .child(self.invite_footer(cx))
    }

    fn invite_close(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div().flex().justify_end().child(
            theme::IconButton::new(
                "close-invite",
                self.t("ui.close"),
                theme::icons::x(),
            )
            .variant(theme::ControlVariant::Ghost)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.close_invite(cx);
                }),
            ),
        )
    }

    fn invite_body(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        match self.invite.panel {
            Panel::Share => self.share_body(cx),
            Panel::Terms => self.terms_body(),
            Panel::History => self.history_body(),
        }
    }

    fn share_body(&self, cx: &Context<'_, Self>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(envelope(&self.theme))
            .child(self.invite_heading("invite.title"))
            .child(self.invite_copy("invite.body"))
            .child(self.invite_copy("invite.footnote"))
            .child(self.share_row(cx))
            .child(self.invite_action_button(
                "copy-invite",
                "invite.copy",
                true,
                cx,
            ))
            .child(self.social_row(cx))
            .child(self.email_block(cx))
    }

    fn terms_body(&self) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.invite_heading("invite.terms"))
            .child(self.invite_copy("invite.terms_body"))
    }

    fn history_body(&self) -> gpui::Div {
        let mut column = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.invite_heading("invite.history"));
        if self.invite.emails.is_empty() {
            column = column.child(self.invite_copy("invite.history_empty"));
        }
        for email in &self.invite.emails {
            column = column.child(
                div().text_size(self.theme.body_size()).child(email.clone()),
            );
        }
        column
    }

    fn share_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .gap_2()
            .child(self.quiet_button("share-web", "invite.share_web", cx))
            .child(self.quiet_button("share-phone", "invite.share_phone", cx))
    }

    fn social_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div().flex().items_center().justify_center().gap_3();
        for (id, mark) in SOCIAL {
            let element_id = SharedString::from(format!("share-{id}"));
            row = row.child(
                theme::IconButton::new(
                    element_id,
                    self.t("invite.share_web"),
                    div().text_size(self.theme.caption_size()).child(*mark),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Medium)
                .render(
                    &self.theme,
                    cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.share_invite(cx);
                    }),
                ),
            );
        }
        row
    }

    fn email_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .child(self.t("invite.email_title")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.invite_email_field(cx))
                    .child(self.invite_action_button(
                        "send-invite",
                        "invite.send",
                        false,
                        cx,
                    )),
            )
    }

    fn invite_email_field(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::TextField::new(
            "invite-email",
            self.t("invite.email_title"),
            self.invite.email.clone(),
        )
        .placeholder(self.t("invite.email_placeholder"))
        .render(&self.theme, cx)
        .flex_1()
    }

    fn invite_footer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        if self.invite.panel == Panel::Share {
            return div()
                .flex()
                .justify_center()
                .gap_6()
                .child(self.text_button(
                    "invite-terms",
                    "invite.terms",
                    Panel::Terms,
                    cx,
                ))
                .child(self.text_button(
                    "invite-history",
                    "invite.history",
                    Panel::History,
                    cx,
                ));
        }
        div().flex().justify_center().child(self.text_button(
            "invite-back",
            "invite.back",
            Panel::Share,
            cx,
        ))
    }

    fn quiet_button(
        &self,
        id: &'static str,
        key: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        // `Button::render` is opaque, so the row share lives on this shell and
        // the control itself fills it.
        div().flex_1().child(
            theme::Button::new(id, self.t(key))
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Medium)
                .full_width()
                .render(
                    &self.theme,
                    cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.share_invite(cx);
                    }),
                ),
        )
    }

    fn invite_action_button(
        &self,
        id: &'static str,
        key: &'static str,
        copy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let button = theme::Button::new(id, self.t(key))
            .variant(if copy {
                theme::ControlVariant::Ghost
            } else {
                theme::ControlVariant::Primary
            })
            .size(theme::ControlSize::Large);
        let button = if copy { button.full_width() } else { button };
        button.render(
            &self.theme,
            cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                if copy {
                    app.copy_invite(cx);
                } else {
                    app.send_invite(cx);
                }
            }),
        )
    }

    fn text_button(
        &self,
        id: &'static str,
        key: &'static str,
        panel: Panel,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::Button::new(id, self.t(key))
            .variant(theme::ControlVariant::Ghost)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(move |app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.show_panel(panel, cx);
                }),
            )
    }

    fn invite_heading(&self, key: &str) -> impl IntoElement {
        div()
            .text_size(self.theme.heading_size())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_center()
            .child(self.t(key))
    }

    fn invite_copy(&self, key: &str) -> impl IntoElement {
        div()
            .text_size(self.theme.body_size())
            .text_color(self.theme.muted())
            .text_center()
            .whitespace_normal()
            .child(self.t(key))
    }
}

fn envelope(theme: &flowy_ui::ResolvedTheme) -> impl IntoElement {
    div()
        .size(px(72.))
        .rounded(px(20.))
        .bg(rgb(0x00f6_e7c4))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(40.))
                .h(px(28.))
                .rounded(px(4.))
                .bg(theme.card())
                .border_1()
                .border_color(rgb(0x00e4_c98a))
                .relative()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(12.))
                        .bg(rgb(0x00fa_f3de)),
                ),
        )
}

pub(super) fn load(root: &Path) -> Book {
    if let Some(file) = read_file(root)
        && file.version == VERSION
        && !file.code.is_empty()
    {
        return Book {
            code: file.code,
            emails: file.emails,
        };
    }
    let book = Book {
        code: fresh_code(),
        emails: Vec::new(),
    };
    save(root, &book.code, &book.emails);
    book
}

fn save(root: &Path, code: &str, emails: &[String]) {
    let file = File {
        version: VERSION,
        code: code.to_string(),
        emails: emails.to_vec(),
    };
    let Ok(bytes) = serde_json::to_vec_pretty(&file) else {
        return;
    };
    let _ = crate::disk::commit(&root.join("invite.json"), &bytes);
}

fn read_file(root: &Path) -> Option<File> {
    let text = fs::read_to_string(root.join("invite.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn fresh_code() -> String {
    let mut bytes = [0_u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return "local".to_string();
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn remember(emails: &mut Vec<String>, email: &str) -> bool {
    let email = email.trim().to_lowercase();
    if !crate::session::valid_email(&email) {
        return false;
    }
    if !emails.iter().any(|saved| saved == &email) {
        emails.push(email);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{load, save};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn code_stays_and_email_is_saved_once() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("flowy-invite-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let first = load(&dir);
        let second = load(&dir);
        assert_eq!(first.code, second.code);
        let mut emails = Vec::new();
        assert!(super::remember(&mut emails, "A@B.com"));
        assert!(super::remember(&mut emails, "a@b.com"));
        assert!(!super::remember(&mut emails, "nope"));
        assert_eq!(emails, vec!["a@b.com".to_string()]);
        save(&dir, &first.code, &emails);
        let stored = load(&dir);
        assert_eq!(stored.code, first.code);
        assert_eq!(stored.emails, emails);
        let _ = fs::remove_dir_all(dir);
    }
}
