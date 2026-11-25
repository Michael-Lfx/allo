//! Independently rendered, variable-height message feed and elapsed-time indicator.
use flowy_agent::{ChatHistory, ChatLine, TurnStatus};
use flowy_i18n::I18n;
use flowy_ui::{self as theme, ResolvedTheme, UiContext};
use gpui::{
    Animation, AnimationExt, Context, FontWeight, ListAlignment, ListState,
    Render, SharedString, Subscription, Window, div, list, prelude::*, px,
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
    /// Verification cards whose explicit override checkbox has been ticked by
    /// the user; always starts empty so override is never pre-checked (§8.3).
    override_checked: HashSet<String>,
    list: ListState,
    expanded_rows: HashSet<u64>,
    measured_heights: HashMap<u64, gpui::Entity<f32>>,
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
    OverrideDeliver {
        report_id: String,
        artifact_path: String,
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

/// Collects deliverable files from the most recent turn that produced any,
/// merging file-change summaries with verified/overridden verification
/// receipts while strictly excluding unsettled failed verifications (§8.3).
fn collect_latest_deliverables(
    lines: &[ChatLine],
) -> Option<Vec<flowy_agent::FileEntry>> {
    let mut turn_starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(ix, line)| {
            matches!(line, ChatLine::User { .. }).then_some(ix)
        })
        .collect();
    if turn_starts.first().copied() != Some(0) {
        turn_starts.insert(0, 0);
    }
    turn_starts.push(lines.len());

    for bounds in turn_starts.windows(2).rev() {
        let mut entries: Vec<flowy_agent::FileEntry> = Vec::new();
        let mut seen = HashSet::new();
        for line in &lines[bounds[0]..bounds[1]] {
            match line {
                ChatLine::FileChanges { files } => {
                    for file in files {
                        if seen.insert(file.path.clone()) {
                            entries.push(file.clone());
                        }
                    }
                }
                ChatLine::FileChange {
                    path,
                    change,
                    added,
                    removed,
                    binary,
                } => {
                    if seen.insert(path.clone()) {
                        entries.push(flowy_agent::FileEntry {
                            path: path.clone(),
                            change: *change,
                            added: *added,
                            removed: *removed,
                            binary: *binary,
                        });
                    }
                }
                ChatLine::Verification {
                    artifact_path,
                    ok,
                    override_used,
                    ..
                } if (*ok || *override_used)
                    && !artifact_path.trim().is_empty()
                    && seen.insert(artifact_path.clone()) =>
                {
                    entries.push(flowy_agent::FileEntry {
                        path: artifact_path.clone(),
                        change: flowy_agent::FileChangeKind::Add,
                        added: 0,
                        removed: 0,
                        binary: false,
                    });
                }
                _ => {}
            }
        }
        if !entries.is_empty() {
            return Some(entries);
        }
    }
    None
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
        (
            ChatLine::Verification {
                report_id: old_id,
                artifact_path: old_path,
                ..
            },
            ChatLine::Verification {
                report_id: new_id,
                artifact_path: new_path,
                ..
            },
        ) => old_id == new_id && old_path == new_path,
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
            override_checked: HashSet::new(),
            session: session.unwrap_or_default(),
            states: HashMap::new(),
            list,
            expanded_rows: HashSet::new(),
            measured_heights: HashMap::new(),
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
        let projection = Projection::build_live(
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
            .chain(
                self.projection
                    .fold_meta
                    .keys()
                    .filter(|row| {
                        projection.fold_meta.get(row)
                            != self.projection.fold_meta.get(row)
                    })
                    .copied(),
            )
            .chain(
                projection
                    .fold_meta
                    .keys()
                    .filter(|row| {
                        projection.fold_meta.get(row)
                            != self.projection.fold_meta.get(row)
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
                    self.measured_heights.remove(id);
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
            if self.expanded_rows.remove(&id) {
                // Keep the measured height cached so re-opening immediately
                // animates smoothly from 0 to its exact measured height.
            } else {
                self.expanded_rows.insert(id);
                self.measured_heights
                    .entry(id)
                    .or_insert_with(|| cx.new(|_| 0.0));
            }
            self.list.remeasure_items(index..index + 1);
            cx.notify();
        }
    }

    fn measured_height_for(
        &self,
        id: u64,
        cx: &Context<'_, Self>,
    ) -> (f32, Option<gpui::Entity<f32>>) {
        self.measured_heights
            .get(&id)
            .map_or((0.0, None), |entity| {
                (*entity.read(cx), Some(entity.clone()))
            })
    }

    fn reveal_collapsible(
        &self,
        anim_id: (&'static str, u64),
        row_id: u64,
        content: gpui::Div,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        // shortcut: first expansion plays opacity+slide while PrepaintBounds
        // captures natural height; subsequent expansions animate 0->H height.
        let policy = self.theme.motion_policy();
        let (measured, handle) = self.measured_height_for(row_id, cx);
        if let Some(entity) = handle {
            crate::motion::auto_height_reveal(
                anim_id,
                measured,
                policy,
                crate::motion::measure_height(content, entity),
            )
        } else {
            crate::motion::auto_height_reveal(
                anim_id, measured, policy, content,
            )
        }
    }

    /// The most recent turn's deliverable files for the drawer toggle,
    /// combining file-change summaries with verified/overridden artifacts
    /// while excluding failed unverified verification cards (§8.3).
    pub(crate) fn latest_deliverables(
        &self,
    ) -> Option<Vec<flowy_agent::FileEntry>> {
        collect_latest_deliverables(&self.lines)
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
            let is_nested_process = matches!(
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
                    self.tf("chat.steps_live_summary", &[("n", &count)])
                } else {
                    let base =
                        self.tf("chat.steps_done_summary", &[("n", &count)]);
                    if m.duration_secs > 0 {
                        let dur = self.format_duration_secs(m.duration_secs);
                        format!("{base} \u{00b7} {dur}")
                    } else {
                        base
                    }
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
                .py(self.theme.space(4.))
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(560.))
                        .px(self.theme.space(16.))
                        .py(self.theme.space(10.))
                        .rounded(px(18.))
                        .border_1()
                        .border_color(self.theme.line())
                        .bg(self.theme.card())
                        .shadow_xs()
                        .text_sm()
                        .text_color(self.theme.ink())
                        .whitespace_normal()
                        .child(text.clone()),
                )
                .into_any_element(),
            ChatLine::Assistant { text } => {
                let show_identity = index == 0
                    || !matches!(
                        self.lines[index - 1],
                        ChatLine::Assistant { .. }
                    );
                div()
                    .id(("chat-assistant", self.row_ids[index]))
                    .w_full()
                    .py(self.theme.space(6.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .when(show_identity, |col| {
                        col.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(self.theme.space(8.))
                                .child(
                                    div()
                                        .size(px(20.))
                                        .flex_none()
                                        .rounded_md()
                                        .bg(self.theme.assistant_badge_bg())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            theme::icons::sparkles()
                                                .size(px(11.))
                                                .text_color(
                                                    self.theme
                                                        .assistant_badge_fg(),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(self.theme.ink())
                                        .child(self.t("chat.router_name")),
                                ),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .pl(px(28.))
                            .text_sm()
                            .whitespace_normal()
                            .child(
                                if self.projection.streaming == Some(index) {
                                    div().child(text.clone())
                                } else {
                                    markdown::render(
                                        text, self.theme, &self.i18n,
                                    )
                                },
                            ),
                    )
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
            } => self.verification_card(
                index,
                report_id,
                artifact_path,
                artifact_hash,
                *ok,
                issues,
                semantic_hint.as_deref(),
                *override_used,
                cx,
            ),
        }
    }

    /// One changed file: type icon, file name, full path, and line delta.
    /// Clicking opens the containing folder, matching the project view action.
    #[expect(
        clippy::too_many_arguments,
        reason = "one row reads better than a struct built at each call site"
    )]
    fn file_change_row(
        &self,
        index: usize,
        path: &str,
        kind: &flowy_agent::FileChangeKind,
        _added: u32,
        _removed: u32,
        _binary: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let icon = crate::file_kind::from_path(path).map_or_else(
            || {
                theme::icons::document_file()
                    .size(px(18.))
                    .text_color(self.theme.muted())
                    .into_any_element()
            },
            |file_kind| file_kind.icon().size(px(20.)).into_any_element(),
        );
        let kind_label = self.t(crate::file_kind::kind_label_key(path));
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
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft()))
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(display_path(path)),
                ),
        )
        .child(
            div()
                .flex_none()
                .px(self.theme.space(6.))
                .py(px(2.))
                .rounded_md()
                .bg(self.theme.soft())
                .text_xs()
                .text_color(self.theme.muted())
                .child(kind_label),
        )
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
        let label =
            self.tf("chat.file_changes", &[("count", &count.to_string())]);
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
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft()))
        .child(
            theme::icons::file_fold()
                .size(px(18.))
                .text_color(self.theme.muted()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .px(self.theme.space(6.))
                .py(px(2.))
                .rounded_md()
                .bg(self.theme.soft())
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t("chat.deliverables")),
        )
        .child(
            theme::icons::chevron_right()
                .size(px(12.))
                .text_color(self.theme.muted()),
        )
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
                        ArtifactAvailability::Available => {
                            "library.availability.available"
                        }
                    }));
                }
                if item.override_used {
                    details.push_str(" · ");
                    details.push_str(&self.t("verify.override.badge"));
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
                .debug_selector(|| "chat-artifact-preview".to_owned())
                .track_focus(&self.row_focus[&self.row_ids[index]])
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
                    cx.emit(ArtifactAction::Preview(id.clone()));
                })),
            )
            .child(crate::library::test_control(
                "chat-artifact-save-as",
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
            ))
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

    fn toggle_override_check(
        &mut self,
        key: String,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.override_checked.remove(&key) {
            self.override_checked.insert(key);
        }
        self.list.remeasure_items(0..self.lines.len());
        cx.notify();
    }

    fn format_verification_issues(
        &self,
        issues: &[flowy_agent::VerificationIssue],
    ) -> Vec<String> {
        issues
            .iter()
            .map(|issue| {
                format!(
                    "{} · {}",
                    self.t(flowy_agent::verify_ui_key(&issue.code)),
                    issue.field
                )
            })
            .collect()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "verification card fields map 1:1 from ChatLine::Verification"
    )]
    fn verification_card(
        &self,
        index: usize,
        report_id: &str,
        artifact_path: &str,
        artifact_hash: &str,
        ok: bool,
        issues: &[flowy_agent::VerificationIssue],
        semantic_hint: Option<&str>,
        override_used: bool,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let row_id = self.row_ids[index];
        let name = artifact_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(artifact_path)
            .to_owned();
        let short_hash: String = artifact_hash.chars().take(8).collect();
        let status_label = if ok {
            self.t("verify.card.passed")
        } else {
            self.t("verify.card.failed")
        };

        let policy = self.theme.motion_policy();

        if ok || override_used {
            let expanded = self.expanded_rows.contains(&row_id);
            let compact_label = if ok {
                self.tf("verify.compact.passed", &[("name", &name)])
            } else {
                self.tf("verify.compact.override", &[("name", &name)])
            };
            let deliverables_for_quick_open =
                ok.then(|| self.latest_deliverables()).flatten();
            let status_pill = if ok {
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_full()
                    .bg(self.theme.status_verified_bg())
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        theme::icons::check()
                            .size(px(12.))
                            .text_color(self.theme.status_verified()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.status_verified())
                            .child(self.t("verify.card.passed")),
                    )
                    .into_any_element()
            } else {
                let badge = self.t("verify.override.badge");
                div()
                    .id(("verify-override-badge", row_id))
                    .debug_selector(|| "chat-verify-override-badge".to_owned())
                    .role(gpui::Role::Status)
                    .aria_label(badge.clone())
                    .flex_none()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_full()
                    .bg(self.theme.status_attention_bg())
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        theme::icons::circle_alert()
                            .size(px(12.))
                            .text_color(self.theme.status_attention()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.status_attention())
                            .child(badge),
                    )
                    .into_any_element()
            };
            let mut toggle = theme::interactive(
                ("verify-compact-toggle", row_id),
                compact_label,
                &self.theme,
            )
            .aria_expanded(expanded)
            .w_full()
            .min_w_0()
            .px(self.theme.space(10.))
            .py(self.theme.space(6.))
            .rounded_xl()
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .child(status_pill)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if ok {
                        self.theme.ink()
                    } else {
                        self.theme.muted()
                    })
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(name),
            );
            if ok && !short_hash.is_empty() {
                toggle =
                    toggle.child(
                        div()
                            .flex_none()
                            .px(self.theme.space(6.))
                            .py(px(1.))
                            .rounded_md()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(self.tf(
                                "verify.card.hash",
                                &[("hash", &short_hash)],
                            )),
                    );
            }
            if let Some(files) = deliverables_for_quick_open {
                toggle = toggle.child(
                    theme::Button::new(
                        ("verify-open-deliverables", row_id),
                        self.t("studio.view_deliverables"),
                    )
                    .variant(theme::ControlVariant::Ghost)
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(OpenDeliverables(files.clone()));
                        }),
                    ),
                );
            }
            toggle = toggle
                .child(
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
                .on_click(cx.listener(move |feed, _, _, cx| {
                    feed.toggle_detail(row_id, cx);
                }));

            let mut card = div()
                .id(("verification-card", row_id))
                .debug_selector(|| "chat-verification-card".to_owned())
                .role(if ok {
                    gpui::Role::Status
                } else {
                    gpui::Role::Alert
                })
                .aria_label(status_label)
                .w_full()
                .max_w(px(560.))
                .min_w_0()
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.card())
                .flex()
                .flex_col()
                .child(toggle);

            if expanded {
                let mut details = div()
                    .w_full()
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .border_t_1()
                    .border_color(self.theme.line())
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(display_path(artifact_path)),
                            )
                            .when(!short_hash.is_empty(), |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px(self.theme.space(6.))
                                        .py(px(1.))
                                        .rounded_md()
                                        .bg(self.theme.soft())
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .child(self.tf(
                                            "verify.card.hash",
                                            &[("hash", &short_hash)],
                                        )),
                                )
                            }),
                    );
                if !issues.is_empty() {
                    let mut list = div().w_full().flex().flex_col().gap(px(4.));
                    for (issue_ix, line_text) in self
                        .format_verification_issues(issues)
                        .into_iter()
                        .enumerate()
                    {
                        list = list.child(
                            div()
                                .id((
                                    "verify-issue",
                                    row_id * 100 + issue_ix as u64,
                                ))
                                .role(gpui::Role::Status)
                                .aria_label(line_text.clone())
                                .text_xs()
                                .text_color(self.theme.ink())
                                .whitespace_normal()
                                .child(line_text),
                        );
                    }
                    details = details.child(list);
                }
                if let Some(hint) = semantic_hint
                    && !hint.trim().is_empty()
                {
                    let hint_label =
                        self.tf("verify.semantic.hint", &[("hint", hint)]);
                    details = details.child(
                        div()
                            .id(("verify-semantic-hint", row_id))
                            .role(gpui::Role::Status)
                            .aria_label(hint_label.clone())
                            .px(self.theme.space(8.))
                            .py(self.theme.space(4.))
                            .rounded_md()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(hint_label),
                    );
                }
                card = card.child(self.reveal_collapsible(
                    ("verify-detail-reveal", row_id),
                    row_id,
                    details,
                    cx,
                ));
            }

            if override_used && !ok {
                return crate::motion::settled_collapse_reveal(
                    ("verify-settled", row_id),
                    policy,
                    card,
                );
            }
            return card.into_any_element();
        }

        let check_key = format!("{report_id}:{artifact_path}");
        let checked = self.override_checked.contains(&check_key);

        let mut header = div()
            .w_full()
            .rounded_t_xl()
            .bg(self.theme.danger_bg())
            .border_b_1()
            .border_color(self.theme.line())
            .px(self.theme.space(12.))
            .py(self.theme.space(10.))
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .child(
                theme::icons::circle_alert()
                    .size(px(16.))
                    .text_color(self.theme.danger()),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.danger())
                    .child(status_label.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(name),
            );
        if !short_hash.is_empty() {
            header = header.child(
                div()
                    .px(self.theme.space(6.))
                    .py(px(1.))
                    .rounded_md()
                    .bg(self.theme.card())
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(
                        self.tf("verify.card.hash", &[("hash", &short_hash)]),
                    ),
            );
        }

        let mut card = div()
            .id(("verification-card", row_id))
            .debug_selector(|| "chat-verification-card".to_owned())
            .role(gpui::Role::Alert)
            .aria_label(status_label)
            .w_full()
            .max_w(px(560.))
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.danger().opacity(0.65))
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .child(header);

        let mut body = div()
            .w_full()
            .px(self.theme.space(12.))
            .py(self.theme.space(10.))
            .flex()
            .flex_col()
            .gap(self.theme.space(8.));

        if !issues.is_empty() {
            let mut list = div().w_full().flex().flex_col().gap(px(5.));
            for (issue_ix, line_text) in self
                .format_verification_issues(issues)
                .into_iter()
                .enumerate()
            {
                list = list.child(
                    div()
                        .id(("verify-issue", row_id * 100 + issue_ix as u64))
                        .role(gpui::Role::Status)
                        .aria_label(line_text.clone())
                        .flex()
                        .items_start()
                        .gap(px(6.))
                        .child(
                            div()
                                .mt(px(5.))
                                .size(px(5.))
                                .flex_none()
                                .rounded_full()
                                .bg(self.theme.danger()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(self.theme.ink())
                                .whitespace_normal()
                                .child(line_text),
                        ),
                );
            }
            body = body.child(list);
        }

        if let Some(hint) = semantic_hint
            && !hint.trim().is_empty()
        {
            let hint_label = self.tf("verify.semantic.hint", &[("hint", hint)]);
            body = body.child(
                div()
                    .id(("verify-semantic-hint", row_id))
                    .role(gpui::Role::Status)
                    .aria_label(hint_label.clone())
                    .px(self.theme.space(8.))
                    .py(self.theme.space(6.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(hint_label),
            );
        }

        card = card.child(body);

        // §8.3: Never render an ordinary "Send / Deliver" primary button when
        // verification fails. Only an explicit unchecked-by-default checkbox
        // unlocks the override action, leaving a permanent audit mark.
        let confirm_label = self.t("verify.override.confirm");
        let toggle_key = check_key;
        let action_report = report_id.to_owned();
        let action_path = artifact_path.to_owned();
        let checkbox = theme::interactive(
            ("verify-override-check", row_id),
            confirm_label.clone(),
            &self.theme,
        )
        .debug_selector(|| "chat-verify-override-check".to_owned())
        .role(gpui::Role::CheckBox)
        .aria_toggled(if checked {
            gpui::accesskit::Toggled::True
        } else {
            gpui::accesskit::Toggled::False
        })
        .flex()
        .items_center()
        .gap(self.theme.space(8.))
        .cursor_pointer()
        .child(
            div()
                .size(px(16.))
                .rounded(px(4.))
                .border_1()
                .border_color(if checked {
                    self.theme.accent()
                } else {
                    self.theme.border_strong()
                })
                .bg(if checked {
                    self.theme.fill()
                } else {
                    self.theme.canvas()
                })
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |mark| {
                    mark.child(
                        theme::icons::check()
                            .size(px(12.))
                            .text_color(self.theme.on_fill()),
                    )
                }),
        )
        .child(
            div()
                .text_xs()
                .text_color(self.theme.ink())
                .child(confirm_label),
        )
        .on_click(cx.listener(move |feed, _, _, cx| {
            feed.toggle_override_check(toggle_key.clone(), cx);
        }));

        let action_btn = theme::Button::new(
            ("verify-override-submit", row_id),
            self.t("verify.override.action"),
        )
        .variant(theme::ControlVariant::Secondary)
        .size(theme::ControlSize::Small)
        .disabled(!checked)
        .render(
            &self.theme,
            cx.listener(move |_, _, _, cx| {
                cx.emit(ArtifactAction::OverrideDeliver {
                    report_id: action_report.clone(),
                    artifact_path: action_path.clone(),
                });
            }),
        );

        card = card.child(
            div()
                .w_full()
                .rounded_b_xl()
                .bg(self.theme.soft().opacity(0.45))
                .border_t_1()
                .border_color(self.theme.line())
                .px(self.theme.space(12.))
                .py(self.theme.space(8.))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(checkbox)
                .child(crate::library::test_control(
                    "chat-verify-override-submit",
                    action_btn,
                )),
        );

        card.into_any_element()
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
                        .text_color(self.theme.status_active()),
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
                theme::interactive(
                    ("chat-reasoning-toggle", row_id),
                    title,
                    &self.theme,
                )
                .aria_expanded(expanded)
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

    fn tool_call_block(
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
                theme::interactive(
                    ("chat-tool-toggle", row_id),
                    accessible,
                    &self.theme,
                )
                .aria_expanded(expanded)
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

    fn tool_output_block(
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
                theme::interactive(
                    ("chat-tool-out-toggle", row_id),
                    display_preview.clone(),
                    &self.theme,
                )
                .aria_expanded(expanded)
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
                                    .flex_1()
                                    .min_w_0()
                                    .max_w(px(760.))
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

#[derive(Clone, Debug)]
pub enum WorkingIndicatorEvent {
    RetryDelivery,
}

impl gpui::EventEmitter<WorkingIndicatorEvent> for WorkingIndicator {}

pub struct WorkingIndicator {
    label: String,
    visible: bool,
    error: bool,
    retry: bool,
    retry_label: String,
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
            retry: false,
            retry_label: String::new(),
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

    pub(crate) fn set_retry(
        &mut self,
        retry: bool,
        retry_label: String,
        cx: &mut Context<'_, Self>,
    ) {
        if self.retry != retry || self.retry_label != retry_label {
            self.retry = retry;
            self.retry_label = retry_label;
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn label_for_test(&self) -> &str {
        &self.label
    }

    #[cfg(test)]
    pub(crate) const fn retry_enabled(&self) -> bool {
        self.retry
    }
}

impl Render for WorkingIndicator {
    fn render(
        &mut self,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let retry = self.retry;
        let retry_label = self.retry_label.clone();
        div().id("chat-working").role(gpui::Role::Status).when(
            self.visible,
            |element| {
                element
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .text_xs()
                    .text_color(if self.error {
                        self.theme.danger()
                    } else {
                        self.theme.muted()
                    })
                    .when(self.error, |row| {
                        row.child(
                            theme::icons::circle_alert()
                                .size(px(14.))
                                .text_color(self.theme.danger()),
                        )
                    })
                    .child(self.label.clone())
                    .when(retry, |row| {
                        row.child(
                            theme::Button::new(
                                "chat-retry-delivery",
                                retry_label,
                            )
                            .size(theme::ControlSize::Small)
                            .variant(theme::ControlVariant::Secondary)
                            .render(
                                &self.theme,
                                cx.listener(|_, _, _, cx| {
                                    cx.emit(
                                        WorkingIndicatorEvent::RetryDelivery,
                                    );
                                }),
                            ),
                        )
                    })
            },
        )
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
                window.draw(cx).clear(cx);
            });
        }
        let focus = cx.read_entity(&feed, |feed, _| {
            feed.artifact_focus("report-id").unwrap()
        });
        cx.update(|window, cx| {
            assert_eq!(window.focused(cx), Some(focus));
        });
        // The row is still a real pointer target: clicking the painted control
        // must dispatch the same action the platform accessibility click did.
        let bounds = cx
            .debug_bounds("chat-artifact-preview")
            .expect("the artifact row paints a named preview control");
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
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
            window.draw(cx).clear(cx);
        });
        // The fold control is a button; clicking the painted row must expand it.
        let fold = cx
            .debug_bounds("chat-turn-fold")
            .expect("the collapsed turn paints its fold control");
        cx.simulate_click(fold.center(), gpui::Modifiers::none());
        cx.run_until_parked();
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
                working.set_retry(true, "Retry delivery".into(), cx);
            });
        });
        cx.read_entity(&indicator, |working, _| {
            assert!(working.visible);
            assert!(!working.error);
            assert_eq!(working.label, "working");
            assert!(working.retry);
            assert_eq!(working.retry_label, "Retry delivery");
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

    #[gpui::test]
    fn verification_card_renders_four_codes_hides_default_save_and_requires_checkbox(
        cx: &mut TestAppContext,
    ) {
        use std::sync::{Arc, Mutex};

        let i18n = I18n::embedded("Flowy");
        let events = Arc::new(Mutex::new(Vec::new()));
        let issues = vec![
            flowy_agent::VerificationIssue {
                code: "ANCHOR_MISSING".into(),
                field: "S0-title".into(),
            },
            flowy_agent::VerificationIssue {
                code: "ANCHOR_DUPLICATE".into(),
                field: "S0-body".into(),
            },
            flowy_agent::VerificationIssue {
                code: "ANCHOR_INVALID".into(),
                field: "S0-extra".into(),
            },
            flowy_agent::VerificationIssue {
                code: "VALUE_MISMATCH".into(),
                field: "S1-body".into(),
            },
        ];
        let sample_issues = issues.clone();
        let (feed, cx) = cx.add_window_view(|_, cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            let mut feed =
                ChatFeed::new(ui, i18n.clone(), Some("verify".into()), cx);
            feed.sync_rows(
                Some("verify".into()),
                vec![ChatLine::Verification {
                    report_id: "report-42".into(),
                    artifact_path: "outputs/sub/deck.pptx".into(),
                    artifact_hash: "a1b2c3d4e5f60000".into(),
                    ok: false,
                    issues: sample_issues,
                    semantic_hint: Some("标题与正文一致".into()),
                    override_used: false,
                }],
                cx,
            );
            feed
        });
        let sink = Arc::clone(&events);
        let _sub = cx.update(|_, cx| {
            cx.subscribe(&feed, move |_, action: &ArtifactAction, _| {
                sink.lock().unwrap().push(action.clone());
            })
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        // Verify distinct localized copy across both zh-CN and en.
        cx.read_entity(&feed, |feed, _| {
            let zh = feed.format_verification_issues(&issues);
            let unique_zh: std::collections::HashSet<&String> =
                zh.iter().collect();
            assert_eq!(unique_zh.len(), 4);
            assert!(zh[0].contains(&i18n.t("verify.anchor_missing")));
            assert!(zh[3].contains(&i18n.t("verify.value_mismatch")));
        });
        cx.update(|_, cx| {
            feed.update(cx, |feed, cx| {
                let mut en = I18n::embedded("Flowy");
                en.set_locale(flowy_i18n::Locale::En);
                feed.sync_i18n(en, cx);
                let formatted = feed.format_verification_issues(&issues);
                let unique_en: std::collections::HashSet<&String> =
                    formatted.iter().collect();
                assert_eq!(unique_en.len(), 4);
            });
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        assert!(
            cx.debug_bounds("chat-verification-card").is_some(),
            "verification card must render"
        );
        assert!(
            cx.debug_bounds("chat-artifact-save-as").is_none(),
            "failed verification card must never render ordinary save/deliver button"
        );

        // Clicking submit before checking the override box does nothing.
        let submit = cx
            .debug_bounds("chat-verify-override-submit")
            .expect("override submit control rendered");
        cx.simulate_click(submit.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert_eq!(events.lock().unwrap().len(), 0);

        // Checking the override box unlocks the explicit override action.
        let check = cx
            .debug_bounds("chat-verify-override-check")
            .expect("override checkbox rendered");
        cx.simulate_click(check.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let submit = cx
            .debug_bounds("chat-verify-override-submit")
            .expect("override submit control rendered");
        cx.simulate_click(submit.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [ArtifactAction::OverrideDeliver {
                report_id,
                artifact_path,
            }] if report_id == "report-42" && artifact_path == "outputs/sub/deck.pptx"
        ));
    }

    #[test]
    fn latest_deliverables_includes_verified_and_overridden_but_excludes_failed()
     {
        let failed_only = vec![
            ChatLine::User {
                text: "build deck".into(),
            },
            ChatLine::Verification {
                report_id: "r1".into(),
                artifact_path: "outputs/bad.pptx".into(),
                artifact_hash: "aaaa".into(),
                ok: false,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
            },
        ];
        assert!(collect_latest_deliverables(&failed_only).is_none());

        let verified_and_overridden = vec![
            ChatLine::User {
                text: "build decks".into(),
            },
            ChatLine::Verification {
                report_id: "r2".into(),
                artifact_path: "outputs/ok.pptx".into(),
                artifact_hash: "bbbb".into(),
                ok: true,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: false,
            },
            ChatLine::Verification {
                report_id: "r3".into(),
                artifact_path: "outputs/override.docx".into(),
                artifact_hash: "cccc".into(),
                ok: false,
                issues: Vec::new(),
                semantic_hint: None,
                override_used: true,
            },
            ChatLine::Artifact {
                artifact_id: "a1".into(),
            },
        ];
        let found = collect_latest_deliverables(&verified_and_overridden)
            .expect("verified and overridden deliverables must surface");
        let paths: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["outputs/ok.pptx", "outputs/override.docx"]);
    }
}
