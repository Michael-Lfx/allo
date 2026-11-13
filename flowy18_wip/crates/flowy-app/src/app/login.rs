use super::state::{LegalDoc, LoginPhase, LoginStep};
use super::{FlowyApp, Screen, SignIn};
use flowy_auth::{EMAIL_OTP_LENGTH, valid_email};
use flowy_ui as theme;
use gpui::{
    ColorExt, Context, FontWeight, Rgba, Window, div, prelude::*, px, rgb, svg,
};
use gpui_ce_elements::editable_text::text_input;

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
            .bg(theme::canvas())
            .p_6()
            .on_action(cx.listener(|app, _: &SignIn, _window, cx| {
                if app.screen == Screen::Login {
                    app.submit_login(cx);
                }
            }))
            .child(
                div()
                    .id("login-shell")
                    .w(px(1040.))
                    .h(px(600.))
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .rounded_xl()
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::card())
                    .shadow_lg()
                    .child(self.login_brand_pane(cx))
                    .child(
                        div()
                            .id("login-panel")
                            .flex_1()
                            .h_full()
                            .min_w(px(360.))
                            .bg(theme::card())
                            .px(px(56.))
                            .py(px(48.))
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
                                            .text_color(theme::ink())
                                            .child(heading),
                                    )
                                    .child(
                                        div()
                                            .mt_2()
                                            .mb_8()
                                            .text_sm()
                                            .text_color(theme::muted())
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
                                            .text_xs()
                                            .text_color(if errorish {
                                                theme::danger()
                                            } else {
                                                theme::muted()
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

    fn login_brand_pane(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("login-brand")
            .w(px(560.))
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(auth_brand_canvas())
            .child(blueprint_scene())
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
                            .border_color(theme::line())
                            .bg(theme::card())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(super::product_mark(30.)),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .letter_spacing(px(-0.6))
                            .text_color(theme::ink())
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
                            .text_sm()
                            .text_color(theme::ink())
                            .opacity(0.78)
                            .child(self.t("login.brand_tagline")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .font_family("Menlo")
                            .child(self.t("login.brand_flow")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .child(self.t("login.brand_meta")),
                    )
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(theme::muted())
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
        let Some(doc) = self.auth.legal else {
            return div().id("legal-layer-empty").into_any_element();
        };
        div()
            .id("legal-layer")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::ink().opacity(0.18))
            .backdrop_blur(px(14.))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.close_legal(cx);
            }))
            .child(self.legal_dialog(doc, cx))
            .into_any_element()
    }

    fn legal_dialog(
        &self,
        doc: LegalDoc,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("legal-dialog")
            .w(px(560.))
            .max_h(px(520.))
            .rounded(px(20.))
            .bg(theme::card())
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
                    .text_color(theme::ink())
                    .child(self.t(doc.title_key())),
            )
            .child(
                div()
                    .id("legal-body")
                    .flex_1()
                    .min_h(px(280.))
                    .max_h(px(360.))
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::soft())
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::ink())
                            .whitespace_normal()
                            .child(doc.body().to_string()),
                    ),
            )
            .child(
                div()
                    .id("legal-confirm")
                    .h(px(40.))
                    .rounded_lg()
                    .bg(auth_accent())
                    .text_color(auth_on_accent())
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
        div()
            .id(id)
            .underline()
            .text_color(theme::ink())
            .opacity(0.72)
            .cursor_pointer()
            .hover(|style| style.text_color(auth_accent()).opacity(1.))
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
                if on_otp {
                    StepMark::Done
                } else {
                    StepMark::Number("1")
                },
                &self.t("login.step_email"),
                true,
            ))
            .child(div().flex_1().h(px(1.)).min_w(px(20.)).bg(theme::line()))
            .child(progress_step(
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
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::ink())
                    .child(self.t("login.email")),
            )
            .child(self.email_field(busy))
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
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
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::ink())
                            .child(self.t("login.otp")),
                    )
                    .child(self.change_email_link(busy, cx)),
            )
            .child(
                div().relative().child(otp_cells(code)).child(
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
                    .text_xs()
                    .text_color(theme::muted())
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
        div()
            .id("sign-in")
            .mt(px(20.))
            .h(px(48.))
            .w_full()
            .rounded_lg()
            .bg(if emphasized {
                auth_accent()
            } else {
                theme::soft()
            })
            .text_color(if emphasized {
                auth_on_accent()
            } else {
                theme::muted()
            })
            .font_weight(FontWeight::SEMIBOLD)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| if enabled { style.opacity(0.92) } else { style })
            .child(label)
            .on_click(cx.listener(move |app, _event, _window, cx| {
                if enabled {
                    app.submit_login(cx);
                }
            }))
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
        div()
            .id("resend-code")
            .mt(px(20.))
            .h(px(48.))
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::card())
            .text_color(if can_resend {
                theme::ink()
            } else {
                theme::muted()
            })
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .cursor_pointer()
            .child(div().child(self.t("login.resend_code")))
            .child(div().text_xs().text_color(theme::muted()).child(label))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                if can_resend {
                    app.send_login_code(cx);
                }
            }))
    }

    fn login_diagnose(
        &self,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("network-diagnose")
            .mt(px(24.))
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(auth_accent())
            .cursor_pointer()
            .child(self.t("login.diagnose"))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                if !busy {
                    app.run_login_diagnose(cx);
                }
            }))
    }

    fn change_email_link(
        &self,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("change-email")
            .text_xs()
            .text_color(auth_accent())
            .cursor_pointer()
            .child(self.t("login.change_email"))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                if !busy {
                    app.reset_login_to_email(cx);
                }
            }))
    }

    fn email_field(&self, locked: bool) -> impl gpui::IntoElement {
        let field = text_input("email")
            .state(self.auth.email.downgrade())
            .placeholder(self.t("login.email_placeholder"))
            .placeholder_color(theme::muted())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .min_h_auto()
            .whitespace_nowrap()
            .px_3()
            .py(px(10.))
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .text_color(theme::ink());
        if locked { field.opacity(0.7) } else { field }
    }

    fn code_field(&self, locked: bool) -> impl gpui::IntoElement {
        let field = text_input("otp")
            .state(self.auth.code.downgrade())
            .placeholder("")
            .placeholder_color(theme::muted())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .h_full()
            .whitespace_nowrap()
            .px_3()
            .py(px(12.))
            .rounded_lg()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .text_color(theme::ink());
        if locked { field.opacity(0.7) } else { field }
    }
}

