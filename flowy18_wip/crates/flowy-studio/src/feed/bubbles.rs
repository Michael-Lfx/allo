//! Turn-fold capsules, user/assistant message bubbles, hover actions, and line dispatch.

use flowy_agent::ChatLine;
use flowy_ui as theme;
use gpui::{Context, FontWeight, SharedString, div, prelude::*, px};

use super::{ChatFeed, OpenTask, QuoteMessage};
use crate::markdown;

impl ChatFeed {
    fn copy_row(
        &mut self,
        row_id: u64,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.copy_epoch = self.copy_epoch.wrapping_add(1);
        let epoch = self.copy_epoch;
        self.copied_row = Some((row_id, epoch));
        cx.notify();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1200))
                .await;
            let _ = weak.update(cx, |feed, cx| {
                if feed.copied_row == Some((row_id, epoch)) {
                    feed.copied_row = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn contextual_action_label(
        base: String,
        raw_text: &str,
    ) -> String {
        let snippet = crate::composer::truncate_quote_excerpt(raw_text, 28);
        if snippet.is_empty() {
            base
        } else {
            format!("{base} \u{00b7} {snippet}")
        }
    }

    fn bubble_action_bar(
        &self,
        row_id: u64,
        hover_group: SharedString,
        role: crate::composer::QuoteRole,
        action_text: String,
        align_end: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let is_copied = self.copied_row.is_some_and(|(id, _)| id == row_id);
        let soft = self.theme.soft();
        let ink = self.theme.ink();
        let verified = self.theme.status_verified();
        let muted = self.theme.muted();
        let copy_label = Self::contextual_action_label(
            if is_copied {
                self.t("chat.copied")
            } else {
                self.t("chat.copy_reply")
            },
            &action_text,
        );
        let copy_icon = if is_copied {
            theme::icons::check().size(px(13.))
        } else {
            theme::icons::copy().size(px(13.))
        };
        let quote_label = Self::contextual_action_label(
            self.t("chat.quote_reply"),
            &action_text,
        );
        let hover_fg = if is_copied { verified } else { ink };
        let (bar_sel, copy_sel, quote_sel): (
            &'static str,
            &'static str,
            &'static str,
        ) = match role {
            crate::composer::QuoteRole::User => {
                ("chat-user-action-bar", "chat-user-copy", "chat-user-quote")
            }
            crate::composer::QuoteRole::Assistant => (
                "chat-final-action-bar",
                "chat-copy-reply",
                "chat-quote-reply",
            ),
        };
        let copy_payload = action_text.clone();
        div()
            .debug_selector(move || bar_sel.to_owned())
            .w_full()
            .h(px(26.))
            .flex()
            .items_center()
            .gap(px(4.))
            .when(align_end, |bar| bar.justify_end().pr(px(4.)))
            .when(!align_end, |bar| bar.justify_start().pl(px(4.)))
            .opacity(if is_copied { 1.0 } else { 0.0 })
            .when(!is_copied, |bar| {
                bar.group_hover(hover_group, |style| style.opacity(1.0))
            })
            .child(
                theme::interactive(
                    ("chat-copy-reply", row_id),
                    copy_label,
                    &self.theme,
                )
                .debug_selector(move || copy_sel.to_owned())
                .size(px(24.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(if is_copied { verified } else { muted })
                .hover(move |style| style.bg(soft).text_color(hover_fg))
                .child(copy_icon)
                .on_click(cx.listener(
                    move |feed, _, _, cx| {
                        feed.copy_row(row_id, copy_payload.clone(), cx);
                    },
                )),
            )
            .child(
                theme::interactive(
                    ("chat-quote-reply", row_id),
                    quote_label,
                    &self.theme,
                )
                .debug_selector(move || quote_sel.to_owned())
                .size(px(24.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(muted)
                .hover(move |style| style.bg(soft).text_color(ink))
                .child(theme::icons::quote().size(px(13.)))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(QuoteMessage {
                        role,
                        text: action_text.clone(),
                    });
                })),
            )
    }

    pub(super) fn history_line(
        &self,
        index: usize,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let line = &self.lines[index];
        let Some(fold) = self.projection.folds.get(&index).copied() else {
            let is_nested_process =
                self.projection.process_rows.contains(&index)
                    || matches!(
                        line,
                        ChatLine::Reasoning { .. }
                            | ChatLine::ToolCall { .. }
                            | ChatLine::Tool { .. }
                            | ChatLine::ToolResult {
                                is_error: false,
                                ..
                            }
                    );
            if is_nested_process {
                return div()
                    .w_full()
                    .ml(px(8.))
                    .pl(self.theme.space(12.))
                    .border_l_1()
                    .border_color(self.theme.line())
                    .child(self.chat_line(index, line, cx))
                    .into_any_element();
            }
            return self.chat_line(index, line, cx).into_any_element();
        };
        let row_id = self.row_ids[index];
        let meta = self.projection.fold_meta.get(&index);
        let key = if fold.expanded {
            "history.hide_work"
        } else {
            "history.show_work"
        };
        let accessible_title = self.t(key);
        let show = !fold.expanded;
        let is_live = meta.is_some_and(|m| m.live);
        let policy = self.theme.motion_policy();

        let summary_text = meta.map_or_else(
            || accessible_title.clone(),
            |m| {
                let count = m.step_count.max(1).to_string();
                if m.live {
                    self.tf("chat.steps_live_summary", &[("count", &count)])
                } else {
                    self.tf("chat.steps_done_summary", &[("count", &count)])
                }
            },
        );
        let active_glimpse = meta
            .filter(|_| !fold.expanded)
            .and_then(|m| m.active_step.clone());

        let leading_indicator = if is_live {
            crate::motion::live_dot(
                ("turn-fold-live-dot", row_id),
                self.theme.status_active(),
                policy,
            )
        } else {
            theme::icons::check()
                .size(px(12.))
                .text_color(self.theme.status_verified())
                .into_any_element()
        };

        // A plain button keeps the standard invoke path (pointer, keyboard and
        // UIA click all land on the same handler); an expanded a11y state would
        // turn the node into a menu button whose invoke is never delivered.
        let capsule = theme::interactive(
            ("turn-fold", row_id),
            accessible_title,
            &self.theme,
        )
        .debug_selector(|| "chat-turn-fold".to_owned())
        .max_w_full()
        .px(self.theme.space(10.))
        .py(px(5.))
        .rounded_full()
        .border_1()
        .border_color(if is_live {
            self.theme.status_active().opacity(0.35)
        } else {
            self.theme.line()
        })
        .bg(if is_live {
            self.theme.status_active_bg()
        } else {
            self.theme.soft().opacity(0.55)
        })
        .flex()
        .items_center()
        .gap(self.theme.space(6.))
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft()))
        .child(leading_indicator)
        .child(
            div()
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(if is_live {
                    self.theme.ink()
                } else {
                    self.theme.muted()
                })
                .child(summary_text),
        )
        .when_some(active_glimpse, |row, glimpse| {
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .text_xs()
                    .text_color(self.theme.muted().opacity(0.8))
                    .child(format!("\u{00b7} {glimpse}")),
            )
        })
        .child(div().flex_none().text_color(self.theme.muted()).child(
            if fold.expanded {
                theme::icons::chevron_down().size(px(12.))
            } else {
                theme::icons::chevron_right().size(px(12.))
            },
        ))
        .on_click(cx.listener(move |feed, _, _, cx| {
            feed.set_turn_expanded(fold.key, show, cx);
        }));

        div()
            .w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(self.theme.space(6.))
            .child(capsule)
            .when(fold.expanded, |col| {
                let body = div()
                    .w_full()
                    .ml(px(8.))
                    .pl(self.theme.space(12.))
                    .border_l_1()
                    .border_color(self.theme.line())
                    .child(self.chat_line(index, line, cx));
                col.child(crate::motion::auto_height_reveal(
                    ("turn-fold-reveal", row_id),
                    0.0,
                    policy,
                    body,
                ))
            })
            .into_any_element()
    }

    fn assistant_card_bubble(
        &self,
        content: impl IntoElement,
        max_width: f32,
        pad_y: f32,
        selector: &'static str,
    ) -> gpui::Div {
        div().max_w_full().min_w_0().flex().child(
            div()
                .debug_selector(move || selector.to_owned())
                .flex_shrink(1.)
                .min_w_0()
                .max_w(px(max_width))
                .px(self.theme.space(16.))
                .py(self.theme.space(pad_y))
                .rounded(px(18.))
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.card())
                .shadow_xs()
                .child(content),
        )
    }

    fn chat_line(
        &self,
        index: usize,
        line: &ChatLine,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        match line {
            ChatLine::User { text } => {
                let row_id = self.row_ids[index];
                let hover_group =
                    SharedString::from(format!("msg-hover-{row_id}"));
                let (quoted, body) = crate::composer::split_quoted_prompt(text);
                let action_text = if body.is_empty() {
                    text.clone()
                } else {
                    body.clone()
                };
                div()
                    .id(("chat-user", row_id))
                    .group(hover_group.clone())
                    .w_full()
                    .py(self.theme.space(4.))
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(self.theme.space(4.))
                    .child(
                        div()
                            .max_w_full()
                            .min_w_0()
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .debug_selector(|| {
                                        "chat-user-bubble".to_owned()
                                    })
                                    .flex_shrink(1.)
                                    .min_w_0()
                                    .max_w(px(560.))
                                    .px(self.theme.space(16.))
                                    .py(self.theme.space(10.))
                                    .rounded(px(18.))
                                    .border_1()
                                    .border_color(self.theme.accent().opacity(
                                        if self.theme.dark {
                                            0.36
                                        } else {
                                            0.24
                                        },
                                    ))
                                    .bg(self.theme.accent().opacity(
                                        if self.theme.dark {
                                            0.22
                                        } else {
                                            0.11
                                        },
                                    ))
                                    .shadow_xs()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .when_some(quoted, |bubble, quote| {
                                        bubble.child(
                                            div()
                                                .debug_selector(|| {
                                                    "chat-user-quote-preview"
                                                        .to_owned()
                                                })
                                                .max_w_full()
                                                .min_w_0()
                                                .pl(px(8.))
                                                .border_l_2()
                                                .border_color(
                                                    self.theme
                                                        .accent()
                                                        .opacity(0.65),
                                                )
                                                .flex()
                                                .items_center()
                                                .gap(px(4.))
                                                .text_xs()
                                                .text_color(self.theme.muted())
                                                .child(div().flex_none().child(
                                                    format!(
                                                        "{}:",
                                                        self.t(
                                                            "chat.quoted_label"
                                                        )
                                                    ),
                                                ))
                                                .child(
                                                    div()
                                                        .min_w_0()
                                                        .flex_shrink(1.)
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .truncate()
                                                        .child(quote.excerpt),
                                                ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_sm()
                                            .text_color(self.theme.ink())
                                            .whitespace_normal()
                                            .child(body),
                                    ),
                            ),
                    )
                    .child(self.bubble_action_bar(
                        row_id,
                        hover_group,
                        crate::composer::QuoteRole::User,
                        action_text,
                        true,
                        cx,
                    ))
                    .into_any_element()
            }
            ChatLine::Assistant { text } => {
                let row_id = self.row_ids[index];
                let is_streaming = self.projection.streaming == Some(index);
                let is_ack = self.projection.ack_rows.contains(&index);
                let is_process = self.projection.process_rows.contains(&index);
                let markdown_body = div()
                    .min_w_0()
                    .text_sm()
                    .whitespace_normal()
                    .child(if is_streaming {
                        markdown::render_streaming(text, self.theme, &self.i18n)
                    } else {
                        markdown::render(text, self.theme, &self.i18n)
                    });
                if is_process {
                    return div()
                        .id(("chat-assistant", row_id))
                        .w_full()
                        .py(self.theme.space(2.))
                        .flex()
                        .flex_col()
                        .child(markdown_body)
                        .into_any_element();
                }
                if is_ack {
                    return div()
                        .id(("chat-assistant", row_id))
                        .w_full()
                        .py(self.theme.space(4.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .child(self.assistant_card_bubble(
                            markdown_body,
                            580.,
                            10.,
                            "chat-ack-bubble",
                        ))
                        .into_any_element();
                }
                if is_streaming && text.trim().is_empty() {
                    return div()
                        .id(("chat-assistant", row_id))
                        .w_full()
                        .into_any_element();
                }
                let hover_group =
                    SharedString::from(format!("msg-hover-{row_id}"));
                let show_actions = !is_streaming && !text.trim().is_empty();
                div()
                    .id(("chat-assistant", row_id))
                    .when(show_actions, |row| row.group(hover_group.clone()))
                    .w_full()
                    .py(self.theme.space(6.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(self.theme.space(4.))
                    .child(self.assistant_card_bubble(
                        markdown_body,
                        640.,
                        12.,
                        "chat-final-bubble",
                    ))
                    .when(show_actions, |col| {
                        col.child(self.bubble_action_bar(
                            row_id,
                            hover_group,
                            crate::composer::QuoteRole::Assistant,
                            text.clone(),
                            false,
                            cx,
                        ))
                    })
                    .into_any_element()
            }
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
            ChatLine::Tool { text } => self
                .tool_output_block(index, text, false, "chat-tool-legacy", cx)
                .into_any_element(),
            ChatLine::ToolResult { text, is_error } => {
                let label = if text.starts_with("chat.publish.") {
                    self.t(text)
                } else {
                    text.clone()
                };
                self.tool_output_block(
                    index,
                    &label,
                    *is_error,
                    "chat-tool-result",
                    cx,
                )
                .into_any_element()
            }
            ChatLine::Status { text } => {
                let label = if text.starts_with("chat.") {
                    self.t(text)
                } else {
                    text.clone()
                };
                div()
                    .id(("chat-status", self.row_ids[index]))
                    .role(gpui::Role::Status)
                    .aria_label(label.clone())
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(label)
                    .into_any_element()
            }
            ChatLine::TaskReceipt { task_id, goal } => {
                let task_id = task_id.clone();
                theme::Button::new(
                    ("chat-task-receipt", self.row_ids[index]),
                    self.i18n.tf("chat.task_receipt", &[("goal", goal)]),
                )
                .render(
                    &self.theme,
                    cx.listener(move |_, _, _, cx| {
                        cx.emit(OpenTask(task_id.clone()));
                    }),
                )
                .into_any_element()
            }
            ChatLine::FileChange {
                path,
                change,
                added,
                removed,
                binary,
            } => self
                .file_change_row(
                    index, path, change, *added, *removed, *binary, cx,
                )
                .into_any_element(),
            ChatLine::FileChanges { files } => {
                self.file_changes_row(index, files, cx).into_any_element()
            }
            ChatLine::Artifact { artifact_id } => self
                .artifact_card(index, artifact_id, cx)
                .into_any_element(),
            ChatLine::Verification {
                report_id,
                artifact_path,
                artifact_hash,
                ok,
                issues,
                semantic_hint,
                override_used,
                provenance_warnings,
                source_session_id,
            } => self.verification_card(
                index,
                source_session_id.as_deref(),
                report_id,
                artifact_path,
                artifact_hash,
                *ok,
                issues,
                semantic_hint.as_deref(),
                *override_used,
                provenance_warnings,
                cx,
            ),
        }
    }
}
