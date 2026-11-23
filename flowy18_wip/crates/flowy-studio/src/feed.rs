//! Independently rendered, variable-height message feed and elapsed-time indicator.
use flowy_agent::{ChatHistory, ChatLine, TurnStatus};
use flowy_i18n::I18n;
use flowy_ui::{self as theme, ResolvedTheme, UiContext};
use gpui::{
    Context, FontWeight, ListAlignment, ListState, Render, SharedString,
    Subscription, Window, div, list, prelude::*, px,
};
use std::collections::{HashMap, HashSet};

use crate::library::{ArtifactAvailability, LibraryItem};
use crate::{history::Projection, markdown};

pub struct ChatFeed {
    lines: Vec<ChatLine>,
    session: String,
    states: HashMap<String, StoredFeed>,
    row_ids: Vec<u64>,
    next_row_id: u64,
    row_focus: HashMap<u64, gpui::FocusHandle>,
    artifacts: HashMap<String, LibraryItem>,
    pending_artifact_focus: Option<String>,
    list: ListState,
    expanded_rows: HashSet<u64>,
    history: ChatHistory,
    projection: Projection,
    expanded_turns: HashMap<String, HashSet<usize>>,
    theme: ResolvedTheme,
    i18n: I18n,
    _theme_sub: Subscription,
    #[cfg(test)]
    rendered: usize,
    #[cfg(test)]
    rendered_rows: HashSet<u64>,
}

pub struct OpenTask(pub String);
#[derive(Clone, Debug)]
pub enum ArtifactAction {
    Preview(String),
    SaveAs(String),
    LocateSource {
        session_id: String,
        artifact_id: String,
    },
}

impl gpui::EventEmitter<OpenTask> for ChatFeed {}
impl gpui::EventEmitter<ArtifactAction> for ChatFeed {}

/// Ask the panel to reveal a file's folder. The feed only emits: opening can
/// fail, and the failure needs the panel's notice channel.
pub struct RevealFile(pub String);

impl gpui::EventEmitter<RevealFile> for ChatFeed {}

/// Ask the panel to open the deliverables drawer for one turn's files.
pub struct OpenDeliverables(pub Vec<flowy_agent::FileEntry>);

impl gpui::EventEmitter<OpenDeliverables> for ChatFeed {}

/// Keeps a long path readable in a narrow card: the tail (folder and file
/// name) matters more than the drive or repository root.
fn display_path(path: &str) -> String {
    const MAX: usize = 64;
    if path.chars().count() <= MAX {
        return path.to_owned();
    }
    let tail: String = path
        .chars()
        .skip(path.chars().count() - (MAX - 1))
        .collect();
    format!("\u{2026}{tail}")
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
        )
        | (
            ChatLine::Artifact { artifact_id: old },
            ChatLine::Artifact { artifact_id: new },
        ) => old == new,
        _ => std::mem::discriminant(old) == std::mem::discriminant(new),
    }
}

impl ChatFeed {
    pub(crate) fn new(
        ui: UiContext,
        i18n: I18n,
        session: Option<String>,
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
        let list = ListState::new(0, ListAlignment::Bottom, px(200.))
            .with_uniform_item_height(px(36.));
        Self {
            lines: Vec::new(),
            row_ids: Vec::new(),
            next_row_id: 0,
            row_focus: HashMap::new(),
            artifacts: HashMap::new(),
            pending_artifact_focus: None,
            session: session.unwrap_or_default(),
            states: HashMap::new(),
            list,
            expanded_rows: HashSet::new(),
            history: ChatHistory::default(),
            projection: Projection::default(),
            expanded_turns: HashMap::new(),
            theme,
            i18n,
            _theme_sub: theme_sub,
            #[cfg(test)]
            rendered: 0,
            #[cfg(test)]
            rendered_rows: HashSet::new(),
        }
    }

    pub(crate) fn set_artifacts(
        &mut self,
        artifacts: Vec<LibraryItem>,
        cx: &mut Context<'_, Self>,
    ) {
        self.artifacts = artifacts
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        self.list.remeasure_items(0..self.lines.len());
        cx.notify();
    }

