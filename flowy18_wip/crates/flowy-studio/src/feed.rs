//! Independently rendered, variable-height message feed and elapsed-time indicator.
use flowy_agent::ChatLine;
use flowy_i18n::I18n;
use flowy_ui::{self as theme, ResolvedTheme, UiContext};
use gpui::{
    Context, FontWeight, ListAlignment, ListState, Render, SharedString,
    Subscription, Window, div, list, prelude::*, px,
};
use std::collections::{HashMap, HashSet};

pub struct ChatFeed {
    lines: Vec<ChatLine>,
    session: String,
    states: HashMap<String, StoredFeed>,
    row_ids: Vec<u64>,
    next_row_id: u64,
    row_focus: HashMap<u64, gpui::FocusHandle>,
    list: ListState,
    expanded_feed: HashSet<String>,
    theme: ResolvedTheme,
    i18n: I18n,
    _theme_sub: Subscription,
    #[cfg(test)]
    rendered: usize,
    #[cfg(test)]
    rendered_rows: HashSet<u64>,
}

struct StoredFeed {
    lines: Vec<ChatLine>,
    row_ids: Vec<u64>,
    list: ListState,
}

/// A tool call has a real domain identity; streamed prose keeps its row
/// identity while its text grows. Exact prefix/suffix matches handle insertions.
fn same_row_kind(old: &ChatLine, new: &ChatLine) -> bool {
    match (old, new) {
        (
            ChatLine::ToolCall { call_id: old, .. },
            ChatLine::ToolCall { call_id: new, .. },
        ) => old == new,
        _ => std::mem::discriminant(old) == std::mem::discriminant(new),
    }
}

impl ChatFeed {
    pub(crate) fn new(
        ui: UiContext,
        i18n: I18n,
        session: Option<String>,
        lines: Vec<ChatLine>,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let theme = ui.theme.read(cx).resolved();
        let theme_sub = cx.observe(&ui.theme, |feed, entity, cx| {
            feed.theme = entity.read(cx).resolved();
            feed.list.remeasure_items(0..feed.lines.len());
            cx.notify();
        });
        // A conservative row estimate keeps bottom-aligned chat feeds
        // measurable before the first paint; real row heights replace it as
        // rows enter the viewport.
        let list = ListState::new(lines.len(), ListAlignment::Bottom, px(200.))
            .with_uniform_item_height(px(36.));
        let row_ids: Vec<_> = (0..lines.len() as u64).collect();
        let row_focus: HashMap<_, _> = row_ids
            .iter()
            .map(|id| (*id, cx.focus_handle().tab_stop(false)))
            .collect();
        list.splice_focusable(
            0..lines.len(),
            row_ids.iter().map(|id| Some(row_focus[id].clone())),
        );
        let next_row_id = lines.len() as u64;
        Self {
            lines,
            row_ids,
            next_row_id,
            row_focus,
            session: session.unwrap_or_default(),
            states: HashMap::new(),
            list,
            expanded_feed: HashSet::new(),
            theme,
            i18n,
            _theme_sub: theme_sub,
            #[cfg(test)]
            rendered: 0,
            #[cfg(test)]
            rendered_rows: HashSet::new(),
        }
    }

