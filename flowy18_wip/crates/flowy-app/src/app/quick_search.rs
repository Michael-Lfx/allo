//! Zero-dependency global Quick Search modal (`Ctrl+K` / `Ctrl+Shift+F`).
//!
//! Searches exclusively across host-owned in-memory projections (< 1ms):
//! 1. Sessions (titles + stripped-Markdown message snippets via `AgentHandle::search_sessions`)
//! 2. Library deliverables (display title, original filename, source session)
//! 3. Registered project spaces (name + folder path)
//! 4. Installed `ClawHub` skills (name, slug, description)
//! 5. Empty-query quick jumps (recent active sessions + core destinations)

use super::{FlowyApp, Section, state::ActiveDialog};
use flowy_ui as ui;
use gpui::{
    AnyElement, Context, Entity, Focusable, Font, FontWeight, Hsla,
    IntoElement, ScrollHandle, StyledText, TextRun, Window, div, prelude::*,
    px,
};
use gpui_kit::base::input::InputState;

const MAX_SESSION_HITS: usize = 6;
const MAX_LIBRARY_HITS: usize = 5;
const MAX_PROJECT_HITS: usize = 4;
const MAX_SKILL_HITS: usize = 4;
const MAX_SCOPED_HITS: usize = 15;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SearchScope {
    #[default]
    All,
    Sessions,
    Library,
    Projects,
    Skills,
}

impl SearchScope {
    pub(super) const ALL: [Self; 5] = [
        Self::All,
        Self::Sessions,
        Self::Library,
        Self::Projects,
        Self::Skills,
    ];

    pub(super) const fn label_key(self) -> &'static str {
        match self {
            Self::All => "search.scope_all",
            Self::Sessions => "search.group_sessions",
            Self::Library => "search.group_library",
            Self::Projects => "search.group_projects",
            Self::Skills => "search.group_skills",
        }
    }

    pub(super) const fn step(self, delta: isize) -> Self {
        let len = Self::ALL.len().cast_signed();
        let idx = match self {
            Self::All => 0,
            Self::Sessions => 1,
            Self::Library => 2,
            Self::Projects => 3,
            Self::Skills => 4,
        };
        let next = (idx + delta).rem_euclid(len).cast_unsigned();
        Self::ALL[next]
    }
}

pub(super) struct QuickSearchState {
    pub(super) query: Entity<InputState>,
    pub(super) scope: SearchScope,
    pub(super) selected: usize,
    pub(super) scroll: ScrollHandle,
}

impl QuickSearchState {
    pub(super) fn new(query: Entity<InputState>) -> Self {
        Self {
            query,
            scope: SearchScope::All,
            selected: 0,
            scroll: ScrollHandle::new(),
        }
    }
}

/// Splits and deduplicates a search query into lowercase character tokens once
/// per render frame so match highlighting avoids redundant per-row work.
fn tokenize_query(query: &str) -> Vec<Vec<char>> {
    let mut tokens: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|tok| tok.chars().flat_map(char::to_lowercase).collect())
        .filter(|tok: &Vec<char>| !tok.is_empty())
        .collect();
    tokens.sort_unstable();
    tokens.dedup();
    tokens
}

/// Finds sorted, merged UTF-8 byte ranges in `text` matching any pre-tokenized
/// lowercase query token. Every returned range is guaranteed to sit on valid
/// UTF-8 char boundaries of `text`, even when `char::to_lowercase` expands one
/// character into multiple scalars (e.g. `İ`).
fn highlight_byte_ranges_tokens(
    text: &str,
    tokens: &[Vec<char>],
) -> Vec<std::ops::Range<usize>> {
    if text.is_empty() || tokens.is_empty() {
        return Vec::new();
    }

    let mut lower_chars = Vec::with_capacity(text.len());
    let mut lower_spans: Vec<(usize, usize)> = Vec::with_capacity(text.len());
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        for lower in ch.to_lowercase() {
            lower_chars.push(lower);
            lower_spans.push((start, end));
        }
    }

    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for tok in tokens {
        let tlen = tok.len();
        if tlen > lower_chars.len() {
            continue;
        }
        let mut i = 0;
        while i + tlen <= lower_chars.len() {
            if lower_chars[i..i + tlen] == tok[..] {
                let byte_start = lower_spans[i].0;
                let byte_end = lower_spans[i + tlen - 1].1;
                ranges.push(byte_start..byte_end);
                i += tlen;
            } else {
                i += 1;
            }
        }
    }
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<std::ops::Range<usize>> =
        Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