    pub(crate) fn focus_artifact(
        &mut self,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(source) = self.history.rows.iter().position(|line| {
            matches!(line, ChatLine::Artifact { artifact_id: id } if id == artifact_id)
        }) else { return false; };
        let Some(row) = self
            .projection
            .source_rows
            .iter()
            .position(|&row| row == source)
        else {
            return false;
        };
        self.list.scroll_to(gpui::ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        self.pending_artifact_focus = Some(artifact_id.to_owned());
        cx.notify();
        true
    }

    pub(crate) fn artifact_focus(
        &self,
        artifact_id: &str,
    ) -> Option<gpui::FocusHandle> {
        let row = self.lines.iter().position(|line| {
            matches!(line, ChatLine::Artifact { artifact_id: id } if id == artifact_id)
        })?;
        self.row_focus.get(self.row_ids.get(row)?).cloned()
    }

    pub(crate) fn sync_history(
        &mut self,
        session: Option<String>,
        history: ChatHistory,
        cx: &mut Context<'_, Self>,
    ) {
        let same_session =
            session.as_deref().unwrap_or_default() == self.session;
        let top = self.list.logical_scroll_top();
        let anchor = same_session
            .then(|| self.projection.source_rows.get(top.item_ix).copied())
            .flatten();
        let anchor = anchor.filter(|&source| {
            history.rows.get(source) == self.history.rows.get(source)
        });
        self.history = history;
        self.apply_projection(session, anchor, cx);
    }

    fn apply_projection(
        &mut self,
        session: Option<String>,
        hidden_anchor: Option<usize>,
        cx: &mut Context<'_, Self>,
    ) {
        let name = session.as_deref().unwrap_or_default();
        let projection = Projection::build(
            &self.history.rows,
            self.history.streaming,
            self.history.status == TurnStatus::Completed,
            self.expanded_turns.get(name),
        );
        let changed = projection != self.projection;
        let remeasure: Vec<_> = self
            .projection
            .streaming
            .into_iter()
            .chain(projection.streaming)
            .chain(
                self.projection
                    .folds
                    .keys()
                    .filter(|row| {
                        projection.folds.get(row)
                            != self.projection.folds.get(row)
                    })
                    .copied(),
            )
            .chain(
                projection
                    .folds
                    .keys()
                    .filter(|row| {
                        projection.folds.get(row)
                            != self.projection.folds.get(row)
                    })
                    .copied(),
            )
            .collect();
        let rows = projection
            .source_rows
            .iter()
            .map(|&index| self.history.rows[index].clone())
            .collect();
        self.projection = projection;
        self.sync_rows(session, rows, cx);
        if let Some(&row) =
            hidden_anchor.and_then(|source| self.projection.hidden.get(&source))
        {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
        if changed {
            for row in
                remeasure.into_iter().filter(|&row| row < self.lines.len())
            {
                self.list.remeasure_items(row..row + 1);
            }
            cx.notify();
        }
    }

    fn set_turn_expanded(
        &mut self,
        key: usize,
        show: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let expanded =
            self.expanded_turns.entry(self.session.clone()).or_default();
        let changed = if show {
            expanded.insert(key)
        } else {
            expanded.remove(&key)
        };
        if !changed {
            return;
        }
        let top = self.list.logical_scroll_top();
        let source = self.projection.source_rows.get(top.item_ix).copied();
        self.apply_projection(Some(self.session.clone()), source, cx);
        if let Some(index) = source.and_then(|source| {
            self.projection
                .source_rows
                .iter()
                .position(|&row| row == source)
        }) {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: index,
                offset_in_item: if self
                    .projection
                    .folds
                    .get(&index)
                    .is_some_and(|fold| !fold.expanded)
                {
                    px(0.)
                } else {
                    top.offset_in_item
                },
            });
        }
    }

