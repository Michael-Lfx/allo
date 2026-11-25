//! Responsive Global Visual Board & Centered Flowchart Modal.
//!
//! Main canvas stays spacious (no always-open right drawer):
//! - Top adaptive KPI strip (`flex_wrap` + `min_w(210px)`): GPU `DonutChart`,
//!   blocker pulse card, deliverable format bar, and session spark bars.
//! - Session-layered cards: active sessions show branch visual gauges; settled
//!   sessions fold into compact 1-line pipeline rows.
//! - Clicking any session opens a centered responsive Flowchart Modal showing
//!   `Router -> Skill ProgressRing -> Gate/Verify -> Deliverable File` plus
//!   in-modal two-step override confirmation and redacted Trace debug logs.

use flowy_ui as theme;
use flowy_ui::charts::{
    DonutSegment, FlowEdgeStyle, donut_chart, flow_connector, progress_ring,
};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, SharedString,
    div, prelude::*, px,
};

use crate::panel::StudioPanel;
use crate::ports::{
    BranchPhase, DeliverableFormat, SessionBoardGroup, TraceLevel,
    WorkBranchSummary, collect_session_traces, display_path_str,
    format_traces_jsonl,
};

/// Top-level session filter on the Global Visual Board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardFilter {
    #[default]
    All,
    Approval,
    Running,
    Delivered,
}

/// Active tab inside the Flowchart Modal detail footer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlowModalTab {
    #[default]
    Action,
    Trace,
}

/// Trace filter level shared between Flowchart Modal and Right Drawer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TraceFilter {
    #[default]
    All,
    WarnAndError,
}

impl StudioPanel {
    fn format_color(&self, format: DeliverableFormat) -> gpui::Rgba {
        match format {
            DeliverableFormat::Sheet => self.theme.file_kind_sheet(),
            DeliverableFormat::Slides => self.theme.file_kind_slides(),
            DeliverableFormat::Doc => self.theme.file_kind_doc(),
            DeliverableFormat::Other => self.theme.file_kind_other(),
        }
    }

