//! Responsive Global Visual Board (`StudioSurface::Board`).
//!
//! Submodules keep concerns separated without extra abstraction layers:
//! - `overview/mod.rs`: KPI strip, filter bar, and session-layered board cards.
//! - `overview/flow_modal.rs`: centered top-level Flowchart Modal & branch actions.
//! - `overview/trace_panel.rs`: shared redacted Trace panel, contract progress
//!   card, and `ReceiptState` visuals.

mod flow_modal;
mod trace_panel;

use flowy_ui as theme;
use flowy_ui::charts::{DonutSegment, donut_chart};
use flowy_ui::status_pill;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};

use crate::panel::StudioPanel;
use crate::ports::{BranchPhase, DeliverableFormat, SessionBoardGroup};

pub use trace_panel::TraceFilter;

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

impl StudioPanel {
    pub(super) fn format_color(&self, format: DeliverableFormat) -> gpui::Rgba {
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
            let active_groups: Vec<&SessionBoardGroup> =
                filtered.iter().copied().filter(|g| g.is_active).collect();
            let settled_groups: Vec<&SessionBoardGroup> =
                filtered.iter().copied().filter(|g| !g.is_active).collect();
            let show_headers =
                !active_groups.is_empty() && !settled_groups.is_empty();
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
                    sessions_col = sessions_col
                        .child(self.board_session_card(group, card_idx, cx));
                    card_idx += 1;
                }
            }
            if !settled_groups.is_empty() {
                if show_headers {
                    sessions_col = sessions_col.child(
                        div()
                            .pt(self.theme.space(6.))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .child(self.tf(
                                "board.group_settled",
                                &[("count", &settled_groups.len().to_string())],
                            )),
                    );
                }
                for group in settled_groups {
                    sessions_col = sessions_col
                        .child(self.board_session_card(group, card_idx, cx));
                    card_idx += 1;
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

        let tile_donut = div()
            .flex_1()
            .min_w(px(190.))
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
        .min_w(px(190.))
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
            .min_w(px(190.))
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
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
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
                    )
                    .when(kpi.other_count > 0, |row| {
                        row.child(
                            div()
                                .text_color(self.theme.file_kind_other())
                                .child(format!("FILE {}", kpi.other_count)),
                        )
                    }),
            );

        let mut spark_bars =
            div().w_full().h(px(22.)).flex().items_end().gap(px(4.));
        for (idx, group) in
            self.board_snapshot.sessions.iter().take(8).enumerate()
        {
            let has_deliverables = group
                .branches
                .iter()
                .any(|b| matches!(b.phase, BranchPhase::Delivered { .. }));
            let bar_h =
                (group.branches.len() as f32).mul_add(4.0, 8.0).min(22.0);
            let color = if group.has_blocker {
                self.theme.status_attention()
            } else if group.is_running {
                self.theme.status_active()
            } else if has_deliverables {
                self.theme.status_verified().opacity(0.6)
            } else {
                self.theme.line()
            };
            let sid = group.session_id.clone();
            let spark_label = format!(
                "{} \u{00b7} {}",
                self.t("board.kpi_sessions"),
                group.title
            );
            spark_bars = spark_bars.child(
                theme::interactive(
                    ("board-spark", idx as u64),
                    spark_label,
                    &self.theme,
                )
                .flex_1()
                .min_w(px(10.))
                .max_w(px(28.))
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
            .min_w(px(190.))
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

        // Two paired rows guarantee 4x1 on wide viewports and a symmetric 2x2
        // grid at 960px (never a 3+1 orphan row).
        let pair_left = div()
            .flex_1()
            .min_w(px(420.))
            .flex()
            .gap(self.theme.space(12.))
            .child(tile_donut)
            .child(tile_blocker);
        let pair_right = div()
            .flex_1()
            .min_w(px(420.))
            .flex()
            .gap(self.theme.space(12.))
            .child(tile_formats)
            .child(tile_sessions);

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(self.theme.space(12.))
            .child(pair_left)
            .child(pair_right)
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

        if !self.projects.is_empty() {
            pills = pills.child(
                div().w(px(1.)).h(px(14.)).mx(px(2.)).bg(self.theme.line()),
            );
            for (p_idx, project) in self.projects.iter().take(4).enumerate() {
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

        if !group.is_active {
            let file_branches: Vec<_> = group
                .branches
                .iter()
                .filter(|b| {
                    b.deliverable_path.is_some()
                        || !matches!(b.phase, BranchPhase::Idle)
                })
                .take(3)
                .collect();
            let has_deliverables = !file_branches.is_empty();
            let mut file_chips =
                div().flex().flex_wrap().items_center().gap(px(6.));
            for branch in file_branches {
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
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(
                        if has_deliverables {
                            self.theme.status_verified()
                        } else {
                            self.theme.muted().opacity(0.6)
                        },
                    ))
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
                    .child(status_pill(
                        self.theme.soft(),
                        self.theme.muted(),
                        project_label,
                        None,
                    )),
            )
            .when(has_deliverables, |row| row.child(file_chips))
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
                BranchPhase::Running { step_label, .. } => {
                    let step = if step_label.is_empty() {
                        self.t("chat.status_running")
                    } else {
                        step_label.clone()
                    };
                    (
                        self.theme.status_active(),
                        format!(
                            "{}% \u{00b7} {step}",
                            branch.progress_pct as u32
                        ),
                        true,
                    )
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
                                .child(
                                    branch.skill_label.resolve(|k| self.t(k)),
                                ),
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
                        .child(status_pill(
                            self.theme.soft(),
                            self.theme.muted(),
                            project_label,
                            None,
                        ))
                        .child(status_pill(
                            badge_bg, badge_fg, badge_text, None,
                        )),
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
}
