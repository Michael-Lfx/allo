use super::{FlowyApp, Screen, SignIn};
use crate::theme;
use gpui::{Context, Window, div, prelude::*, px};
use gpui_ce_elements::editable_text::text_input;

impl FlowyApp {
    pub(super) fn login_canvas(
        &self,
        _window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let creating = !self.auth.account_exists;
        let title = if creating {
            self.t("login.create_title")
        } else {
            self.t("login.sign_in_title")
        };
        let button = if creating {
            self.t("login.create_button")
        } else {
            self.t("login.sign_in_button")
        };
        let count = self.password_count(cx);
        let error = self.auth.login_error.map(|key| self.t(key));
        let hidden = !self.auth.password_visible;

        div()
            .id("login")
            .key_context("Login")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::canvas())
            .on_action(cx.listener(|app, _: &SignIn, _window, cx| {
                if app.screen == Screen::Login {
                    app.submit_login(cx);
                }
            }))
            .child(
                div()
                    .w(px(420.))
                    .bg(theme::card())
                    .border_1()
                    .border_color(theme::line())
                    .rounded_xl()
                    .shadow_md()
                    .p_8()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(wordmark())
                    .child(div().text_2xl().child(title))
                    .child(self.hint(creating))
                    .child(label(&self.t("login.email")))
                    .child(self.email_field())
                    .child(self.password_label(cx))
                    .child(self.password_field(hidden))
                    .child(self.password_meta(hidden, count))
                    .children(error.map(|message| {
                        div()
                            .text_sm()
                            .text_color(theme::danger())
                            .child(message)
                    }))
                    .child(
                        div()
                            .id("sign-in")
                            .h(px(40.))
                            .rounded_lg()
                            .bg(theme::fill())
                            .text_color(theme::on_fill())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.opacity(0.9))
                            .child(button)
                            .on_click(cx.listener(
                                |app, _event, _window, cx| {
                                    app.submit_login(cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .child(self.t("login.keep_days")),
                    ),
            )
    }

    fn password_label(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let action = if self.auth.password_visible {
            self.t("login.hide")
        } else {
            self.t("login.show")
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(label(&self.t("login.password")))
            .child(
                div()
                    .id("toggle-password")
                    .text_xs()
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .child(action)
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.auth.password_visible = !app.auth.password_visible;
                        cx.notify();
                    })),
            )
    }

    fn hint(&self, creating: bool) -> impl IntoElement {
        let text = if creating {
            self.t("login.create_hint")
        } else {
            self.t("login.sign_in_hint")
        };
        div().text_sm().text_color(theme::muted()).child(text)
    }

    fn email_field(&self) -> impl gpui::IntoElement {
        text_input("email")
            .state(self.auth.email.downgrade())
            .placeholder(self.t("login.email_placeholder"))
            .placeholder_color(theme::muted())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .min_h_auto()
            .whitespace_nowrap()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .text_color(theme::ink())
    }

    fn password_field(&self, hidden: bool) -> impl gpui::IntoElement {
        let color = if hidden { theme::soft() } else { theme::ink() };
        text_input("password")
            .state(self.auth.password.downgrade())
            .placeholder(self.t("login.password_placeholder"))
            .placeholder_color(theme::muted())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .min_h_auto()
            .whitespace_nowrap()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .text_color(color)
    }

    fn password_meta(&self, hidden: bool, count: usize) -> impl IntoElement {
        let count = count.to_string();
        let text = if hidden {
            self.tf("login.password_hidden", &[("count", &count)])
        } else {
            self.t("login.password_visible")
        };
        div().text_xs().text_color(theme::muted()).child(text)
    }
}

fn wordmark() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(super::product_mark(22.))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(crate::brand::product().to_string()),
        )
}

fn label(text: &str) -> impl IntoElement {
    div().text_sm().child(text.to_string())
}