enum StepMark {
    Number(&'static str),
    Done,
}

const STEP_CHECK_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="#000" d="M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.75.75 0 0 1 1.06-1.06L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0z"/></svg>"##;

fn progress_step(mark: StepMark, label: &str, active: bool) -> impl IntoElement {
    let fg = if active {
        auth_on_accent()
    } else {
        theme::muted()
    };
    let badge = div()
        .size(px(26.))
        .rounded_full()
        .border_1()
        .border_color(if active {
            auth_accent()
        } else {
            theme::line()
        })
        .bg(if active { auth_accent() } else { theme::card() })
        .text_color(fg)
        .flex()
        .items_center()
        .justify_center();
    let badge = match mark {
        StepMark::Number(number) => badge
            .text_xs()
            .font_family("Menlo")
            .child(number.to_string())
            .into_any_element(),
        StepMark::Done => badge
            .child(svg().data(STEP_CHECK_SVG).size(px(14.)).text_color(fg))
            .into_any_element(),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(badge)
        .child(
            div()
                .text_xs()
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if active { theme::ink() } else { theme::muted() })
                .child(label.to_string()),
        )
}

fn otp_cells(code: &str) -> impl IntoElement {
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
                    auth_accent()
                } else {
                    theme::line()
                })
                .bg(theme::soft())
                .flex()
                .items_center()
                .justify_center()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .font_family("Menlo")
                .text_color(theme::ink())
                .child(if filled {
                    ch.to_string()
                } else {
                    String::new()
                }),
        );
    }
    row
}

fn blueprint_scene() -> impl IntoElement {
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
        .child(wire_dot(270., 160.))
        .child(wire_dot(310., 250.))
        .child(wire_dot(250., 340.))
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

fn wire_dot(x: f32, y: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .size(px(8.))
        .rounded_full()
        .border_1()
        .border_color(rgb(0x00a8_a49c))
        .bg(theme::card())
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

fn auth_accent() -> Rgba {
    rgb(0x000f_766e)
}

fn auth_on_accent() -> Rgba {
    rgb(0x00ff_ffff)
}

fn auth_brand_canvas() -> Rgba {
    rgb(0x00e8_e6e1)
}
