use crate::panel::StudioEvent;
use flowy_agent::{ChatLine, TurnStatus};
use flowy_ui as theme;
use gpui::{Context, FontWeight, SharedString, div, prelude::*, px};

const SUGGESTIONS: &[&str] = &[
    "gallery.documentary",
    "gallery.budget",
    "gallery.fashion",
];

impl crate::panel::StudioPanel {
    pub(crate) fn chat_canvas(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let lines = self.agent.lines();
        let status = self.agent.turn_status();
        let running = status.is_busy();

        let mut feed = div()
            .id("chat-feed")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .px_8()
            .pt_4()
            .pb_3();

        for (index, line) in lines.iter().enumerate() {
            feed = feed.child(self.chat_line(index, line, cx));
        }

        if running {
            feed = feed.child(self.working_indicator(&status));
        }

        if matches!(status, TurnStatus::Completed) && !running {
            feed = feed.child(self.rating_row(cx)).child(self.suggestions(cx));
        }

        div()
            .id("chat")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(feed)
            .child(self.chat_composer(running, cx))
    }

    fn chat_line(
        &self,
        index: usize,
        line: &ChatLine,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        match line {
            ChatLine::User { text } => div()
                .id(("chat-user", index))
                .w_full()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(540.))
                        .px_3()
                        .py_2()
                        .rounded_xl()
                        .bg(theme::soft())
                        .text_sm()
                        .whitespace_normal()
                        .child(text.clone()),
                )
                .into_any_element(),
            ChatLine::Assistant { text } => div()
                .id(("chat-assistant", index))
                .w_full()
                .py_1()
                .text_sm()
                .whitespace_normal()
                .child(text.clone())
                .into_any_element(),
            ChatLine::Reasoning {
                text,
                duration_secs,
                live,
            } => self
                .reasoning_block(index, text, *duration_secs, *live, cx)
                .into_any_element(),
            ChatLine::ToolCall {
                name,
                target,
                running,
                ..
            } => self
                .tool_call_block(index, name, target, *running, cx)
                .into_any_element(),
            ChatLine::Tool { text } => div()
                .id(("chat-tool-legacy", index))
                .ml(px(6.))
                .pl_3()
                .border_l_1()
                .border_color(theme::line())
                .text_xs()
                .text_color(theme::muted())
                .child(text.clone())
                .into_any_element(),
            ChatLine::ToolResult { text, is_error } => div()
                .id(("chat-tool-result", index))
                .ml(px(6.))
                .pl_3()
                .border_l(px(3.))
                .border_color(if *is_error {
                    theme::danger()
                } else {
                    theme::line()
                })
                .text_xs()
                .text_color(if *is_error {
                    theme::danger()
                } else {
                    theme::muted()
                })
                .whitespace_normal()
                .child(text.clone())
                .into_any_element(),
            ChatLine::Status { text } => {
                let label = if text.starts_with("chat.") {
                    self.t(text)
                } else {
                    text.clone()
                };
                div()
                    .id(("chat-status", index))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(div().child("✓"))
                    .child(label)
                    .into_any_element()
            }
        }
    }

    fn reasoning_block(
        &self,
        index: usize,
        text: &str,
        duration_secs: u64,
        live: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let key = format!("reasoning:{index}");
        let expanded = live || self.expanded_feed.contains(&key);
        let title = if live {
            self.t("chat.thinking")
        } else {
            let duration = self.format_duration_secs(duration_secs);
            self.tf("chat.thought_for", &[("duration", &duration)])
        };
        let key_for_click = key.clone();
        let mut block = div()
            .id(("chat-reasoning", index))
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id(("chat-reasoning-toggle", index))
                    .h(px(28.))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::muted())
                            .child(title),
                    )
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        if panel.expanded_feed.contains(&key_for_click) {
                            panel.expanded_feed.remove(&key_for_click);
                        } else {
                            panel.expanded_feed.insert(key_for_click.clone());
                        }
                        cx.notify();
                    })),
            );
        if expanded && !text.trim().is_empty() {
            block = block.child(
                div()
                    .ml(px(6.))
                    .pl_3()
                    .border_l_1()
                    .border_color(theme::line())
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::muted())
                            .child("thinking"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .whitespace_normal()
                            .child(text.to_owned()),
                    ),
            );
        }
        block
    }

    fn tool_call_block(
        &self,
        index: usize,
        name: &str,
        target: &str,
        running: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let key = format!("tool:{index}");
        let expanded = running || self.expanded_feed.contains(&key);
        let rail = if running {
            theme::accent()
        } else {
            theme::muted()
        };
        let key_for_click = key.clone();
        let label = if target.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {target}")
        };
        let mut row = div()
            .id(("chat-tool", index))
            .w_full()
            .ml(px(6.))
            .border_l(px(3.))
            .border_color(rail)
            .bg(theme::soft())
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("chat-tool-toggle", index))
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::card()))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::ink())
                            .child(SharedString::from(name.to_owned())),
                    )
                    .when(!target.is_empty(), |el| {
                        el.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme::muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(SharedString::from(target.to_owned())),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        if panel.expanded_feed.contains(&key_for_click) {
                            panel.expanded_feed.remove(&key_for_click);
                        } else {
                            panel.expanded_feed.insert(key_for_click.clone());
                        }
                        cx.notify();
                    })),
            );
        if expanded && !target.is_empty() {
            row = row.child(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(label),
            );
        }
        row
    }

    fn working_indicator(&self, status: &TurnStatus) -> impl IntoElement {
        div()
            .id("chat-working")
            .h(px(22.))
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(theme::muted())
                    .child(div().text_xs().child("·"))
                    .child(div().text_xs().child("·"))
                    .child(div().text_xs().child("·")),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::muted())
                    .child(self.working_label(status)),
            )
    }

    fn rating_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div()
            .id("chat-rating")
            .flex()
            .items_center()
            .gap_2()
            .pt_2()
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("chat.rate_prompt")),
            );
        for star in 1..=5 {
            row = row.child(
                div()
                    .id(("chat-star", star))
                    .px_1()
                    .cursor_pointer()
                    .child("★")
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.rate_result(star, cx);
                    })),
            );
        }
        row
    }

    fn suggestions(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut column = div().id("chat-suggestions").flex().flex_col().gap_2().pt_2();
        for (index, key) in SUGGESTIONS.iter().enumerate() {
            let key = *key;
            let label = self.t(key);
            column = column.child(
                div()
                    .id(("chat-suggest", index))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::soft())
                    .text_sm()
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.fill_prompt_key(key, cx);
                    })),
            );
        }
        column
    }

    pub(crate) fn rate_result(&mut self, stars: u32, cx: &mut Context<'_, Self>) {
        cx.emit(StudioEvent::Rated { stars });
        self.emit_notice(Some("chat.rate_thanks"), cx);
    }
}
