use crate::panel::StudioEvent;
use flowy_agent::{ChatLine, TurnStatus};
use flowy_ui as theme;
use gpui::{Context, SharedString, div, prelude::*, px};
use gpui_ce_elements::editable_text::text_area;

const SUGGESTIONS: &[&str] = &[
    "gallery.documentary",
    "gallery.budget",
    "gallery.fashion",
];

impl crate::panel::StudioPanel {
    pub(crate) fn chat_canvas(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let lines = self.agent.lines();
        let status = self.agent.turn_status();
        let running = matches!(status, TurnStatus::Running);

        let mut feed = div()
            .id("chat-feed")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .px_8()
            .pt_4();

        for (index, line) in lines.iter().enumerate() {
            feed = feed.child(self.chat_line(index, line, cx));
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
            .child(self.chat_status_strip(&status))
            .child(self.chat_composer(running, cx))
    }

    fn chat_line(
        &self,
        index: usize,
        line: &ChatLine,
        _cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        match line {
            ChatLine::User { text } => div()
                .id(("chat-user", index))
                .w_full()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(520.))
                        .px_3()
                        .py_2()
                        .rounded_xl()
                        .bg(theme::soft())
                        .text_sm()
                        .whitespace_normal()
                        .child(text.clone()),
                ),
            ChatLine::Assistant { text } => div()
                .id(("chat-assistant", index))
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::muted())
                        .child(self.product.to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .whitespace_normal()
                        .child(text.clone()),
                ),
            ChatLine::Tool { text } => div()
                .id(("chat-tool", index))
                .text_xs()
                .text_color(theme::muted())
                .child(text.clone()),
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
                    .child(div().child("✓"))
                    .child(label)
            }
        }
    }

    fn chat_status_strip(&self, status: &TurnStatus) -> impl IntoElement {
        let label = match status {
            TurnStatus::Error { message } => message.clone(),
            other => self.t(other.key()),
        };
        div()
            .id("chat-status-strip")
            .px_8()
            .py_2()
            .border_t_1()
            .border_color(theme::line())
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(theme::muted())
            .child(match status {
                TurnStatus::Completed => "✓",
                TurnStatus::Running => "…",
                TurnStatus::Stopped => "■",
                TurnStatus::Error { .. } => "!",
                TurnStatus::Idle => "·",
            })
            .child(label)
    }

    fn chat_composer(
        &self,
        running: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("chat-composer")
            .px_8()
            .pb_6()
            .pt_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .rounded_xl()
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::card())
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        text_area("chat-prompt")
                            .state(self.prompt.downgrade())
                            .placeholder(SharedString::from(
                                self.tf(
                                    "thread.prompt_placeholder",
                                    &[("product", self.product)],
                                ),
                            )),
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
                                    .child(self.chat_chip(
                                        "chat-attach",
                                        self.t("studio.attach"),
                                        "notice.attach_later",
                                        cx,
                                    ))
                                    .child(self.chat_chip(
                                        "chat-cloud",
                                        self.t("thread.cloud"),
                                        "notice.cloud_later",
                                        cx,
                                    )),
                            )
                            .child(if running {
                                div()
                                    .id("chat-stop")
                                    .px_3()
                                    .py_1()
                                    .rounded_lg()
                                    .bg(theme::soft())
                                    .text_sm()
                                    .cursor_pointer()
                                    .child(self.t("chat.stop"))
                                    .on_click(cx.listener(|panel, _, _, cx| {
                                        panel.stop_turn(cx);
                                    }))
                            } else {
                                div()
                                    .id("chat-send")
                                    .px_3()
                                    .py_1()
                                    .rounded_lg()
                                    .bg(theme::fill())
                                    .text_color(theme::on_fill())
                                    .text_sm()
                                    .cursor_pointer()
                                    .child(self.t("studio.send"))
                                    .on_click(cx.listener(|panel, _, _, cx| {
                                        panel.send_prompt(cx);
                                    }))
                            }),
                    ),
            )
    }

    fn chat_chip(
        &self,
        id: &'static str,
        label: String,
        notice: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded_md()
            .text_xs()
            .text_color(theme::muted())
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |panel, _, _, cx| {
                panel.note(notice, cx);
            }))
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