    /// Global Visual Board surface (`StudioSurface::Board`).
    pub(crate) fn board_canvas(&self, cx: &Context<'_, Self>) -> AnyElement {
        let snapshot = &self.board_snapshot;
        let filtered: Vec<&SessionBoardGroup> = snapshot
            .sessions
            .iter()
            .filter(|group| {
                if let Some(proj) = self.board_project_filter.as_deref()
                    && group.project_id.as_deref() != Some(proj)
                {
                    return false;
                }
                match self.board_filter {
                    BoardFilter::All => true,
                    BoardFilter::Approval => group.has_blocker,
                    BoardFilter::Running => group.is_running,
                    BoardFilter::Delivered => !group.is_active,
                }
            })
            .collect();

        let mut sessions_col =
            div().w_full().flex().flex_col().gap(self.theme.space(10.));

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
                    ),
            );
        } else {
            for (idx, group) in filtered.iter().enumerate() {
                sessions_col = sessions_col
                    .child(self.board_session_card(group, idx as u64, cx));
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
            .gap(self.theme.space(14.))
            .child(self.board_kpi_strip(cx))
            .child(self.board_filter_bar(cx))
            .child(sessions_col)
            .into_any_element()
    }

    /// Adaptive 2x2 / 4x1 KPI visualization strip (`flex_wrap` + `min_w(210px)`).
    fn board_kpi_strip(&self, cx: &Context<'_, Self>) -> AnyElement {
        let kpi = &self.board_snapshot.kpi;
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

        // Tile 1: Status Donut Ring
        let tile_donut = div()
            .flex_1()
            .min_w(px(210.))
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .items_center()
            .gap(self.theme.space(12.))
            .child(donut_chart(
                "board-kpi-donut",
                52.0,
                segments,
                kpi.total_branches.to_string(),
                &self.theme,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .whitespace_nowrap()
                            .child(self.t("board.kpi_branches")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(self.theme.space(8.))
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
                                self.t("board.stat_done"),
                            )),
                    ),
            );

        // Tile 2: Blocker Pulse Card (1-click opens first blocker flowchart modal)
        let has_blockers = kpi.approval_count > 0;
        let blocker_fg = if has_blockers {
            self.theme.status_attention()
        } else {
            self.theme.status_verified()
        };
        let blocker_bg = if has_blockers {
            self.theme.status_attention_bg()
        } else {
            self.theme.status_verified_bg()
        };
        let blocker_label = if has_blockers {
            self.t("board.kpi_blockers")
        } else {
            self.t("board.kpi_clear")
        };
        let tile_blocker = theme::interactive(
            "board-kpi-blocker",
            blocker_label.clone(),
            &self.theme,
        )
        .flex_1()
        .min_w(px(210.))
        .p(self.theme.space(12.))
        .rounded_xl()
        .border_1()
        .border_color(if has_blockers {
            self.theme.status_attention().opacity(0.45)
        } else {
            self.theme.line()
        })
        .bg(blocker_bg)
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(self.theme.space(8.))
        .on_click(cx.listener(|panel, _, _, cx| {
            panel.open_first_blocker_modal(cx);
        }))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(blocker_fg)
                        .whitespace_nowrap()
                        .child(blocker_label),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(blocker_fg)
                        .child(kpi.approval_count.to_string()),
                ),
        )
        .when(has_blockers, |card| {
            card.child(
                div()
                    .flex_none()
                    .px(self.theme.space(8.))
                    .py(px(4.))
                    .rounded_lg()
                    .bg(self.theme.card())
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(blocker_fg)
                    .whitespace_nowrap()
                    .child(self.t("board.jump_blocker")),
            )
        });

        // Tile 3: Local Deliverable Format Segmented Bar
        let format_total = (kpi.sheet_count
            + kpi.slides_count
            + kpi.doc_count
            + kpi.other_count)
            .max(1);
        let mut bar_track = div()
            .w_full()
            .h(px(7.))
            .rounded_full()
            .overflow_hidden()
            .bg(self.theme.soft())
            .flex()
            .gap(px(2.));
        for (count, color) in [
            (kpi.sheet_count, self.theme.file_kind_sheet()),
            (kpi.slides_count, self.theme.file_kind_slides()),
            (kpi.doc_count, self.theme.file_kind_doc()),
            (kpi.other_count, self.theme.file_kind_other()),
        ] {
            if count > 0 {
                let share = (count as f32 / format_total as f32).max(0.06);
                bar_track = bar_track
                    .child(div().h_full().bg(color).w(gpui::relative(share)));
            }
        }
        let tile_formats = div()
            .flex_1()
            .min_w(px(210.))
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
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
                    .gap(self.theme.space(6.))
                    .text_xs()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .whitespace_nowrap()
                            .child(self.t("board.kpi_deliverables")),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.status_verified())
                            .whitespace_nowrap()
                            .child(kpi.delivered_count.to_string()),
                    ),
            )
            .child(bar_track)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(self.theme.file_kind_sheet())
                            .child(format!("XLSX {}", kpi.sheet_count)),
                    )
                    .child(
                        div()
                            .text_color(self.theme.file_kind_slides())
                            .child(format!("PPTX {}", kpi.slides_count)),
                    )
                    .child(
                        div()
                            .text_color(self.theme.file_kind_doc())
                            .child(format!("DOCX {}", kpi.doc_count)),
                    ),
            );

        // Tile 4: Active Session Spark Bars
        let mut spark_bars =
            div().w_full().h(px(22.)).flex().items_end().gap(px(4.));
        for (idx, group) in
            self.board_snapshot.sessions.iter().take(8).enumerate()
        {
            let bar_h =
                (group.branches.len() as f32).mul_add(4.0, 8.0).min(22.0);
            let color = if group.has_blocker {
                self.theme.status_attention()
            } else if group.is_running {
                self.theme.status_active()
            } else {
                self.theme.status_verified().opacity(0.55)
            };
            let sid = group.session_id.clone();
            spark_bars = spark_bars.child(
                theme::interactive(
                    ("board-spark", idx as u64),
                    group.title.clone(),
                    &self.theme,
                )
                .flex_1()
                .h(px(bar_h))
                .rounded_sm()
                .bg(color)
                .cursor_pointer()
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open_flow_modal(sid.clone(), None, cx);
                    },
                )),
            );
        }
        let tile_sessions = div()
            .flex_1()
            .min_w(px(210.))
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .justify_between()
            .gap(self.theme.space(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .whitespace_nowrap()
                            .child(self.t("board.kpi_sessions")),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(format!(
                                "{} / {}",
                                kpi.active_sessions, kpi.total_sessions
                            )),
                    ),
            )
            .child(spark_bars)
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(self.t("board.click_hint")),
            );

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(self.theme.space(12.))
            .child(tile_donut)
            .child(tile_blocker)
            .child(tile_formats)
            .child(tile_sessions)
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

    fn board_filter_bar(&self, cx: &Context<'_, Self>) -> AnyElement {
        let kpi = &self.board_snapshot.kpi;
        let filters = [
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
                        panel.board_filter = filter;
                        cx.notify();
                    },
                )),
            );
        }

        if let Some(proj_id) = &self.board_project_filter {
            let proj_name = self
                .projects
                .iter()
                .find(|p| &p.id == proj_id)
                .map_or_else(|| proj_id.clone(), |p| p.name.clone());
            pills = pills.child(
                theme::interactive(
                    "board-clear-project",
                    proj_name.clone(),
                    &self.theme,
                )
                .px(self.theme.space(10.))
                .py(px(4.))
                .rounded_full()
                .bg(self.theme.status_active_bg())
                .text_xs()
                .text_color(self.theme.status_active())
                .cursor_pointer()
                .child(format!("{proj_name} \u{00d7}"))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.board_project_filter = None;
                    cx.notify();
                })),
            );
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
                    .variant(theme::ControlVariant::Secondary)
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.new_conversation(cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn board_session_card(
        &self,
        group: &SessionBoardGroup,
        index: u64,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let sid_card = group.session_id.clone();
        let project_label = group
            .project_name
            .clone()
            .unwrap_or_else(|| self.t("studio.space_default_sub"));

        // Settled / non-active sessions collapse into a crisp 1-line pipeline row.
        if !group.is_active {
            let mut file_chips =
                div().flex().flex_wrap().items_center().gap(px(6.));
            for branch in group.branches.iter().take(3) {
                let color = self.format_color(branch.format);
                file_chips = file_chips.child(
                    div()
                        .px(px(7.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(self.theme.soft())
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(div().size(px(6.)).rounded_full().bg(color))
                        .child(branch.deliverable_name.clone()),
                );
            }

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
            .child(
                div()
                    .flex_1()
                    .min_w(px(180.))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .size(px(7.))
                            .flex_none()
                            .rounded_full()
                            .bg(self.theme.status_verified()),
                    )
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
                    .child(
                        div()
                            .flex_none()
                            .px(px(7.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(project_label),
                    ),
            )
            .child(file_chips)
            .child(
                div()
                    .flex_none()
                    .px(self.theme.space(8.))
                    .py(px(3.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.ink())
                    .child(self.t("board.open_flow")),
            )
            .into_any_element();
        }

        // Active / blocker sessions show branch visual bars & open flowchart on click.
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
        for (b_ix, branch) in group.branches.iter().enumerate() {
            let sid_branch = group.session_id.clone();
            let bid = branch.id.clone();
            let (dot_color, state_label, live) = match &branch.phase {
                BranchPhase::NeedsApproval { .. } => (
                    self.theme.status_attention(),
                    self.t("board.state_needs_approval"),
                    false,
                ),
                BranchPhase::Running { step_label, .. } => (
                    self.theme.status_active(),
                    format!("{}% · {step_label}", branch.progress_pct as u32),
                    true,
                ),
                BranchPhase::Queued { .. } => {
                    (self.theme.muted(), self.t("chat.bg_task_queued"), false)
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
            let dot = if live {
                crate::motion::live_dot(
                    ("board-branch-dot", index * 16 + b_ix as u64),
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

            branches_col = branches_col.child(
                theme::interactive(
                    ("board-branch-row", index * 16 + b_ix as u64),
                    branch.deliverable_name.clone(),
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
                                .child(branch.skill_label.clone()),
                        )
                        .child(
                            div()
                                .px(px(5.))
                                .py(px(1.))
                                .rounded_sm()
                                .bg(format_color)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(self.theme.on_ink())
                                .child(branch.format.tag()),
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
                                .child(branch.deliverable_name.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
                        .child(
                            div()
                                .w(px(56.))
                                .h(px(5.))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(self.theme.card())
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(dot_color)
                                        .w(gpui::relative(
                                            (branch.progress_pct / 100.0)
                                                .clamp(0.08, 1.0),
                                        )),
                                ),
                        )
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
                        .child(
                            div()
                                .flex_none()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded_full()
                                .bg(self.theme.soft())
                                .text_xs()
                                .text_color(self.theme.muted())
                                .child(project_label),
                        )
                        .child(
                            div()
                                .flex_none()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded_full()
                                .bg(badge_bg)
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(badge_fg)
                                .child(badge_text),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .px(self.theme.space(10.))
                        .py(px(4.))
                        .rounded_lg()
                        .bg(self.theme.soft())
                        .border_1()
                        .border_color(self.theme.line())
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .child(self.t("board.open_flow")),
                ),
        )
        .child(branches_col)
        .into_any_element()
    }

    /// Centered wide Flowchart Modal opened when clicking any session on the board.
    pub(crate) fn flow_modal_overlay(
        &self,
        cx: &Context<'_, Self>,
    ) -> Option<AnyElement> {
        let session_id = self.flow_modal_session.as_deref()?;
        let group = self
            .board_snapshot
            .sessions
            .iter()
            .find(|g| g.session_id == session_id)?;
        let selected_branch = self
            .flow_modal_branch
            .as_deref()
            .and_then(|bid| group.branches.iter().find(|b| b.id == bid))
            .or_else(|| {
                group.branches.iter().find(|b| {
                    matches!(b.phase, BranchPhase::NeedsApproval { .. })
                })
            })
            .or_else(|| group.branches.first())?;

        let open_sid = group.session_id.clone();
        let can_open_session = group.session_id != "unassigned-queue";

        let header = div()
            .w_full()
            .px(self.theme.space(20.))
            .py(self.theme.space(12.))
            .border_b_1()
            .border_color(self.theme.line())
            .bg(self.theme.soft().opacity(0.45))
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(8.))
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
                                |row, name| {
                                    row.child(
                                        div()
                                            .flex_none()
                                            .px(px(8.))
                                            .py(px(2.))
                                            .rounded_full()
                                            .bg(self.theme.card())
                                            .border_1()
                                            .border_color(self.theme.line())
                                            .text_xs()
                                            .text_color(self.theme.muted())
                                            .child(name),
                                    )
                                },
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(group.workspace_display.clone()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .when(can_open_session, |row| {
                        row.child(
                            theme::Button::new(
                                "flow-modal-open-session",
                                self.t("drawer.work_open_session"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.close_flow_modal(cx);
                                    panel.open_session(&open_sid, cx);
                                }),
                            ),
                        )
                    })
                    .child(
                        theme::Button::new(
                            "flow-modal-close",
                            self.t("chat.close"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.close_flow_modal(cx);
                            }),
                        ),
                    ),
            );

        // Horizontal visual flowchart inside generous modal canvas
        let mut branch_rows = div()
            .flex()
            .flex_col()
            .gap(self.theme.space(10.))
            .when(group.branches.len() > 1, |col| {
                col.ml(self.theme.space(12.))
                    .pl(px(4.))
                    .border_l_2()
                    .border_color(self.theme.line())
            });
        for (b_ix, branch) in group.branches.iter().enumerate() {
            branch_rows = branch_rows.child(self.modal_flow_row(
                branch,
                branch.id == selected_branch.id,
                b_ix as u64,
                cx,
            ));
        }

        let flow_canvas = div()
            .id("flow-modal-canvas")
            .w_full()
            .overflow_x_scroll()
            .p(self.theme.space(20.))
            .border_b_1()
            .border_color(self.theme.line())
            .bg(self.theme.canvas())
            .flex()
            .items_center()
            .child(
                div()
                    .flex_none()
                    .px(self.theme.space(14.))
                    .py(self.theme.space(10.))
                    .rounded_xl()
                    .bg(self.theme.assistant_badge_bg())
                    .text_color(self.theme.assistant_badge_fg())
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        theme::icons::sparkles()
                            .size(px(14.))
                            .text_color(self.theme.assistant_badge_fg()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.t("dock.chief_assistant")),
                            )
                            .child(div().text_xs().opacity(0.8).child(
                                self.tf(
                                    "board.branch_count",
                                    &[(
                                        "count",
                                        &group.branches.len().to_string(),
                                    )],
                                ),
                            )),
                    ),
            )
            .child(branch_rows);

        // Modal bottom detail area (Action / Trace tabs)
        let tab_bar = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .p(px(2.))
                    .rounded_lg()
                    .bg(self.theme.soft())
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(self.modal_tab_button(
                        FlowModalTab::Action,
                        self.t("board.modal_tab_action"),
                        cx,
                    ))
                    .child(self.modal_tab_button(
                        FlowModalTab::Trace,
                        self.t("board.modal_tab_trace"),
                        cx,
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t("board.esc_hint")),
            );

        let detail_body = match self.flow_modal_tab {
            FlowModalTab::Action => {
                self.modal_action_panel(group, selected_branch, cx)
            }
            FlowModalTab::Trace => {
                self.modal_trace_panel(&group.session_id, cx)
            }
        };

        let footer = div()
            .id("flow-modal-detail")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(self.theme.space(16.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(tab_bar)
            .child(detail_body);

        let modal_body = div()
            .w_full()
            .flex()
            .flex_col()
            .min_h_0()
            .child(header)
            .child(flow_canvas)
            .child(footer);

        let panel_entity = cx.entity().downgrade();
        Some(theme::centered_modal_sheet(
            "flow-modal-layer",
            "flow-modal-panel",
            SharedString::from(group.title.clone()),
            1040.0,
            &self.theme,
            &self.ui,
            cx,
            move |_window, cx| {
                let _ = panel_entity.update(cx, |panel, cx| {
                    panel.close_flow_modal(cx);
                });
            },
            modal_body,
        ))
    }

    fn modal_flow_row(
        &self,
        branch: &WorkBranchSummary,
        selected: bool,
        idx: u64,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let policy = self.theme.motion_policy();
        let (is_app, is_run, is_done) = match &branch.phase {
            BranchPhase::NeedsApproval { .. } => (true, false, false),
            BranchPhase::Running { .. } | BranchPhase::Queued { .. } => {
                (false, true, false)
            }
            BranchPhase::Delivered { .. } | BranchPhase::Idle => {
                (false, false, true)
            }
        };
        let e1_color = if is_done {
            self.theme.status_verified()
        } else {
            self.theme.status_active()
        };
        let e2_color = if is_done {
            self.theme.status_verified()
        } else if is_app {
            self.theme.status_attention()
        } else {
            self.theme.status_active()
        };
        let e3_color = if is_done {
            self.theme.status_verified()
        } else {
            self.theme.line()
        };
        let e1_style = if is_run {
            FlowEdgeStyle::Flowing
        } else {
            FlowEdgeStyle::Completed
        };
        let e2_style = if is_run {
            FlowEdgeStyle::Flowing
        } else if is_done {
            FlowEdgeStyle::Completed
        } else {
            FlowEdgeStyle::Pending
        };
        let e3_style = if is_done {
            FlowEdgeStyle::Completed
        } else {
            FlowEdgeStyle::Pending
        };

        let gate_title = match &branch.phase {
            BranchPhase::NeedsApproval { .. } => {
                self.t("board.state_needs_approval")
            }
            BranchPhase::Running { step_label, .. } => step_label.clone(),
            BranchPhase::Queued { .. } => self.t("chat.bg_task_queued"),
            BranchPhase::Delivered { override_used, .. } => {
                if *override_used {
                    self.t("verify.override.badge")
                } else {
                    self.t("verify.card.passed")
                }
            }
            BranchPhase::Idle => self.t("chat.status_idle"),
        };

        let border_selected = if selected {
            self.theme.ink()
        } else {
            self.theme.line()
        };
        let bid1 = branch.id.clone();
        let bid2 = branch.id.clone();
        let bid3 = branch.id.clone();
        let format_color = self.format_color(branch.format);

        div()
            .flex()
            .items_center()
            .child(flow_connector(
                ("fm-e1", idx),
                30.0,
                e1_color,
                e1_style,
                policy,
            ))
            // Node 1: Skill + ProgressRing
            .child(
                theme::interactive(
                    ("fm-n1", idx),
                    branch.skill_label.clone(),
                    &self.theme,
                )
                .w(px(176.))
                .flex_none()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(border_selected)
                .bg(self.theme.card())
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.flow_modal_branch = Some(bid1.clone());
                    panel.flow_modal_override_ack = false;
                    cx.notify();
                }))
                .child(progress_ring(
                    ("fm-ring", idx),
                    24.0,
                    branch.progress_pct,
                    e1_color,
                    is_run,
                    &self.theme,
                ))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(branch.skill_label.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .child(format!(
                                    "{}%",
                                    branch.progress_pct as u32
                                )),
                        ),
                ),
            )
            .child(flow_connector(
                ("fm-e2", idx),
                32.0,
                e2_color,
                e2_style,
                policy,
            ))
            // Node 2: Gate / Verification Node
            .child(
                theme::interactive(
                    ("fm-n2", idx),
                    gate_title.clone(),
                    &self.theme,
                )
                .w(px(184.))
                .flex_none()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(if is_app {
                    self.theme.status_attention()
                } else {
                    border_selected
                })
                .bg(if is_app {
                    self.theme.status_attention_bg()
                } else {
                    self.theme.card()
                })
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.flow_modal_branch = Some(bid2.clone());
                    panel.flow_modal_override_ack = false;
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(22.))
                        .flex_none()
                        .rounded_full()
                        .bg(if is_app {
                            self.theme.status_attention()
                        } else if is_done {
                            self.theme.status_verified_bg()
                        } else {
                            self.theme.status_active_bg()
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if is_app {
                            self.theme.on_ink()
                        } else if is_done {
                            self.theme.status_verified()
                        } else {
                            self.theme.status_active()
                        })
                        .child(if is_app {
                            "!"
                        } else if is_done {
                            "\u{2713}"
                        } else {
                            "\u{25cf}"
                        }),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if is_app {
                                    self.theme.status_attention()
                                } else if is_done {
                                    self.theme.status_verified()
                                } else {
                                    self.theme.ink()
                                })
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(gate_title),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(branch.detail.clone()),
                        ),
                ),
            )
            .child(flow_connector(
                ("fm-e3", idx),
                32.0,
                e3_color,
                e3_style,
                policy,
            ))
            // Node 3: Local Deliverable File Node
            .child(
                theme::interactive(
                    ("fm-n3", idx),
                    branch.deliverable_name.clone(),
                    &self.theme,
                )
                .w(px(204.))
                .flex_none()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(border_selected)
                .bg(self.theme.card())
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.flow_modal_branch = Some(bid3.clone());
                    panel.flow_modal_override_ack = false;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_none()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(format_color)
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(self.theme.on_ink())
                        .child(branch.format.tag()),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(branch.deliverable_name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(match &branch.phase {
                                    BranchPhase::Delivered {
                                        hash_short, ..
                                    } if !hash_short.is_empty() => {
                                        format!("sha256:{hash_short}")
                                    }
                                    _ => self.t("chat.deliverables"),
                                }),
                        ),
                ),
            )
            .into_any_element()
    }

    fn modal_tab_button(
        &self,
        tab: FlowModalTab,
        label: String,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let selected = self.flow_modal_tab == tab;
        theme::interactive(
            match tab {
                FlowModalTab::Action => "fm-tab-action",
                FlowModalTab::Trace => "fm-tab-trace",
            },
            label.clone(),
            &self.theme,
        )
        .px(self.theme.space(10.))
        .py(px(4.))
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .bg(if selected {
            self.theme.card()
        } else {
            self.theme.soft()
        })
        .text_color(if selected {
            self.theme.ink()
        } else {
            self.theme.muted()
        })
        .child(label)
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.flow_modal_tab = tab;
            cx.notify();
        }))
        .into_any_element()
    }

    fn modal_action_panel(
        &self,
        group: &SessionBoardGroup,
        branch: &WorkBranchSummary,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let left_card = match &branch.phase {
            BranchPhase::NeedsApproval {
                report_id,
                artifact_path,
                summary,
                ..
            } => {
                let ack = self.flow_modal_override_ack;
                let has_report = !report_id.trim().is_empty();
                let sid = group.session_id.clone();
                let rid = report_id.clone();
                let apath = artifact_path.clone();
                div()
                    .flex_1()
                    .min_w(px(240.))
                    .rounded_xl()
                    .border_1()
                    .border_color(self.theme.status_attention().opacity(0.5))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .rounded_t_xl()
                            .px(self.theme.space(12.))
                            .py(self.theme.space(8.))
                            .bg(self.theme.status_attention_bg())
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.status_attention())
                            .child(self.t("board.approval_heading")),
                    )
                    .child(
                        div()
                            .p(self.theme.space(12.))
                            .bg(self.theme.canvas())
                            .flex()
                            .flex_col()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.ink())
                                    .whitespace_normal()
                                    .child(summary.clone()),
                            )
                            .child(
                                theme::interactive(
                                    "fm-override-ack",
                                    self.t("verify.override.confirm_label"),
                                    &self.theme,
                                )
                                .flex()
                                .items_center()
                                .gap(self.theme.space(6.))
                                .cursor_pointer()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .on_click(cx.listener(|panel, _, _, cx| {
                                    panel.flow_modal_override_ack =
                                        !panel.flow_modal_override_ack;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .size(px(14.))
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(if ack {
                                            self.theme.status_attention()
                                        } else {
                                            self.theme.border_strong()
                                        })
                                        .bg(if ack {
                                            self.theme.status_attention()
                                        } else {
                                            self.theme.card()
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_xs()
                                        .text_color(self.theme.on_ink())
                                        .child(if ack {
                                            "\u{2713}"
                                        } else {
                                            ""
                                        }),
                                )
                                .child(self.t("verify.override.confirm_label")),
                            )
                            .child(
                                theme::Button::new(
                                    "fm-override-submit",
                                    self.t("verify.override.action"),
                                )
                                .variant(theme::ControlVariant::Primary)
                                .size(theme::ControlSize::Small)
                                .disabled(!(ack && has_report))
                                .render(
                                    &self.theme,
                                    cx.listener(move |panel, _, _, cx| {
                                        panel.confirm_override_for_session(
                                            &sid, &rid, &apath, cx,
                                        );
                                    }),
                                ),
                            ),
                    )
                    .into_any_element()
            }
            _ => div()
                .flex_1()
                .min_w(px(240.))
                .p(self.theme.space(12.))
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.canvas())
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(self.theme.ink())
                        .child(branch.skill_label.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(branch.detail.clone()),
                )
                .into_any_element(),
        };

        let file_path = branch.deliverable_path.clone();
        let file_card = div()
            .flex_1()
            .min_w(px(240.))
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(branch.deliverable_name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(file_path.as_deref().map_or_else(
                                || group.workspace_display.clone(),
                                display_path_str,
                            )),
                    ),
            )
            .when_some(file_path, |card, path| {
                let open_path = path.clone();
                let reveal_path = path;
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
                        .child(
                            theme::Button::new(
                                "fm-open-file",
                                self.t("deliverables.open_external"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    if open::that(&open_path).is_err() {
                                        panel.emit_notice(
                                            Some("library.error.open"),
                                            cx,
                                        );
                                    }
                                }),
                            ),
                        )
                        .child(
                            theme::Button::new(
                                "fm-reveal-folder",
                                self.t("board.reveal_folder"),
                            )
                            .variant(theme::ControlVariant::Ghost)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.reveal_file_in_os(&reveal_path, cx);
                                }),
                            ),
                        ),
                )
            });

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(self.theme.space(12.))
            .child(left_card)
            .child(file_card)
            .into_any_element()
    }

    pub(crate) fn modal_trace_panel(
        &self,
        session_id: &str,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let entries = collect_session_traces(
            &self.agent,
            session_id,
            &self.pending_receipts,
        );
        let filtered: Vec<_> = entries
            .iter()
            .filter(|entry| match self.trace_filter {
                TraceFilter::All => true,
                TraceFilter::WarnAndError => {
                    matches!(entry.level, TraceLevel::Warn | TraceLevel::Error)
                }
            })
            .collect();
        let copy_jsonl = format_traces_jsonl(&entries);

        let controls = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(self.trace_filter_pill(
                        TraceFilter::All,
                        self.t("trace.filter_all"),
                        cx,
                    ))
                    .child(self.trace_filter_pill(
                        TraceFilter::WarnAndError,
                        self.t("trace.filter_warn"),
                        cx,
                    )),
            )
            .child(
                theme::Button::new(
                    "trace-copy-jsonl",
                    self.t("trace.copy_jsonl"),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(move |panel, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            copy_jsonl.clone(),
                        ));
                        panel.emit_notice(Some("trace.copied"), cx);
                    }),
                ),
            );

        let mut list = div().w_full().flex().flex_col().gap(px(6.));
        if filtered.is_empty() {
            list = list.child(
                div()
                    .p(self.theme.space(12.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.45))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t("trace.empty")),
            );
        } else {
            for (idx, entry) in filtered.iter().rev().take(60).enumerate() {
                let (bg, fg) = match entry.level {
                    TraceLevel::Info => (self.theme.canvas(), self.theme.ink()),
                    TraceLevel::Warn => (
                        self.theme.status_attention_bg(),
                        self.theme.status_attention(),
                    ),
                    TraceLevel::Error => {
                        (self.theme.danger_bg(), self.theme.danger())
                    }
                };
                list = list.child(
                    div()
                        .id(("trace-row", idx as u64))
                        .w_full()
                        .px(self.theme.space(10.))
                        .py(px(6.))
                        .rounded_lg()
                        .border_1()
                        .border_color(self.theme.line())
                        .bg(bg)
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg)
                                        .child(entry.event),
                                )
                                .child(
                                    div()
                                        .text_color(self.theme.muted())
                                        .child(format!("#{}", entry.seq)),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .whitespace_normal()
                                .child(entry.summary.clone()),
                        ),
                );
            }
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(controls)
            .child(list)
            .into_any_element()
    }

    fn trace_filter_pill(
        &self,
        filter: TraceFilter,
        label: String,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let selected = self.trace_filter == filter;
        theme::interactive(
            match filter {
                TraceFilter::All => "trace-filter-all",
                TraceFilter::WarnAndError => "trace-filter-warn",
            },
            label.clone(),
            &self.theme,
        )
        .px(self.theme.space(8.))
        .py(px(3.))
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
            self.theme.muted()
        })
        .child(label)
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.trace_filter = filter;
            cx.notify();
        }))
        .into_any_element()
    }
}
