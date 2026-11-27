//! Independently rendered, variable-height message feed and elapsed-time indicator.

use std::collections::{HashMap, HashSet};

use flowy_agent::{ChatHistory, ChatLine, TurnStatus};
use flowy_i18n::I18n;
use flowy_ui::{ResolvedTheme, UiContext};
use gpui::{
    Context, ListAlignment, ListState, Render, SharedString, Subscription,
    Window, div, list, prelude::*, px,
};

use crate::history::Projection;
use crate::library::LibraryItem;

mod bubbles;
mod cards;
mod indicator;
mod process_blocks;
mod settlement;
#[cfg(test)]
mod tests;

pub use indicator::{WorkingIndicator, WorkingIndicatorEvent};
#[cfg(test)]
use settlement::collect_latest_deliverables;
pub use settlement::{
    DeliverableItem, DeliverableVerifyBadge, FailedVerificationItem,
};
use settlement::{
    analyze_session_verifications, attach_badges, collect_settled_entries,
};

pub struct ChatFeed {
    pub(in crate::feed) lines: Vec<ChatLine>,
    session: String,
    states: HashMap<String, StoredFeed>,
    pub(in crate::feed) row_ids: Vec<u64>,
    next_row_id: u64,
    pub(in crate::feed) row_focus: HashMap<u64, gpui::FocusHandle>,
    pub(in crate::feed) artifacts: HashMap<String, LibraryItem>,
    pending_row_focus: Option<u64>,
    /// Verification cards whose explicit override checkbox has been ticked by
    /// the user; always starts empty so override is never pre-checked (§8.3).
    pub(in crate::feed) override_checked: HashSet<String>,
    pub(in crate::feed) list: ListState,
    pub(in crate::feed) expanded_rows: HashSet<u64>,
    measured_heights: HashMap<u64, gpui::Entity<f32>>,
    pub(in crate::feed) copied_row: Option<(u64, u64)>,
    pub(in crate::feed) copy_epoch: u64,
    verification_settlement: HashMap<String, bool>,
    verification_badges: HashMap<String, DeliverableVerifyBadge>,
    cached_latest_deliverables: Option<Vec<flowy_agent::FileEntry>>,
    cached_latest_items: Option<Vec<DeliverableItem>>,
    cached_all_items: Option<Vec<DeliverableItem>>,
    cached_failed_verifications: Vec<FailedVerificationItem>,
    cached_session_artifacts: Vec<LibraryItem>,
    cached_has_trace_attention: bool,
    history: ChatHistory,
    suppress_trailing_stopped: bool,
    pub(in crate::feed) projection: Projection,
    expanded_turns: HashMap<String, HashSet<usize>>,
    pub(in crate::feed) theme: ResolvedTheme,
    pub(in crate::feed) i18n: I18n,
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
        session_id: Option<String>,
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

/// Ask the panel to attach a single truncated quote from a chat bubble.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteMessage {
    pub role: crate::composer::QuoteRole,
    pub text: String,
}

impl gpui::EventEmitter<QuoteMessage> for ChatFeed {}

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