    fn sync_rows(
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
        let old_ids = &self.row_ids[prefix..old_end];
        let new_ids = &ids[prefix..new_end];
        if old_ids != new_ids {
            self.list.splice_focusable(
                prefix..old_end,
                new_ids.iter().map(|id| Some(self.row_focus[id].clone())),
            );
            for (offset, id) in old_ids.iter().enumerate() {
                if new_ids.get(offset) != Some(id) {
                    self.row_focus.remove(id);
                    self.expanded_rows.remove(id);
                }
            }
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

    fn toggle_detail(&mut self, id: u64, cx: &mut Context<'_, Self>) {
        if let Some(index) = self.row_ids.iter().position(|&row| row == id) {
            if !self.expanded_rows.remove(&id) {
                self.expanded_rows.insert(id);
            }
            self.list.remeasure_items(index..index + 1);
            cx.notify();
        }
    }

    /// The most recent turn that changed files, for the drawer toggle.
    /// Old sessions only carry the single-file row, so both shapes count.
    pub(crate) fn latest_deliverables(
        &self,
    ) -> Option<Vec<flowy_agent::FileEntry>> {
        self.lines.iter().rev().find_map(|line| match line {
            // An empty summary would open a drawer with nothing in it.
            ChatLine::FileChanges { files } if !files.is_empty() => {
                Some(files.clone())
            }
            ChatLine::FileChange {
                path,
                change,
                added,
                removed,
                binary,
            } => Some(vec![flowy_agent::FileEntry {
                path: path.clone(),
                change: *change,
                added: *added,
                removed: *removed,
                binary: *binary,
            }]),
            _ => None,
        })
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
    fn history_line(
        &self,
        index: usize,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let line = &self.lines[index];
        let Some(fold) = self.projection.folds.get(&index).copied() else {
            return self.chat_line(index, line, cx).into_any_element();
        };
        let key = if fold.expanded {
            "history.hide_work"
        } else {
            "history.show_work"
        };
        let title = self.t(key);
        let show = !fold.expanded;
        // A plain button keeps the standard invoke path (pointer, keyboard and
        // UIA click all land on the same handler); an expanded a11y state would
        // turn the node into a menu button whose invoke is never delivered.
        let button =
            theme::Button::new(("turn-fold", self.row_ids[index]), title)
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(move |feed, _, _, cx| {
                        feed.set_turn_expanded(fold.key, show, cx);
                    }),
                )
                .min_h(px(0.))
                .text_xs()
                .text_color(self.theme.muted())
                .px(self.theme.space(4.))
                .py(self.theme.space(4.));
        div()
            .flex()
            .flex_col()
            .items_start()
            .child(button)
            .when(fold.expanded, |row| {
                row.child(self.chat_line(index, line, cx))
            })
            .into_any_element()
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
                .child(if self.projection.streaming == Some(index) {
                    div().child(text.clone())
                } else {
                    markdown::render(text, self.theme, &self.i18n)
                })
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
        }
    }

    /// One changed file: type icon, file name, full path, and line delta.
    /// Clicking opens the containing folder, matching the project view action.
    fn file_change_row(
        &self,
        index: usize,
        path: &str,
        kind: &flowy_agent::FileChangeKind,
        added: u32,
        removed: u32,
        binary: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let icon = crate::file_kind::from_path(path)
            .map_or_else(theme::icons::code_file, |kind| kind.icon());
        // Line counts only mean something for text; a binary change shows a dash.
        let delta = if binary {
            "\u{2014}".to_owned()
        } else if removed == 0 {
            format!("+{added}")
        } else {
            format!("+{added} \u{2212}{removed}")
        };
        let target = path.to_owned();
        let _ = kind;
        theme::interactive(
            ("chat-file-change", self.row_ids[index]),
            SharedString::from(name.to_owned()),
            &self.theme,
        )
        .w_full()
        .px(self.theme.space(12.))
        .py(self.theme.space(8.))
        .gap(self.theme.space(8.))
        .flex()
        .items_center()
        .rounded_lg()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .cursor_pointer()
        .child(icon.size(px(20.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_sm().child(name.to_owned()))
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(display_path(path)),
                ),
        )
        .child(div().text_xs().child(delta))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.emit(RevealFile(target.clone()));
        }))
    }

    /// One turn's file work, folded into a single summary row.
    fn file_changes_row(
        &self,
        index: usize,
        files: &[flowy_agent::FileEntry],
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let count = files.len();
        let added: u32 = files.iter().map(|file| file.added).sum();
        let removed: u32 = files.iter().map(|file| file.removed).sum();
        // Every file unreadable as text means the totals say nothing.
        let all_binary = files.iter().all(|file| file.binary);
        let label =
            self.tf("chat.file_changes", &[("count", &count.to_string())]);
        let delta = if all_binary {
            "\u{2014}".to_owned()
        } else if removed == 0 {
            format!("+{added}")
        } else {
            format!("+{added} \u{2212}{removed}")
        };
        let all = files.to_vec();
        theme::interactive(
            ("chat-file-changes", self.row_ids[index]),
            SharedString::from(label.clone()),
            &self.theme,
        )
        .w_full()
        .px(self.theme.space(12.))
        .py(self.theme.space(8.))
        .gap(self.theme.space(8.))
        .flex()
        .items_center()
        .rounded_lg()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .cursor_pointer()
        .child(theme::icons::code_file().size(px(20.)))
        .child(div().flex_1().min_w_0().text_sm().child(label))
        .child(div().text_xs().child(delta))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.emit(OpenDeliverables(all.clone()));
        }))
    }

    fn artifact_card(
        &self,
        index: usize,
        artifact_id: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = artifact_id.to_owned();
        let save_id = id.clone();
        let item = self.artifacts.get(artifact_id);
        let icon = item
            .and_then(crate::library::artifact_file_icon)
            .map_or_else(
                || theme::icons::file_fold().size(px(24.)).into_any_element(),
                |icon| icon.size(px(24.)).into_any_element(),
            );
        let title = item
            .map_or_else(|| self.t("chat.artifact"), |item| item.title.clone());
        let unavailable = item.is_none_or(|item| {
            item.availability != ArtifactAvailability::Available
        });
        let deleted = item.is_some_and(|item| {
            item.availability == ArtifactAvailability::Deleted
        });
        let details = item.map_or_else(
            || self.t("library.loading"),
            |item| {
                let size = item.size_bytes.unwrap_or_default();
                let format = item.extension.to_uppercase();
                let mut details = self.i18n.tf(
                    "library.preview.file_info",
                    &[
                        ("format", &format),
                        ("size", &format!("{:.1}", size as f64 / 1024.)),
                    ],
                );
                if item.availability != ArtifactAvailability::Available {
                    details.push_str(" · ");
                    details.push_str(&self.t(match item.availability {
                        ArtifactAvailability::Missing => {
                            "library.availability.missing"
                        }
                        ArtifactAvailability::Corrupt => {
                            "library.availability.corrupt"
                        }
                        ArtifactAvailability::Deleted => {
                            "library.error.deleted"
                        }
                        ArtifactAvailability::Available => {
                            "library.availability.available"
                        }
                    }));
                }
                details
            },
        );
        let source = item
            .filter(|item| {
                !item.source_deleted
                    && !item.source_unavailable
                    && !item.source_session_id.is_empty()
            })
            .map(|item| item.source_session_id.clone());
        div()
            .id(("artifact-card", self.row_ids[index]))
            .w_full()
            .max_w(px(540.))
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .px(self.theme.space(12.))
            .py(self.theme.space(10.))
            .child(
                theme::interactive(
                    ("artifact-preview", self.row_ids[index]),
                    title.clone(),
                    &self.theme,
                )
                .track_focus(&self.row_focus[&self.row_ids[index]])
                .aria_disabled(deleted)
                .tab_stop(!deleted)
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(self.theme.space(10.))
                .child(icon)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(self.theme.space(3.))
                        .child(
                            theme::Title::new(theme::TitleLevel::Item, title)
                                .truncate()
                                .render(&self.theme),
                        )
                        .child(
                            theme::Text::new(theme::TextRole::Meta, details)
                                .truncate()
                                .render(&self.theme),
                        ),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    if !deleted {
                        cx.emit(ArtifactAction::Preview(id.clone()));
                    }
                })),
            )
            .child(
                theme::IconButton::new(
                    ("artifact-save-as", self.row_ids[index]),
                    self.t("library.save_as"),
                    theme::icons::download(),
                )
                .disabled(unavailable)
                .render(
                    &self.theme,
                    cx.listener(move |_, _, _, cx| {
                        cx.emit(ArtifactAction::SaveAs(save_id.clone()));
                    }),
                ),
            )
            .when_some(source, |card, session_id| {
                let artifact_id = artifact_id.to_owned();
                card.child(
                    theme::IconButton::new(
                        ("artifact-locate-source", self.row_ids[index]),
                        self.t("library.locate_task"),
                        theme::icons::external_link(),
                    )
                    .render(
                        &self.theme,
                        cx.listener(move |_, _, _, cx| {
                            cx.emit(ArtifactAction::LocateSource {
                                session_id: session_id.clone(),
                                artifact_id: artifact_id.clone(),
                            });
                        }),
                    ),
                )
            })
    }
    fn reasoning_block(
        &self,
        index: usize,
        text: &str,
        duration_secs: u64,
        live: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let row_id = self.row_ids[index];
        let expanded = live || self.expanded_rows.contains(&row_id);
        let title = if live {
            self.t("chat.thinking")
        } else {
            let duration = self.format_duration_secs(duration_secs);
            self.tf("chat.thought_for", &[("duration", &duration)])
        };
        let mut block = div()
            .id(("chat-reasoning", row_id))
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(4.))
            .child(
                theme::interactive(
                    ("chat-reasoning-toggle", row_id),
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
                    move |feed, _, _, cx| {
                        feed.toggle_detail(row_id, cx);
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
        let row_id = self.row_ids[index];
        let expanded = running || self.expanded_rows.contains(&row_id);
        let rail = if running {
            self.theme.accent()
        } else {
            self.theme.muted()
        };
        let label = if target.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {target}")
        };
        let mut row = div()
            .id(("chat-tool", row_id))
            .w_full()
            .ml(px(6.))
            .border_l(px(3.))
            .border_color(rail)
            .bg(self.theme.soft())
            .flex()
            .flex_col()
            .child(
                theme::interactive(
                    ("chat-tool-toggle", row_id),
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
                    move |feed, _, _, cx| {
                        feed.toggle_detail(row_id, cx);
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        if let Some(artifact_id) = self.pending_artifact_focus.take()
            && let Some(row) = self.lines.iter().position(|line| {
                matches!(line, ChatLine::Artifact { artifact_id: id } if id == &artifact_id)
            })
            && let Some(focus) = self.row_focus.get(&self.row_ids[row])
        {
            focus.focus(window, cx);
        }
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
                            .when(
                                !matches!(
                                    feed.lines[index],
                                    ChatLine::Artifact { .. }
                                ),
                                |row| {
                                    row.track_focus(
                                        &feed.row_focus[&feed.row_ids[index]],
                                    )
                                },
                            )
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(760.))
                                    .min_w_0()
                                    .px(feed.theme.space(32.))
                                    .py(feed.theme.space(4.))
                                    .child(feed.history_line(index, cx)),
                            )
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

    #[cfg(test)]
    pub(crate) const fn is_visible(&self) -> bool {
        self.visible
    }

    #[cfg(test)]
    pub(crate) const fn is_error(&self) -> bool {
        self.error
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> &str {
        &self.label
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
    fn durable_artifact_anchor_resolves_the_projected_row_in_long_history(
        cx: &mut TestAppContext,
    ) {
        let (feed, cx) = cx.add_window_view(|_, cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            ChatFeed::new(
                ui,
                I18n::embedded("Flowy"),
                Some("source".into()),
                cx,
            )
        });
        let mut rows = Vec::new();
        for index in 0..500 {
            rows.push(ChatLine::User {
                text: format!("request {index}"),
            });
            rows.push(ChatLine::Assistant {
                text: format!("answer {index}"),
            });
        }
        rows.extend([
            ChatLine::User {
                text: "make report".into(),
            },
            ChatLine::ToolResult {
                text: "write".into(),
                is_error: false,
            },
            ChatLine::ToolResult {
                text: "publish".into(),
                is_error: false,
            },
            ChatLine::Assistant {
                text: "Report ready".into(),
            },
            ChatLine::Artifact {
                artifact_id: "report-id".into(),
            },
        ]);
        feed.update(cx, |feed, cx| {
            feed.sync_history(Some("source".into()), ChatHistory { rows, status: TurnStatus::Completed, streaming: None }, cx);
            assert!(feed.focus_artifact("report-id", cx));
            let projected = feed.list.logical_scroll_top().item_ix;
            assert!(matches!(&feed.lines[projected], ChatLine::Artifact { artifact_id } if artifact_id == "report-id"));
            assert_eq!(feed.projection.source_rows[projected], 1004);
            assert!(feed.artifact_focus("report-id").is_some());
            assert!(!feed.focus_artifact("absent-id", cx));
        });
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let _subscription = feed.update(cx, |_, cx| {
            cx.subscribe(&feed, move |_, _, event: &ArtifactAction, _| {
                captured.lock().unwrap().push(event.clone());
            })
        });
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.set_a11y_forced(true);
                window.draw(cx).clear(cx);
            });
        }
        let focus = cx.read_entity(&feed, |feed, _| {
            feed.artifact_focus("report-id").unwrap()
        });
        let label = cx.read_entity(&feed, |feed, _| feed.t("chat.artifact"));
        cx.update(|window, cx| {
            assert_eq!(window.focused(cx), Some(focus));
            let node_id = window
                .a11y_tree()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == gpui::accesskit::Role::Button
                        && node.label() == Some(label.as_str())
                })
                .map(|(id, node)| {
                    assert!(
                        node.supports_action(gpui::accesskit::Action::Click)
                    );
                    *id
                })
                .expect(
                    "focused artifact is exposed as a named actionable button",
                );
            assert!(window.a11y_node_bounds(node_id).is_some());
            window.perform_a11y_action(
                node_id,
                gpui::accesskit::Action::Click,
                None,
                cx,
            );
        });
        cx.run_until_parked();
        assert!(
            matches!(events.lock().unwrap().as_slice(), [ArtifactAction::Preview(id)] if id == "report-id")
        );
    }

    #[gpui::test]
    fn folding_a_read_detail_returns_to_its_turn_control(
        cx: &mut TestAppContext,
    ) {
        let (feed, cx) = cx.add_window_view(|_, cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            ChatFeed::new(ui, I18n::embedded("Flowy"), Some("fold".into()), cx)
        });
        let mut rows = vec![
            ChatLine::User {
                text: "request".into(),
            },
            ChatLine::Tool {
                text: "first detail".into(),
            },
            ChatLine::Tool {
                text: "second detail".into(),
            },
            ChatLine::Assistant {
                text: "**answer**".into(),
            },
        ];
        rows.extend((0..100).map(|id| ChatLine::User {
            text: format!("next {id}"),
        }));
        feed.update(cx, |feed, cx| {
            feed.sync_history(
                Some("fold".into()),
                ChatHistory {
                    rows,
                    streaming: None,
                    status: TurnStatus::Completed,
                },
                cx,
            );
            assert!(!feed.projection.folds[&1].expanded);
            feed.list.scroll_to(ListOffset {
                item_ix: 1,
                offset_in_item: px(0.),
            });
        });
        cx.update(|window, cx| {
            window.set_a11y_forced(true);
            window.draw(cx).clear(cx);
            let id = window
                .a11y_tree()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("展开执行过程"))
                .map(|(id, _)| *id)
                .unwrap();
            window.perform_a11y_action(
                id,
                gpui::accesskit::Action::Click,
                None,
                cx,
            );
        });
        feed.update(cx, |feed, cx| {
            assert!(feed.projection.folds[&1].expanded);
            let detail_id = feed.row_ids[2];
            feed.toggle_detail(detail_id, cx);
            assert!(feed.expanded_rows.contains(&detail_id));
            feed.list.scroll_to(ListOffset {
                item_ix: 2,
                offset_in_item: px(7.),
            });
            feed.set_turn_expanded(1, false, cx);
            let top = feed.list.logical_scroll_top();
            assert_eq!(top.item_ix, 1);
            assert_eq!(top.offset_in_item, px(0.));
            assert!(!feed.row_focus.contains_key(&detail_id));
            assert!(!feed.expanded_rows.contains(&detail_id));
        });
    }

    #[gpui::test]
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI test harness passes &mut TestAppContext"
    )]
    fn working_indicator_keeps_and_clears_the_error_state(
        cx: &mut TestAppContext,
    ) {
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
            let mut feed = ChatFeed::new(
                ui,
                I18n::embedded("Flowy"),
                Some("first".into()),
                cx,
            );
            feed.sync_rows(Some("first".into()), rows, cx);
            feed
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
            let mut feed = ChatFeed::new(
                ui,
                I18n::embedded("Flowy"),
                Some("first".into()),
                cx,
            );
            feed.sync_rows(
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
            );
            feed
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
