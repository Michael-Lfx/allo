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
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, Window,
    div, prelude::*, px,
};
use gpui_kit::base::input::InputState;

const MAX_SESSION_HITS: usize = 6;
const MAX_LIBRARY_HITS: usize = 5;
const MAX_PROJECT_HITS: usize = 4;
const MAX_SKILL_HITS: usize = 4;

pub(super) struct QuickSearchState {
    pub(super) query: Entity<InputState>,
    pub(super) selected: usize,
    pub(super) focus_pending: bool,
}

impl QuickSearchState {
    pub(super) const fn new(query: Entity<InputState>) -> Self {
        Self {
            query,
            selected: 0,
            focus_pending: false,
        }
    }
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.active_dialog == ActiveDialog::QuickSearch {
            self.active_dialog = ActiveDialog::None;
            cx.notify();
            return;
        }
        self.quick_search.selected = 0;
        self.quick_search.focus_pending = true;
        self.set_input_text(&self.quick_search.query, "", cx);
        self.activate_dialog(ActiveDialog::QuickSearch, cx);
        let focus = self.quick_search.query.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    pub(super) fn move_quick_search_selection(
        &mut self,
        delta: isize,
        cx: &mut Context<'_, Self>,
    ) {
        let count = self.quick_search_results(cx).len();
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
                self.select_more_page(super::more::Page::Skills, cx);
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
        let studio = self.studio.read(cx);
        let mut rows = Vec::new();

        if query.is_empty() {
            for hit in studio.agent.search_sessions("", 5) {
                let title = if hit.title.trim().is_empty() {
                    self.t("dock.new_task")
                } else {
                    hit.title
                };
                let badge = hit.pinned.then(|| self.t("chat.pinned_badge"));
                rows.push(QuickSearchRow {
                    section_key: "search.group_recent",
                    title,
                    subtitle: hit.snippet,
                    badge,
                    target: SearchTarget::Session(hit.session_id),
                    score: hit.score,
                });
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
            return rows;
        }

        // 1. Sessions (titles + body message snippets)
        for hit in studio.agent.search_sessions(&query, MAX_SESSION_HITS) {
            let title = if hit.title.trim().is_empty() {
                self.t("dock.new_task")
            } else {
                hit.title
            };
            let badge = if hit.archived {
                Some(self.t("chat.archived_badge"))
            } else if hit.pinned {
                Some(self.t("chat.pinned_badge"))
            } else {
                None
            };
            rows.push(QuickSearchRow {
                section_key: "search.group_sessions",
                title,
                subtitle: hit.snippet,
                badge,
                target: SearchTarget::Session(hit.session_id),
                score: hit.score,
            });
        }

        // 2. Library deliverables
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
                (score > 0).then(|| QuickSearchRow {
                    section_key: "search.group_library",
                    title: item.title.clone(),
                    subtitle: (!item.source_title.is_empty())
                        .then(|| item.source_title.clone()),
                    badge: (!item.extension.is_empty())
                        .then(|| item.extension.to_ascii_uppercase()),
                    target: SearchTarget::Artifact(item.id.clone()),
                    score,
                })
            })
            .collect();
        lib_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
        lib_hits.truncate(MAX_LIBRARY_HITS);
        rows.extend(lib_hits);

        // 3. Projects
        let mut proj_hits: Vec<QuickSearchRow> = studio
            .projects
            .iter()
            .filter_map(|proj| {
                let path_str = proj.root.to_string_lossy();
                let score =
                    score_fields(&query, &[proj.name.as_str(), &path_str]);
                (score > 0).then(|| QuickSearchRow {
                    section_key: "search.group_projects",
                    title: proj.name.clone(),
                    subtitle: Some(flowy_studio::ports::redact_trace_text(
                        &path_str,
                    )),
                    badge: proj.pinned.then(|| self.t("chat.pinned_badge")),
                    target: SearchTarget::Project(proj.id.clone()),
                    score,
                })
            })
            .collect();
        proj_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
        proj_hits.truncate(MAX_PROJECT_HITS);
        rows.extend(proj_hits);

        // 4. Installed skills
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
                (score > 0).then(|| QuickSearchRow {
                    section_key: "search.group_skills",
                    title: title.to_owned(),
                    subtitle: (!skill.description.trim().is_empty())
                        .then(|| skill.description.trim().to_owned()),
                    badge: Some(format!("/skill:{id_str}")),
                    target: SearchTarget::Skill,
                    score,
                })
            })
            .collect();
        skill_hits.sort_by_key(|a| std::cmp::Reverse(a.score));
        skill_hits.truncate(MAX_SKILL_HITS);
        rows.extend(skill_hits);

        rows
    }

    pub(super) fn quick_search_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        if self.quick_search.focus_pending {
            self.quick_search.focus_pending = false;
            let focus = self.quick_search.query.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
        let rows = self.quick_search_results(cx);
        let selected = if rows.is_empty() {
            0
        } else {
            self.quick_search.selected.min(rows.len() - 1)
        };
        self.quick_search.selected = selected;
        let frame = self.theme.modal_size(window.viewport_size());

        let mut list = div()
            .id("quick-search-list")
            .w_full()
            .max_h(px(360.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px_2()
            .py_2();

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
                let subtitle = row.subtitle;
                let badge = row.badge;
                list = list.child(
                    ui::interactive(
                        ("quick-search-row", idx as u64),
                        title.clone(),
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
                                            .child(title),
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
                            .when_some(subtitle, |col, sub| {
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
    use super::score_fields;

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
}
