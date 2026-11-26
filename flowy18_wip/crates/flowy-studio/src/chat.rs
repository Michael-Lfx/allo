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
            .when(matches!(self.status, TurnStatus::Completed), |column| {
                column.child(
                    div().w_full().flex().justify_center().child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(crate::STAGE_MAX_W))
                            .px(self.theme.space(32.))
                            .child(self.rating_row(cx)),
                    ),
                )
            })
            .child(self.pending_cards_slot(cx))
            .child(self.stop_handoff_slot(cx))
            .child(self.bg_tasks_dock_slot(cx))
            .child(self.composer.clone())
    }

    fn bg_tasks_pill_trigger(
        &self,
        summary: crate::router_works::BgPillSummary,
        scale: f32,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let is_attention =
            summary.tone == crate::router_works::BgPillTone::Attention;
        let dot_color = if is_attention {
            self.theme.status_attention()
        } else {
            self.theme.status_active()
        };
        let border_color = if is_attention {
            self.theme.status_attention().opacity(0.45)
        } else {
            self.theme.line()
        };
        let label = summary.label;
        let count = summary.count;
        let dot_el = crate::motion::live_dot(
            "bg-pill-dot",
            dot_color,
            self.theme.motion_policy(),
        );
        let text_el = div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(if is_attention {
                self.theme.status_attention()
            } else {
                self.theme.ink()
            })
            .child(label.clone());
        let label_el = if scale > 0.0 {
            text_el
                .with_animation(
                    ("bg-pill-count", count as u64),
                    Animation::new(crate::motion::COUNTER_ROLL)
                        .with_easing(crate::motion::damped_pop()),
                    move |el, delta| {
                        el.opacity(delta.clamp(0.0, 1.0))
                            .relative()
                            .top(px(6. * scale * (1.0 - delta)))
                    },
                )
                .into_any_element()
        } else {
            text_el.into_any_element()
        };
        let trigger = theme::interactive("bg-tasks-pill", label, &self.theme)
            // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
            // `Action::Expand`, so keeping a plain button stays clickable via Windows UIA.
            .rounded_full()
            .border_1()
            .border_color(border_color)
            .bg(self.theme.pill_surface())
            .shadow_sm()
            .px(self.theme.space(10.))
            .py(px(3.))
            .flex()
            .items_center()
            .gap(self.theme.space(6.))
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .child(dot_el)
            .child(label_el)
            .when_some(summary.detail, |el, detail| {
                el.child(
                    div()
                        .max_w(px(180.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(format!("\u{00b7} {detail}")),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_color(self.theme.muted())
                    .child(if self.is_bg_tasks_open() {
                        theme::icons::chevron_up().size(px(12.))
                    } else {
                        theme::icons::chevron_down().size(px(12.))
                    }),
            )
            .on_click(cx.listener(|panel, _, _, cx| {
                panel.toggle_bg_tasks(cx);
            }));
        theme::anchor_trigger(trigger, "bg-tasks-trigger", &self.ui)
    }

    fn bg_tasks_dock_slot(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let pill = self.router_bg_pill_summary();
        let followup_badge = self.router_followup_badge();
        let scale = crate::motion::travel_scale(self.theme.motion_policy());
        let left = div()
            .flex()
            .items_center()
            .gap(self.theme.space(6.))
            .min_w_0()
            .when_some(pill, |slot, summary| {
                slot.child(self.bg_tasks_pill_trigger(summary, scale, cx))
            })
            .children(followup_badge);
        let row = div()
            .flex_1()
            .min_w_0()
            .max_w(px(crate::STAGE_MAX_W))
            .min_h(px(28.))
            .px(self.theme.space(32.))
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(left)
            .child(self.working.clone());
        div()
            .id("bg-tasks-slot")
            .w_full()
            .flex()
            .justify_center()
            .child(row)
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
