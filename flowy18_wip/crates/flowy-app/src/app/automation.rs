use super::FlowyApp;
use crate::theme;
use gpui::{Context, SharedString, div, prelude::*, px, rgb};
use gpui_ce_elements::editable_text::text_area;

const COLUMN: f32 = 720.0;

impl FlowyApp {
    pub(super) fn automation_canvas(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("automation")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .mx_auto()
                    .px_8()
                    .pt_10()
                    .pb_12()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(28.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t("auto.workflow_title")),
                    )
                    .child(self.automation_composer(cx))
                    .child(self.launch_panel(cx))
                    .child(self.suggestions_panel(cx)),
            )
    }

    fn automation_composer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .rounded(px(20.))
            .shadow_sm()
            .px_4()
            .pt_3()
            .pb_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.composer_chip("auto-cloud", "auto.cloud", cx))
                    .child(self.composer_chip(
                        "auto-project",
                        "auto.pick_project",
                        cx,
                    )),
            )
            .child(
                text_area("auto-prompt")
                    .state(self.studio.prompt.downgrade())
                    .placeholder(self.tf(
                        "auto.prompt_placeholder",
                        &[("product", crate::brand::product())],
                    ))
                    .placeholder_color(theme::muted())
                    .caret_color(gpui::rgb_to_hsla(theme::ink()))
                    .caret_blink_interval_500ms()
                    .w_full()
                    .h(px(56.))
                    .whitespace_normal()
                    .overflow_y_scroll()
                    .text_color(theme::ink()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(tool_mark("+"))
                            .child(tool_mark("#"))
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_full()
                                    .bg(rgb(0x00e8_f0fe))
                                    .text_xs()
                                    .text_color(rgb(0x001a_73e8))
                                    .child(self.t("dock.automation")),
                            ),
                    )
                    .child(
                        div()
                            .id("auto-send")
                            .size(px(32.))
                            .rounded_full()
                            .bg(theme::fill())
                            .text_color(theme::on_fill())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .cursor_pointer()
                            .child("↑")
                            .on_click(cx.listener(|app, _e, _w, cx| {
                                app.notice = Some("auto.later");
                                cx.notify();
                            })),
                    ),
            )
    }

    fn composer_chip(
        &self,
        id: &'static str,
        key: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(id))
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .text_xs()
            .text_color(theme::muted())
            .cursor_pointer()
            .child(self.t(key))
            .on_click(cx.listener(|app, _e, _w, cx| {
                app.notice = Some("auto.later");
                cx.notify();
            }))
    }

    fn launch_panel(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .w_full()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .rounded_xl()
            .px_4()
            .pt_3()
            .pb_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t("auto.mode_label")),
                    )
                    .child(
                        div()
                            .id("auto-mode-close")
                            .size(px(24.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::soft()))
                            .child("×")
                            .on_click(cx.listener(|app, _e, _w, cx| {
                                app.notice = Some("auto.later");
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_3()
                    .child(self.mode_card(
                        "auto-mode-schedule",
                        "auto.schedule_title",
                        "auto.schedule_body",
                        None,
                        cx,
                    ))
                    .child(self.mode_card(
                        "auto-mode-trigger",
                        "auto.trigger_title",
                        "auto.trigger_body",
                        None,
                        cx,
                    ))
                    .child(self.mode_card(
                        "auto-mode-advanced",
                        "auto.advanced_title",
                        "auto.advanced_body",
                        Some("auto.beta"),
                        cx,
                    )),
            )
    }

    fn mode_card(
        &self,
        id: &'static str,
        title: &'static str,
        body: &'static str,
        badge: Option<&'static str>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(id))
            .flex_1()
            .min_w_0()
            .h(px(132.))
            .rounded_xl()
            .border_1()
            .border_color(theme::line())
            .bg(theme::soft())
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .hover(|style| style.bg(theme::card()))
            .on_click(cx.listener(|app, _e, _w, cx| {
                app.notice = Some("auto.later");
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t(title)),
                    )
                    .children(badge.map(|key| {
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(theme::line())
                            .text_xs()
                            .text_color(theme::muted())
                            .child(self.t(key))
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t(body)),
            )
    }

    fn suggestions_panel(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t("auto.suggestions")),
                    )
                    .child(
                        div()
                            .id("auto-refresh")
                            .size(px(28.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::soft()))
                            .child("↻")
                            .on_click(cx.listener(|app, _e, _w, cx| {
                                app.notice = Some("auto.later");
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_3()
                    .items_stretch()
                    .child(self.suggestion_column(
                        "auto-s1",
                        "G",
                        rgb(0x00ea_4335),
                        "auto.suggest_growth_title",
                        "auto.suggest_growth_body",
                        cx,
                    ))
                    .child(self.suggestion_column(
                        "auto-s2",
                        "S",
                        rgb(0x0095_bf47),
                        "auto.suggest_return_title",
                        "auto.suggest_return_body",
                        cx,
                    ))
                    .child(self.suggestion_column(
                        "auto-s3",
                        "∞",
                        rgb(0x0008_66ff),
                        "auto.suggest_ads_title",
                        "auto.suggest_ads_body",
                        cx,
                    )),
            )
    }

    fn suggestion_column(
        &self,
        id: &'static str,
        mark: &'static str,
        tint: gpui::Rgba,
        title: &'static str,
        body: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(id))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(|app, _e, _w, cx| {
                app.notice = Some("auto.later");
                cx.notify();
            }))
            .child(
                div()
                    .w_full()
                    .h(px(148.))
                    .rounded_xl()
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::card())
                    .shadow_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(40.))
                            .rounded_lg()
                            .bg(tint)
                            .text_color(theme::on_ink())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(mark),
                    ),
            )
            .child(
                div()
                    .px_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t(title)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .whitespace_normal()
                            .child(self.t(body)),
                    ),
            )
    }
}

fn tool_mark(glyph: &'static str) -> impl IntoElement {
    div()
        .size(px(28.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(theme::muted())
        .child(glyph)
}
