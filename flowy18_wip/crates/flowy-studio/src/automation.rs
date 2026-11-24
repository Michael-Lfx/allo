use flowy_ui as theme;
use flowy_ui::text_area;
use gpui::{Context, SharedString, div, prelude::*, px};

const COLUMN: f32 = 720.0;

impl crate::panel::StudioPanel {
    pub fn automation_canvas(
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
                    .px(self.theme.space(32.))
                    .pt(self.theme.space(40.))
                    .pb(self.theme.space(48.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(24.))
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

    fn automation_composer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .w_full()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .rounded(px(20.))
            .shadow_sm()
            .px(self.theme.space(16.))
            .pt(self.theme.space(12.))
            .pb(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(self.composer_chip("auto-cloud", "auto.cloud", cx))
                    .child(self.composer_chip(
                        "auto-project",
                        "auto.pick_project",
                        cx,
                    )),
            )
            .child(text_area("auto-prompt", &self.prompt).into_any_element())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(8.))
                            .child(tool_mark(theme::icons::plus(), &self.theme))
                            .child(tool_mark(theme::icons::hash(), &self.theme))
                            .child(
                                div()
                                    .px(self.theme.space(8.))
                                    .py(self.theme.space(4.))
                                    .rounded_full()
                                    .bg(self.theme.soft())
                                    .text_xs()
                                    .text_color(self.theme.ink())
                                    .child(self.t("dock.automation")),
                            ),
                    )
                    .child(
                        theme::IconButton::new(
                            "auto-send",
                            self.t("studio.send"),
                            theme::icons::send(),
                        )
                        .size(theme::ControlSize::Medium)
                        .variant(theme::ControlVariant::Primary)
                        .render(
                            &self.theme,
                            cx.listener(|app, _e, _w, cx| {
                                app.notice = Some("auto.later");
                                cx.notify();
                            }),
                        ),
                    ),
            )
    }

    fn composer_chip(
        &self,
        id: &'static str,
        key: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::interactive(SharedString::from(id), self.t(key), &self.theme)
            .px(self.theme.space(8.))
            .py(self.theme.space(4.))
            .rounded_md()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft())
            .text_xs()
            .text_color(self.theme.muted())
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
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .rounded_xl()
            .px(self.theme.space(16.))
            .pt(self.theme.space(12.))
            .pb(self.theme.space(16.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
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
                        theme::IconButton::new(
                            "auto-mode-close",
                            self.t("ui.close"),
                            theme::icons::x(),
                        )
                        .size(theme::ControlSize::Small)
                        .variant(theme::ControlVariant::Ghost)
                        .render(
                            &self.theme,
                            cx.listener(|app, _e, _w, cx| {
                                app.notice = Some("auto.later");
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap(self.theme.space(12.))
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
        theme::interactive(SharedString::from(id), self.t(title), &self.theme)
            .flex_1()
            .min_w_0()
            .h(px(132.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft())
            .p(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.card()))
            .on_click(cx.listener(|app, _e, _w, cx| {
                app.notice = Some("auto.later");
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t(title)),
                    )
                    .children(badge.map(|key| {
                        div()
                            .px(self.theme.space(4.))
                            .rounded_sm()
                            .bg(self.theme.line())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(self.t(key))
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t(body)),
            )
    }

    fn suggestions_panel(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
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
                        theme::IconButton::new(
                            "auto-refresh",
                            self.t("ui.next"),
                            theme::icons::refresh_cw(),
                        )
                        .size(theme::ControlSize::Small)
                        .variant(theme::ControlVariant::Ghost)
                        .render(
                            &self.theme,
                            cx.listener(|app, _e, _w, cx| {
                                app.notice = Some("auto.later");
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap(self.theme.space(12.))
                    .items_stretch()
                    .child(self.suggestion_column(
                        "auto-s1",
                        "G",
                        self.suggestion_colors(theme::Accent::Red),
                        "auto.suggest_growth_title",
                        "auto.suggest_growth_body",
                        cx,
                    ))
                    .child(self.suggestion_column(
                        "auto-s2",
                        "S",
                        self.suggestion_colors(theme::Accent::Green),
                        "auto.suggest_return_title",
                        "auto.suggest_return_body",
                        cx,
                    ))
                    .child(self.suggestion_column(
                        "auto-s3",
                        theme::icons::infinity().size(px(20.)),
                        self.suggestion_colors(theme::Accent::Blue),
                        "auto.suggest_ads_title",
                        "auto.suggest_ads_body",
                        cx,
                    )),
            )
    }

    fn suggestion_colors(&self, accent: theme::Accent) -> theme::ControlColors {
        theme::ResolvedTheme::resolve(
            theme::ThemeOptions {
                accent,
                ..self.theme.options
            },
            self.theme.dark,
        )
        .control_colors(theme::ControlVariant::Primary)
    }

    fn suggestion_column(
        &self,
        id: &'static str,
        mark: impl IntoElement,
        colors: theme::ControlColors,
        title: &'static str,
        body: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::interactive(SharedString::from(id), self.t(title), &self.theme)
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
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
                    .border_color(self.theme.line())
                    .bg(self.theme.card())
                    .shadow_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(40.))
                            .rounded_lg()
                            .bg(colors.background)
                            .text_color(colors.foreground)
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
                    .px(self.theme.space(4.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(4.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t(title)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(self.t(body)),
                    ),
            )
    }
}

fn tool_mark(
    icon: theme::icons::Icon,
    theme: &theme::ResolvedTheme,
) -> impl IntoElement {
    div()
        .size(px(28.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(theme.muted())
        .child(icon.text_color(theme.muted()))
}