    pub(crate) fn sync_rows(
        &mut self,
        session: Option<String>,
        lines: Vec<ChatLine>,
        cx: &mut Context<'_, Self>,
    ) {
        let session = session.unwrap_or_default();
        let changed_session = self.session != session;
        if changed_session {
            self.states.insert(
                self.session.clone(),
                StoredFeed {
                    lines: std::mem::take(&mut self.lines),
                    row_ids: std::mem::take(&mut self.row_ids),
                    list: self.list.clone(),
                },
            );
            let stored =
                self.states.remove(&session).unwrap_or_else(|| StoredFeed {
                    lines: Vec::new(),
                    row_ids: Vec::new(),
                    list: ListState::new(0, ListAlignment::Bottom, px(200.))
                        .with_uniform_item_height(px(36.)),
                });
            self.lines = stored.lines;
            self.row_ids = stored.row_ids;
            self.list = stored.list;
            self.session = session;
            // Saved sessions may have been measured with another theme/locale.
            self.list.remeasure_items(0..self.lines.len());
        }
        if self.lines == lines {
            if changed_session {
                cx.notify();
            }
            return;
        }
        let old_len = self.lines.len();
        let new_len = lines.len();
        let prefix = self
            .lines
            .iter()
            .zip(&lines)
            .take_while(|(old, new)| old == new)
            .count();
        let suffix = self.lines[prefix..]
            .iter()
            .rev()
            .zip(lines[prefix..].iter().rev())
            .take_while(|(old, new)| old == new)
            .count();
        let old_end = old_len - suffix;
        let new_end = new_len - suffix;
        let old_top = self.list.logical_scroll_top();
        let top_id = self.row_ids.get(old_top.item_ix).copied();
        let mut ids = self.row_ids[..prefix].to_vec();
        for (offset, row) in lines[prefix..new_end].iter().enumerate() {
            let previous = prefix + offset;
            if previous < old_end && same_row_kind(&self.lines[previous], row) {
                ids.push(self.row_ids[previous]);
            } else {
                let id = self.next_row_id;
                ids.push(id);
                self.row_focus.insert(id, cx.focus_handle().tab_stop(false));
                self.next_row_id += 1;
            }
        }
        ids.extend_from_slice(&self.row_ids[old_end..]);
        // Splicing a replacement would reset an offset inside a streaming row.
        // Remeasure only when the number of rows is unchanged.
        if self.row_ids[prefix..old_end] != ids[prefix..new_end] {
            self.list.splice_focusable(
                prefix..old_end,
                ids[prefix..new_end]
                    .iter()
                    .map(|id| Some(self.row_focus[id].clone())),
            );
        }
        if let Some(top) = top_id
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
        {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: top,
                offset_in_item: old_top.offset_in_item,
            });
        }
        self.list.remeasure_items(prefix..new_end);
        self.row_ids = ids;
        self.lines = lines;
        cx.notify();
    }

    pub(crate) fn sync_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
        self.list.remeasure_items(0..self.lines.len());
        cx.notify();
    }

    fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }
    fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
        self.i18n.tf(key, vars)
    }
    fn format_duration_secs(&self, secs: u64) -> String {
        self.tf("chat.duration_secs", &[("n", &secs.max(1).to_string())])
    }
    fn chat_line(
        &self,
        index: usize,
        line: &ChatLine,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        match line {
            ChatLine::User { text } => div()
                .id(("chat-user", self.row_ids[index]))
                .w_full()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(540.))
                        .px(self.theme.space(12.))
                        .py(self.theme.space(8.))
                        .rounded_xl()
                        .bg(self.theme.soft())
                        .text_sm()
                        .whitespace_normal()
                        .child(text.clone()),
                )
                .into_any_element(),
            ChatLine::Assistant { text } => div()
                .id(("chat-assistant", self.row_ids[index]))
                .w_full()
                .py(self.theme.space(4.))
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
                .id(("chat-tool-legacy", self.row_ids[index]))
                .ml(px(6.))
                .pl(self.theme.space(12.))
                .border_l_1()
                .border_color(self.theme.line())
                .text_xs()
                .text_color(self.theme.muted())
                .child(text.clone())
                .into_any_element(),
            ChatLine::ToolResult { text, is_error } => div()
                .id(("chat-tool-result", self.row_ids[index]))
                .when(*is_error, |row| {
                    row.role(gpui::Role::Alert)
                        .aria_live(gpui::accesskit::Live::Polite)
                        .aria_live_atomic(true)
                })
                .ml(px(6.))
                .pl(self.theme.space(12.))
                .border_l(px(3.))
                .border_color(if *is_error {
                    self.theme.danger()
                } else {
                    self.theme.line()
                })
                .text_xs()
                .text_color(if *is_error {
                    self.theme.danger()
                } else {
                    self.theme.muted()
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
                    .id(("chat-status", self.row_ids[index]))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(theme::icons::check())
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
        let key = format!("{}:reasoning:{}", self.session, self.row_ids[index]);
        let expanded = live || self.expanded_feed.contains(&key);
        let title = if live {
            self.t("chat.thinking")
        } else {
            let duration = self.format_duration_secs(duration_secs);
            self.tf("chat.thought_for", &[("duration", &duration)])
        };
        let key_for_click = key;
        let row_id = self.row_ids[index];
        let mut block = div()
            .id(("chat-reasoning", self.row_ids[index]))
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(4.))
            .child(
                theme::interactive(
                    ("chat-reasoning-toggle", self.row_ids[index]),
                    title.clone(),
                    &self.theme,
                )
                .aria_expanded(expanded)
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(4.))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .rounded_md()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(div().text_xs().text_color(self.theme.muted()).child(
                    if expanded {
                        theme::icons::chevron_down()
                    } else {
                        theme::icons::chevron_right()
                    },
                ))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.muted())
                        .child(title),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        if panel.expanded_feed.contains(&key_for_click) {
                            panel.expanded_feed.remove(&key_for_click);
                        } else {
                            panel.expanded_feed.insert(key_for_click.clone());
                        }
                        if let Some(index) =
                            panel.row_ids.iter().position(|id| *id == row_id)
                        {
                            panel.list.remeasure_items(index..index + 1);
                        }
                        cx.notify();
                    },
                )),
            );
        if expanded && !text.trim().is_empty() {
            block = block.child(
                div()
                    .ml(px(6.))
                    .pl(self.theme.space(12.))
                    .border_l_1()
                    .border_color(self.theme.line())
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(4.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.muted())
                            .child(self.t("chat.thinking")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
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
        let key = format!("{}:tool:{}", self.session, self.row_ids[index]);
        let expanded = running || self.expanded_feed.contains(&key);
        let rail = if running {
            self.theme.accent()
        } else {
            self.theme.muted()
        };
        let key_for_click = key;
        let row_id = self.row_ids[index];
        let label = if target.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {target}")
        };
        let mut row = div()
            .id(("chat-tool", self.row_ids[index]))
            .w_full()
            .ml(px(6.))
            .border_l(px(3.))
            .border_color(rail)
            .bg(self.theme.soft())
            .flex()
            .flex_col()
            .child(
                theme::interactive(
                    ("chat-tool-toggle", self.row_ids[index]),
                    label.clone(),
                    &self.theme,
                )
                .aria_expanded(expanded)
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(12.))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.card()))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(self.theme.ink())
                        .child(SharedString::from(name.to_owned())),
                )
                .when(!target.is_empty(), |el| {
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
                .child(div().text_xs().text_color(self.theme.muted()).child(
                    if expanded {
                        theme::icons::chevron_down()
                    } else {
                        theme::icons::chevron_right()
                    },
                ))
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        if panel.expanded_feed.contains(&key_for_click) {
                            panel.expanded_feed.remove(&key_for_click);
                        } else {
                            panel.expanded_feed.insert(key_for_click.clone());
                        }
                        if let Some(index) =
                            panel.row_ids.iter().position(|id| *id == row_id)
                        {
                            panel.list.remeasure_items(index..index + 1);
                        }
                        cx.notify();
                    },
                )),
            );
        if expanded && !target.is_empty() {
            row = row.child(
                div()
                    .px(self.theme.space(12.))
                    .pb(self.theme.space(8.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(label),
            );
        }
        row
    }
}

