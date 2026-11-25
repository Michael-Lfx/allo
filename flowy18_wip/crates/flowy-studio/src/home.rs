//! Home (Fresh Desk) stage rendering.
//!
//! Extracted from `studio.rs` so the Home / Spaces experience can evolve
//! independently from the active Router chat timeline.

use flowy_ui as theme;
use gpui::{Animation, AnimationExt, Context, Window, div, prelude::*, px};

/// Maximum width of the calm Home stage, shared with the composer and starters.
const HOME_STAGE_W: f32 = 760.;

impl crate::panel::StudioPanel {
    pub(crate) fn studio(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        // The width is left to flex: this column takes whatever the drawer
        // leaves, so an open drawer shrinks it without any pixel math.
        div()
            .id("studio-scroll")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
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
        let active_work = self.background_work_count();
        div()
            // `w_full` collapses inside a centred flex row; `flex_1` takes the
            // width from the parent so narrow windows stay responsive.
            .flex_1()
            .min_w_0()
            .max_w(px(HOME_STAGE_W))
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
            .when(active_work > 0, |hero| {
                hero.child(self.home_desk_resume(active_work, cx))
            })
    }

    fn home_desk_resume(
        &self,
        active_work: usize,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let resume_label = self.t("home.desk_resume");
        let count = active_work.to_string();
        let count_label =
            self.tf("chat.bg_tasks_pill", &[("count", count.as_str())]);
        theme::anchor_trigger(
            theme::interactive(
                "home-desk-resume",
                resume_label.clone(),
                &self.theme,
            )
            .aria_expanded(self.bg_tasks_open)
            .mt(self.theme.space(4.))
            .px(self.theme.space(12.))
            .py(self.theme.space(4.))
            .rounded_full()
            .border_1()
            .border_color(self.theme.line())
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
            .child(
                div()
                    .size(px(6.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(self.theme.status_active())
                    .with_animation(
                        "home-desk-resume-dot",
                        Animation::new(crate::motion::PULSE)
                            .repeat()
                            .with_easing(crate::motion::pulsing_sine()),
                        |el, t| el.opacity(0.45_f32.mul_add(t, 0.55)),
                    ),
            )
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
                    .bg(self.theme.soft())
                    .text_size(self.theme.caption_size())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(self.theme.status_active())
                    .child(count_label),
            )
            .child(
                div().flex_none().text_color(self.theme.muted()).child(
                    if self.bg_tasks_open {
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
