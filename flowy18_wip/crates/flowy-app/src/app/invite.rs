use super::FlowyApp;
use flowy_ui as theme;
use gpui::{
    ClipboardItem, ColorExt, Context, SharedString, div, prelude::*, px, rgb,
};
use gpui_ce_elements::editable_text::text_input;
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
const SOCIAL: &[(&str, &str, u32)] = &[
    ("wa", "W", 0x0025_d366),
    ("fb", "f", 0x0018_77f2),
    ("x", "X", 0x0011_1111),
    ("in", "in", 0x000a_66c2),
    ("rd", "r", 0x00ff_4500),
];

impl FlowyApp {
    pub(super) fn open_invite(&mut self, cx: &mut Context<'_, Self>) {
        self.invite.open = true;
        self.invite.panel = Panel::Share;
        self.invite.status = None;
        self.persona.dialog = super::more::Dialog::None;
        cx.notify();
    }

    pub(super) fn close_invite(&mut self, cx: &mut Context<'_, Self>) {
        self.invite.open = false;
        self.invite.status = None;
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
        let email = self.invite.email.read(cx).as_str();
        if !remember(&mut self.invite.emails, email) {
            self.invite.status = Some("invite.bad_email");
            cx.notify();
            return;
        }
        save(self.store.root(), &self.invite.code, &self.invite.emails);
        self.invite
            .email
            .update(cx, |state, cx| state.emplace("", cx));
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
        div()
            .id("invite-layer")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::ink().opacity(0.28))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.close_invite(cx);
            }))
            .child(self.invite_card(cx))
    }

    fn invite_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("invite-card")
            .w(px(440.))
            .max_h(px(640.))
            .overflow_y_scroll()
            .bg(theme::card())
            .rounded_xl()
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
            .child(Self::invite_close(cx))
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
                    .text_xs()
                    .text_color(theme::muted())
                    .child(text)
            }))
            .child(self.invite_footer(cx))
    }

    fn invite_close(cx: &Context<'_, Self>) -> impl IntoElement {
        div().flex().justify_end().child(
            div()
                .id("close-invite")
                .size(px(28.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(theme::soft()))
                .child("×")
                .on_click(cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.close_invite(cx);
                })),
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
            .child(envelope())
            .child(self.invite_heading("invite.title"))
            .child(self.invite_copy("invite.body"))
            .child(self.invite_copy("invite.footnote"))
            .child(self.share_row(cx))
            .child(self.primary_button("copy-invite", "invite.copy", true, cx))
            .child(Self::social_row(cx))
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
            column = column.child(div().text_sm().child(email.clone()));
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

    fn social_row(cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div().flex().items_center().justify_center().gap_3();
        for (id, mark, color) in SOCIAL {
            let element_id = SharedString::from(format!("share-{id}"));
            row = row.child(
                div()
                    .id(element_id)
                    .size(px(32.))
                    .rounded_full()
                    .bg(rgb(*color))
                    .text_color(theme::on_ink())
                    .text_xs()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(*mark)
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.share_invite(cx);
                    })),
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
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("invite.email_title")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.invite_email_field())
                    .child(self.primary_button(
                        "send-invite",
                        "invite.send",
                        false,
                        cx,
                    )),
            )
    }

    fn invite_email_field(&self) -> impl IntoElement {
        text_input("invite-email")
            .state(self.invite.email.downgrade())
            .placeholder(self.t("invite.email_placeholder"))
            .placeholder_color(theme::muted())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .flex_1()
            .min_h_auto()
            .whitespace_nowrap()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .text_sm()
            .text_color(theme::ink())
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
        div()
            .id(id)
            .flex_1()
            .h(px(36.))
            .rounded_lg()
            .bg(theme::soft())
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .cursor_pointer()
            .hover(|style| style.bg(theme::line()))
            .child(self.t(key))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.share_invite(cx);
            }))
    }

    fn primary_button(
        &self,
        id: &'static str,
        key: &'static str,
        copy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(40.))
            .when(copy, gpui::Styled::w_full)
            .px_4()
            .rounded_lg()
            .bg(theme::fill())
            .text_color(theme::on_fill())
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .text_sm()
            .cursor_pointer()
            .hover(|style| style.opacity(0.9))
            .child(self.t(key))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                if copy {
                    app.copy_invite(cx);
                } else {
                    app.send_invite(cx);
                }
            }))
    }

    fn text_button(
        &self,
        id: &'static str,
        key: &'static str,
        panel: Panel,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .text_sm()
            .text_color(theme::muted())
            .cursor_pointer()
            .child(self.t(key))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.show_panel(panel, cx);
            }))
    }

    fn invite_heading(&self, key: &str) -> impl IntoElement {
        div()
            .text_lg()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_center()
            .child(self.t(key))
    }

    fn invite_copy(&self, key: &str) -> impl IntoElement {
        div()
            .text_sm()
            .text_color(theme::muted())
            .text_center()
            .whitespace_normal()
            .child(self.t(key))
    }
}

fn envelope() -> impl IntoElement {
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
                .bg(theme::card())
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
