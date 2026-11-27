//! Responsive Master-Detail Mission Control Progress Board (`StudioSurface::Board`).
//!
//! Layout contract (Direction C):
//! - Compact two-row top control bar: interactive status/today pills + blocker CTA +
//!   new task on Row 1; project filters + clickable format filters (`PDF`, `MD`,
//!   `DOCX`, `XLSX`, `PPTX`, `CODE`) on Row 2.
//! - Responsive split measured via `PrepaintBounds` (never stale `viewport_size()`):
//!   * `>= 920px`: always-on Master-Detail split (left action-first queue +
//!     right persistent 3-tab Inspector).
//!   * `< 920px`: full-width action queue; selecting any session opens a full-height
//!     right slide drawer with sticky bottom approval controls.

mod inspector;
mod trace_panel;

use flowy_ui as theme;
use flowy_ui::status_pill;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, Window, div,
    prelude::*, px,
};

use crate::panel::{StudioEvent, StudioPanel};
use crate::ports::{
    BoardKpiSummary, BranchPhase, DeliverableFormat, LocalDateKey,
    SessionBoardGroup, civil_month_day_in_tz, is_older_than_7d_in_tz,
    local_days_ago, local_tz_offset_secs, matches_day_filter,
    normalize_epoch_ms, now_epoch_ms,
};

pub use inspector::BoardInspectorTab;
pub use trace_panel::TraceFilter;

/// Minimum content width (excluding left dock sidebar) for side-by-side
/// Master-Detail rendering before adapting into a narrow slide-over drawer.
pub const BOARD_SPLIT_MIN_W: f32 = 920.0;

/// Top-level session filter on the Progress Board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardFilter {
    #[default]
    All,
    Approval,
    Running,
    Delivered,
    Archived,
}

/// Pure helper deciding whether to show Master-Detail side-by-side or narrow
/// list + slide drawer from the measured board width.
#[must_use]
pub fn is_wide_board_layout(board_width_px: f32) -> bool {
    board_width_px >= BOARD_SPLIT_MIN_W
}

/// Pure filter predicate shared by Board UI rendering and unit tests.
#[must_use]
pub fn matches_board_filters(
    group: &SessionBoardGroup,
    status_filter: BoardFilter,
    project_filter: Option<&str>,
    format_filter: Option<DeliverableFormat>,
    day_filter: Option<LocalDateKey>,
    now_ms: u64,
    tz_offset_secs: i32,
) -> bool {
    if project_filter
        .is_some_and(|proj| group.project_id.as_deref() != Some(proj))
    {
        return false;
    }
    let status_ok = match status_filter {
        BoardFilter::Archived => group.archived,
        BoardFilter::All => !group.archived,
        BoardFilter::Approval => !group.archived && group.has_blocker,
        BoardFilter::Running => !group.archived && group.is_running,
        BoardFilter::Delivered => group.is_delivered_settled(),
    };
    if !status_ok {
        return false;
    }
    if let Some(fmt) = format_filter
        && !group.has_formal_format(fmt)
    {
        return false;
    }
    if status_filter != BoardFilter::Archived
        && let Some(day_key) = day_filter
        && !matches_day_filter(
            group.updated_at,
            now_ms,
            tz_offset_secs,
            day_key,
        )
    {
        return false;
    }
    true
}

fn trim_ellipsis(text: &str) -> &str {
    text.trim()
        .trim_end_matches('\u{2026}')
        .trim_end_matches("...")
        .trim()
}

/// Detects when a running branch's fallback caption merely echoes the parent
/// session title (including `clip_label`'s trailing `…` or `...`).
fn duplicates_session_title(title: &str, caption: &str) -> bool {
    let title = trim_ellipsis(title);
    let caption = trim_ellipsis(caption);
    if title.is_empty() || caption.is_empty() {
        return false;
    }
    title.starts_with(caption) || caption.starts_with(title)
}

impl StudioPanel {
    pub(super) fn format_color(&self, format: DeliverableFormat) -> gpui::Rgba {
        match format {
            DeliverableFormat::Sheet => self.theme.file_kind_sheet(),
            DeliverableFormat::Slides => self.theme.file_kind_slides(),
            DeliverableFormat::Doc => self.theme.file_kind_doc(),
            DeliverableFormat::Pdf => self.theme.file_kind_pdf(),
            DeliverableFormat::Markdown => self.theme.file_kind_markdown(),
            DeliverableFormat::CodeWeb => self.theme.file_kind_code(),
            DeliverableFormat::Other => self.theme.file_kind_other(),
        }
    }

    pub(super) fn format_tag_label(&self, format: DeliverableFormat) -> String {
        match format {
            DeliverableFormat::Pdf => self.t("board.format_pdf"),
            DeliverableFormat::Markdown => self.t("board.format_md"),
            DeliverableFormat::CodeWeb => self.t("deliverables.kind_code"),
            DeliverableFormat::Other => self.t("deliverables.kind_file"),
            other => other.tag().to_owned(),
        }
    }

