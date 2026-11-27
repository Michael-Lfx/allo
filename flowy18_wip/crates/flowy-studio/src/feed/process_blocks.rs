//! Collapsible reasoning, tool-call, and tool-output blocks in the message feed.

use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, Context, FontWeight, SharedString, div,
    prelude::*, px,
};

use super::ChatFeed;

/// Compact single-line snippet of the opening line of a tool output.
fn head_line_preview(text: &str, max_chars: usize) -> (String, bool) {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(first) = lines.next() else {
        return (String::new(), false);
    };
    let has_more_lines = lines.next().is_some();
    let collapsed: String =
        first.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    let truncated = chars.next().is_some() || has_more_lines;
    if truncated {
        (format!("{head}\u{2026}"), true)
    } else {
        (head, false)
    }
}

impl ChatFeed {
    pub(super) fn reasoning_block(
        &self,
        index: usize,
        text: &str,
        duration_secs: u64,
        live: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let row_id = self.row_ids[index];
        let expanded = self.expanded_rows.contains(&row_id);
        let has_body = !text.trim().is_empty();
        let glimpse = (!expanded)
            .then(|| crate::history::last_line_snippet(text, 56))
            .flatten();
        let animate_pulse =
            crate::motion::travel_scale(self.theme.motion_policy()) > 0.0;
        let title = if live {
            self.t("chat.reasoning_live")
        } else {
            let duration = self.format_duration_secs(duration_secs);
            self.tf("chat.thought_for", &[("duration", &duration)])
        };
        let leading = if live {
            let row = div()
                .flex_none()
                .flex()
                .items_center()
                .gap(self.theme.space(6.))
                .child(
                    theme::icons::loader_circle()
                        .size(px(12.))
                        .text_color(self.theme.status_active())
                        .spinning(self.theme.motion_policy()),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.muted())
                        .child(title.clone()),
                );
            if animate_pulse {
                row.with_animation(
                    ("reasoning-live", row_id),
                    Animation::new(crate::motion::PULSE)
                        .repeat()
                        .with_easing(crate::motion::pulsing_sine()),
                    |el, delta| el.opacity(0.45f32.mul_add(-delta, 1.0)),
                )
                .into_any_element()
            } else {
                row.into_any_element()
            }
        } else {
            div()
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(self.theme.muted())
                .child(title.clone())
                .into_any_element()
        };
        let mut block = div()
            .id(("chat-reasoning", row_id))
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(4.))
            .child(
                // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
                // `Action::Expand`, which breaks Windows UIA button clicks.
                theme::interactive(
                    ("chat-reasoning-toggle", row_id),
                    title,
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(6.))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .rounded_md()
                .when(has_body, |row| {
                    row.cursor_pointer()
                        .hover(|style| style.bg(self.theme.soft()))
                })
                .when(has_body, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(if expanded {
                                theme::icons::chevron_down().size(px(12.))
                            } else {
                                theme::icons::chevron_right().size(px(12.))
                            }),
                    )
                })
                .child(leading)
                .when_some(glimpse, |row, snippet| {
                    row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(self.theme.muted().opacity(0.75))
                            .child(snippet),
                    )
                })
                .on_click(cx.listener(
                    move |feed, _, _, cx| {
                        if has_body {
                            feed.toggle_detail(row_id, cx);
                        }
                    },
                )),
            );
        if expanded && has_body {
            let content = div()
                .ml(px(8.))
                .pl(self.theme.space(12.))
                .py(self.theme.space(4.))
                .border_l_1()
                .border_color(self.theme.line())
                .text_xs()
                .text_color(self.theme.muted())
                .whitespace_normal()
                .child(text.to_owned());
            block = block.child(self.reveal_collapsible(
                ("reasoning-reveal", row_id),
                row_id,
                content,
                cx,
            ));
        }
        block
    }

    pub(super) fn tool_call_block(
        &self,
        index: usize,
        name: &str,
        target: &str,
        running: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let row_id = self.row_ids[index];
        let has_target = !target.is_empty();
        let expanded = has_target && self.expanded_rows.contains(&row_id);
        let heading = if running {
            self.tf("chat.tool_live", &[("name", name)])
        } else {
            name.to_owned()
        };
        let accessible = if target.is_empty() {
            heading.clone()
        } else {
            format!("{heading} {target}")
        };
        let status_mark = if running {
            crate::motion::live_dot(
                ("tool-live-dot", row_id),
                self.theme.status_active(),
                self.theme.motion_policy(),
            )
        } else {
            theme::icons::check()
                .size(px(12.))
                .text_color(self.theme.status_verified())
                .into_any_element()
        };
        let mut row = div()
            .id(("chat-tool", row_id))
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft().opacity(0.45))
            .flex()
            .flex_col()
            .child(
                // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
                // `Action::Expand`, which breaks Windows UIA button clicks.
                theme::interactive(
                    ("chat-tool-toggle", row_id),
                    accessible,
                    &self.theme,
                )
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(10.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .when(has_target, |el| {
                    el.cursor_pointer()
                        .hover(|style| style.bg(self.theme.soft()))
                })
                .child(status_mark)
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .child(SharedString::from(heading)),
                )
                .when(has_target, |el| {
                    el.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(target.to_owned())),
                    )
                })
                .when(!has_target, |el| el.child(div().flex_1()))
                .when(has_target, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(if expanded {
                                theme::icons::chevron_down().size(px(12.))
                            } else {
                                theme::icons::chevron_right().size(px(12.))
                            }),
                    )
                })
                .on_click(cx.listener(
                    move |feed, _, _, cx| {
                        if has_target {
                            feed.toggle_detail(row_id, cx);
                        }
                    },
                )),
            );
        if expanded {
            let content = div()
                .px(self.theme.space(10.))
                .pb(self.theme.space(8.))
                .pt(self.theme.space(4.))
                .border_t_1()
                .border_color(self.theme.line())
                .text_xs()
                .text_color(self.theme.muted())
                .whitespace_normal()
                .child(format!("{name} {target}"));
            row = row.child(self.reveal_collapsible(
                ("tool-reveal", row_id),
                row_id,
                content,
                cx,
            ));
        }
        row
    }

    pub(super) fn tool_output_block(
        &self,
        index: usize,
        text: &str,
        is_error: bool,
        id_prefix: &'static str,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let row_id = self.row_ids[index];
        if is_error {
            return div()
                .id((id_prefix, row_id))
                .role(gpui::Role::Alert)
                .w_full()
                .rounded_lg()
                .border_1()
                .border_color(self.theme.danger())
                .bg(self.theme.danger_bg())
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .flex()
                .items_start()
                .gap(self.theme.space(8.))
                .child(
                    theme::icons::circle_alert()
                        .size(px(14.))
                        .text_color(self.theme.danger()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(self.theme.danger())
                        .whitespace_normal()
                        .child(text.to_owned()),
                )
                .into_any_element();
        }
        let (preview, expandable) = head_line_preview(text, 72);
        let expanded = expandable && self.expanded_rows.contains(&row_id);
        let display_preview = if preview.is_empty() {
            text.trim().to_owned()
        } else {
            preview
        };
        let mut block = div()
            .id((id_prefix, row_id))
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft().opacity(0.3))
            .flex()
            .flex_col()
            .child(
                // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
                // `Action::Expand`, which breaks Windows UIA button clicks.
                theme::interactive(
                    ("chat-tool-out-toggle", row_id),
                    display_preview.clone(),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(10.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .when(expandable, |row| {
                    row.cursor_pointer()
                        .hover(|style| style.bg(self.theme.soft()))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(display_preview),
                )
                .when(expandable, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(if expanded {
                                theme::icons::chevron_down().size(px(12.))
                            } else {
                                theme::icons::chevron_right().size(px(12.))
                            }),
                    )
                })
                .on_click(cx.listener(
                    move |feed, _, _, cx| {
                        if expandable {
                            feed.toggle_detail(row_id, cx);
                        }
                    },
                )),
            );
        if expanded {
            let content = div()
                .px(self.theme.space(10.))
                .pb(self.theme.space(8.))
                .pt(self.theme.space(4.))
                .border_t_1()
                .border_color(self.theme.line())
                .text_xs()
                .text_color(self.theme.muted())
                .whitespace_normal()
                .child(text.to_owned());
            block = block.child(self.reveal_collapsible(
                ("tool-out-reveal", row_id),
                row_id,
                content,
                cx,
            ));
        }
        block.into_any_element()
    }
}