#[cfg(test)]
fn highlight_byte_ranges(
    text: &str,
    query: &str,
) -> Vec<std::ops::Range<usize>> {
    let tokens = tokenize_query(query);
    highlight_byte_ranges_tokens(text, &tokens)
}

fn highlighted_text(
    text: String,
    tokens: &[Vec<char>],
    base_font: Font,
    base_color: gpui::Rgba,
    accent: gpui::Rgba,
    accent_bg: gpui::Rgba,
) -> AnyElement {
    let ranges = highlight_byte_ranges_tokens(&text, tokens);
    if ranges.is_empty() {
        return text.into_any_element();
    }
    let mut match_font = base_font.clone();
    match_font.weight = FontWeight::SEMIBOLD;
    let base_hsla = Hsla::from(base_color);
    let accent_hsla = Hsla::from(accent);
    let bg_hsla = Hsla::from(accent_bg);

    let mut runs = Vec::with_capacity(ranges.len() * 2 + 1);
    let mut cursor = 0_usize;
    for range in ranges {
        if range.start > cursor {
            runs.push(TextRun {
                len: range.start - cursor,
                font: base_font.clone(),
                color: base_hsla,
                ..Default::default()
            });
        }
        if range.end > range.start {
            runs.push(TextRun {
                len: range.end - range.start,
                font: match_font.clone(),
                color: accent_hsla,
                background_color: Some(bg_hsla),
                ..Default::default()
            });
        }
        cursor = range.end;
    }
    if cursor < text.len() {
        runs.push(TextRun {
            len: text.len() - cursor,
            font: base_font,
            color: base_hsla,
            ..Default::default()
        });
    }
    StyledText::new(text).with_runs(runs).into_any_element()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SearchTarget {
    Session(String),
    Artifact(String),
    Project(String),
    Skill,
    NavigateBoard,
    NavigateLibrary,
    NewTask,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct QuickSearchRow {
    pub(super) section_key: &'static str,
    pub(super) title: String,
    pub(super) subtitle: Option<String>,
    pub(super) badge: Option<String>,
    pub(super) target: SearchTarget,
    pub(super) score: u32,
}

/// Pure multi-token scorer used for library items, projects, and skills.
#[must_use]
pub(super) fn score_fields(query: &str, fields: &[&str]) -> u32 {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return 0;
    }
    let full = tokens.join(" ");
    let mut best = 0_u32;
    for (idx, field) in fields.iter().enumerate() {
        let trimmed = field.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        let weight = if idx == 0 { 100 } else { 70 };
        if lower == full {
            best = best.max(weight + 25);
        } else if lower.starts_with(&full) {
            best = best.max(weight + 10);
        } else if tokens.iter().all(|tok| lower.contains(tok.as_str())) {
            best = best.max(weight);
        }
    }
    if best > 0 {
        return best;
    }
    // Cross-field match (all tokens appear somewhere across the fields).
    let combined = fields.join(" ").to_lowercase();
    if tokens.iter().all(|tok| combined.contains(tok.as_str())) {
        return 45;
    }
    0
}

impl FlowyApp {
    pub(super) fn open_quick_search(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.active_dialog == ActiveDialog::QuickSearch {
            self.active_dialog = ActiveDialog::None;
            cx.notify();
            return;
        }
        self.quick_search.scope = SearchScope::All;
        self.quick_search.selected = 0;
        self.quick_search.scroll.scroll_to_item(0);
        self.set_input_text(&self.quick_search.query, "", cx);
        // Do NOT manually focus `query` before `OverlayHost::set_modal` runs in
        // `render`: `OverlayHost` captures the prior focus handle for Escape
        // restore and automatically focuses the dialog's first input via
        // `focus_after_paint -> cycle_within`.
        self.activate_dialog(ActiveDialog::QuickSearch, cx);
    }

    pub(super) fn set_quick_search_scope(
        &mut self,
        scope: SearchScope,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let focus = self.quick_search.query.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        if self.quick_search.scope == scope {
            return;
        }
        self.quick_search.scope = scope;
        self.quick_search.selected = 0;
        self.quick_search.scroll.scroll_to_item(0);
        cx.notify();
    }

    pub(super) fn cycle_quick_search_scope(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let next = self.quick_search.scope.step(delta);
        self.set_quick_search_scope(next, window, cx);
    }

    pub(super) fn move_quick_search_selection(
        &mut self,
        delta: isize,
        cx: &mut Context<'_, Self>,
    ) {
        let rows = self.quick_search_results(cx);
        let count = rows.len();
        if count == 0 {
            self.quick_search.selected = 0;
            return;
        }
        let current = self.quick_search.selected.min(count - 1);
        let next = if delta < 0 {
            if current == 0 { count - 1 } else { current - 1 }
        } else {
            (current + 1) % count
        };
        self.quick_search.selected = next;
        // Count preceding section headers so `scroll_to_item` targets the
        // actual child index inside `quick-search-list`.
        let mut child_idx = 0_usize;
        let mut last_section: Option<&'static str> = None;
        for (idx, row) in rows.iter().enumerate() {
            if last_section != Some(row.section_key) {
                last_section = Some(row.section_key);
                child_idx += 1;
            }
            if idx == next {
                break;
            }
            child_idx += 1;
        }
        self.quick_search.scroll.scroll_to_item(if next == 0 {
            0
        } else {
            child_idx
        });
        cx.notify();
    }

    pub(super) fn confirm_quick_search(&mut self, cx: &mut Context<'_, Self>) {
        let rows = self.quick_search_results(cx);
        if rows.is_empty() {
            return;
        }
        let idx = self.quick_search.selected.min(rows.len() - 1);
        self.activate_quick_search_target(rows[idx].target.clone(), cx);
    }

    fn activate_quick_search_target(
        &mut self,
        target: SearchTarget,
        cx: &mut Context<'_, Self>,
    ) {
        self.active_dialog = ActiveDialog::None;
        match target {
            SearchTarget::Session(session_id) => {
                self.push_section(Section::Studio, cx);
                self.studio.update(cx, |panel, cx| {
                    panel.open_session(&session_id, cx);
                });
            }
            SearchTarget::Artifact(artifact_id) => {
                self.push_section(Section::Library, cx);
                self.open_artifact_preview(&artifact_id, cx);
            }
            SearchTarget::Project(project_id) => {
                self.projects_open = true;
                self.expanded_projects.insert(project_id.clone());
                self.push_section(Section::Studio, cx);
                let result = self.studio.update(cx, |panel, cx| {
                    panel.open_project(&project_id, cx)
                });
                if let Err(key) = result {
                    self.studio.update(cx, |panel, cx| panel.note(key, cx));
                }
            }
            SearchTarget::Skill => {
                self.push_section(Section::Skills, cx);
            }
            SearchTarget::NavigateBoard => {
                self.push_section(Section::Board, cx);
            }
            SearchTarget::NavigateLibrary => {
                self.push_section(Section::Library, cx);
            }
            SearchTarget::NewTask => {
                self.start_library_conversation(cx);
            }
        }
        cx.notify();
    }

    pub(super) fn quick_search_results(
        &self,
        cx: &Context<'_, Self>,
    ) -> Vec<QuickSearchRow> {
        let query = self.quick_search.query.read(cx).value().trim().to_owned();
        let scope = self.quick_search.scope;
        let studio = self.studio.read(cx);
        let mut rows = Vec::new();

        if query.is_empty() {
            match scope {
                SearchScope::All => {
                    for hit in studio.agent.search_sessions("", 5) {
                        rows.push(self.session_hit_row(
                            hit,
                            "search.group_recent",
                            false,
                        ));
                    }
                    rows.push(QuickSearchRow {
                        section_key: "search.group_actions",
                        title: self.t("dock.new_task"),
                        subtitle: None,
                        badge: None,
                        target: SearchTarget::NewTask,
                        score: 10,
                    });
                    rows.push(QuickSearchRow {
                        section_key: "search.group_actions",
                        title: self.t("dock.board"),
                        subtitle: None,
                        badge: None,
                        target: SearchTarget::NavigateBoard,
                        score: 9,
                    });
                    rows.push(QuickSearchRow {
                        section_key: "search.group_actions",
                        title: self.t("dock.library"),
                        subtitle: None,
                        badge: None,
                        target: SearchTarget::NavigateLibrary,
                        score: 8,
                    });
                }
                SearchScope::Sessions => {
                    for hit in studio.agent.search_sessions("", 10) {
                        rows.push(self.session_hit_row(
                            hit,
                            "search.group_sessions",
                            true,
                        ));
                    }
                }
                SearchScope::Library => {
                    let library = self.library.read(cx);
                    let mut items: Vec<_> = library.items().iter().collect();
                    items.sort_by(|a, b| {
                        b.delivered_at_ms.cmp(&a.delivered_at_ms)
                    });
                    for item in items.into_iter().take(10) {
                        rows.push(Self::library_hit_row(item, 10));
                    }
                }
                SearchScope::Projects => {
                    let mut projects: Vec<_> = studio.projects.iter().collect();
                    projects.sort_by(|a, b| {
                        b.pinned
                            .cmp(&a.pinned)
                            .then_with(|| a.name.cmp(&b.name))
                    });
                    for proj in projects.into_iter().take(10) {
                        rows.push(self.project_hit_row(proj, 10));
                    }
                }
                SearchScope::Skills => {
                    for skill in self.skills.installed.iter().take(10) {
                        rows.push(Self::skill_hit_row(skill, 10));
                    }
                }
            }
            return rows;
        }

        // 1. Sessions (titles + body message snippets, defense-in-depth redacted)
        if matches!(scope, SearchScope::All | SearchScope::Sessions) {
            let cap = if scope == SearchScope::Sessions {
                MAX_SCOPED_HITS
            } else {
                MAX_SESSION_HITS
            };
            for hit in studio.agent.search_sessions(&query, cap) {
                rows.push(self.session_hit_row(
                    hit,
                    "search.group_sessions",
                    true,
                ));
            }
        }

        // 2. Library deliverables
        if matches!(scope, SearchScope::All | SearchScope::Library) {
            let cap = if scope == SearchScope::Library {
                MAX_SCOPED_HITS
            } else {
                MAX_LIBRARY_HITS
            };
            let mut lib_hits: Vec<QuickSearchRow> = self
                .library
                .read(cx)
                .items()
                .iter()
                .filter_map(|item| {
                    let score = score_fields(
                        &query,
                        &[
                            item.title.as_str(),
                            item.original_name.as_str(),
                            item.source_title.as_str(),
                            item.extension.as_str(),
                        ],
                    );
                    (score > 0).then(|| Self::library_hit_row(item, score))
                })
                .collect();
            lib_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
            lib_hits.truncate(cap);
            rows.extend(lib_hits);
        }

        // 3. Projects
        if matches!(scope, SearchScope::All | SearchScope::Projects) {
            let cap = if scope == SearchScope::Projects {
                MAX_SCOPED_HITS
            } else {
                MAX_PROJECT_HITS
            };
            let mut proj_hits: Vec<QuickSearchRow> = studio
                .projects
                .iter()
                .filter_map(|proj| {
                    let path_str = proj.root.to_string_lossy();
                    let score =
                        score_fields(&query, &[proj.name.as_str(), &path_str]);
                    (score > 0).then(|| self.project_hit_row(proj, score))
                })
                .collect();
            proj_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
            proj_hits.truncate(cap);
            rows.extend(proj_hits);
        }

        // 4. Installed skills
        if matches!(scope, SearchScope::All | SearchScope::Skills) {
            let cap = if scope == SearchScope::Skills {
                MAX_SCOPED_HITS
            } else {
                MAX_SKILL_HITS
            };
            let mut skill_hits: Vec<QuickSearchRow> = self
                .skills
                .installed
                .iter()
                .filter_map(|skill| {
                    let id_str =
                        format!("{}/{}", skill.id.owner_handle, skill.id.slug);
                    let title = if skill.name.trim().is_empty() {
                        skill.id.slug.as_str()
                    } else {
                        skill.name.as_str()
                    };
                    let score = score_fields(
                        &query,
                        &[title, &id_str, skill.description.as_str()],
                    );
                    (score > 0).then(|| Self::skill_hit_row(skill, score))
                })
                .collect();
            skill_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
            skill_hits.truncate(cap);
            rows.extend(skill_hits);
        }

        rows
    }

    fn library_hit_row(
        item: &flowy_studio::library::LibraryItem,
        score: u32,
    ) -> QuickSearchRow {
        let ext = item.extension.trim_start_matches('.');
        QuickSearchRow {
            section_key: "search.group_library",
            title: item.title.clone(),
            subtitle: (!item.source_title.is_empty())
                .then(|| item.source_title.clone()),
            badge: (!ext.is_empty()).then(|| ext.to_ascii_uppercase()),
            target: SearchTarget::Artifact(item.id.clone()),
            score,
        }
    }

    fn project_hit_row(
        &self,
        proj: &flowy_studio::Project,
        score: u32,
    ) -> QuickSearchRow {
        let path_str = proj.root.to_string_lossy();
        QuickSearchRow {
            section_key: "search.group_projects",
            title: proj.name.clone(),
            subtitle: Some(flowy_studio::ports::redact_trace_text(&path_str)),
            badge: proj.pinned.then(|| self.t("chat.pinned_badge")),
            target: SearchTarget::Project(proj.id.clone()),
            score,
        }
    }

    fn skill_hit_row(
        skill: &flowy_agent::InstalledSkill,
        score: u32,
    ) -> QuickSearchRow {
        let id_str = format!("{}/{}", skill.id.owner_handle, skill.id.slug);
        let title = if skill.name.trim().is_empty() {
            skill.id.slug.clone()
        } else {
            skill.name.clone()
        };
        QuickSearchRow {
            section_key: "search.group_skills",
            title,
            subtitle: (!skill.description.trim().is_empty())
                .then(|| skill.description.trim().to_owned()),
            badge: Some(format!("/skill:{id_str}")),
            target: SearchTarget::Skill,
            score,
        }
    }

    fn session_hit_row(
        &self,
        hit: flowy_agent::SessionSearchHit,
        section_key: &'static str,
        include_archived_badge: bool,
    ) -> QuickSearchRow {
        let title = if hit.title.trim().is_empty() {
            self.t("dock.new_task")
        } else {
            flowy_studio::ports::redact_trace_text(&hit.title)
        };
        let subtitle = hit
            .snippet
            .as_deref()
            .map(flowy_studio::ports::redact_trace_text);
        let badge = if include_archived_badge && hit.archived {
            Some(self.t("chat.archived_badge"))
        } else if hit.pinned {
            Some(self.t("chat.pinned_badge"))
        } else {
            None
        };
        QuickSearchRow {
            section_key,
            title,
            subtitle,
            badge,
            target: SearchTarget::Session(hit.session_id),
            score: hit.score,
        }
    }

    pub(super) fn quick_search_layer(
        &mut self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let query_tokens =
            tokenize_query(self.quick_search.query.read(cx).value().trim());
        let rows = self.quick_search_results(cx);
        let selected = if rows.is_empty() {
            0
        } else {
            self.quick_search.selected.min(rows.len() - 1)
        };
        self.quick_search.selected = selected;
        let frame = self.theme.modal_size(window.viewport_size());
        let sub_font = ui::ui_font();
        let mut title_font = sub_font.clone();
        title_font.weight = FontWeight::MEDIUM;
        let accent = self.theme.status_active();
        let accent_bg = self.theme.status_active_bg();

        let mut scope_bar = div()
            .w_full()
            .px_3()
            .py(px(7.))
            .border_b_1()
            .border_color(self.theme.line())
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.));
        for (idx, scope) in SearchScope::ALL.into_iter().enumerate() {
            let active = self.quick_search.scope == scope;
            let label = self.t(scope.label_key());
            scope_bar = scope_bar.child(
                ui::interactive(
                    ("quick-search-scope", idx as u64),
                    label.clone(),
                    &self.theme,
                )
                .px(px(9.))
                .py(px(3.))
                .rounded_full()
                .cursor_pointer()
                .text_xs()
                .font_weight(if active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .bg(if active {
                    self.theme.ink()
                } else {
                    self.theme.soft()
                })
                .text_color(if active {
                    self.theme.on_ink()
                } else {
                    self.theme.muted()
                })
                .hover(|s| {
                    if active {
                        s
                    } else {
                        s.text_color(self.theme.ink())
                    }
                })
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.set_quick_search_scope(scope, window, cx);
                }))
                .child(label),
            );
        }

        let mut list = ui::scroll_area("quick-search-list", &self.theme)
            .track_scroll(&self.quick_search.scroll)
            .max_h_viewport(px(360.))
            .a11y_label(self.t("a11y.scroll_quick_search"))
            .viewport_style(|v| v.flex().flex_col().gap(px(2.)).px_2().py_2());

        if rows.is_empty() {
            list = list.child(
                div()
                    .py_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(self.t("search.empty")),
            );
        } else {
            let mut last_section: Option<&'static str> = None;
            for (idx, row) in rows.into_iter().enumerate() {
                if last_section != Some(row.section_key) {
                    last_section = Some(row.section_key);
                    list = list.child(
                        div()
                            .px_2()
                            .pt(if idx == 0 { px(2.) } else { px(8.) })
                            .pb(px(3.))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .child(self.t(row.section_key)),
                    );
                }
                let is_selected = idx == selected;
                let target = row.target.clone();
                let icon = search_row_icon(&row.target, self.theme);
                let title = row.title;
                let title_node = highlighted_text(
                    title.clone(),
                    &query_tokens,
                    title_font.clone(),
                    self.theme.ink(),
                    accent,
                    accent_bg,
                );
                let subtitle_node = row.subtitle.map(|sub| {
                    highlighted_text(
                        sub,
                        &query_tokens,
                        sub_font.clone(),
                        self.theme.muted(),
                        accent,
                        accent_bg,
                    )
                });
                let badge = row.badge;
                list = list.child(
                    ui::interactive(
                        ("quick-search-row", idx as u64),
                        title,
                        &self.theme,
                    )
                    .w_full()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(if is_selected {
                        self.theme.dock_selected()
                    } else {
                        self.theme.canvas()
                    })
                    .hover(|el| el.bg(self.theme.dock_selected()))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.activate_quick_search_target(target.clone(), cx);
                    }))
                    .child(icon)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .truncate()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(self.theme.ink())
                                            .child(title_node),
                                    )
                                    .when_some(badge, |line, label| {
                                        line.child(
                                            div()
                                                .flex_none()
                                                .px(px(6.))
                                                .py(px(1.))
                                                .rounded_md()
                                                .bg(self.theme.soft())
                                                .text_xs()
                                                .text_color(self.theme.muted())
                                                .child(label),
                                        )
                                    }),
                            )
                            .when_some(subtitle_node, |col, sub| {
                                col.child(
                                    div()
                                        .w_full()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .child(sub),
                                )
                            }),
                    ),
                );
            }
        }

        let dialog = ui::dialog(
            "quick-search-card",
            self.t("titlebar.search"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(560.))
        .max_w(frame.width)
        .max_h(frame.height)
        .bg(self.theme.canvas())
        .rounded(self.theme.modal_radius())
        .border_1()
        .border_color(self.theme.line())
        .shadow_md()
        .flex()
        .flex_col()
        .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
        .on_action(cx.listener(
            |app, _: &flowy_studio::SubmitComposer, _, cx| {
                app.confirm_quick_search(cx);
            },
        ))
        .child(
            div()
                .w_full()
                .px_3()
                .py(px(10.))
                .border_b_1()
                .border_color(self.theme.line())
                .flex()
                .items_center()
                .gap_2()
                .child(
                    ui::icons::search()
                        .size(px(16.))
                        .text_color(self.theme.muted()),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        ui::TextField::new(
                            "quick-search-input",
                            self.t("titlebar.search"),
                            self.quick_search.query.clone(),
                        )
                        .placeholder(self.t("search.placeholder"))
                        .quiet()
                        .render(&self.theme, cx),
                    ),
                )
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(self.theme.soft())
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child("Esc"),
                ),
        )
        .child(scope_bar)
        .child(list)
        .child(
            div()
                .w_full()
                .px_3()
                .py(px(8.))
                .border_t_1()
                .border_color(self.theme.line())
                .flex()
                .items_center()
                .justify_between()
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t("search.footer_hint"))
                .child("Ctrl+K"),
        );

        ui::modal_layer("quick-search-layer", &self.theme, &self.ui, dialog)
    }
}

