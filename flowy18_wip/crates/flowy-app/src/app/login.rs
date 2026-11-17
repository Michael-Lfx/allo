use super::state::{LegalDoc, LoginPhase, LoginStep};
use super::{FlowyApp, Screen, SignIn};
use flowy_auth::{EMAIL_OTP_LENGTH, valid_email};
use flowy_ui as theme;
use flowy_ui::text_input;
use gpui::{Context, FontWeight, Window, div, prelude::*, px, rgb};

impl FlowyApp {
    pub(super) fn login_canvas(
        &self,
        _window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let busy = matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying | LoginPhase::Success
        );
        let on_otp = self.auth.step == LoginStep::Otp;
        let email = self.auth.email.read(cx).as_str().to_string();
        let code = self.auth.code.read(cx).as_str().to_string();
        let email_ok = valid_email(&email);
        let can_send =
            !busy && !on_otp && email_ok && self.auth.cooldown_left == 0;
        let primary = match (self.auth.step, self.auth.phase) {
            (_, LoginPhase::Sending) => self.t("login.sending"),
            (_, LoginPhase::Verifying) => self.t("login.verifying"),
            (_, LoginPhase::Success) => self.t("login.success"),
            (LoginStep::Otp, _) => self.t("login.verify_button"),
            (LoginStep::Email, _) => self.t("login.send_code"),
        };
        let heading = if on_otp {
            self.t("login.step_otp")
        } else {
            self.t("login.welcome_title")
        };
        let description = if on_otp {
            self.tf("login.otp_sent_to", &[("email", &email)])
        } else {
            self.t("login.welcome_desc")
        };
        let message = self.auth.message.map(|key| {
            if key == "login.cooldown" {
                let secs = self.auth.cooldown_left.to_string();
                self.tf(key, &[("seconds", &secs)])
            } else {
                self.t(key)
            }
        });
        let errorish = self.auth.message.is_some_and(|key| {
            key.starts_with("auth.") || key == "login.send_first"
        });

