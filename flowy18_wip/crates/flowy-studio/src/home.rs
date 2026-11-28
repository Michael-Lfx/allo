//! Home (Fresh Desk) stage rendering.
//!
//! Extracted from `studio.rs` so the Home / Spaces experience can evolve
//! independently from the active Router chat timeline.

use flowy_ui as theme;
use gpui::{Context, Window, div, prelude::*, px};

impl crate::panel::StudioPanel {
    pub(crate) fn studio(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        // The width is left to flex: this column takes whatever the drawer
        // leaves, so an open drawer shrinks it without any pixel math.
        theme::scroll_area("studio-scroll", &self.theme)
            .a11y_label(self.t("a11y.scroll_home_stage"))
            .debug_selector(|| "studio-scroll".to_owned())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .viewport_style(|s| s.flex().flex_col())
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .pt(self.theme.space(64.))
                    .pb(self.theme.space(32.))
                    .gap(self.theme.space(16.))
                    // Each block centres itself; a parent `items_center`
                    // would collapse `w_full` children to their content.
                    .child(
                        div()
                            .w_full()
                            .px(self.theme.space(32.))
                            .flex()
                            .justify_center()
                            .child(self.home_hero(cx)),
                    )
                    .child(self.composer.clone())
                    .child(
                        div()
                            .w_full()
                            .px(self.theme.space(32.))
                            .flex()
                            .justify_center()
                            .child(self.starter_section(window, cx)),
                    ),
            )
    }

    fn home_hero(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let title = self.t("home.title");
        let bg_pill = self.router_bg_pill_summary();
        div()
            // `w_full` collapses inside a centred flex row; `flex_1` takes the
            // width from the parent so narrow windows stay responsive.
            .flex_1()
            .min_w_0()
            .max_w(px(crate::STAGE_MAX_W))
            .flex()
            .flex_col()
            .items_center()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .size(px(36.))
                    .rounded_xl()
                    .bg(self.theme.assistant_badge_bg())
                    .text_color(self.theme.assistant_badge_fg())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        theme::icons::sparkles()
                            .size(px(16.))
                            .text_color(self.theme.assistant_badge_fg()),
                    ),
            )
            .child(
                div()
                    .id("home-goal")
                    .role(gpui::Role::Heading)
                    .aria_level(1)
                    .aria_label(title.clone())
                    .w_full()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .text_center()
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(520.))
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .text_center()
                    .whitespace_normal()
                    .child(self.t("home.subtitle")),
            )
            .when_some(bg_pill, |hero, summary| {
                hero.child(self.home_desk_resume(summary, cx))
            })
    }

    fn home_desk_resume(
        &self,
        summary: crate::router_works::BgPillSummary,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let resume_label = self.t("home.desk_resume");
        let is_attention =
            summary.tone == crate::router_works::BgPillTone::Attention;
        let dot_color = if is_attention {
            self.theme.status_attention()
        } else {
            self.theme.status_active()
        };
        let badge_bg = if is_attention {
            self.theme.status_attention_bg()
        } else {
            self.theme.status_active_bg()
        };
        let border_color = if is_attention {
            self.theme.status_attention().opacity(0.45)
        } else {
            self.theme.line()
        };
        let count_label = summary.label;
        let dot_el = crate::motion::live_dot(
            "home-desk-resume-dot",
            dot_color,
            self.theme.motion_policy(),
        );
        theme::anchor_trigger(
            theme::interactive(
                "home-desk-resume",
                resume_label.clone(),
                &self.theme,
            )
            // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
            // `Action::Expand`, so keeping a plain button stays clickable via Windows UIA.
            .mt(self.theme.space(4.))
            .px(self.theme.space(12.))
            .py(self.theme.space(4.))
            .rounded_full()
            .border_1()
            .border_color(border_color)
            .bg(self.theme.pill_surface())
            .shadow_sm()
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .cursor_pointer()
            .hover(|pill| pill.bg(self.theme.soft()))
            .on_click(cx.listener(|panel, _event, _window, cx| {
                panel.toggle_bg_tasks(cx);
            }))
            .child(dot_el)
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.ink())
                    .child(resume_label),
            )
            .child(
                div()
                    .px(self.theme.space(8.))
                    .py(px(1.))
                    .rounded_full()
                    .bg(badge_bg)
                    .text_size(self.theme.caption_size())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(dot_color)
                    .child(count_label),
            )
            .child(
                div().flex_none().text_color(self.theme.muted()).child(
                    if self.is_bg_tasks_open() {
                        theme::icons::chevron_up().size(px(12.))
                    } else {
                        theme::icons::chevron_down().size(px(12.))
                    },
                ),
            ),
            "bg-tasks-trigger",
            &self.ui,
        )
    }
}