fn search_row_icon(
    target: &SearchTarget,
    theme: ui::ResolvedTheme,
) -> AnyElement {
    let icon = match target {
        SearchTarget::Session(_) => ui::icons::list_check(),
        SearchTarget::Artifact(_) => ui::icons::file_text(),
        SearchTarget::Project(_) => ui::icons::folder(),
        SearchTarget::Skill => ui::icons::sparkles(),
        SearchTarget::NavigateBoard => ui::icons::layout_grid(),
        SearchTarget::NavigateLibrary => ui::icons::library(),
        SearchTarget::NewTask => ui::icons::square_pen(),
    };
    icon.size(px(15.))
        .text_color(theme.muted())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{SearchScope, highlight_byte_ranges, score_fields};

    #[test]
    fn score_fields_prefers_exact_and_prefix_over_cross_field() {
        let exact = score_fields("周报", &["周报", "摘要"]);
        let prefix = score_fields("周报", &["周报汇总", "摘要"]);
        let body = score_fields("周报", &["日常任务", "本周周报汇总"]);
        let cross = score_fields("销售 deck", &["Q3 销售", "report_deck.pptx"]);
        assert!(exact > prefix);
        assert!(prefix > body);
        assert!(body > cross);
        assert!(cross > 0);
        assert_eq!(score_fields("不存在", &["周报", "摘要"]), 0);
    }

    #[test]
    fn highlight_byte_ranges_handles_cjk_unicode_case_expansion_and_overlaps() {
        // CJK + ASCII mixed case
        let text = "Q3 销售周报 Revenue_Deck.pptx";
        let ranges = highlight_byte_ranges(text, "周报 deck");
        let matched: Vec<&str> =
            ranges.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(matched, vec!["周报", "Deck"]);

        // Overlapping tokens merge into one contiguous range.
        let overlap = "abcdef";
        let merged = highlight_byte_ranges(overlap, "abc bcd");
        assert_eq!(merged, vec![0..4]);

        // Unicode case-expansion (`İ` U+0130 -> `i` + combining dot) & emoji
        // must always produce valid UTF-8 char boundaries without panicking.
        let turkish = "🚀 İstanbul report";
        let tr_ranges = highlight_byte_ranges(turkish, "report");
        assert_eq!(tr_ranges.len(), 1);
        assert_eq!(&turkish[tr_ranges[0].clone()], "report");
        for r in highlight_byte_ranges(turkish, "i") {
            assert!(turkish.is_char_boundary(r.start));
            assert!(turkish.is_char_boundary(r.end));
        }

        assert_eq!(
            highlight_byte_ranges("", "abc"),
            Vec::<std::ops::Range<usize>>::new()
        );
        assert_eq!(
            highlight_byte_ranges("abc", "   "),
            Vec::<std::ops::Range<usize>>::new()
        );
    }

    #[test]
    fn search_scope_step_wraps_forward_and_backward() {
        assert_eq!(SearchScope::All.step(1), SearchScope::Sessions);
        assert_eq!(SearchScope::Skills.step(1), SearchScope::All);
        assert_eq!(SearchScope::All.step(-1), SearchScope::Skills);
        assert_eq!(SearchScope::Library.step(-1), SearchScope::Sessions);
    }
}