const fn line_affects_deliverables_cache(line: &ChatLine) -> bool {
    match line {
        ChatLine::User { .. }
        | ChatLine::FileChange { .. }
        | ChatLine::FileChanges { .. }
        | ChatLine::Artifact { .. }
        | ChatLine::Verification { .. }
        | ChatLine::ToolResult { is_error: true, .. } => true,
        ChatLine::Assistant { .. }
        | ChatLine::Reasoning { .. }
        | ChatLine::Tool { .. }
        | ChatLine::ToolCall { .. }
        | ChatLine::ToolResult {
            is_error: false, ..
        }
        | ChatLine::Status { .. }
        | ChatLine::TaskReceipt { .. } => false,
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
            pending_row_focus: None,
            override_checked: HashSet::new(),
            session: session.unwrap_or_default(),
            states: HashMap::new(),
            list,
            expanded_rows: HashSet::new(),
            measured_heights: HashMap::new(),
            copied_row: None,
            copy_epoch: 0,
            verification_settlement: HashMap::new(),
            verification_badges: HashMap::new(),
            cached_latest_deliverables: None,
            cached_latest_items: None,
            cached_all_items: None,
            cached_failed_verifications: Vec::new(),
            cached_session_artifacts: Vec::new(),
            cached_has_trace_attention: false,
            history: ChatHistory::default(),
            suppress_trailing_stopped: false,
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
        self.refresh_session_artifacts_cache();
        self.list.remeasure_items(0..self.lines.len());
        cx.notify();
    }

    pub fn artifact_title(&self, artifact_id: &str) -> Option<String> {
        self.artifacts
            .get(artifact_id)
            .map(|item| item.title.clone())
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
        let Some(&row_id) = self.row_ids.get(row) else {
            return false;
        };
        self.list.scroll_to(gpui::ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        self.pending_row_focus = Some(row_id);
        cx.notify();
        true
    }

    pub fn focus_verification_row(
        &mut self,
        report_id: &str,
        artifact_path: &str,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let row = if report_id.is_empty() {
            None
        } else {
            self.lines.iter().rposition(|line| {
                matches!(line, ChatLine::Verification { report_id: id, .. } if id == report_id)
            })
        }
        .or_else(|| {
            let target = crate::ports::canonical_path_key(artifact_path);
            self.lines.iter().rposition(|line| {
                matches!(
                    line,
                    ChatLine::Verification { artifact_path: p, .. }
                        if crate::ports::canonical_path_key(p) == target
                )
            })
        });
        let Some(row) = row else {
            return false;
        };
        let Some(&row_id) = self.row_ids.get(row) else {
            return false;
        };
        self.expanded_rows.insert(row_id);
        self.list.remeasure_items(row..row + 1);
        self.list.scroll_to(gpui::ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        self.pending_row_focus = Some(row_id);
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

    pub(crate) const fn set_suppress_trailing_stopped(
        &mut self,
        suppress: bool,
    ) {
        self.suppress_trailing_stopped = suppress;
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
        let mut projection = Projection::build_live(
            &self.history.rows,
            self.history.streaming,
            !self.history.status.is_busy(),
            self.expanded_turns.get(name),
        );
        // When a detailed StopHandoffManifest card already surfaces "已停止"
        // directly below the feed, omit the redundant trailing status row at
        // projection time so no empty list item or padding row is emitted.
        // Fast-cancels and restored stopped sessions without a manifest keep
        // the trailing status row visible.
        if self.suppress_trailing_stopped
            && matches!(self.history.status, TurnStatus::Stopped)
            && projection.source_rows.last().is_some_and(|&idx| {
                matches!(
                    self.history.rows.get(idx),
                    Some(ChatLine::Status { text })
                        if text == "chat.status_stopped"
                )
            })
        {
            projection.source_rows.pop();
        }
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
            .chain(
                self.projection
                    .ack_rows
                    .symmetric_difference(&projection.ack_rows)
                    .copied(),
            )
            .chain(
                self.projection
                    .process_rows
                    .symmetric_difference(&projection.process_rows)
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

    pub(in crate::feed) fn set_turn_expanded(
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
        let old_len = self.lines.len();
        let new_len = lines.len();
        let prefix = self
            .lines
            .iter()
            .zip(&lines)
            .take_while(|(old, new)| old == new)
            .count();
        if prefix == old_len && old_len == new_len {
            if changed_session {
                self.refresh_deliverables_cache();
                cx.notify();
            }
            return;
        }
        let suffix = self.lines[prefix..]
            .iter()
            .rev()
            .zip(lines[prefix..].iter().rev())
            .take_while(|(old, new)| old == new)
            .count();
        let old_end = old_len - suffix;
        let new_end = new_len - suffix;
        let deliverables_dirty = changed_session
            || self.lines[prefix..old_end]
                .iter()
                .any(line_affects_deliverables_cache)
            || lines[prefix..new_end]
                .iter()
                .any(line_affects_deliverables_cache);
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
        if deliverables_dirty {
            self.refresh_deliverables_cache();
        }
        cx.notify();
    }

    pub(in crate::feed) fn toggle_detail(
        &mut self,
        id: u64,
        cx: &mut Context<'_, Self>,
    ) {
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

    pub(in crate::feed) fn reveal_collapsible(
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

    fn refresh_session_artifacts_cache(&mut self) {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for line in self.lines.iter().rev() {
            if let ChatLine::Artifact { artifact_id } = line
                && seen.insert(artifact_id.as_str())
                && let Some(item) = self.artifacts.get(artifact_id)
            {
                resolved.push(item.clone());
            }
        }
        self.cached_session_artifacts = resolved;
    }

    fn refresh_deliverables_cache(&mut self) {
        let (settled, badges, failures) =
            analyze_session_verifications(&self.lines);
        self.cached_failed_verifications = failures
            .into_iter()
            .map(|verdict| {
                let formatted = self.format_verification_issues(verdict.issues);
                let summary = if formatted.is_empty() {
                    self.t("verify.card.failed")
                } else {
                    formatted.join(" · ")
                };
                FailedVerificationItem {
                    report_id: verdict.report_id.to_owned(),
                    artifact_path: verdict.artifact_path.to_owned(),
                    issue_count: verdict.issues.len(),
                    summary,
                }
            })
            .collect();
        let latest_entries =
            collect_settled_entries(&self.lines, &settled, true);
        let all_entries = collect_settled_entries(&self.lines, &settled, false);
        self.cached_latest_items = latest_entries
            .clone()
            .map(|entries| attach_badges(entries, &badges));
        self.cached_all_items =
            all_entries.map(|entries| attach_badges(entries, &badges));
        self.cached_latest_deliverables = latest_entries;
        self.verification_settlement = settled;
        self.verification_badges = badges;
        self.cached_has_trace_attention =
            !self.cached_failed_verifications.is_empty()
                || self.lines.iter().any(|line| match line {
                    ChatLine::ToolResult { is_error: true, .. } => true,
                    ChatLine::Verification {
                        provenance_warnings,
                        ..
                    } => !provenance_warnings.is_empty(),
                    _ => false,
                });
        self.refresh_session_artifacts_cache();
    }

    /// The most recent turn's deliverable files for the drawer toggle,
    /// combining file-change summaries with verified/overridden artifacts
    /// while excluding failed unverified verification cards (§8.3).
    pub(crate) fn latest_deliverables(
        &self,
    ) -> Option<Vec<flowy_agent::FileEntry>> {
        self.cached_latest_deliverables.clone()
    }

    pub fn latest_deliverable_items(&self) -> Option<Vec<DeliverableItem>> {
        self.cached_latest_items.clone()
    }

    pub fn all_deliverable_items(&self) -> Option<Vec<DeliverableItem>> {
        self.cached_all_items.clone()
    }

    #[must_use]
    pub fn failed_verifications(&self) -> &[FailedVerificationItem] {
        &self.cached_failed_verifications
    }

    #[must_use]
    pub fn session_artifacts(&self) -> &[LibraryItem] {
        &self.cached_session_artifacts
    }

    #[must_use]
    pub const fn has_trace_attention(&self) -> bool {
        self.cached_has_trace_attention
    }

    /// Cached count of current-session deliverables (O(1) for `top_bar`).
    #[must_use]
    pub(crate) fn latest_deliverable_count(&self) -> usize {
        self.cached_latest_deliverables.as_ref().map_or(0, Vec::len)
    }

    /// Returns `true` when `path` has an unsettled failed verification as its
    /// latest chronological verdict in the active session (§8.3).
    #[must_use]
    pub(crate) fn is_path_failed(&self, path: &str) -> bool {
        let key = crate::ports::canonical_path_key(path);
        self.verification_settlement.get(&key) == Some(&false)
    }

    /// Filters a historical file list against the session's latest
    /// verification settlement and deduplicates canonical paths (§8.3).
    pub(crate) fn filter_settled_deliverables(
        &self,
        files: Vec<flowy_agent::FileEntry>,
    ) -> Vec<flowy_agent::FileEntry> {
        let mut seen = HashSet::new();
        files
            .into_iter()
            .filter(|file| {
                let key = crate::ports::canonical_path_key(&file.path);
                self.verification_settlement.get(&key) != Some(&false)
                    && seen.insert(key)
            })
            .collect()
    }

    pub fn filter_settled_deliverable_items(
        &self,
        files: Vec<flowy_agent::FileEntry>,
    ) -> Vec<DeliverableItem> {
        let filtered = self.filter_settled_deliverables(files);
        attach_badges(filtered, &self.verification_badges)
    }

    pub(crate) fn sync_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
        self.list.remeasure_items(0..self.lines.len());
        cx.notify();
    }

    pub(in crate::feed) fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }
    pub(in crate::feed) fn tf(
        &self,
        key: &str,
        vars: &[(&str, &str)],
    ) -> String {
        self.i18n.tf(key, vars)
    }
    pub(in crate::feed) fn format_duration_secs(&self, secs: u64) -> String {
        self.tf("chat.duration_secs", &[("n", &secs.max(1).to_string())])
    }
}

impl Render for ChatFeed {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        if let Some(row_id) = self.pending_row_focus.take()
            && let Some(focus) = self.row_focus.get(&row_id)
        {
            focus.focus(window, cx);
        }
        let weak = cx.weak_entity();
        flowy_ui::scroll_list_host(
            SharedString::from(format!("chat-feed-{}", self.session)),
            &self.list,
            &self.theme,
            self.t("a11y.scroll_chat_feed"),
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
                                .max_w(px(crate::STAGE_MAX_W))
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
