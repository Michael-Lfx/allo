//! Responsive Global Visual Board (`StudioSurface::Board`).
//!
//! Submodules keep concerns separated without extra abstraction layers:
//! - `overview/mod.rs`: KPI strip, filter bar, 7-day fold, and de-noised session rows.
//! - `overview/flow_modal.rs`: centered top-level Flowchart Modal & branch actions.
//! - `overview/trace_panel.rs`: shared redacted Trace panel, contract progress
//!   card, and `ReceiptState` visuals.

mod flow_modal;
mod trace_panel;

use flowy_ui as theme;
use flowy_ui::charts::{DonutSegment, donut_chart};
use flowy_ui::status_pill;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, div, prelude::*,
    px,
};

use crate::panel::{StudioEvent, StudioPanel};
use crate::ports::{
    BoardKpiSummary, BranchPhase, DailyActivityBucket, DeliverableFormat,
    LocalDateKey, SessionBoardGroup, civil_month_day_in_tz,
    compute_daily_7d_in_tz, is_older_than_7d_in_tz, local_days_ago,
    local_tz_offset_secs, matches_day_filter, normalize_epoch_ms, now_epoch_ms,
};

pub use trace_panel::TraceFilter;

/// Top-level session filter on the Global Visual Board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardFilter {
    #[default]
    All,
    Approval,
    Running,
    Delivered,
    Archived,
}

/// Active tab inside the Flowchart Modal detail footer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlowModalTab {
    #[default]
    Action,
    Trace,
}

/// Detects when a running branch's fallback caption merely echoes the parent
/// session title (including `clip_label`'s trailing `…` or `...`).
fn duplicates_session_title(title: &str, caption: &str) -> bool {
    let title = title.trim();
    let trimmed = caption
        .trim()
        .trim_end_matches('\u{2026}')
        .trim_end_matches("...")
        .trim();
    !title.is_empty()
        && !trimmed.is_empty()
        && (title.starts_with(trimmed) || trimmed.starts_with(title))
}

/// Returns an honest percentage only when backed by a real delivery or a
/// concrete `WorkContract` deliverable denominator. Unbounded running/queued
/// turns return `None` so the UI never displays fabricated percentages.
pub(super) fn real_contract_progress_pct(
    branch: &crate::ports::WorkBranchSummary,
) -> Option<f32> {
    if matches!(branch.phase, BranchPhase::Delivered { .. }) {
        return Some(100.0);
    }
    let contract = branch.work_progress.as_ref()?;
    let total = contract.required_labels.len();
    if total == 0 {
        return None;
    }
    Some(
        ((contract.delivered_count.min(total) as f32 / total as f32) * 100.0)
            .clamp(0.0, 100.0),
    )
}

impl StudioPanel {
    pub(super) fn format_color(&self, format: DeliverableFormat) -> gpui::Rgba {
        match format {
            DeliverableFormat::Sheet => self.theme.file_kind_sheet(),
            DeliverableFormat::Slides => self.theme.file_kind_slides(),
            DeliverableFormat::Doc => self.theme.file_kind_doc(),
            DeliverableFormat::CodeWeb => self.theme.file_kind_code(),
            DeliverableFormat::Other => self.theme.file_kind_other(),
        }
    }

    pub(super) fn format_tag_label(&self, format: DeliverableFormat) -> String {
        match format {
            DeliverableFormat::CodeWeb => self.t("deliverables.kind_code"),
            DeliverableFormat::Other => self.t("deliverables.kind_file"),
            other => other.tag().to_owned(),
        }
    }

