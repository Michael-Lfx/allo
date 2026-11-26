//! Local environment status popover and shared floating card helper.

use flowy_ui as theme;
use flowy_ui::status_pill;
use gpui::{Context, FontWeight, IntoElement, Stateful, div, prelude::*, px};

use crate::panel::StudioPanel;

impl StudioPanel {
    pub(crate) fn popover_card(
        &self,
        id: &'static str,
        width: f32,
        padding: f32,
        gap: f32,
    ) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .occlude()
            .w(px(width))
            .p(self.theme.space(padding))
            .rounded_xl()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap(px(gap))
    }

    pub(crate) fn env_status_popover(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let env = &self.local_env;
        let l3_enabled = env.semantic_verify_enabled;
        let ready = matches!(env.overall, crate::ports::EnvSeverity::Ready);
        let (pill_bg, pill_fg, pill_text) = if ready {
            (
                self.theme.status_verified_bg(),
                self.theme.status_verified(),
                self.t("env.pill_ready"),
            )
        } else {
            (
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
                self.t("env.pill_attention"),
            )
        };
        let mut card = self
            .popover_card("env-status-card", 300.0, 14.0, 8.0)
            .child(
                div()
                    .pb(self.theme.space(6.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(self.t("env.popover_title")),
                    )
                    .child(status_pill(pill_bg, pill_fg, pill_text, None)),
            );

        for check in &env.checks {
            let color = match check.severity {
                crate::ports::EnvSeverity::Ready => {
                    self.theme.status_verified()
                }
                crate::ports::EnvSeverity::Warning => {
                    self.theme.status_attention()
                }
            };
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(self.theme.muted())
                            .child(self.t(check.label_key)),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color)
                            .child(check.detail.resolve(|k| self.t(k))),
                    ),
            );
        }

        card.child(
            div()
                .pt(self.theme.space(8.))
                .border_t_1()
                .border_color(self.theme.line())
                .flex()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.ink())
                        .child(self.t("env.toggle_l3_label")),
                )
                .child(
                    theme::interactive(
                        "env-toggle-l3",
                        format!(
                            "{} \u{00b7} {}",
                            self.t("env.toggle_l3_label"),
                            if l3_enabled {
                                self.t("env.state_on")
                            } else {
                                self.t("env.state_off")
                            }
                        ),
                        &self.theme,
                    )
                    .px(self.theme.space(10.))
                    .py(px(4.))
                    .rounded_full()
                    .border_1()
                    .border_color(if l3_enabled {
                        self.theme.status_verified().opacity(0.45)
                    } else {
                        self.theme.line()
                    })
                    .bg(if l3_enabled {
                        self.theme.status_verified_bg()
                    } else {
                        self.theme.soft()
                    })
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if l3_enabled {
                        self.theme.status_verified()
                    } else {
                        self.theme.muted()
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(self.theme.dock_hover()))
                    .when(l3_enabled, |pill| {
                        pill.child(theme::icons::check().size(px(11.)))
                    })
                    .child(if l3_enabled {
                        self.t("env.state_on")
                    } else {
                        self.t("env.state_off")
                    })
                    .on_click(cx.listener(
                        |panel, _, _, cx| {
                            panel.toggle_semantic_verify(cx);
                        },
                    )),
                ),
        )
    }
}