impl Render for ChatFeed {
    fn render(
        &mut self,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let weak = cx.weak_entity();
        div()
            .id(SharedString::from(format!("chat-feed-{}", self.session)))
            .flex_1()
            .min_h_0()
            .size_full()
            .child(
                list(self.list.clone(), move |index, _, cx| {
                    weak.update(cx, |feed, cx| {
                        #[cfg(test)]
                        {
                            feed.rendered += 1;
                            feed.rendered_rows.insert(feed.row_ids[index]);
                        }
                        div()
                            .id(("chat-row", feed.row_ids[index]))
                            .track_focus(&feed.row_focus[&feed.row_ids[index]])
                            .px(feed.theme.space(32.))
                            .py(feed.theme.space(4.))
                            .child(feed.chat_line(
                                index,
                                &feed.lines[index],
                                cx,
                            ))
                            .into_any_element()
                    })
                    .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full(),
            )
    }
}

pub struct WorkingIndicator {
    label: String,
    visible: bool,
    error: bool,
    theme: ResolvedTheme,
    _theme_sub: Subscription,
}

impl WorkingIndicator {
    pub(crate) fn new(ui: UiContext, cx: &mut Context<'_, Self>) -> Self {
        let theme = ui.theme.read(cx).resolved();
        let theme_sub = cx.observe(&ui.theme, |indicator, entity, cx| {
            indicator.theme = entity.read(cx).resolved();
            cx.notify();
        });
        Self {
            label: String::new(),
            visible: false,
            error: false,
            theme,
            _theme_sub: theme_sub,
        }
    }

    pub(crate) fn set_label(
        &mut self,
        label: String,
        visible: bool,
        error: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if self.label != label || self.visible != visible || self.error != error
        {
            self.label = label;
            self.visible = visible;
            self.error = error;
            cx.notify();
        }
    }
}

impl Render for WorkingIndicator {
    fn render(
        &mut self,
        _: &mut Window,
        _: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("chat-working")
            .role(gpui::Role::Status)
            .aria_live(if self.error {
                gpui::accesskit::Live::Polite
            } else {
                gpui::accesskit::Live::Off
            })
            .aria_live_atomic(true)
            .when(self.visible, |element| {
                element
                    .px(self.theme.space(32.))
                    .py(self.theme.space(4.))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .text_xs()
                    .text_color(if self.error {
                        self.theme.danger()
                    } else {
                        self.theme.muted()
                    })
                    .child(if self.error {
                        theme::icons::circle_alert()
                    } else {
                        theme::icons::loader_circle()
                    })
                    .child(self.label.clone())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{ListOffset, TestAppContext};

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test harness passes &mut TestAppContext"
    )]
    fn working_indicator_keeps_and_clears_the_error_state(cx: &mut TestAppContext) {
        let indicator = cx.update(|cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            cx.new(|cx| WorkingIndicator::new(ui, cx))
        });
        cx.update(|cx| {
            indicator.update(cx, |working, cx| {
                working.set_label("boom".into(), true, true, cx);
            });
        });
        cx.read_entity(&indicator, |working, _| {
            assert!(working.visible);
            assert!(working.error);
            assert_eq!(working.label, "boom");
        });
        cx.update(|cx| {
            indicator.update(cx, |working, cx| {
                working.set_label("working".into(), true, false, cx);
            });
        });
        cx.read_entity(&indicator, |working, _| {
            assert!(working.visible);
            assert!(!working.error);
            assert_eq!(working.label, "working");
        });
    }

    #[gpui::test]
    fn thousand_rows_are_lazy_and_session_scroll_is_restored(
        cx: &mut TestAppContext,
    ) {
        let (feed, cx) = cx.add_window_view(|_, cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            let rows = (0..1_000)
                .map(|i| ChatLine::Assistant {
                    text: format!("Message {i}"),
                })
                .collect();
            ChatFeed::new(
                ui,
                I18n::embedded("Flowy"),
                Some("first".into()),
                rows,
                cx,
            )
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.read_entity(&feed, |feed, _| {
            assert!(
                feed.rendered > 0 && feed.rendered < 100,
                "rendered {} rows",
                feed.rendered
            );
        });
        cx.update(|_, cx| {
            feed.update(cx, |feed, cx| {
                feed.list.scroll_to(ListOffset {
                    item_ix: 450,
                    offset_in_item: px(3.),
                });
                let first = feed.lines.clone();
                feed.sync_rows(
                    Some("second".into()),
                    vec![ChatLine::User {
                        text: "other".into(),
                    }],
                    cx,
                );
                feed.sync_rows(Some("first".into()), first, cx);
                assert_eq!(feed.list.logical_scroll_top().item_ix, 450);
                assert_eq!(
                    feed.list.logical_scroll_top().offset_in_item,
                    px(3.)
                );
            });
        });
        let focus = cx.read_entity(&feed, |feed, _| {
            feed.row_focus[&feed.row_ids[0]].clone()
        });
        cx.update(|window, cx| {
            focus.focus(window, cx);
            feed.update(cx, |feed, _| {
                feed.rendered_rows.clear();
            });
            window.draw(cx).clear(cx);
        });
        cx.read_entity(&feed, |feed, _| {
            assert!(
                feed.rendered_rows.contains(&feed.row_ids[0]),
                "a focused row stays rendered outside the viewport"
            );
            assert!(feed.rendered_rows.len() < 100);
        });
    }

    #[gpui::test]
    fn streaming_multiline_prepend_and_presentation_changes_keep_row_identity(
        cx: &mut TestAppContext,
    ) {
        let (feed, cx) = cx.add_window_view(|_, cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            ChatFeed::new(
                ui,
                I18n::embedded("Flowy"),
                Some("first".into()),
                vec![
                    ChatLine::User {
                        text: "Question".into(),
                    },
                    ChatLine::Assistant {
                        text: "Answer".into(),
                    },
                    ChatLine::ToolCall {
                        name: "read".into(),
                        target: "file.rs".into(),
                        call_id: "call-1".into(),
                        running: true,
                    },
                ]
                .into_iter()
                .chain((0..100).map(|i| ChatLine::Assistant {
                    text: format!("History row {i}"),
                }))
                .collect(),
                cx,
            )
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        // Use a scrollable history and explicitly reveal the streamed row;
        // a bottom-aligned list shorter than its viewport has no logical top row.
        cx.update(|_, cx| {
            feed.update(cx, |feed, cx| {
                feed.list.scroll_to(ListOffset {
                    item_ix: 1,
                    offset_in_item: px(0.),
                });
                cx.notify();
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let original_ids =
            cx.read_entity(&feed, |feed, _| feed.row_ids.clone());
        cx.update(|_, cx| {
            feed.update(cx, |feed, cx| {
                feed.list.scroll_to(ListOffset {
                    item_ix: 1,
                    offset_in_item: px(3.),
                });
                let mut rows = feed.lines.clone();
                rows[1] = ChatLine::Assistant {
                    text: format!(
                        "First line\nSecond line\n{}",
                        "streamed words ".repeat(220)
                    ),
                };
                rows[2] = ChatLine::ToolCall {
                    name: "read".into(),
                    target: "file.rs".into(),
                    call_id: "call-1".into(),
                    running: false,
                };
                feed.sync_rows(Some("first".into()), rows, cx);
                assert_eq!(feed.row_ids, original_ids);
                assert_eq!(feed.list.logical_scroll_top().item_ix, 1);
                assert_eq!(
                    feed.list.logical_scroll_top().offset_in_item,
                    px(3.)
                );
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.read_entity(&feed, |feed, _| {
            // GPUI's TestTextSystem deliberately reports one visual line for
            // all text. Native wrapping height is covered by the platform
            // visual matrix; here we verify the row remains measured and its
            // identity/scroll anchor survive streaming updates.
            assert!(feed.list.bounds_for_item(1).is_some());
        });
        cx.update(|_, cx| {
            feed.update(cx, |feed, cx| {
                feed.list.scroll_to(ListOffset {
                    item_ix: 1,
                    offset_in_item: px(3.),
                });
                let mut rows = feed.lines.clone();
                rows.insert(
                    0,
                    ChatLine::Status {
                        text: "Earlier history".into(),
                    },
                );
                feed.sync_rows(Some("first".into()), rows, cx);
                assert_eq!(&feed.row_ids[1..], original_ids.as_slice());
                assert_eq!(feed.list.logical_scroll_top().item_ix, 2);
                assert_eq!(
                    feed.list.logical_scroll_top().offset_in_item,
                    px(3.)
                );
                let mut i18n = I18n::embedded("Flowy");
                i18n.set_locale(flowy_i18n::Locale::Ar);
                feed.sync_i18n(i18n, cx);
                feed.theme = ResolvedTheme::resolve(
                    theme::ThemeOptions {
                        mode: theme::ThemeMode::Dark,
                        ..theme::ThemeOptions::default()
                    },
                    false,
                );
                feed.list.remeasure_items(0..feed.lines.len());
                assert_eq!(&feed.row_ids[1..], original_ids.as_slice());
                assert_eq!(feed.list.logical_scroll_top().item_ix, 2);
                assert_eq!(
                    feed.list.logical_scroll_top().offset_in_item,
                    px(3.)
                );
            });
        });
    }
}