    fn format_relative_time(
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

    fn format_bucket_day_label(&self, bucket: &DailyActivityBucket) -> String {
        if bucket.day_offset == 0 {
            self.t("board.kpi_today")
        } else {
            format!("{}/{}", bucket.month, bucket.day)
        }
    }

    fn format_bucket_summary(&self, bucket: &DailyActivityBucket) -> String {
        let day_label = self.format_bucket_day_label(bucket);
        self.tf(
            "board.day_summary",
            &[
                ("date", &day_label),
                ("active", &bucket.active_or_blocked.to_string()),
                ("done", &bucket.completed_or_delivered.to_string()),
            ],
        )
    }

    /// Global Visual Board surface (`StudioSurface::Board`).
    pub(crate) fn board_canvas(&self, cx: &Context<'_, Self>) -> AnyElement {
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
            BoardKpiSummary::from_groups(&project_filtered)
        } else {
            snapshot.kpi.clone()
        };
        let scoped_daily_7d = if self.board_project_filter.is_some()
            || snapshot.daily_7d.is_empty()
        {
            compute_daily_7d_in_tz(&project_filtered, now_ms, tz_offset_secs)
        } else {
            snapshot.daily_7d.clone()
        };
        let today_key = scoped_daily_7d.last().map(|b| b.date_key);
        let active_day_filter = if self.board_filter == BoardFilter::Archived {
            None
        } else {
            self.board_day_filter
        };
        let filtered: Vec<&SessionBoardGroup> = project_filtered
            .iter()
            .copied()
            .filter(|group| match self.board_filter {
                BoardFilter::Archived => group.archived,
                BoardFilter::All => !group.archived,
                BoardFilter::Approval => !group.archived && group.has_blocker,
                BoardFilter::Running => !group.archived && group.is_running,
                BoardFilter::Delivered => {
                    !group.archived && group.has_delivered
                }
            })
            .filter(|group| {
                active_day_filter.is_none_or(|day_key| {
                    matches_day_filter(
                        group.updated_at,
                        now_ms,
                        tz_offset_secs,
                        day_key,
                    )
                })
            })
            .collect();

        let mut sessions_col =
            div().w_full().flex().flex_col().gap(self.theme.space(8.));

        if filtered.is_empty() {
            sessions_col = sessions_col.child(
                div()
                    .w_full()
                    .py(px(48.))
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
                            .child(self.t("board.empty_detail")),
                    )
                    .when(active_day_filter.is_some(), |empty| {
                        empty.child(
                            div().pt(px(4.)).child(
                                theme::Button::new(
                                    "board-clear-day-filter",
                                    self.t("board.clear_day_filter"),
                                )
                                .variant(theme::ControlVariant::Secondary)
                                .size(theme::ControlSize::Small)
                                .render(
                                    &self.theme,
                                    cx.listener(|panel, _, _, cx| {
                                        panel.board_day_filter = None;
                                        cx.notify();
                                    }),
                                ),
                            ),
                        )
                    }),
            );
        } else if self.board_filter == BoardFilter::Archived {
            for (card_idx, group) in filtered.into_iter().enumerate() {
                sessions_col = sessions_col.child(self.board_session_card(
                    group,
                    card_idx as u64,
                    now_ms,
                    tz_offset_secs,
                    cx,
                ));
            }
        } else {
            let active_groups: Vec<&SessionBoardGroup> =
                filtered.iter().copied().filter(|g| g.is_active).collect();
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
            let show_headers =
                !active_groups.is_empty() && !recent_settled.is_empty();
            let mut card_idx: u64 = 0;

            if !active_groups.is_empty() {
                if show_headers {
                    sessions_col = sessions_col.child(
                        div()
                            .pt(px(2.))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .child(self.tf(
                                "board.group_active",
                                &[("count", &active_groups.len().to_string())],
                            )),
                    );
                }
                for group in active_groups {
                    sessions_col = sessions_col.child(self.board_session_card(
                        group,
                        card_idx,
                        now_ms,
                        tz_offset_secs,
                        cx,
                    ));
                    card_idx += 1;
                }
            }
            if !recent_settled.is_empty() {
                if show_headers {
                    sessions_col = sessions_col.child(
                        div()
                            .pt(self.theme.space(6.))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .child(self.tf(
                                "board.group_settled",
                                &[("count", &recent_settled.len().to_string())],
                            )),
                    );
                }
                for group in recent_settled {
                    sessions_col = sessions_col.child(self.board_session_card(
                        group,
                        card_idx,
                        now_ms,
                        tz_offset_secs,
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
                    .mt(self.theme.space(6.))
                    .px(self.theme.space(8.))
                    .py(px(6.))
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|style| style.bg(self.theme.soft()))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .on_click(cx.listener(|panel, _, _, cx| {
                        panel.board_older_expanded =
                            !panel.board_older_expanded;
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
                        sessions_col =
                            sessions_col.child(self.board_session_card(
                                group,
                                card_idx,
                                now_ms,
                                tz_offset_secs,
                                cx,
                            ));
                        card_idx += 1;
                    }
                }
            }
        }

        div()
            .id("board-scroll")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .px(self.theme.space(20.))
            .py(self.theme.space(16.))
            .flex()
            .flex_col()
            .gap(self.theme.space(16.))
            .child(self.board_kpi_strip(
                &scoped_kpi,
                &scoped_daily_7d,
                active_day_filter,
                cx,
            ))
            .child(self.board_filter_bar(
                &scoped_kpi,
                today_key,
                active_day_filter,
                cx,
            ))
            .child(sessions_col)
            .into_any_element()
    }

    /// Expanded 3-card KPI visualization header (~148px tall):
    /// 1. Enlarged 76px progress donut + inline blocker CTA
    /// 2. Real 7-day daily activity & delivery bar chart
    /// 3. Expanded 5-category deliverable composition bar
    fn board_kpi_strip(
        &self,
        kpi: &BoardKpiSummary,
        daily_7d: &[DailyActivityBucket],
        selected_day: Option<LocalDateKey>,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let segments = vec![
            DonutSegment::new(
                kpi.delivered_count as f32,
                self.theme.status_verified(),
            ),
            DonutSegment::new(
                kpi.running_count as f32,
                self.theme.status_active(),
            ),
            DonutSegment::new(
                kpi.approval_count as f32,
                self.theme.status_attention(),
            ),
        ];

        let has_blockers = kpi.approval_count > 0;
        let health_pill: AnyElement = if has_blockers {
            let label = format!(
                "{} {} \u{00b7} {}",
                kpi.approval_count,
                self.t("board.kpi_blockers"),
                self.t("board.jump_blocker")
            );
            theme::interactive("board-kpi-blocker", label.clone(), &self.theme)
                .px(self.theme.space(10.))
                .py(px(5.))
                .rounded_lg()
                .border_1()
                .border_color(self.theme.status_attention().opacity(0.45))
                .bg(self.theme.status_attention_bg())
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(6.))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.open_first_blocker_modal(cx);
                }))
                .child(
                    theme::icons::circle_alert()
                        .size(px(13.))
                        .text_color(self.theme.status_attention()),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.status_attention())
                        .whitespace_nowrap()
                        .child(label),
                )
                .into_any_element()
        } else {
            div()
                .px(self.theme.space(8.))
                .py(px(4.))
                .rounded_lg()
                .bg(self.theme.status_verified_bg())
                .flex()
                .items_center()
                .gap(px(5.))
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
                        .whitespace_nowrap()
                        .child(self.t("board.kpi_clear")),
                )
                .into_any_element()
        };

        let card_progress = div()
            .flex_1()
            .min_w(px(270.))
            .min_h(px(148.))
            .p(self.theme.space(16.))
            .rounded_2xl()
            .border_1()
            .border_color(if has_blockers {
                self.theme.status_attention().opacity(0.4)
            } else {
                self.theme.line()
            })
            .bg(self.theme.card())
            .flex()
            .items_center()
            .gap(self.theme.space(16.))
            .child(donut_chart(
                "board-kpi-donut",
                76.0,
                segments,
                kpi.total_branches.to_string(),
                &self.theme,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(self.theme.ink())
                                    .whitespace_nowrap()
                                    .child(self.t("board.kpi_branches")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .whitespace_nowrap()
                                    .child(self.tf(
                                        "board.kpi_trend_sub",
                                        &[(
                                            "count",
                                            &kpi.total_sessions.to_string(),
                                        )],
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(self.theme.space(10.))
                            .gap_y(px(4.))
                            .text_xs()
                            .child(self.kpi_dot_stat(
                                self.theme.status_attention(),
                                kpi.approval_count,
                                self.t("board.stat_approval"),
                            ))
                            .child(self.kpi_dot_stat(
                                self.theme.status_active(),
                                kpi.running_count,
                                self.t("board.stat_running"),
                            ))
                            .child(self.kpi_dot_stat(
                                self.theme.status_verified(),
                                kpi.delivered_count,
                                self.t("board.filter_delivered"),
                            )),
                    )
                    .child(div().flex().items_center().child(health_pill)),
            );

        // Card 2: Real 7-day local calendar-day activity & delivery bar chart.
        let week_total: usize =
            daily_7d.iter().map(DailyActivityBucket::total).sum();
        let max_day = daily_7d
            .iter()
            .map(DailyActivityBucket::total)
            .max()
            .unwrap_or(0)
            .max(1);
        let selected_bucket = selected_day
            .and_then(|key| daily_7d.iter().find(|b| b.date_key == key));
        let mut chart_cols = div()
            .w_full()
            .h(px(88.))
            .flex()
            .items_end()
            .justify_between()
            .gap(px(4.));
        for bucket in daily_7d {
            let total = bucket.total();
            let bar_max_h: f32 = 46.0;
            let bar_track = if total > 0 {
                let fill_h = ((total as f32 / max_day as f32) * bar_max_h)
                    .clamp(7.0, bar_max_h);
                let active_h = if bucket.active_or_blocked > 0 {
                    ((bucket.active_or_blocked as f32 / total as f32) * fill_h)
                        .clamp(3.0, fill_h)
                } else {
                    0.0
                };
                let done_h = (fill_h - active_h).max(0.0);
                div()
                    .w_full()
                    .max_w(px(24.))
                    .h(px(bar_max_h))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .when(active_h > 0.0, |col| {
                        col.child(
                            div()
                                .w_full()
                                .h(px(active_h))
                                .bg(self.theme.status_active()),
                        )
                    })
                    .when(done_h > 0.0, |col| {
                        col.child(
                            div()
                                .w_full()
                                .h(px(done_h))
                                .bg(self.theme.status_verified()),
                        )
                    })
            } else {
                div()
                    .w_full()
                    .max_w(px(24.))
                    .h(px(bar_max_h))
                    .rounded_md()
                    .bg(self.theme.soft().opacity(0.65))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(div().w_full().h(px(3.)).bg(self.theme.line()))
            };
            let day_label = self.format_bucket_day_label(bucket);
            let a11y_label = self.format_bucket_summary(bucket);
            let is_today = bucket.day_offset == 0;
            let is_selected = selected_day == Some(bucket.date_key);
            let is_dimmed = selected_day.is_some() && !is_selected;
            let date_key = bucket.date_key;
            chart_cols = chart_cols.child(
                theme::interactive(
                    ("board-day-bar", u64::from(bucket.day_offset)),
                    a11y_label,
                    &self.theme,
                )
                .flex_1()
                .min_w_0()
                .h_full()
                .px(px(2.))
                .py(px(3.))
                .rounded_lg()
                .border_1()
                .border_color(if is_selected {
                    self.theme.status_active().opacity(0.7)
                } else {
                    gpui::transparent_black().into()
                })
                .bg(if is_selected {
                    self.theme.status_active_bg()
                } else {
                    gpui::transparent_black().into()
                })
                .when(is_dimmed, |col| col.opacity(0.55))
                .cursor_pointer()
                .hover(|style| {
                    if is_selected {
                        style
                    } else {
                        style.bg(self.theme.soft()).opacity(1.0)
                    }
                })
                .flex()
                .flex_col()
                .items_center()
                .justify_end()
                .gap(px(3.))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    if panel.board_day_filter == Some(date_key) {
                        panel.board_day_filter = None;
                    } else {
                        panel.board_day_filter = Some(date_key);
                        if panel.board_filter == BoardFilter::Archived {
                            panel.board_filter = BoardFilter::All;
                        }
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .h(px(13.))
                        .text_xs()
                        .font_weight(if is_selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if is_selected {
                            self.theme.status_active()
                        } else if total > 0 {
                            self.theme.ink()
                        } else {
                            self.theme.muted().opacity(0.4)
                        })
                        .child(if total > 0 {
                            total.to_string()
                        } else {
                            String::new()
                        }),
                )
                .child(bar_track)
                .child(
                    div()
                        .text_xs()
                        .font_weight(if is_selected || is_today {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if is_selected {
                            self.theme.status_active()
                        } else if is_today {
                            self.theme.ink()
                        } else {
                            self.theme.muted()
                        })
                        .whitespace_nowrap()
                        .child(day_label),
                ),
            );
        }

        let trend_meta = if let Some(bucket) = selected_bucket {
            self.format_bucket_summary(bucket)
        } else {
            self.tf(
                "board.kpi_trend_sub",
                &[("count", &week_total.to_string())],
            )
        };

        let card_trend = div()
            .flex_1()
            .min_w(px(270.))
            .min_h(px(148.))
            .p(self.theme.space(16.))
            .rounded_2xl()
            .border_1()
            .border_color(if selected_bucket.is_some() {
                self.theme.status_active().opacity(0.45)
            } else {
                self.theme.line()
            })
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .whitespace_nowrap()
                            .child(self.t("board.kpi_trend_7d")),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if selected_bucket.is_some() {
                                self.theme.status_active()
                            } else {
                                self.theme.muted()
                            })
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(trend_meta),
                    ),
            )
            .child(chart_cols);

        // Card 3: Expanded 5-category deliverable composition bar.
        let format_total = (kpi.sheet_count
            + kpi.slides_count
            + kpi.doc_count
            + kpi.code_count
            + kpi.other_count)
            .max(1);
        let raw_shares: Vec<(f32, gpui::Rgba)> = [
            (kpi.sheet_count, self.theme.file_kind_sheet()),
            (kpi.slides_count, self.theme.file_kind_slides()),
            (kpi.doc_count, self.theme.file_kind_doc()),
            (kpi.code_count, self.theme.file_kind_code()),
            (kpi.other_count, self.theme.file_kind_other()),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, color)| {
            ((count as f32 / format_total as f32).max(0.06), color)
        })
        .collect();
        let share_sum: f32 = raw_shares
            .iter()
            .map(|(share, _)| *share)
            .sum::<f32>()
            .max(1.0);
        let mut bar_track = div()
            .w_full()
            .h(px(10.))
            .rounded_full()
            .overflow_hidden()
            .bg(self.theme.soft())
            .flex()
            .gap(px(2.));
        for (share, color) in raw_shares {
            bar_track = bar_track.child(
                div()
                    .h_full()
                    .bg(color)
                    .w(gpui::relative(share / share_sum)),
            );
        }

        let card_formats = div()
            .flex_1()
            .min_w(px(270.))
            .min_h(px(148.))
            .p(self.theme.space(16.))
            .rounded_2xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .justify_between()
            .gap(self.theme.space(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .whitespace_nowrap()
                            .child(self.t("board.kpi_deliverables")),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(self.theme.status_verified())
                            .whitespace_nowrap()
                            .child(kpi.delivered_count.to_string()),
                    ),
            )
            .child(bar_track)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(self.theme.space(12.))
                    .gap_y(px(6.))
                    .text_xs()
                    .child(self.format_legend_item(
                        self.theme.file_kind_sheet(),
                        "XLSX".to_owned(),
                        kpi.sheet_count,
                    ))
                    .child(self.format_legend_item(
                        self.theme.file_kind_slides(),
                        "PPTX".to_owned(),
                        kpi.slides_count,
                    ))
                    .child(self.format_legend_item(
                        self.theme.file_kind_doc(),
                        "DOCX".to_owned(),
                        kpi.doc_count,
                    ))
                    .child(self.format_legend_item(
                        self.theme.file_kind_code(),
                        self.t("deliverables.kind_code"),
                        kpi.code_count,
                    ))
                    .child(self.format_legend_item(
                        self.theme.file_kind_other(),
                        self.t("deliverables.kind_file"),
                        kpi.other_count,
                    )),
            );

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(self.theme.space(12.))
            .child(card_progress)
            .child(card_trend)
            .child(card_formats)
            .into_any_element()
    }

    fn format_legend_item(
        &self,
        color: gpui::Rgba,
        label: String,
        count: usize,
    ) -> AnyElement {
        let dimmed = count == 0;
        div()
            .flex()
            .items_center()
            .gap(px(5.))
            .whitespace_nowrap()
            .child(div().size(px(6.)).rounded_full().bg(if dimmed {
                color.opacity(0.4)
            } else {
                color
            }))
            .child(
                div()
                    .text_color(if dimmed {
                        self.theme.muted().opacity(0.7)
                    } else {
                        self.theme.muted()
                    })
                    .child(label),
            )
            .child(
                div()
                    .font_weight(if dimmed {
                        FontWeight::NORMAL
                    } else {
                        FontWeight::SEMIBOLD
                    })
                    .text_color(if dimmed {
                        self.theme.muted().opacity(0.6)
                    } else {
                        self.theme.ink()
                    })
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn kpi_dot_stat(
        &self,
        color: gpui::Rgba,
        count: usize,
        label: String,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .whitespace_nowrap()
            .child(div().size(px(6.)).rounded_full().bg(color))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .child(count.to_string()),
            )
            .child(div().text_color(self.theme.muted()).child(label))
            .into_any_element()
    }

    fn board_filter_bar(
        &self,
        kpi: &BoardKpiSummary,
        today_key: Option<LocalDateKey>,
        active_day_filter: Option<LocalDateKey>,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut filters = vec![
            (
                BoardFilter::All,
                self.tf(
                    "board.filter_all",
                    &[("count", &kpi.total_sessions.to_string())],
                ),
            ),
            (BoardFilter::Approval, self.t("board.filter_approval")),
            (BoardFilter::Running, self.t("board.filter_running")),
            (BoardFilter::Delivered, self.t("board.filter_delivered")),
        ];
        if kpi.archived_sessions > 0
            || self.board_filter == BoardFilter::Archived
        {
            filters.push((
                BoardFilter::Archived,
                self.tf(
                    "board.filter_archived",
                    &[("count", &kpi.archived_sessions.to_string())],
                ),
            ));
        }

        let mut pills = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(self.theme.space(6.));
        for (idx, (filter, label)) in filters.into_iter().enumerate() {
            let selected = self.board_filter == filter;
            pills = pills.child(
                theme::interactive(
                    ("board-filter", idx as u64),
                    label.clone(),
                    &self.theme,
                )
                .px(self.theme.space(10.))
                .py(px(4.))
                .rounded_full()
                .cursor_pointer()
                .text_xs()
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
                .child(label)
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        if filter == BoardFilter::All
                            && panel.board_filter == BoardFilter::All
                        {
                            panel.board_project_filter = None;
                            panel.board_day_filter = None;
                        } else if filter == BoardFilter::Archived {
                            panel.board_day_filter = None;
                        }
                        panel.board_filter = filter;
                        cx.notify();
                    },
                )),
            );
        }

        if let Some(day_key) = active_day_filter {
            let day_label = if today_key == Some(day_key) {
                self.t("board.kpi_today")
            } else {
                format!("{}/{}", day_key.1, day_key.2)
            };
            let chip_label = format!("{day_label} \u{00d7}");
            pills = pills.child(
                theme::interactive(
                    "board-day-filter-chip",
                    self.t("board.clear_day_filter"),
                    &self.theme,
                )
                .px(self.theme.space(8.))
                .py(px(4.))
                .rounded_full()
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .bg(self.theme.status_active_bg())
                .text_color(self.theme.status_active())
                .child(chip_label)
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.board_day_filter = None;
                    cx.notify();
                })),
            );
        }

        if !self.projects.is_empty() {
            pills = pills.child(
                div().w(px(1.)).h(px(14.)).mx(px(2.)).bg(self.theme.line()),
            );
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
                let label = if active {
                    format!("{} \u{00d7}", project.name)
                } else {
                    project.name.clone()
                };
                pills = pills.child(
                    theme::interactive(
                        ("board-proj-filter", p_idx as u64),
                        project.name.clone(),
                        &self.theme,
                    )
                    .max_w(px(140.))
                    .px(self.theme.space(8.))
                    .py(px(4.))
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
                    .child(label)
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
        }

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(pills)
            .child(
                theme::Button::new("board-new-task", self.t("dock.new_task"))
                    .variant(theme::ControlVariant::Ghost)
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.new_task(cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn board_row_actions(
        &self,
        session_id: &str,
        row_title: &str,
        index: u64,
        include_flow_icon: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let sid_chat = session_id.to_owned();
        let sid_flow = session_id.to_owned();
        let sid_menu = session_id.to_owned();
        let open_chat_label =
            format!("{row_title} \u{00b7} {}", self.t("board.open_chat"));
        let open_flow_label =
            format!("{row_title} \u{00b7} {}", self.t("board.open_flow"));
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
                    panel.open_session(&sid_chat, cx);
                }))
                .child(theme::icons::arrow_up_right().size(px(13.))),
            )
            .when(include_flow_icon, |row| {
                row.child(
                    theme::interactive(
                        ("board-row-flow", index),
                        open_flow_label,
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
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        cx.stop_propagation();
                        panel.open_flow_modal(sid_flow.clone(), None, cx);
                    }))
                    .child(theme::icons::workflow().size(px(13.))),
                )
            })
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
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let sid_card = group.session_id.clone();
        let sid_right = group.session_id.clone();
        let is_real_session = !group.is_synthetic();
        let rel_time =
            self.format_relative_time(group.updated_at, now_ms, tz_offset_secs);

        if !group.is_active {
            let all_file_branches: Vec<_> = group
                .branches
                .iter()
                .filter(|b| {
                    b.deliverable_path.is_some()
                        || !matches!(b.phase, BranchPhase::Idle)
                })
                .collect();
            let total_files =
                group.total_file_count.max(all_file_branches.len());
            let shown_files: Vec<_> =
                all_file_branches.into_iter().take(3).collect();
            let overflow_files = total_files.saturating_sub(shown_files.len());
            let has_deliverables = !shown_files.is_empty();

            let mut file_chips =
                div().flex().flex_wrap().items_center().gap(px(5.));
            for branch in shown_files {
                let color = self.format_color(branch.format);
                file_chips = file_chips.child(
                    div()
                        .max_w(px(200.))
                        .px(px(7.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(self.theme.soft())
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_xs()
                        .text_color(self.theme.ink())
                        .child(
                            div()
                                .size(px(6.))
                                .flex_none()
                                .rounded_full()
                                .bg(color),
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

            let preview_snippet =
                (!has_deliverables).then(|| group.preview.clone()).flatten();

            return theme::interactive(
                ("board-session-compact", index),
                group.title.clone(),
                &self.theme,
            )
            .w_full()
            .px(self.theme.space(14.))
            .py(self.theme.space(10.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft().opacity(0.6)))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(self.theme.space(10.))
            .on_click(cx.listener(move |panel, _, _, cx| {
                panel.open_flow_modal(sid_card.clone(), None, cx);
            }))
            .when(is_real_session, |row| {
                row.on_mouse_down(
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
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(200.))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(
                        if has_deliverables {
                            self.theme.status_verified()
                        } else {
                            self.theme.muted().opacity(0.5)
                        },
                    ))
                    .when(group.pinned, |row| {
                        row.child(
                            theme::icons::pin()
                                .size(px(12.))
                                .flex_none()
                                .text_color(self.theme.accent()),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(group.title.clone()),
                    )
                    .when_some(group.project_name.clone(), |row, project| {
                        row.child(status_pill(
                            self.theme.soft(),
                            self.theme.muted(),
                            project,
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
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .when(has_deliverables, |right| right.child(file_chips))
                    .when_some(preview_snippet, |right, snippet| {
                        right.child(
                            div()
                                .max_w(px(260.))
                                .text_xs()
                                .text_color(self.theme.muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(snippet),
                        )
                    })
                    .when(!rel_time.is_empty(), |right| {
                        right.child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted().opacity(0.8))
                                .whitespace_nowrap()
                                .child(rel_time),
                        )
                    })
                    .when(is_real_session, |right| {
                        right.child(self.board_row_actions(
                            &group.session_id,
                            &group.title,
                            index,
                            false,
                            cx,
                        ))
                    }),
            )
            .into_any_element();
        }

        let (badge_bg, badge_fg, badge_text) = if group.has_blocker {
            (
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
                self.t("board.badge_approval"),
            )
        } else {
            (
                self.theme.status_active_bg(),
                self.theme.status_active(),
                self.t("board.badge_running"),
            )
        };

        let mut branches_col = div().w_full().flex().flex_col().gap(px(6.));
        for branch in &group.branches {
            let sid_branch = group.session_id.clone();
            let bid = branch.id.clone();
            let real_pct = real_contract_progress_pct(branch);
            let (dot_color, state_label, live) = match &branch.phase {
                BranchPhase::NeedsApproval { .. } => (
                    self.theme.status_attention(),
                    self.t("board.state_needs_approval"),
                    false,
                ),
                BranchPhase::DeliveryFailed => (
                    self.theme.status_attention(),
                    self.t("chat.status_delivery_failed"),
                    false,
                ),
                BranchPhase::Running { step_label, .. } => {
                    let step = if step_label.is_empty() {
                        self.t("chat.status_running")
                    } else {
                        step_label.clone()
                    };
                    let label = if let Some(pct) = real_pct {
                        format!(
                            "{}% \u{00b7} {step}",
                            pct.clamp(0.0, 100.0).round() as u32
                        )
                    } else {
                        step
                    };
                    (self.theme.status_active(), label, true)
                }
                BranchPhase::Queued { state, .. } => {
                    self.receipt_state_visual(*state)
                }
                BranchPhase::Delivered { override_used, .. } => (
                    self.theme.status_verified(),
                    if *override_used {
                        self.t("verify.override.badge")
                    } else {
                        self.t("verify.card.passed")
                    },
                    false,
                ),
                BranchPhase::Idle => {
                    (self.theme.muted(), self.t("chat.status_idle"), false)
                }
            };
            let format_color = self.format_color(branch.format);
            let dot_id = gpui::SharedString::from(format!(
                "board-branch-dot-{}",
                branch.id
            ));
            let row_id = gpui::SharedString::from(format!(
                "board-branch-row-{}",
                branch.id
            ));
            let dot = if live {
                crate::motion::live_dot(
                    dot_id,
                    dot_color,
                    self.theme.motion_policy(),
                )
            } else {
                div()
                    .size(px(6.))
                    .flex_none()
                    .rounded_full()
                    .bg(dot_color)
                    .into_any_element()
            };
            let has_file_deliverable = branch.deliverable_path.is_some();
            let bar_ratio = real_pct.map_or(0.0, |pct| {
                if pct <= 0.0 || !pct.is_finite() {
                    0.0
                } else {
                    (pct / 100.0).clamp(0.08, 1.0)
                }
            });
            let branch_caption = if !has_file_deliverable
                && matches!(branch.phase, BranchPhase::Running { .. })
                && duplicates_session_title(
                    &group.title,
                    &branch.deliverable_name,
                )
            {
                self.t("chat.thinking")
            } else {
                branch.deliverable_name.clone()
            };

            branches_col = branches_col.child(
                theme::interactive(
                    row_id,
                    branch_caption.clone(),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(10.))
                .py(self.theme.space(6.))
                .rounded_lg()
                .bg(self.theme.soft())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.dock_hover()))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.open_flow_modal(
                        sid_branch.clone(),
                        Some(bid.clone()),
                        cx,
                    );
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(160.))
                        .flex()
                        .items_center()
                        .gap(self.theme.space(6.))
                        .child(dot)
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(self.theme.ink())
                                .whitespace_nowrap()
                                .child(
                                    branch.skill_label.resolve(|k| self.t(k)),
                                ),
                        )
                        .when(has_file_deliverable, |row| {
                            row.child(
                                div()
                                    .px(px(5.))
                                    .py(px(1.))
                                    .rounded_sm()
                                    .bg(format_color)
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(self.theme.on_ink())
                                    .child(
                                        self.format_tag_label(branch.format),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(branch_caption),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
                        .when(real_pct.is_some(), |right| {
                            right.child(
                                div()
                                    .w(px(56.))
                                    .h(px(5.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .bg(self.theme.card())
                                    .when(bar_ratio > 0.0, |track| {
                                        track.child(
                                            div()
                                                .h_full()
                                                .rounded_full()
                                                .bg(dot_color)
                                                .w(gpui::relative(bar_ratio)),
                                        )
                                    }),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(dot_color)
                                .whitespace_nowrap()
                                .child(state_label),
                        ),
                ),
            );
        }

        theme::interactive(
            ("board-session-active", index),
            group.title.clone(),
            &self.theme,
        )
        .w_full()
        .p(self.theme.space(14.))
        .rounded_2xl()
        .border_1()
        .border_color(if group.has_blocker {
            self.theme.status_attention().opacity(0.45)
        } else {
            self.theme.line()
        })
        .bg(self.theme.card())
        .cursor_pointer()
        .flex()
        .flex_col()
        .gap(self.theme.space(10.))
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.open_flow_modal(sid_card.clone(), None, cx);
        }))
        .when(is_real_session, |card| {
            card.on_mouse_down(
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
            )
        })
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(180.))
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
                        .child(
                            div()
                                .size(px(24.))
                                .flex_none()
                                .rounded_lg()
                                .bg(self.theme.assistant_badge_bg())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    theme::icons::sparkles()
                                        .size(px(12.))
                                        .text_color(
                                            self.theme.assistant_badge_fg(),
                                        ),
                                ),
                        )
                        .when(group.pinned, |row| {
                            row.child(
                                theme::icons::pin()
                                    .size(px(12.))
                                    .flex_none()
                                    .text_color(self.theme.accent()),
                            )
                        })
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(group.title.clone()),
                        )
                        .when_some(
                            group.project_name.clone(),
                            |row, project| {
                                row.child(status_pill(
                                    self.theme.soft(),
                                    self.theme.muted(),
                                    project,
                                    None,
                                ))
                            },
                        )
                        .when(group.archived, |row| {
                            row.child(status_pill(
                                self.theme.soft(),
                                self.theme.muted(),
                                self.t("chat.archived_badge"),
                                None,
                            ))
                        })
                        .child(status_pill(
                            badge_bg, badge_fg, badge_text, None,
                        )),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
                        .when(!rel_time.is_empty(), |right| {
                            right.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted().opacity(0.8))
                                    .whitespace_nowrap()
                                    .child(rel_time),
                            )
                        })
                        .when(is_real_session, |right| {
                            right.child(self.board_row_actions(
                                &group.session_id,
                                &group.title,
                                index,
                                true,
                                cx,
                            ))
                        }),
                ),
        )
        .child(branches_col)
        .into_any_element()
    }
}