    pub(super) fn format_relative_time(
        &self,
        updated_at: u64,
        now_ms: u64,
        tz_offset_secs: i32,
    ) -> String {
        let ts = normalize_epoch_ms(updated_at);
        if ts == 0 {
            return String::new();
        }
        let age_secs = now_ms.saturating_sub(ts) / 1_000;
        if age_secs < 60 {
            self.t("time.just_now")
        } else if age_secs < 3_600 {
            let mins = (age_secs / 60).max(1);
            self.tf("time.mins_ago", &[("n", &mins.to_string())])
        } else if age_secs < 86_400 {
            let hours = (age_secs / 3_600).max(1);
            self.tf("time.hours_ago", &[("n", &hours.to_string())])
        } else if age_secs < 30 * 86_400 {
            let days = local_days_ago(ts, now_ms, tz_offset_secs)
                .filter(|d| *d > 0)
                .map_or_else(|| (age_secs / 86_400).max(1), i64::cast_unsigned);
            self.tf("time.days_ago", &[("n", &days.to_string())])
        } else {
            let (month, day) = civil_month_day_in_tz(ts, tz_offset_secs);
            format!("{month}/{day}")
        }
    }

    /// Global Visual Board surface (`StudioSurface::Board`).
    pub(crate) fn board_canvas(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let snapshot = &self.board_snapshot;
        let now_ms = if snapshot.generated_at_ms > 0 {
            snapshot.generated_at_ms
        } else {
            now_epoch_ms()
        };
        let tz_offset_secs = if snapshot.generated_at_ms > 0 {
            snapshot.tz_offset_secs
        } else {
            local_tz_offset_secs()
        };
        let project_filtered: Vec<&SessionBoardGroup> = snapshot
            .sessions
            .iter()
            .filter(|group| {
                self.board_project_filter.as_deref().is_none_or(|proj| {
                    group.project_id.as_deref() == Some(proj)
                })
            })
            .collect();
        let scoped_kpi = if self.board_project_filter.is_some() {
            BoardKpiSummary::from_groups_in_tz(
                &project_filtered,
                now_ms,
                tz_offset_secs,
            )
        } else {
            snapshot.kpi.clone()
        };
        let today_key = snapshot.daily_7d.last().map(|b| b.date_key);
        let active_day_filter = if self.board_filter == BoardFilter::Archived {
            None
        } else {
            self.board_day_filter
        };
        let filtered: Vec<&SessionBoardGroup> = project_filtered
            .iter()
            .copied()
            .filter(|group| {
                matches_board_filters(
                    group,
                    self.board_filter,
                    self.board_project_filter.as_deref(),
                    self.board_format_filter,
                    active_day_filter,
                    now_ms,
                    tz_offset_secs,
                )
            })
            .collect();

        let is_wide = is_wide_board_layout(self.board_width_px);
        let queue_list = self.board_action_queue(
            &filtered,
            active_day_filter,
            now_ms,
            tz_offset_secs,
            !is_wide,
            cx,
        );

        let body: AnyElement = if is_wide {
            let queue_width = if self.board_width_px >= 1220.0 {
                px(430.)
            } else {
                px(380.)
            };
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_row()
                .gap(self.theme.space(14.))
                .child(
                    theme::scroll_area("board-scroll", &self.theme)
                        .a11y_label(self.t("a11y.scroll_board_queue"))
                        .w(queue_width)
                        .flex_none()
                        .h_full()
                        .min_h_0()
                        .viewport_style({
                            let pr = self.theme.space(4.);
                            move |v| v.pr(pr)
                        })
                        .child(queue_list),
                )
                .child(self.board_inspector_panel(
                    false,
                    now_ms,
                    tz_offset_secs,
                    window,
                    cx,
                ))
                .into_any_element()
        } else if self.has_board_drawer() {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_row()
                .child(
                    theme::interactive(
                        "board-drawer-scrim",
                        self.t("board.esc_hint"),
                        &self.theme,
                    )
                    .w(px(56.))
                    .flex_none()
                    .h_full()
                    .cursor_pointer()
                    .bg(self.theme.soft().opacity(0.45))
                    .hover(|s| s.bg(self.theme.soft().opacity(0.7)))
                    .on_click(cx.listener(
                        |panel, _, _, cx| {
                            panel.close_board_drawer(cx);
                        },
                    )),
                )
                .child(self.board_inspector_panel(
                    true,
                    now_ms,
                    tz_offset_secs,
                    window,
                    cx,
                ))
                .into_any_element()
        } else {
            theme::scroll_area("board-scroll", &self.theme)
                .a11y_label(self.t("a11y.scroll_board_queue"))
                .flex_1()
                .min_w_0()
                .h_full()
                .min_h_0()
                .child(queue_list)
                .into_any_element()
        };

        let weak_panel = cx.entity().downgrade();
        let root = div()
            .id("board-root")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .px(self.theme.space(20.))
            .py(self.theme.space(14.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(self.board_top_controls(
                &scoped_kpi,
                today_key,
                active_day_filter,
                !is_wide,
                cx,
            ))
            .child(body);

        theme::PrepaintBounds::new(root, move |bounds, cx| {
            let next_w = f32::from(bounds.size.width);
            if next_w >= 200.0 {
                let _ = weak_panel.update(cx, |panel, cx| {
                    if is_wide_board_layout(next_w)
                        && panel.board_inspector.drawer_open
                    {
                        panel.board_inspector.close_drawer();
                    }
                    if (panel.board_width_px - next_w).abs() > 2.0 {
                        panel.board_width_px = next_w;
                        cx.notify();
                    }
                });
            }
        })
        .into_any_element()
    }

    fn board_top_controls(
        &self,
        kpi: &BoardKpiSummary,
        today_key: Option<LocalDateKey>,
        active_day_filter: Option<LocalDateKey>,
        narrow_layout: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let row1 = self.board_status_strip(kpi, today_key, narrow_layout, cx);
        let row2 =
            self.board_secondary_filters(kpi, today_key, active_day_filter, cx);

        div()
            .w_full()
            .flex_none()
            .p(self.theme.space(10.))
            .rounded_2xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(row1)
            .child(row2)
            .into_any_element()
    }

    fn board_status_strip(
        &self,
        kpi: &BoardKpiSummary,
        today_key: Option<LocalDateKey>,
        narrow_layout: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut status_pills = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(self.theme.space(6.));

        let all_label = self.tf(
            "board.filter_all",
            &[("count", &kpi.total_sessions.to_string())],
        );
        let blocker_label = if kpi.blocker_sessions > 0 {
            self.tf(
                "board.status_blockers",
                &[("count", &kpi.blocker_sessions.to_string())],
            )
        } else {
            self.t("board.filter_approval")
        };
        let running_label = if kpi.running_sessions > 0 {
            self.tf(
                "board.status_running",
                &[("count", &kpi.running_sessions.to_string())],
            )
        } else {
            self.t("board.filter_running")
        };

        let mut filters = vec![
            (BoardFilter::All, "bf-all", all_label, None),
            (
                BoardFilter::Approval,
                "bf-approval",
                blocker_label,
                (kpi.blocker_sessions > 0)
                    .then(|| self.theme.status_attention()),
            ),
            (
                BoardFilter::Running,
                "bf-running",
                running_label,
                (kpi.running_sessions > 0).then(|| self.theme.status_active()),
            ),
            (
                BoardFilter::Delivered,
                "bf-delivered",
                self.t("board.filter_delivered"),
                None,
            ),
        ];
        if kpi.archived_sessions > 0
            || self.board_filter == BoardFilter::Archived
        {
            filters.push((
                BoardFilter::Archived,
                "bf-archived",
                self.tf(
                    "board.filter_archived",
                    &[("count", &kpi.archived_sessions.to_string())],
                ),
                None,
            ));
        }

        for (filter, id, label, dot) in filters {
            let selected = self.board_filter == filter;
            status_pills = status_pills.child(
                theme::interactive(id, label.clone(), &self.theme)
                    .px(self.theme.space(10.))
                    .py(px(4.))
                    .rounded_full()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_xs()
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .bg(if selected {
                        self.theme.ink()
                    } else {
                        self.theme.soft()
                    })
                    .text_color(if selected {
                        self.theme.on_ink()
                    } else {
                        self.theme.ink()
                    })
                    .when_some(dot, |pill, color| {
                        pill.child(div().size(px(6.)).rounded_full().bg(
                            if selected { self.theme.on_ink() } else { color },
                        ))
                    })
                    .child(label)
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        if filter == BoardFilter::All
                            && panel.board_filter == BoardFilter::All
                        {
                            panel.board_project_filter = None;
                            panel.board_day_filter = None;
                            panel.board_format_filter = None;
                        } else if filter == BoardFilter::Archived {
                            panel.board_day_filter = None;
                        }
                        panel.board_filter = filter;
                        cx.notify();
                    })),
            );
        }

