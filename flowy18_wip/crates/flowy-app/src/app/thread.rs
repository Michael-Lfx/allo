use super::FlowyApp;
use crate::chat::{Item, MockStream};
use crate::theme;
use gpui::{Context, SharedString, div, prelude::*, px, rgb};
use gpui_ce_elements::editable_text::text_area;

impl FlowyApp {
    pub(super) fn thread_canvas(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let Some(thread) = &self.studio.thread else {
            return div().into_any_element();
        };
        div()
            .id("thread")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("thread-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.thread_timeline(thread, cx)),
            )
            .child(self.thread_composer(thread, cx))
            .into_any_element()
    }

    fn thread_timeline(
        &self,
        thread: &crate::chat::Thread,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut column = div()
            .w_full()
            .max_w(px(720.))
            .mx_auto()
            .px_8()
            .pt_8()
            .pb_10()
            .flex()
            .flex_col()
            .gap_4();
        let last = thread.items.len().saturating_sub(1);
        for (index, item) in thread.items.iter().enumerate() {
            let caret = thread.streaming
                && index == last
                && matches!(item, Item::Text(_));
            column = column.child(self.thread_item(index, item, caret, cx));
        }
        column
    }

    fn thread_item(
        &self,
        index: usize,
        item: &Item,
        caret: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        match item {
            Item::Text(text) => {
                let mut row = div()
                    .id(("thread-text", index))
                    .text_sm()
                    .whitespace_normal()
                    .text_color(theme::ink())
                    .child(text.clone());
                if caret {
                    row = row.child(
                        div()
                            .ml(px(1.))
                            .inline()
                            .text_color(theme::muted())
                            .child("|"),
                    );
                }
                row.into_any_element()
            }
            Item::Status { label } => div()
                .id(("thread-status", index))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(18.))
                        .rounded_md()
                        .bg(theme::soft())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(theme::muted())
                        .child("≡"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::muted())
                        .child(label.clone()),
                )
                .into_any_element(),
            Item::Confirm => self.confirm_card(cx).into_any_element(),
            Item::Waiting(label) => div()
                .id(("thread-wait", index))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(8.))
                        .rounded_full()
                        .bg(rgb(0x00e8_8a3a)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x00c4_6b20))
                        .child(label.clone()),
                )
                .into_any_element(),
        }
    }

    fn confirm_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("media-confirm")
            .w_full()
            .rounded_xl()
            .border_1()
            .border_color(theme::line())
            .bg(theme::card())
            .shadow_sm()
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t("thread.confirm_title")),
            )
            .child(Self::confirm_point("Aa", self.t("thread.confirm_ownership")))
            .child(Self::confirm_point("()", self.t("thread.confirm_consent")))
            .child(self.confirm_check(
                "confirm-rights",
                "thread.confirm_rights",
                self.studio.confirm_rights,
                true,
                cx,
            ))
            .child(self.confirm_check(
                "confirm-terms",
                "thread.confirm_terms",
                self.studio.confirm_terms,
                false,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        div()
                            .id("confirm-cancel")
                            .px_4()
                            .py_2()
                            .rounded_lg()
                            .bg(theme::soft())
                            .text_sm()
                            .cursor_pointer()
                            .child(self.t("thread.cancel"))
                            .on_click(cx.listener(|app, _e, _w, cx| {
                                app.cancel_thread_confirm(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("confirm-agree")
                            .px_4()
                            .py_2()
                            .rounded_lg()
                            .bg(if self.studio.confirm_rights
                                && self.studio.confirm_terms
                            {
                                theme::fill()
                            } else {
                                theme::soft()
                            })
                            .text_color(if self.studio.confirm_rights
                                && self.studio.confirm_terms
                            {
                                theme::on_fill()
                            } else {
                                theme::muted()
                            })
                            .text_sm()
                            .cursor_pointer()
                            .child(self.t("thread.agree"))
                            .on_click(cx.listener(|app, _e, _w, cx| {
                                app.agree_thread_confirm(cx);
                            })),
                    ),
            )
    }

    fn confirm_point(mark: &'static str, text: String) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .gap_3()
            .child(div().text_sm().child(mark))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .whitespace_normal()
                    .child(text),
            )
    }

    fn confirm_check(
        &self,
        id: &'static str,
        key: &'static str,
        on: bool,
        rights: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(id))
            .flex()
            .items_start()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(move |app, _e, _w, cx| {
                if rights {
                    app.studio.confirm_rights = !app.studio.confirm_rights;
                } else {
                    app.studio.confirm_terms = !app.studio.confirm_terms;
                }
                cx.notify();
            }))
            .child(
                div()
                    .mt(px(1.))
                    .size(px(16.))
                    .rounded_sm()
                    .border_1()
                    .border_color(theme::line())
                    .bg(if on { theme::fill() } else { theme::card() })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(theme::on_fill())
                    .child(if on { "✓" } else { "" }),
            )
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t(key)),
            )
    }

    fn thread_composer(
        &self,
        thread: &crate::chat::Thread,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut stack = div().px_8().pb_5().flex().flex_col().gap_2();
        if let Some(chip) = &thread.context_chip {
            stack = stack.child(
                div()
                    .w_full()
                    .max_w(px(720.))
                    .mx_auto()
                    .flex()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_lg()
                            .border_1()
                            .border_color(theme::line())
                            .bg(theme::card())
                            .text_xs()
                            .text_color(theme::muted())
                            .child(chip.clone()),
                    ),
            );
        }
        stack.child(
            div()
                .w_full()
                .max_w(px(720.))
                .mx_auto()
                .bg(theme::card())
                .border_1()
                .border_color(theme::line())
                .rounded(px(20.))
                .shadow_md()
                .px_4()
                .pt_3()
                .pb_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    text_area("thread-prompt")
                        .state(self.studio.prompt.downgrade())
                        .placeholder(self.tf(
                            "thread.prompt_placeholder",
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
                                .gap_3()
                                .child(Self::composer_icon(
                                    "thread-attach",
                                    "+",
                                    "notice.attach_later",
                                    cx,
                                ))
                                .child(Self::composer_icon(
                                    "thread-link",
                                    "#",
                                    "notice.attach_later",
                                    cx,
                                ))
                                .child(
                                    div()
                                        .id("thread-cloud")
                                        .text_xs()
                                        .text_color(theme::muted())
                                        .cursor_pointer()
                                        .child(self.t("thread.cloud"))
                                        .on_click(cx.listener(
                                            |app, _e, _w, cx| {
                                                app.note(
                                                    "notice.attach_later",
                                                    cx,
                                                );
                                            },
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(Self::composer_icon(
                                    "thread-mic",
                                    "o",
                                    "notice.attach_later",
                                    cx,
                                ))
                                .child(
                                    div()
                                        .id("thread-send")
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
                                        .on_click(cx.listener(
                                            |app, _e, _w, cx| {
                                                app.send_prompt(cx);
                                            },
                                        )),
                                ),
                        ),
                ),
        )
    }

    fn composer_icon(
        id: &'static str,
        mark: &'static str,
        notice: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(id))
            .size(px(28.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(theme::muted())
            .cursor_pointer()
            .hover(|style| style.bg(theme::soft()))
            .child(mark)
            .on_click(cx.listener(move |app, _e, _w, cx| {
                app.note(notice, cx);
            }))
    }

    pub(super) fn open_thread(
        &mut self,
        title: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.studio.confirm_rights = false;
        self.studio.confirm_terms = false;
        self.studio.stream_task = None;
        self.studio.thread =
            Some(crate::chat::Thread::empty("demo", title.clone()));
        self.studio.stream =
            Some(MockStream::demo(&title, crate::brand::product()));
        if !self.studio.tasks.iter().any(|task| task == &title) {
            self.studio.tasks.insert(0, title);
        }
        self.section = super::Section::Studio;
        self.notice = None;
        self.pump_stream(cx);
        cx.notify();
    }

    pub(super) fn clear_thread(&mut self, cx: &mut Context<'_, Self>) {
        self.studio.stream_task = None;
        self.studio.stream = None;
        self.studio.thread = None;
        self.studio.confirm_rights = false;
        self.studio.confirm_terms = false;
        cx.notify();
    }

    fn cancel_thread_confirm(&mut self, cx: &mut Context<'_, Self>) {
        self.clear_thread(cx);
        self.notice = Some("thread.confirm_cancelled");
    }

    fn agree_thread_confirm(&mut self, cx: &mut Context<'_, Self>) {
        if !(self.studio.confirm_rights && self.studio.confirm_terms) {
            self.notice = Some("thread.confirm_required");
            cx.notify();
            return;
        }
        let accepted = self.t("thread.confirm_accepted");
        let Some(thread) = self.studio.thread.as_mut() else {
            return;
        };
        thread.items.retain(|item| {
            !matches!(item, Item::Confirm | Item::Waiting(_))
        });
        thread.waiting = false;
        thread.streaming = true;
        self.studio.confirm_rights = false;
        self.studio.confirm_terms = false;
        self.studio.stream_task = None;
        self.studio.stream = Some(MockStream::after_confirm(&accepted));
        self.notice = None;
        self.pump_stream(cx);
        cx.notify();
    }

    fn pump_stream(&mut self, cx: &Context<'_, Self>) {
        self.studio.stream_task = None;
        let Some(stream) = self.studio.stream.as_mut() else {
            return;
        };
        let Some((delay, beat)) = stream.pop() else {
            self.studio.stream = None;
            if let Some(thread) = self.studio.thread.as_mut() {
                thread.streaming = false;
            }
            return;
        };
        self.studio.stream_task = Some(cx.spawn(async move |weak, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let _ = weak.update(cx, |app, cx| {
                let done = matches!(beat, crate::chat::Beat::Done);
                if let Some(thread) = app.studio.thread.as_mut() {
                    thread.apply(beat);
                }
                if done
                    || app
                        .studio
                        .stream
                        .as_ref()
                        .is_none_or(MockStream::is_empty)
                {
                    app.studio.stream = None;
                    if let Some(thread) = app.studio.thread.as_mut() {
                        thread.streaming = false;
                    }
                } else {
                    app.pump_stream(cx);
                }
                cx.notify();
            });
        }));
    }
}
