use crate::panel::StudioEvent;
use flowy_agent::TurnStatus;
use flowy_ui as theme;
use gpui::{Animation, AnimationExt, Context, FontWeight, div, prelude::*, px};

impl crate::panel::StudioPanel {
    pub(crate) fn chat_canvas(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("chat")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.feed.clone())
            .child(self.working.clone())
            .when(matches!(self.status, TurnStatus::Completed), |column| {
                column.child(
                    div()
                        .w_full()
                        .max_w(px(760.))
                        .mx_auto()
                        .px(self.theme.space(32.))
                        .child(self.rating_row(cx)),
                )
            })
            .child(self.bg_tasks_dock_slot(cx))
            .child(self.composer.clone())
    }

    fn bg_tasks_dock_slot(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let count = self.background_work_count();
        div()
            .id("bg-tasks-slot")
            .w_full()
            .max_w(px(760.))
            .mx_auto()
            .h(px(28.))
            .px(self.theme.space(32.))
            .flex()
            .items_center()
            .justify_start()
            .when(count > 0, |slot| {
                let label = self
                    .tf("chat.bg_tasks_pill", &[("count", &count.to_string())]);
                slot.child(theme::anchor_trigger(
                    theme::interactive(
                        "bg-tasks-pill",
                        label.clone(),
                        &self.theme,
                    )
                    .aria_expanded(self.bg_tasks_open)
                    .rounded_full()
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.pill_surface())
                    .shadow_sm()
                    .px(self.theme.space(10.))
                    .py(px(3.))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .cursor_pointer()
                    .hover(|style| style.bg(self.theme.soft()))
                    .child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(self.theme.status_active())
                            .with_animation(
                                "bg-pill-dot",
                                Animation::new(crate::motion::PULSE)
                                    .repeat()
                                    .with_easing(crate::motion::pulsing_sine()),
                                |dot, delta| {
                                    dot.opacity(0.6f32.mul_add(-delta, 1.0))
                                },
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.ink())
                            .child(label)
                            .with_animation(
                                ("bg-pill-count", count as u64),
                                Animation::new(crate::motion::COUNTER_ROLL)
                                    .with_easing(crate::motion::damped_pop()),
                                |el, delta| {
                                    el.opacity(delta.clamp(0.0, 1.0))
                                        .relative()
                                        .top(px(6. * (1.0 - delta)))
                                },
                            ),
                    )
                    .child(
                        div().flex_none().text_color(self.theme.muted()).child(
                            if self.bg_tasks_open {
                                theme::icons::chevron_up().size(px(12.))
                            } else {
                                theme::icons::chevron_down().size(px(12.))
                            },
                        ),
                    )
                    .on_click(cx.listener(
                        |panel, _, _, cx| {
                            panel.toggle_bg_tasks(cx);
                        },
                    )),
                    "bg-tasks-trigger",
                    &self.ui,
                ))
            })
    }

    fn rating_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div()
            .id("chat-rating")
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .pt(self.theme.space(8.))
            .child(
                div()
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(self.t("chat.rate_prompt")),
            );
        for star in 1..=5 {
            row = row.child(
                theme::interactive(
                    ("chat-star", star),
                    self.tf("ui.rating", &[("n", &star.to_string())]),
                    &self.theme,
                )
                .px(self.theme.space(4.))
                .cursor_pointer()
                .child(theme::icons::star())
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.rate_result(star, cx);
                    },
                )),
            );
        }
        row
    }

    pub(crate) fn rate_result(
        &mut self,
        stars: u32,
        cx: &mut Context<'_, Self>,
    ) {
        cx.emit(StudioEvent::Rated { stars });
        self.emit_notice(Some("chat.rate_thanks"), cx);
    }
}