        if let Some(today) = today_key {
            let active = self.board_day_filter == Some(today);
            let today_label = self.tf(
                "board.status_today_done",
                &[("count", &kpi.today_completed_sessions.to_string())],
            );
            status_pills = status_pills.child(
                theme::interactive(
                    "bf-today-pill",
                    today_label.clone(),
                    &self.theme,
                )
                .px(self.theme.space(10.))
                .py(px(4.))
                .rounded_full()
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_xs()
                .bg(if active {
                    self.theme.status_active_bg()
                } else {
                    self.theme.soft()
                })
                .text_color(if active {
                    self.theme.status_active()
                } else {
                    self.theme.muted()
                })
                .child(div().size(px(6.)).rounded_full().bg(
                    if kpi.today_completed_sessions > 0 || active {
                        self.theme.status_verified()
                    } else {
                        self.theme.muted().opacity(0.45)
                    },
                ))
                .child(today_label)
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        if panel.board_day_filter == Some(today) {
                            panel.board_day_filter = None;
                        } else {
                            if panel.board_filter == BoardFilter::Archived {
                                panel.board_filter = BoardFilter::All;
                            }
                            panel.board_day_filter = Some(today);
                        }
                        cx.notify();
                    },
                )),
            );
        }

        let review_cta = (kpi.blocker_sessions > 0).then(|| {
            let label = self.tf(
                "board.cta_review_short",
                &[("count", &kpi.blocker_sessions.to_string())],
            );
            theme::Button::new("board-jump-review", label)
                .variant(theme::ControlVariant::Primary)
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(move |panel, _, _, cx| {
                        panel.jump_to_first_blocker(narrow_layout, cx);
                    }),
                )
        });

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(status_pills)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .children(review_cta)
                    .child(
                        theme::Button::new(
                            "board-new-task",
                            self.t("dock.new_task"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.new_task(cx);
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn board_secondary_filters(
        &self,
        kpi: &BoardKpiSummary,
        today_key: Option<LocalDateKey>,
        active_day_filter: Option<LocalDateKey>,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut row = div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(self.theme.space(6.));

        if !self.projects.is_empty() {
            let mut visible_projects: Vec<&crate::projects::Project> =
                self.projects.iter().take(4).collect();
            if let Some(active_pid) = self.board_project_filter.as_deref()
                && !visible_projects.iter().any(|p| p.id == active_pid)
                && let Some(active_proj) =
                    self.projects.iter().find(|p| p.id == active_pid)
            {
                visible_projects.push(active_proj);
            }
            for (p_idx, project) in visible_projects.into_iter().enumerate() {
                let pid = project.id.clone();
                let active = self.board_project_filter.as_deref()
                    == Some(project.id.as_str());
                let display_label = if active {
                    format!("{} \u{00d7}", project.name)
                } else {
                    project.name.clone()
                };
                let a11y_label = self.tf(
                    "board.filter_project_a11y",
                    &[("name", &project.name)],
                );
                row = row.child(
                    theme::interactive(
                        ("board-proj-filter", p_idx as u64),
                        a11y_label,
                        &self.theme,
                    )
                    .max_w(px(150.))
                    .px(self.theme.space(8.))
                    .py(px(3.))
                    .rounded_full()
                    .cursor_pointer()
                    .text_xs()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .bg(if active {
                        self.theme.status_active_bg()
                    } else {
                        self.theme.soft()
                    })
                    .text_color(if active {
                        self.theme.status_active()
                    } else {
                        self.theme.muted()
                    })
                    .child(display_label)
                    .on_click(cx.listener(
                        move |panel, _, _, cx| {
                            if panel.board_project_filter.as_deref()
                                == Some(pid.as_str())
                            {
                                panel.board_project_filter = None;
                            } else {
                                panel.board_project_filter = Some(pid.clone());
                            }
                            cx.notify();
                        },
                    )),
                );
            }
            row = row.child(
                div().w(px(1.)).h(px(14.)).mx(px(2.)).bg(self.theme.line()),
            );
        }

        if let Some(day_key) = active_day_filter {
            let day_label = if today_key == Some(day_key) {
                self.t("board.kpi_today")
            } else {
                format!("{}/{}", day_key.1, day_key.2)
            };
            row = row.child(
                theme::interactive(
                    "board-day-filter-chip",
                    self.t("board.clear_day_filter"),
                    &self.theme,
                )
                .px(self.theme.space(8.))
                .py(px(3.))
                .rounded_full()
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .bg(self.theme.status_active_bg())
                .text_color(self.theme.status_active())
                .child(format!("{day_label} \u{00d7}"))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.board_day_filter = None;
                    cx.notify();
                })),
            );
        }

        let format_counts = [
            (DeliverableFormat::Sheet, kpi.sheet_count),
            (DeliverableFormat::Slides, kpi.slides_count),
            (DeliverableFormat::Doc, kpi.doc_count),
            (DeliverableFormat::Pdf, kpi.pdf_count),
            (DeliverableFormat::Markdown, kpi.markdown_count),
            (DeliverableFormat::CodeWeb, kpi.code_count),
            (DeliverableFormat::Other, kpi.other_count),
        ];
        let any_formats = format_counts.iter().any(|(_, c)| *c > 0);
        if any_formats || self.board_format_filter.is_some() {
            let all_fmt_active = self.board_format_filter.is_none();
            let all_fmt_label = self.t("board.format_all");
            row = row.child(
                theme::interactive(
                    "bf-fmt-all",
                    all_fmt_label.clone(),
                    &self.theme,
                )
                .px(self.theme.space(8.))
                .py(px(3.))
                .rounded_full()
                .cursor_pointer()
                .text_xs()
                .bg(if all_fmt_active {
                    self.theme.ink()
                } else {
                    self.theme.soft()
                })
                .text_color(if all_fmt_active {
                    self.theme.on_ink()
                } else {
                    self.theme.muted()
                })
                .child(all_fmt_label)
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.board_format_filter = None;
                    cx.notify();
                })),
            );
            for (ix, (fmt, count)) in format_counts.into_iter().enumerate() {
                let active = self.board_format_filter == Some(fmt);
                if count == 0 && !active {
                    continue;
                }
                let tag = self.format_tag_label(fmt);
                let label = format!("{tag} {count}");
                let color = self.format_color(fmt);
                row = row.child(
                    theme::interactive(
                        ("bf-fmt", ix as u64),
                        label.clone(),
                        &self.theme,
                    )
                    .px(self.theme.space(8.))
                    .py(px(3.))
                    .rounded_full()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_xs()
                    .bg(if active {
                        self.theme.ink()
                    } else {
                        self.theme.soft()
                    })
                    .text_color(if active {
                        self.theme.on_ink()
                    } else {
                        self.theme.ink()
                    })
                    .child(div().size(px(6.)).rounded_full().bg(color))
                    .child(label)
                    .on_click(cx.listener(
                        move |panel, _, _, cx| {
                            if panel.board_format_filter == Some(fmt) {
                                panel.board_format_filter = None;
                            } else {
                                panel.board_format_filter = Some(fmt);
                            }
                            cx.notify();
                        },
                    )),
                );
            }
        }

        row.into_any_element()
    }

    fn board_action_queue(
        &self,
        filtered: &[&SessionBoardGroup],
        active_day_filter: Option<LocalDateKey>,
        now_ms: u64,
        tz_offset_secs: i32,
        narrow_layout: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut sessions_col =
            div().w_full().flex().flex_col().gap(self.theme.space(8.));

        if filtered.is_empty() {
            return sessions_col
                .child(self.board_empty_card(active_day_filter, cx))
                .into_any_element();
        }

        if self.board_filter == BoardFilter::Archived {
            for (card_idx, group) in filtered.iter().enumerate() {
                sessions_col = sessions_col.child(self.board_session_card(
                    group,
                    card_idx as u64,
                    now_ms,
                    tz_offset_secs,
                    narrow_layout,
                    cx,
                ));
            }
            return sessions_col.into_any_element();
        }

        let blockers: Vec<&SessionBoardGroup> =
            filtered.iter().copied().filter(|g| g.has_blocker).collect();
        let running: Vec<&SessionBoardGroup> = filtered
            .iter()
            .copied()
            .filter(|g| !g.has_blocker && g.is_running)
            .collect();
        let recent_settled: Vec<&SessionBoardGroup> = filtered
            .iter()
            .copied()
            .filter(|g| {
                !g.is_active
                    && (active_day_filter.is_some()
                        || g.pinned
                        || !is_older_than_7d_in_tz(
                            g.updated_at,
                            now_ms,
                            tz_offset_secs,
                        ))
            })
            .collect();
        let older_settled: Vec<&SessionBoardGroup> = filtered
            .iter()
            .copied()
            .filter(|g| {
                active_day_filter.is_none()
                    && !g.is_active
                    && !g.pinned
                    && is_older_than_7d_in_tz(
                        g.updated_at,
                        now_ms,
                        tz_offset_secs,
                    )
            })
            .collect();

        let mut card_idx: u64 = 0;
        if !blockers.is_empty() {
            sessions_col = sessions_col.child(self.queue_section_heading(
                self.tf(
                    "board.group_blockers",
                    &[("count", &blockers.len().to_string())],
                ),
                Some(self.theme.status_attention()),
            ));
            for group in blockers {
                sessions_col = sessions_col.child(self.board_session_card(
                    group,
                    card_idx,
                    now_ms,
                    tz_offset_secs,
                    narrow_layout,
                    cx,
                ));
                card_idx += 1;
            }
        }

        if !running.is_empty() {
            sessions_col = sessions_col.child(self.queue_section_heading(
                self.tf(
                    "board.group_running",
                    &[("count", &running.len().to_string())],
                ),
                Some(self.theme.status_active()),
            ));
            for group in running {
                sessions_col = sessions_col.child(self.board_session_card(
                    group,
                    card_idx,
                    now_ms,
                    tz_offset_secs,
                    narrow_layout,
                    cx,
                ));
                card_idx += 1;
            }
        }

        if !recent_settled.is_empty() {
            sessions_col = sessions_col.child(self.queue_section_heading(
                self.tf(
                    "board.group_settled",
                    &[("count", &recent_settled.len().to_string())],
                ),
                None,
            ));
            for group in recent_settled {
                sessions_col = sessions_col.child(self.board_session_card(
                    group,
                    card_idx,
                    now_ms,
                    tz_offset_secs,
                    narrow_layout,
                    cx,
                ));
                card_idx += 1;
            }
        }

        if !older_settled.is_empty() {
            let older_label = self.tf(
                "board.group_older",
                &[("count", &older_settled.len().to_string())],
            );
            let chevron = if self.board_older_expanded {
                theme::icons::chevron_down()
            } else {
                theme::icons::chevron_right()
            };
            sessions_col = sessions_col.child(
                theme::interactive(
                    "board-older-divider",
                    older_label.clone(),
                    &self.theme,
                )
                .w_full()
                .mt(self.theme.space(4.))
                .px(self.theme.space(8.))
                .py(px(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.board_older_expanded = !panel.board_older_expanded;
                    cx.notify();
                }))
                .child(chevron.size(px(13.)).text_color(self.theme.muted()))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.muted())
                        .whitespace_nowrap()
                        .child(older_label),
                )
                .child(div().flex_1().h(px(1.)).bg(self.theme.line())),
            );
            if self.board_older_expanded {
                for group in older_settled {
                    sessions_col = sessions_col.child(self.board_session_card(
                        group,
                        card_idx,
                        now_ms,
                        tz_offset_secs,
                        narrow_layout,
                        cx,
                    ));
                    card_idx += 1;
                }
            }
        }

        sessions_col.into_any_element()
    }

    fn queue_section_heading(
        &self,
        label: String,
        tint: Option<gpui::Rgba>,
    ) -> AnyElement {
        div()
            .pt(px(2.))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tint.unwrap_or_else(|| self.theme.muted()))
            .child(label)
            .into_any_element()
    }

    fn board_empty_card(
        &self,
        active_day_filter: Option<LocalDateKey>,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let has_any_filter = active_day_filter.is_some()
            || self.board_format_filter.is_some()
            || self.board_project_filter.is_some()
            || self.board_filter != BoardFilter::All;

        div()
            .w_full()
            .py(px(40.))
            .px(self.theme.space(16.))
            .rounded_2xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .items_center()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.ink())
                    .child(self.t("board.empty_title")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("board.empty_detail")),
            )
            .when(has_any_filter, |empty| {
                empty.child(
                    div().pt(px(4.)).child(
                        theme::Button::new(
                            "board-clear-filters",
                            self.t("board.clear_day_filter"),
                        )
                        .variant(theme::ControlVariant::Secondary)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.board_filter = BoardFilter::All;
                                panel.board_day_filter = None;
                                panel.board_format_filter = None;
                                panel.board_project_filter = None;
                                cx.notify();
                            }),
                        ),
                    ),
                )
            })
            .into_any_element()
    }

    fn board_row_actions(
        &self,
        session_id: &str,
        row_title: &str,
        index: u64,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let sid_chat = session_id.to_owned();
        let sid_menu = session_id.to_owned();
        let open_chat_label =
            format!("{row_title} \u{00b7} {}", self.t("board.open_chat"));
        let more_label =
            format!("{row_title} \u{00b7} {}", self.t("library.more"));

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                theme::interactive(
                    ("board-row-chat", index),
                    open_chat_label,
                    &self.theme,
                )
                .size(px(24.))
                .rounded_md()
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_color(self.theme.muted())
                .hover(|s| {
                    s.bg(self.theme.dock_hover()).text_color(self.theme.ink())
                })
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.close_board_drawer(cx);
                    panel.open_session(&sid_chat, cx);
                }))
                .child(theme::icons::arrow_up_right().size(px(13.))),
            )
            .child({
                let anchor_id = format!("board-row-menu-{sid_menu}");
                let anchor_emit = anchor_id.clone();
                theme::anchor_trigger(
                    theme::interactive(
                        ("board-row-menu", index),
                        more_label,
                        &self.theme,
                    )
                    .size(px(24.))
                    .rounded_md()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(self.theme.muted())
                    .hover(|s| {
                        s.bg(self.theme.dock_hover())
                            .text_color(self.theme.ink())
                    })
                    .on_click(cx.listener(move |_panel, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(StudioEvent::ToggleSessionMenu {
                            session_id: sid_menu.clone(),
                            anchor_id: anchor_emit.clone(),
                        });
                    }))
                    .child(theme::icons::ellipsis().size(px(13.))),
                    anchor_id,
                    &self.ui,
                )
            })
            .into_any_element()
    }

    fn board_session_card(
        &self,
        group: &SessionBoardGroup,
        index: u64,
        now_ms: u64,
        tz_offset_secs: i32,
        narrow_layout: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let sid_card = group.session_id.clone();
        let sid_right = group.session_id.clone();
        let is_real_session = !group.is_synthetic();
        let is_selected = self.board_inspector.session.as_deref()
            == Some(group.session_id.as_str());
        let rel_time =
            self.format_relative_time(group.updated_at, now_ms, tz_offset_secs);

        let a11y_label = self.tf("board.card_a11y", &[("title", &group.title)]);

        let formal_files: Vec<_> = group
            .branches
            .iter()
            .filter(|b| b.deliverable_path.is_some())
            .collect();
        let shown_files: Vec<_> =
            formal_files.iter().copied().take(3).collect();
        let overflow_files =
            formal_files.len().saturating_sub(shown_files.len());

        let mut file_chips =
            div().flex().flex_wrap().items_center().gap(px(5.));
        for branch in &shown_files {
            let color = self.format_color(branch.format);
            let tag = self.format_tag_label(branch.format);
            file_chips = file_chips.child(
                div()
                    .max_w(px(190.))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_xs()
                    .text_color(self.theme.ink())
                    .child(
                        div()
                            .flex_none()
                            .px(px(4.))
                            .rounded_sm()
                            .bg(color)
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(self.theme.on_ink())
                            .child(tag),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(branch.deliverable_name.clone()),
                    ),
            );
        }
        if overflow_files > 0 {
            file_chips = file_chips.child(
                div()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(format!("+{overflow_files}")),
            );
        }

        let live_hint = group.branches.iter().find_map(|b| match &b.phase {
            BranchPhase::NeedsApproval { summary, .. } => Some(summary.clone()),
            BranchPhase::DeliveryFailed => {
                Some(self.t("chat.status_delivery_failed"))
            }
            BranchPhase::Running { step_label, .. } => {
                let caption = if !step_label.is_empty() {
                    step_label.clone()
                } else if !duplicates_session_title(
                    &group.title,
                    &b.deliverable_name,
                ) {
                    b.deliverable_name.clone()
                } else {
                    self.t("chat.thinking")
                };
                Some(caption)
            }
            BranchPhase::Queued { state, .. } => {
                let (_, label, _) = self.receipt_state_visual(*state);
                Some(label)
            }
            BranchPhase::Delivered { .. } | BranchPhase::Idle => None,
        });

        let summary_line = if shown_files.is_empty() {
            live_hint.or_else(|| group.preview.clone())
        } else {
            None
        };

        let status_dot_color = if group.has_blocker {
            self.theme.status_attention()
        } else if group.is_running {
            self.theme.status_active()
        } else if !formal_files.is_empty() {
            self.theme.status_verified()
        } else {
            self.theme.muted().opacity(0.5)
        };

        let border_color = if is_selected {
            self.theme.accent()
        } else if group.has_blocker {
            self.theme.status_attention().opacity(0.45)
        } else if group.is_running {
            self.theme.status_active().opacity(0.4)
        } else {
            self.theme.line()
        };

        let mut card = theme::interactive(
            ("board-session-card", index),
            a11y_label,
            &self.theme,
        )
        .w_full()
        .p(self.theme.space(12.))
        .rounded_xl()
        .border_1()
        .border_color(border_color)
        .bg(if is_selected {
            self.theme.soft().opacity(0.65)
        } else {
            self.theme.card()
        })
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft().opacity(0.6)))
        .flex()
        .flex_col()
        .gap(self.theme.space(6.))
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.select_board_session(
                sid_card.clone(),
                None,
                narrow_layout,
                cx,
            );
        }));

        if is_real_session {
            card = card.on_mouse_down(
                MouseButton::Right,
                cx.listener(
                    move |_panel, event: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(StudioEvent::OpenSessionMenu {
                            session_id: sid_right.clone(),
                            position: event.position,
                        });
                    },
                ),
            );
        }

        card.child(
            div()
                .w_full()
                .flex()
                .items_start()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap(self.theme.space(7.))
                        .child(
                            div()
                                .mt(px(5.))
                                .size(px(7.))
                                .flex_none()
                                .rounded_full()
                                .bg(status_dot_color),
                        )
                        .when(group.pinned, |row| {
                            row.child(
                                theme::icons::pin()
                                    .size(px(12.))
                                    .flex_none()
                                    .mt(px(3.))
                                    .text_color(self.theme.accent()),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(self.theme.ink())
                                .whitespace_normal()
                                .child(group.title.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(6.))
                        .when(!rel_time.is_empty(), |right| {
                            right.child(
                                div()
                                    .text_xs()
                                    .text_color(
                                        self.theme.muted().opacity(0.85),
                                    )
                                    .whitespace_nowrap()
                                    .child(rel_time),
                            )
                        })
                        .when(is_real_session, |right| {
                            right.child(self.board_row_actions(
                                &group.session_id,
                                &group.title,
                                index,
                                cx,
                            ))
                        }),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(self.theme.space(6.))
                .when_some(group.project_name.clone(), |row, project| {
                    row.child(status_pill(
                        self.theme.soft(),
                        self.theme.muted(),
                        project,
                        None,
                    ))
                })
                .when(group.has_blocker, |row| {
                    row.child(status_pill(
                        self.theme.status_attention_bg(),
                        self.theme.status_attention(),
                        self.t("board.badge_approval"),
                        None,
                    ))
                })
                .when(!group.has_blocker && group.is_running, |row| {
                    row.child(status_pill(
                        self.theme.status_active_bg(),
                        self.theme.status_active(),
                        self.t("board.badge_running"),
                        None,
                    ))
                })
                .when(group.archived, |row| {
                    row.child(status_pill(
                        self.theme.soft(),
                        self.theme.muted(),
                        self.t("chat.archived_badge"),
                        None,
                    ))
                })
                .when(!group.child_workers.is_empty(), |row| {
                    row.child(status_pill(
                        self.theme.soft(),
                        self.theme.ink(),
                        self.tf(
                            "board.section_delegated_workers",
                            &[(
                                "count",
                                &group.child_workers.len().to_string(),
                            )],
                        ),
                        None,
                    ))
                }),
        )
        .when(!shown_files.is_empty(), |c| c.child(file_chips))
        .when_some(summary_line, |c, snippet| {
            c.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(snippet),
            )
        })
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{DeliverableFormat, SkillLabel, WorkBranchSummary};

    fn demo_group(id: &str) -> SessionBoardGroup {
        SessionBoardGroup {
            session_id: id.into(),
            title: "Demo".into(),
            project_id: None,
            project_name: None,
            workspace_display: String::new(),
            updated_at: 1_700_000_000_000,
            pinned: false,
            archived: false,
            preview: None,
            assistant_summary: None,
            branches: vec![WorkBranchSummary {
                id: "b1".into(),
                owner_session_id: None,
                skill_label: SkillLabel::Key("board.skill_work"),
                phase: BranchPhase::Delivered {
                    override_used: false,
                    hash_short: String::new(),
                    provenance_warnings: Vec::new(),
                },
                deliverable_name: "notes.md".into(),
                deliverable_path: Some("/tmp/notes.md".into()),
                format: DeliverableFormat::Markdown,
                is_workspace_change: false,
                progress_pct: 100.0,
                detail: String::new(),
                work_progress: None,
            }],
            workspace_changes: Vec::new(),
            child_workers: Vec::new(),
            total_branch_count: 1,
            total_file_count: 1,
            session_contract: None,
            has_blocker: false,
            has_delivered: true,
            is_running: false,
            is_active: false,
            group_kpi: BoardKpiSummary::default(),
        }
    }

    #[test]
    fn responsive_split_breakpoint_matches_contract() {
        assert!(!is_wide_board_layout(660.0));
        assert!(!is_wide_board_layout(840.0));
        assert!(is_wide_board_layout(920.0));
        assert!(is_wide_board_layout(1040.0));
        assert!(is_wide_board_layout(1440.0));
    }

    #[test]
    fn delivered_filter_excludes_blocked_and_running_groups() {
        let now = 1_700_000_000_000;
        let settled = demo_group("s-ok");
        assert!(matches_board_filters(
            &settled,
            BoardFilter::Delivered,
            None,
            None,
            None,
            now,
            0
        ));

        let mut blocked = demo_group("s-block");
        blocked.has_blocker = true;
        blocked.is_active = true;
        assert!(!matches_board_filters(
            &blocked,
            BoardFilter::Delivered,
            None,
            None,
            None,
            now,
            0
        ));

        let mut running = demo_group("s-run");
        running.is_running = true;
        running.is_active = true;
        assert!(!matches_board_filters(
            &running,
            BoardFilter::Delivered,
            None,
            None,
            None,
            now,
            0
        ));
    }

    #[test]
    fn format_filter_matches_formal_deliverables_only() {
        let now = 1_700_000_000_000;
        let group = demo_group("s-md");
        assert!(matches_board_filters(
            &group,
            BoardFilter::All,
            None,
            Some(DeliverableFormat::Markdown),
            None,
            now,
            0
        ));
        assert!(!matches_board_filters(
            &group,
            BoardFilter::All,
            None,
            Some(DeliverableFormat::Pdf),
            None,
            now,
            0
        ));
    }
}