        div()
            .id("login")
            .key_context("Login")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(self.theme.canvas())
            .p_6()
            .on_action(cx.listener(|app, _: &SignIn, _window, cx| {
                if app.screen == Screen::Login
                    && app.active_dialog == super::state::ActiveDialog::None
                {
                    app.submit_login(cx);
                }
            }))
            .child(
                div()
                    .id("login-shell")
                    .w_full()
                    .max_w(px(1040.))
                    .h_full()
                    .max_h(px(600.))
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .rounded_xl()
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.card())
                    .shadow_lg()
                    .child(self.login_brand_pane(cx))
                    .child(
                        div()
                            .id("login-panel")
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .overflow_y_scroll()
                            .bg(self.theme.card())
                            .px(self.theme.space(32.))
                            .py(self.theme.space(24.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .w_full()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(self.login_progress(on_otp))
                                    .child(
                                        div()
                                            .text_3xl()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .letter_spacing(px(-0.6))
                                            .text_color(self.theme.ink())
                                            .child(heading),
                                    )
                                    .child(
                                        div()
                                            .mt_2()
                                            .mb_8()
                                            .text_size(self.theme.body_size())
                                            .text_color(self.theme.muted())
                                            .child(description),
                                    )
                                    .child(if on_otp {
                                        self.login_otp_form(busy, &code, cx)
                                            .into_any_element()
                                    } else {
                                        self.login_email_form(busy, cx)
                                            .into_any_element()
                                    })
                                    .children(message.map(|text| {
                                        div()
                                            .mt_3()
                                            .text_size(
                                                self.theme.caption_size(),
                                            )
                                            .text_color(if errorish {
                                                self.theme.danger()
                                            } else {
                                                self.theme.muted()
                                            })
                                            .child(text)
                                    }))
                                    .child(if on_otp {
                                        self.login_resend(busy, cx)
                                            .into_any_element()
                                    } else {
                                        self.login_primary(
                                            &primary,
                                            can_send
                                                || matches!(
                                                    self.auth.phase,
                                                    LoginPhase::Sending
                                                ),
                                            can_send,
                                            cx,
                                        )
                                        .into_any_element()
                                    })
                                    .when(!on_otp, |el| {
                                        el.child(self.login_diagnose(busy, cx))
                                    }),
                            ),
                    ),
            )
    }

    fn login_brand_pane(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("login-brand")
            .w(px(560.))
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(self.theme.auth_brand_canvas())
            .child(blueprint_scene(&self.theme))
            .child(
                div()
                    .absolute()
                    .top(px(28.))
                    .left(px(32.))
                    .right(px(32.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(48.))
                            .rounded_lg()
                            .border_1()
                            .border_color(self.theme.line())
                            .bg(self.theme.card())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(super::product_mark(30.)),
                    )
                    .child(
                        div()
                            .text_size(self.theme.heading_size())
                            .font_weight(FontWeight::SEMIBOLD)
                            .letter_spacing(px(-0.6))
                            .text_color(self.theme.ink())
                            .child(crate::brand::product().to_string()),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(32.))
                    .left(px(32.))
                    .w(px(340.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .text_color(self.theme.ink())
                            .opacity(0.78)
                            .child(self.t("login.brand_tagline")),
                    )
                    .child(
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .font_family("Menlo")
                            .child(self.t("login.brand_flow")),
                    )
                    .child(
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .child(self.t("login.brand_meta")),
                    )
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .child(self.legal_link(
                                "login-legal-privacy",
                                &self.t("login.legal_privacy"),
                                LegalDoc::Privacy,
                                cx,
                            ))
                            .child(div().child(self.t("login.legal_and")))
                            .child(self.legal_link(
                                "login-legal-terms",
                                &self.t("login.legal_terms"),
                                LegalDoc::Terms,
                                cx,
                            )),
                    ),
            )
    }

    pub(super) fn legal_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let super::state::ActiveDialog::Legal(doc) = self.active_dialog else {
            return div().id("legal-layer-empty").into_any_element();
        };
        theme::modal_layer(
            "legal-layer",
            &self.theme,
            &self.ui,
            self.legal_dialog(doc, cx),
        )
        .into_any_element()
    }

    fn legal_dialog(
        &self,
        doc: LegalDoc,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::dialog(
            "legal-dialog",
            self.t(doc.title_key()),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(560.))
        .max_h(px(520.))
        .rounded(self.theme.modal_radius())
        .bg(self.theme.card())
        .shadow_lg()
        .px_8()
        .pt_6()
        .pb_6()
        .flex()
        .flex_col()
        .gap_4()
        .on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(self.theme.ink())
                .child(self.t(doc.title_key())),
        )
        .child(
            div()
                .id("legal-body")
                .flex_1()
                .min_h(px(180.))
                .max_h(px(360.))
                .overflow_y_scroll()
                .rounded_lg()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.soft())
                .px_4()
                .py_3()
                .child(
                    div()
                        .text_size(self.theme.body_size())
                        .text_color(self.theme.ink())
                        .whitespace_normal()
                        .child(doc.body().to_string()),
                ),
        )
        .child(
            theme::interactive(
                "legal-confirm",
                self.t("model.got_it"),
                &self.theme,
            )
            .h(px(40.))
            .rounded_lg()
            .bg(self.theme.auth_accent())
            .text_color(self.theme.auth_on_accent())
            .font_weight(FontWeight::SEMIBOLD)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.opacity(0.92))
            .child(self.t("model.got_it"))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.close_legal(cx);
            })),
        )
    }

    fn legal_link(
        &self,
        id: &'static str,
        label: &str,
        doc: LegalDoc,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let label = label.to_string();
        theme::interactive(id, label.clone(), &self.theme)
            .underline()
            .text_color(self.theme.ink())
            .opacity(0.72)
            .cursor_pointer()
            .hover(|style| {
                style.text_color(self.theme.auth_accent()).opacity(1.)
            })
            .child(label)
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.open_legal(doc, cx);
            }))
    }

    fn login_progress(&self, on_otp: bool) -> impl IntoElement {
        div()
            .mb(px(44.))
            .flex()
            .items_center()
            .gap_2()
            .child(progress_step(
                &self.theme,
                if on_otp {
                    StepMark::Done
                } else {
                    StepMark::Number("1")
                },
                &self.t("login.step_email"),
                true,
            ))
            .child(
                div()
                    .flex_1()
                    .h(px(1.))
                    .min_w(px(20.))
                    .bg(self.theme.line()),
            )
            .child(progress_step(
                &self.theme,
                StepMark::Number("2"),
                &self.t("login.step_otp"),
                on_otp,
            ))
    }

    fn login_email_form(
        &self,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let _ = cx;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .child(self.t("login.email")),
            )
            .child(self.email_field(busy))
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("login.auto_register")),
            )
    }

    fn login_otp_form(
        &self,
        busy: bool,
        code: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(self.t("login.otp")),
                    )
                    .child(self.change_email_link(busy, cx)),
            )
            .child(
                div().relative().child(otp_cells(&self.theme, code)).child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .opacity(0.02)
                        .child(self.code_field(busy)),
                ),
            )
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("login.otp_hint")),
            )
    }

    fn login_primary(
        &self,
        label: &str,
        emphasized: bool,
        enabled: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let label = label.to_string();
        theme::button(
            "sign-in",
            label.clone(),
            !enabled,
            &self.theme,
            cx.listener(|app, _, _, cx| app.submit_login(cx)),
        )
        .mt(px(20.))
        .h(px(48.))
        .w_full()
        .rounded_lg()
        .bg(if emphasized {
            self.theme.auth_accent()
        } else {
            self.theme.soft()
        })
        .text_color(if emphasized {
            self.theme.auth_on_accent()
        } else {
            self.theme.muted()
        })
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| if enabled { style.opacity(0.92) } else { style })
        .child(label)
    }

    fn login_resend(
        &self,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let can_resend = !busy && self.auth.cooldown_left == 0;
        let label = if self.auth.cooldown_left > 0 {
            self.tf(
                "login.cooldown",
                &[("seconds", &self.auth.cooldown_left.to_string())],
            )
        } else {
            self.t("login.resend_code")
        };
        theme::button(
            "resend-code",
            self.t("login.resend_code"),
            !can_resend,
            &self.theme,
            cx.listener(|app, _, _, cx| app.send_login_code(cx)),
        )
        .mt(px(20.))
        .h(px(48.))
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .text_color(if can_resend {
            self.theme.ink()
        } else {
            self.theme.muted()
        })
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .cursor_pointer()
        .child(div().child(self.t("login.resend_code")))
        .child(
            div()
                .text_size(self.theme.caption_size())
                .text_color(self.theme.muted())
                .child(label),
        )
    }

    fn login_diagnose(
        &self,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::button(
            "network-diagnose",
            self.t("login.diagnose"),
            busy,
            &self.theme,
            cx.listener(|app, _, _, cx| app.run_login_diagnose(cx)),
        )
        .mt(px(24.))
        .text_size(self.theme.body_size())
        .font_weight(FontWeight::MEDIUM)
        .text_color(self.theme.auth_accent())
        .cursor_pointer()
        .child(self.t("login.diagnose"))
    }

    fn change_email_link(
        &self,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::button(
            "change-email",
            self.t("login.change_email"),
            busy,
            &self.theme,
            cx.listener(|app, _, _, cx| app.reset_login_to_email(cx)),
        )
        .text_size(self.theme.caption_size())
        .text_color(self.theme.auth_accent())
        .cursor_pointer()
        .child(self.t("login.change_email"))
    }

    fn email_field(&self, locked: bool) -> impl gpui::IntoElement {
        let field = text_input("email")
            .aria_label(self.t("login.email"))
            .state(self.auth.email.downgrade())
            .accepts_input(!locked)
            .placeholder(self.t("login.email_placeholder"))
            .placeholder_color(self.theme.muted())
            .caret_color(gpui::rgb_to_hsla(self.theme.ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .min_h_auto()
            .whitespace_nowrap()
            .px_3()
            .py(px(10.))
            .rounded_lg()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft())
            .text_color(self.theme.ink());
        if locked { field.opacity(0.7) } else { field }
    }

    fn code_field(&self, locked: bool) -> impl gpui::IntoElement {
        let field = text_input("otp")
            .aria_label(self.t("login.step_otp"))
            .state(self.auth.code.downgrade())
            .accepts_input(!locked)
            .placeholder("")
            .placeholder_color(self.theme.muted())
            .caret_color(gpui::rgb_to_hsla(self.theme.ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .h_full()
            .whitespace_nowrap()
            .px_3()
            .py(px(12.))
            .rounded_lg()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft())
            .text_color(self.theme.ink());
        if locked { field.opacity(0.7) } else { field }
    }
}

enum StepMark {
    Number(&'static str),
    Done,
}

fn progress_step(
    theme: &flowy_ui::ResolvedTheme,
    mark: StepMark,
    label: &str,
    active: bool,
) -> impl IntoElement {
    let fg = if active {
        theme.auth_on_accent()
    } else {
        theme.muted()
    };
    let badge = div()
        .size(px(26.))
        .rounded_full()
        .border_1()
        .border_color(if active {
            theme.auth_accent()
        } else {
            theme.line()
        })
        .bg(if active {
            theme.auth_accent()
        } else {
            theme.card()
        })
        .text_color(fg)
        .flex()
        .items_center()
        .justify_center();
    let badge = match mark {
        StepMark::Number(number) => badge
            .text_size(theme.caption_size())
            .font_family("Menlo")
            .child(number.to_string())
            .into_any_element(),
        StepMark::Done => badge
            .child(theme::icons::check().size(px(14.)).text_color(fg))
            .into_any_element(),
    };
    div().flex().items_center().gap_2().child(badge).child(
        div()
            .text_size(theme.caption_size())
            .font_weight(if active {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(if active { theme.ink() } else { theme.muted() })
            .child(label.to_string()),
    )
}

fn otp_cells(theme: &flowy_ui::ResolvedTheme, code: &str) -> impl IntoElement {
    let mut row = div().flex().gap_2().w_full();
    for index in 0..EMAIL_OTP_LENGTH {
        let ch = code.chars().nth(index).unwrap_or(' ');
        let filled = !ch.is_whitespace();
        row = row.child(
            div()
                .id(("otp-cell", index as u64))
                .flex_1()
                .h(px(52.))
                .rounded_lg()
                .border_1()
                .border_color(if filled {
                    theme.auth_accent()
                } else {
                    theme.line()
                })
                .bg(theme.soft())
                .flex()
                .items_center()
                .justify_center()
                .text_size(theme.heading_size())
                .font_weight(FontWeight::SEMIBOLD)
                .font_family("Menlo")
                .text_color(theme.ink())
                .child(if filled {
                    ch.to_string()
                } else {
                    String::new()
                }),
        );
    }
    row
}

fn blueprint_scene(theme: &flowy_ui::ResolvedTheme) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .opacity(0.55)
        .child(wire_card(120., 90., 150., 96.))
        .child(wire_card(300., 70., 130., 88.))
        .child(wire_card(200., 210., 170., 110.))
        .child(wire_card(390., 240., 120., 84.))
        .child(wire_card(140., 360., 140., 90.))
        .child(wire_card(330., 380., 160., 100.))
        .child(wire_dot(theme, 270., 160.))
        .child(wire_dot(theme, 310., 250.))
        .child(wire_dot(theme, 250., 340.))
        .child(wire_line(195., 140., 100., 1.))
        .child(wire_line(270., 160., 1., 90.))
        .child(wire_line(250., 265., 140., 1.))
        .child(wire_line(250., 340., 1., 40.))
}

fn wire_card(x: f32, y: f32, w: f32, h: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .rounded_md()
        .border_1()
        .border_color(rgb(0x00c4_c0b8))
        .bg(rgb(0x00f7_f6f3))
        .child(
            div()
                .mt_3()
                .mx_3()
                .h(px(8.))
                .w(px(w * 0.45))
                .rounded_sm()
                .bg(rgb(0x00d8_d4cc)),
        )
        .child(
            div()
                .mt_2()
                .mx_3()
                .h(px(6.))
                .w(px(w * 0.7))
                .rounded_sm()
                .bg(rgb(0x00e4_e0d8)),
        )
        .child(
            div()
                .mt_2()
                .mx_3()
                .h(px(6.))
                .w(px(w * 0.55))
                .rounded_sm()
                .bg(rgb(0x00e4_e0d8)),
        )
}

fn wire_dot(
    theme: &flowy_ui::ResolvedTheme,
    x: f32,
    y: f32,
) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .size(px(8.))
        .rounded_full()
        .border_1()
        .border_color(rgb(0x00a8_a49c))
        .bg(theme.card())
}

fn wire_line(x: f32, y: f32, w: f32, h: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .bg(rgb(0x00c4_c0b8))
}
