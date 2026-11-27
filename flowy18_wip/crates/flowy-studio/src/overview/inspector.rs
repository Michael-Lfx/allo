//! Master-Detail Inspector panel and narrow-viewport slide drawer for `StudioSurface::Board`.
//!
//! Replaces the retired `FlowModal` popup with three high-signal tabs:
//! 1. `Overview` (审批与成果): blocker / active controls, inline rich preview via
//!    `drawer_preview`, full uncapped formal deliverables, collapsible workspace
//!    script changes, assistant answer summary, and a sticky non-scrolling
//!    approval footer so primary actions never clip on 900x640 / 1080x720.
//! 2. `Timeline` (步骤时间线): clickable Router -> Worker delegation strip plus
//!    reused `drawer_steps::structured_steps_view`.
//! 3. `Trace`: reused redacted `trace_panel::modal_trace_panel`.

use flowy_agent::{ChatLine, ReceiptState, VerificationIssue};
use flowy_ui as theme;
use flowy_ui::status_pill;
use gpui::{
    AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px,
};

use crate::deliverables::short_parent_path;
use crate::panel::StudioPanel;
use crate::ports::{
    BranchPhase, SessionBoardGroup, WorkBranchSummary, display_path_str,
};

/// Active tab inside the Master-Detail Board Inspector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardInspectorTab {
    #[default]
    Overview,
    Timeline,
    Trace,
}

enum StickyFooterAction {
    OverrideApproval {
        action_sid: String,
        report_id: String,
        artifact_path: String,
    },
    DeliveryFailed {
        retry_sid: String,
    },
    OpenApprovalChat {
        open_sid: String,
    },
}

impl StudioPanel {
    /// Renders the right-hand Master-Detail Inspector or narrow slide drawer.
    pub(super) fn board_inspector_panel(
        &self,
        is_narrow_drawer: bool,
        now_ms: u64,
        tz_offset_secs: i32,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let Some(group) = self.resolve_board_inspector() else {
            return self.inspector_empty_state(is_narrow_drawer, cx);
        };
        let active_branch = self.selected_board_branch(group);
        let sticky_action = Self::sticky_footer_action(group, active_branch);

        let body: AnyElement = match self.board_inspector.tab {
            BoardInspectorTab::Overview => {
                self.inspector_overview_tab(group, active_branch, window, cx)
            }
            BoardInspectorTab::Timeline => {
                self.inspector_timeline_tab(group, window, cx)
            }
            BoardInspectorTab::Trace => {
                let trace_sid = self
                    .board_inspector
                    .worker_session
                    .as_deref()
                    .unwrap_or(group.session_id.as_str());
                div()
                    .id("board-inspector-trace-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .scrollbar_width(px(6.))
                    .child(
                        div()
                            .w_full()
                            .px(self.theme.space(16.))
                            .py(self.theme.space(12.))
                            .child(self.modal_trace_panel(trace_sid, cx)),
                    )
                    .into_any_element()
            }
        };

        let mut shell = div()
            .id(if is_narrow_drawer {
                "board-inspector-drawer"
            } else {
                "board-inspector-pane"
            })
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .overflow_hidden();

        if is_narrow_drawer {
            shell = shell
                .border_l_1()
                .border_color(self.theme.border_strong())
                .rounded_l(self.theme.modal_radius());
        } else {
            shell = shell
                .border_1()
                .border_color(self.theme.line())
                .rounded_2xl();
        }

        shell
            .child(self.inspector_header(
                group,
                is_narrow_drawer,
                now_ms,
                tz_offset_secs,
                window,
                cx,
            ))
            .child(self.inspector_tab_bar(is_narrow_drawer, cx))
            .child(body)
            .when_some(sticky_action, |panel, action| {
                panel.child(self.inspector_sticky_footer(
                    action,
                    is_narrow_drawer,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn selected_board_branch<'a>(
        &self,
        group: &'a SessionBoardGroup,
    ) -> Option<&'a WorkBranchSummary> {
        self.board_inspector
            .branch
            .as_deref()
            .and_then(|bid| {
                group
                    .branches
                    .iter()
                    .chain(group.workspace_changes.iter())
                    .find(|b| b.id == bid)
            })
            .or_else(|| group.branches.iter().find(|b| b.phase.is_blocker()))
            .or_else(|| group.branches.first())
    }

    fn sticky_footer_action(
        group: &SessionBoardGroup,
        selected_branch: Option<&WorkBranchSummary>,
    ) -> Option<StickyFooterAction> {
        let approval = selected_branch
            .filter(|b| matches!(b.phase, BranchPhase::NeedsApproval { .. }))
            .or_else(|| {
                group.branches.iter().find(|b| {
                    matches!(b.phase, BranchPhase::NeedsApproval { .. })
                })
            });
        if let Some(branch) = approval
            && let BranchPhase::NeedsApproval {
                report_id,
                artifact_path,
                ..
            } = &branch.phase
        {
            let action_sid = branch
                .owner_session_id
                .clone()
                .unwrap_or_else(|| group.session_id.clone());
            if !report_id.trim().is_empty() {
                return Some(StickyFooterAction::OverrideApproval {
                    action_sid,
                    report_id: report_id.clone(),
                    artifact_path: artifact_path.clone(),
                });
            }
            return Some(StickyFooterAction::OpenApprovalChat {
                open_sid: group.session_id.clone(),
            });
        }

        let delivery_fail = selected_branch
            .filter(|b| matches!(b.phase, BranchPhase::DeliveryFailed))
            .or_else(|| {
                group
                    .branches
                    .iter()
                    .find(|b| matches!(b.phase, BranchPhase::DeliveryFailed))
            });
        if let Some(branch) = delivery_fail {
            let retry_sid = branch
                .owner_session_id
                .clone()
                .unwrap_or_else(|| group.session_id.clone());
            return Some(StickyFooterAction::DeliveryFailed { retry_sid });
        }
        None
    }

    fn inspector_empty_state(
        &self,
        is_narrow_drawer: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        div()
            .id("board-inspector-empty")
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded_2xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .p(self.theme.space(24.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(self.theme.space(8.))
            .when(is_narrow_drawer, |box_el| {
                box_el.child(
                    theme::Button::new(
                        "board-empty-close-drawer",
                        self.t("board.close_drawer"),
                    )
                    .variant(theme::ControlVariant::Secondary)
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.close_board_drawer(cx);
                        }),
                    ),
                )
            })
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .whitespace_normal()
                    .child(self.t("board.inspector_empty_title")),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("board.inspector_empty_detail")),
            )
            .into_any_element()
    }

    fn inspector_header(
        &self,
        group: &SessionBoardGroup,
        is_narrow_drawer: bool,
        now_ms: u64,
        tz_offset_secs: i32,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let open_sid = group.session_id.clone();
        let can_open_session = !group.is_synthetic();
        let rel_time =
            self.format_relative_time(group.updated_at, now_ms, tz_offset_secs);
        let formal_count = group
            .branches
            .iter()
            .filter(|b| b.deliverable_path.is_some())
            .count();

        let close_btn = is_narrow_drawer.then(|| {
            self.icon_tooltip_button(
                "board-drawer-close",
                "board-drawer-close-tip",
                self.t("board.close_drawer"),
                theme::icons::x().size(px(14.)),
                theme::ControlVariant::Ghost,
                |panel, _, cx| {
                    panel.close_board_drawer(cx);
                },
                window,
                cx,
            )
        });

        let mut header = div()
            .w_full()
            .px(self.theme.space(16.))
            .py(self.theme.space(12.))
            .border_b_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .items_start()
            .justify_between()
            .gap(self.theme.space(10.));
        if is_narrow_drawer {
            header = header.rounded_tl(self.theme.modal_radius());
        } else {
            header = header.rounded_t_2xl();
        }

        header
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .whitespace_normal()
                            .child(group.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .when_some(
                                group.project_name.clone(),
                                |row, proj| {
                                    row.child(status_pill(
                                        self.theme.soft(),
                                        self.theme.muted(),
                                        proj,
                                        None,
                                    ))
                                },
                            )
                            .when(group.has_blocker, |row| {
                                row.child(status_pill(
                                    self.theme.status_attention_bg(),
                                    self.theme.status_attention(),
                                    self.t("board.badge_approval"),
                                    None,
                                ))
                            })
                            .when(
                                !group.has_blocker && group.is_running,
                                |row| {
                                    row.child(status_pill(
                                        self.theme.status_active_bg(),
                                        self.theme.status_active(),
                                        self.t("board.badge_running"),
                                        None,
                                    ))
                                },
                            )
                            .when(formal_count > 0, |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .whitespace_nowrap()
                                        .child(self.tf(
                                            "board.branch_count",
                                            &[(
                                                "count",
                                                &formal_count.to_string(),
                                            )],
                                        )),
                                )
                            })
                            .when(!rel_time.is_empty(), |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(
                                            self.theme.muted().opacity(0.85),
                                        )
                                        .whitespace_nowrap()
                                        .child(rel_time),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .when(can_open_session, |row| {
                        row.child(
                            theme::Button::new(
                                "board-inspector-open-chat",
                                self.t("board.open_chat"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .trailing_icon(
                                theme::icons::arrow_up_right().size(px(12.)),
                            )
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.close_board_drawer(cx);
                                    panel.open_session(&open_sid, cx);
                                }),
                            ),
                        )
                    })
                    .children(close_btn),
            )
            .into_any_element()
    }

    fn inspector_tab_bar(
        &self,
        is_narrow_drawer: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let tabs = [
            (
                BoardInspectorTab::Overview,
                "board-tab-overview",
                self.t("board.inspector_tab_overview"),
            ),
            (
                BoardInspectorTab::Timeline,
                "board-tab-timeline",
                self.t("board.inspector_tab_steps"),
            ),
            (
                BoardInspectorTab::Trace,
                "board-tab-trace",
                self.t("board.inspector_tab_trace"),
            ),
        ];

        let mut bar = div()
            .w_full()
            .px(self.theme.space(16.))
            .py(self.theme.space(8.))
            .border_b_1()
            .border_color(self.theme.line())
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(6.));

        let mut group = div().flex().items_center().gap(self.theme.space(6.));
        for (tab, id, label) in tabs {
            let active = self.board_inspector.tab == tab;
            group = group.child(
                theme::interactive(id, label.clone(), &self.theme)
                    .px(self.theme.space(10.))
                    .py(px(5.))
                    .rounded_lg()
                    .cursor_pointer()
                    .text_xs()
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .bg(if active {
                        self.theme.soft()
                    } else {
                        self.theme.card()
                    })
                    .text_color(if active {
                        self.theme.ink()
                    } else {
                        self.theme.muted()
                    })
                    .hover(|s| s.bg(self.theme.soft().opacity(0.75)))
                    .child(label)
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.select_board_tab(tab, cx);
                    })),
            );
        }
        bar = bar.child(group);
        if is_narrow_drawer {
            bar = bar.child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted().opacity(0.8))
                    .whitespace_nowrap()
                    .child(self.t("board.esc_hint")),
            );
        }
        bar.into_any_element()
    }

    fn inspector_overview_tab(
        &self,
        group: &SessionBoardGroup,
        active_branch: Option<&WorkBranchSummary>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        if let (Some(path), Some(body)) =
            (&self.drawer_preview.file, &self.drawer_preview.body)
        {
            return div()
                .id("board-inspector-preview-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(6.))
                .child(
                    div()
                        .w_full()
                        .px(self.theme.space(16.))
                        .py(self.theme.space(12.))
                        .child(
                            div()
                                .w_full()
                                .rounded_xl()
                                .border_1()
                                .border_color(self.theme.line())
                                .bg(self.theme.canvas())
                                .overflow_hidden()
                                .child(self.drawer_preview_view(
                                    path, body, window, cx,
                                )),
                        ),
                )
                .into_any_element();
        }

        let mut col = div()
            .w_full()
            .px(self.theme.space(16.))
            .py(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.));

        for card in self.inspector_status_cards(group, active_branch, cx) {
            col = col.child(card);
        }

        let formal: Vec<&WorkBranchSummary> = group
            .branches
            .iter()
            .filter(|b| b.deliverable_path.is_some())
            .collect();
        if !formal.is_empty() {
            col = col.child(self.inspector_deliverables_section(
                "board-formal-deliverables",
                self.tf(
                    "board.section_formal_deliverables",
                    &[("count", &formal.len().to_string())],
                ),
                &formal,
                window,
                cx,
            ));
        }

        if !group.workspace_changes.is_empty() {
            let expanded = self.board_inspector.workspace_changes_expanded
                || formal.is_empty();
            col = col.child(self.inspector_workspace_changes_section(
                &group.workspace_changes,
                expanded,
                window,
                cx,
            ));
        }

        if let Some(summary) = group.assistant_summary.as_ref() {
            col = col.child(self.inspector_answer_summary_card(summary));
        }

        if formal.is_empty()
            && group.workspace_changes.is_empty()
            && group.assistant_summary.is_none()
            && !group.has_blocker
            && !group.is_running
        {
            col = col.child(
                div()
                    .p(self.theme.space(12.))
                    .rounded_xl()
                    .bg(self.theme.soft().opacity(0.45))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("board.no_blockers")),
            );
        }

        div()
            .id("board-inspector-overview-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .scrollbar_width(px(6.))
            .child(col)
            .into_any_element()
    }

    fn inspector_status_cards(
        &self,
        group: &SessionBoardGroup,
        active_branch: Option<&WorkBranchSummary>,
        cx: &Context<'_, Self>,
    ) -> Vec<AnyElement> {
        let mut cards = Vec::new();

        // Surface NeedsApproval detail cards cleanly (CTA button lives in sticky footer).
        for branch in &group.branches {
            if let BranchPhase::NeedsApproval {
                artifact_path,
                summary,
                issues,
                semantic_hint,
                provenance_warnings,
                ..
            } = &branch.phase
            {
                cards.push(self.inspector_approval_evidence_card(
                    artifact_path,
                    summary,
                    issues,
                    semantic_hint.as_deref(),
                    provenance_warnings,
                    active_branch.is_some_and(|a| a.id == branch.id),
                ));
            }
        }

        // Running / Queued / DeliveryFailed status controls.
        for branch in &group.branches {
            match &branch.phase {
                BranchPhase::DeliveryFailed => {
                    cards.push(
                        self.inspector_delivery_failed_banner(group, branch),
                    );
                }
                BranchPhase::Running { .. } => {
                    cards.push(self.inspector_running_card(group, branch, cx));
                }
                BranchPhase::Queued { receipt_id, state } => {
                    cards.push(
                        self.inspector_queued_card(
                            branch, receipt_id, *state, cx,
                        ),
                    );
                }
                BranchPhase::NeedsApproval { .. }
                | BranchPhase::Delivered { .. }
                | BranchPhase::Idle => {}
            }
        }

        if cards.is_empty()
            && let Some(prog) = group.session_contract.as_ref()
        {
            cards.push(self.contract_progress_checklist(prog));
        }

        cards
    }

    fn inspector_approval_evidence_card(
        &self,
        artifact_path: &str,
        summary: &str,
        issues: &[VerificationIssue],
        semantic_hint: Option<&str>,
        provenance_warnings: &[String],
        highlighted: bool,
    ) -> AnyElement {
        let mut body = div()
            .p(self.theme.space(12.))
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .whitespace_normal()
                    .child(crate::ports::basename(artifact_path)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(summary.to_owned()),
            );

        for issue in issues.iter().take(6) {
            let kind = self.t(flowy_agent::verify_ui_key(&issue.code));
            let row_text = if issue.field.trim().is_empty() {
                kind
            } else {
                format!("{kind} \u{00b7} {}", issue.field)
            };
            body = body.child(
                div()
                    .text_xs()
                    .text_color(self.theme.status_attention())
                    .whitespace_normal()
                    .child(row_text),
            );
        }

        if let Some(hint) = semantic_hint
            && !hint.trim().is_empty()
        {
            body = body.child(
                div()
                    .px(self.theme.space(8.))
                    .py(px(4.))
                    .rounded_md()
                    .bg(self.theme.status_attention_bg())
                    .text_xs()
                    .text_color(self.theme.ink())
                    .whitespace_normal()
                    .child(self.tf("verify.semantic.hint", &[("hint", hint)])),
            );
        }

        if !provenance_warnings.is_empty() {
            body = body.child(
                div()
                    .px(self.theme.space(8.))
                    .py(px(4.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .text_xs()
                    .text_color(self.theme.status_attention())
                    .whitespace_normal()
                    .child(format!(
                        "{}: {}",
                        self.t("verify.provenance_invalid"),
                        provenance_warnings.join(", ")
                    )),
            );
        }

        div()
            .w_full()
            .rounded_xl()
            .border_1()
            .border_color(if highlighted {
                self.theme.status_attention()
            } else {
                self.theme.status_attention().opacity(0.5)
            })
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
            .child(body)
            .into_any_element()
    }

    fn inspector_delivery_failed_banner(
        &self,
        group: &SessionBoardGroup,
        branch: &WorkBranchSummary,
    ) -> AnyElement {
        let mut card = div()
            .w_full()
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.status_attention().opacity(0.5))
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.status_attention())
                    .child(self.t("chat.status_delivery_failed")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(branch.detail.clone()),
            );
        if let Some(prog) = branch
            .work_progress
            .as_ref()
            .or(group.session_contract.as_ref())
        {
            card = card.child(self.contract_progress_checklist(prog));
        }
        card.into_any_element()
    }

    fn inspector_running_card(
        &self,
        group: &SessionBoardGroup,
        branch: &WorkBranchSummary,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let stop_sid = branch
            .owner_session_id
            .clone()
            .unwrap_or_else(|| group.session_id.clone());
        let can_stop = !group.is_synthetic();
        div()
            .w_full()
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.status_active().opacity(0.45))
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .child(crate::motion::live_dot(
                                SharedString::from(format!(
                                    "bi-live-{}",
                                    branch.id
                                )),
                                self.theme.status_active(),
                                self.theme.motion_policy(),
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(self.theme.ink())
                                    .child(
                                        branch
                                            .skill_label
                                            .resolve(|k| self.t(k)),
                                    ),
                            ),
                    )
                    .when(can_stop, |row| {
                        row.child(
                            theme::Button::new(
                                "bi-stop-session",
                                self.t("drawer.work_stop"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.stop_session_turn(&stop_sid, cx);
                                }),
                            ),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(branch.detail.clone()),
            )
            .when_some(
                branch
                    .work_progress
                    .as_ref()
                    .or(group.session_contract.as_ref()),
                |card, prog| card.child(self.contract_progress_checklist(prog)),
            )
            .into_any_element()
    }

    fn inspector_queued_card(
        &self,
        branch: &WorkBranchSummary,
        receipt_id: &str,
        state: ReceiptState,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let (state_color, state_label, _) = self.receipt_state_visual(state);
        let cancel_id = receipt_id.to_owned();
        let retry_id = receipt_id.to_owned();
        let can_cancel = matches!(
            state,
            ReceiptState::Queued
                | ReceiptState::Running
                | ReceiptState::Retrying
                | ReceiptState::Quarantined
        );
        let is_dead = state == ReceiptState::Dead;
        let retrying = self.retrying_receipts.contains(receipt_id);
        div()
            .w_full()
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(if is_dead {
                self.theme.danger().opacity(0.5)
            } else {
                self.theme.line()
            })
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(branch.skill_label.resolve(|k| self.t(k))),
                    )
                    .child(status_pill(
                        self.theme.soft(),
                        state_color,
                        state_label,
                        None,
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(branch.detail.clone()),
            )
            .when_some(branch.work_progress.as_ref(), |card, prog| {
                card.child(self.contract_progress_checklist(prog))
            })
            .when(can_cancel || is_dead, |card| {
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
                        .when(is_dead, |row| {
                            row.child(
                                theme::Button::new(
                                    SharedString::from(format!(
                                        "bi-retry-{retry_id}"
                                    )),
                                    self.t("board.retry_dead_receipt"),
                                )
                                .variant(theme::ControlVariant::Primary)
                                .size(theme::ControlSize::Small)
                                .disabled(retrying)
                                .render(
                                    &self.theme,
                                    cx.listener(move |panel, _, _, cx| {
                                        panel.retry_dead_receipt(&retry_id, cx);
                                    }),
                                ),
                            )
                        })
                        .when(can_cancel, |row| {
                            row.child(
                                theme::Button::new(
                                    SharedString::from(format!(
                                        "bi-cancel-{cancel_id}"
                                    )),
                                    self.t("board.cancel_receipt"),
                                )
                                .variant(theme::ControlVariant::Secondary)
                                .size(theme::ControlSize::Small)
                                .render(
                                    &self.theme,
                                    cx.listener(move |panel, _, _, cx| {
                                        panel.cancel_receipt(&cancel_id, cx);
                                    }),
                                ),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn inspector_deliverables_section(
        &self,
        id_prefix: &'static str,
        heading: String,
        items: &[&WorkBranchSummary],
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let mut section = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.muted())
                    .child(heading),
            );

        for (ix, branch) in items.iter().enumerate() {
            section = section.child(self.inspector_deliverable_row(
                id_prefix, ix as u64, branch, window, cx,
            ));
        }
        section.into_any_element()
    }

    fn inspector_workspace_changes_section(
        &self,
        changes: &[WorkBranchSummary],
        expanded: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let heading = self.tf(
            "board.section_workspace_changes",
            &[("count", &changes.len().to_string())],
        );
        let chevron = if expanded {
            theme::icons::chevron_down()
        } else {
            theme::icons::chevron_right()
        };

        let mut section = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .child(
                theme::interactive(
                    "board-workspace-changes-toggle",
                    heading.clone(),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(8.))
                .py(px(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|s| s.bg(self.theme.soft()))
                .flex()
                .items_center()
                .gap(self.theme.space(6.))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.board_inspector.workspace_changes_expanded =
                        !panel.board_inspector.workspace_changes_expanded;
                    cx.notify();
                }))
                .child(chevron.size(px(13.)).text_color(self.theme.muted()))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.muted())
                        .child(heading),
                ),
            );

        if expanded {
            for (ix, branch) in changes.iter().enumerate() {
                section = section.child(self.inspector_deliverable_row(
                    "board-ws-change",
                    ix as u64,
                    branch,
                    window,
                    cx,
                ));
            }
        }
        section.into_any_element()
    }

    fn inspector_deliverable_row(
        &self,
        id_prefix: &'static str,
        index: u64,
        branch: &WorkBranchSummary,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let Some(path) = branch.deliverable_path.as_ref() else {
            return div().into_any_element();
        };
        let fmt_color = self.format_color(branch.format);
        let fmt_tag = self.format_tag_label(branch.format);
        let parent = short_parent_path(path);
        let full_clean = display_path_str(path);
        let meta_line = if parent.is_empty() {
            full_clean
        } else {
            parent
        };
        let preview_path = path.clone();
        let row_preview_path = path.clone();
        let open_path = path.clone();
        let reveal_path = path.clone();
        let can_open = flowy_agent::can_open_with_default_app(path);
        let can_preview =
            !matches!(branch.phase, BranchPhase::NeedsApproval { .. });
        let is_previewing =
            self.drawer_preview.file.as_deref() == Some(path.as_str());

        let (status_bg, status_fg, status_text) = match &branch.phase {
            BranchPhase::NeedsApproval { .. } => (
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
                self.t("board.badge_approval"),
            ),
            BranchPhase::Delivered { override_used, .. } => (
                if *override_used {
                    self.theme.status_attention_bg()
                } else {
                    self.theme.status_verified_bg()
                },
                if *override_used {
                    self.theme.status_attention()
                } else {
                    self.theme.status_verified()
                },
                if *override_used {
                    self.t("verify.override.badge")
                } else {
                    self.t("verify.card.passed")
                },
            ),
            _ => (
                self.theme.soft(),
                self.theme.muted(),
                self.format_tag_label(branch.format),
            ),
        };

        let preview_btn = can_preview.then(|| {
            theme::Button::new(
                (id_prefix, index * 10 + 1),
                format!(
                    "{} · {}",
                    self.t("board.preview_file"),
                    branch.deliverable_name
                ),
            )
            .variant(if is_previewing {
                theme::ControlVariant::Primary
            } else {
                theme::ControlVariant::Secondary
            })
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.set_preview_file(Some(preview_path.clone()), cx);
                }),
            )
        });

        let open_btn = can_open.then(|| {
            self.icon_tooltip_button(
                (id_prefix, index * 10 + 2),
                format!("{id_prefix}-open-tip-{index}"),
                format!(
                    "{} · {}",
                    self.t("deliverables.open_external"),
                    branch.deliverable_name
                ),
                theme::icons::external_link().size(px(14.)),
                theme::ControlVariant::Ghost,
                move |panel, _, cx| {
                    cx.stop_propagation();
                    panel.open_file_externally(&open_path, cx);
                },
                window,
                cx,
            )
        });

        let reveal_btn = self.icon_tooltip_button(
            (id_prefix, index * 10 + 3),
            format!("{id_prefix}-reveal-tip-{index}"),
            format!(
                "{} · {}",
                self.t("board.reveal_folder"),
                branch.deliverable_name
            ),
            theme::icons::folder_open().size(px(14.)),
            theme::ControlVariant::Ghost,
            move |panel, _, cx| {
                cx.stop_propagation();
                panel.reveal_file(&reveal_path, cx);
            },
            window,
            cx,
        );

        let mut row = theme::interactive(
            (id_prefix, index * 10),
            SharedString::from(branch.deliverable_name.clone()),
            &self.theme,
        )
        .w_full()
        .min_w_0()
        .px(self.theme.space(10.))
        .py(self.theme.space(8.))
        .rounded_xl()
        .border_1()
        .border_color(if is_previewing {
            self.theme.accent()
        } else {
            self.theme.line()
        })
        .bg(self.theme.canvas())
        .cursor_pointer()
        .hover(|s| s.bg(self.theme.soft().opacity(0.6)))
        .flex()
        .flex_col()
        .gap(px(6.));

        if can_preview {
            row = row.on_click(cx.listener(move |panel, _, _, cx| {
                panel.set_preview_file(Some(row_preview_path.clone()), cx);
            }));
        }

        row.child(
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
                        .gap(self.theme.space(8.))
                        .child(
                            div()
                                .flex_none()
                                .mt(px(1.))
                                .px(px(6.))
                                .py(px(1.))
                                .rounded_sm()
                                .bg(fmt_color)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(self.theme.on_ink())
                                .child(fmt_tag),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(self.theme.ink())
                                        .whitespace_normal()
                                        .child(branch.deliverable_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .whitespace_normal()
                                        .child(meta_line),
                                ),
                        ),
                )
                .child(status_pill(status_bg, status_fg, status_text, None)),
        )
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap(self.theme.space(4.))
                .children(preview_btn)
                .children(open_btn)
                .child(reveal_btn),
        )
        .into_any_element()
    }

    fn inspector_answer_summary_card(&self, summary: &str) -> AnyElement {
        div()
            .w_full()
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
                    .text_color(self.theme.muted())
                    .child(self.t("board.section_answer_summary")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.ink())
                    .whitespace_normal()
                    .child(summary.to_owned()),
            )
            .into_any_element()
    }

    fn inspector_timeline_tab(
        &self,
        group: &SessionBoardGroup,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut col = div()
            .w_full()
            .px(self.theme.space(16.))
            .py(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(10.));

        if !group.child_workers.is_empty() {
            let heading = self.tf(
                "board.section_delegated_workers",
                &[("count", &group.child_workers.len().to_string())],
            );
            let mut pills =
                div().w_full().flex().flex_wrap().gap(self.theme.space(6.));
            let root_selected = self.board_inspector.worker_session.is_none();
            pills = pills.child(
                theme::interactive(
                    "bi-worker-root",
                    self.t("chat.router_name"),
                    &self.theme,
                )
                .px(self.theme.space(10.))
                .py(px(5.))
                .rounded_lg()
                .border_1()
                .border_color(if root_selected {
                    self.theme.accent()
                } else {
                    self.theme.line()
                })
                .bg(if root_selected {
                    self.theme.soft()
                } else {
                    self.theme.canvas()
                })
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(self.theme.ink())
                .child(self.t("chat.router_name"))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.select_board_worker(None, cx);
                })),
            );

            for (ix, worker) in group.child_workers.iter().enumerate() {
                let wid = worker.session_id.clone();
                let selected = self.board_inspector.worker_session.as_deref()
                    == Some(wid.as_str());
                let badge = if worker.has_blocker {
                    self.t("board.badge_approval")
                } else if worker.is_running {
                    self.t("board.badge_running")
                } else {
                    self.tf(
                        "board.branch_count",
                        &[("count", &worker.deliverable_count.to_string())],
                    )
                };
                pills = pills.child(
                    theme::interactive(
                        ("bi-worker-chip", ix as u64),
                        worker.title.clone(),
                        &self.theme,
                    )
                    .px(self.theme.space(10.))
                    .py(px(5.))
                    .rounded_lg()
                    .border_1()
                    .border_color(if selected {
                        self.theme.accent()
                    } else if worker.has_blocker {
                        self.theme.status_attention().opacity(0.6)
                    } else {
                        self.theme.line()
                    })
                    .bg(if selected {
                        self.theme.soft()
                    } else {
                        self.theme.canvas()
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(self.theme.ink())
                    .child(
                        div()
                            .max_w(px(180.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(worker.title.clone()),
                    )
                    .child(
                        div()
                            .text_color(if worker.has_blocker {
                                self.theme.status_attention()
                            } else if worker.is_running {
                                self.theme.status_active()
                            } else {
                                self.theme.muted()
                            })
                            .child(badge),
                    )
                    .on_click(cx.listener(
                        move |panel, _, _, cx| {
                            panel.select_board_worker(Some(wid.clone()), cx);
                        },
                    )),
                );
            }

            col = col.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.muted())
                            .child(heading),
                    )
                    .child(pills),
            );
        }

        let target_sid = self
            .board_inspector
            .worker_session
            .as_deref()
            .unwrap_or(group.session_id.as_str());
        let target_running = self
            .board_inspector
            .worker_session
            .as_deref()
            .map_or(group.is_running, |wid| {
                group
                    .child_workers
                    .iter()
                    .find(|w| w.session_id == wid)
                    .is_some_and(|w| w.is_running)
            });
        let cached_lines: &[ChatLine] = self
            .cached_board_lines
            .as_ref()
            .filter(|(sid, _)| sid == target_sid)
            .map_or(&[], |(_, lines)| lines.as_slice());

        div()
            .id("board-inspector-timeline-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .scrollbar_width(px(6.))
            .child(col.child(self.structured_steps_view(
                target_sid,
                cached_lines,
                target_running,
                window,
                cx,
            )))
            .into_any_element()
    }

    fn inspector_sticky_footer(
        &self,
        action: StickyFooterAction,
        is_narrow_drawer: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut footer = div()
            .w_full()
            .flex_none()
            .px(self.theme.space(16.))
            .py(self.theme.space(10.))
            .border_t_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.));

        if is_narrow_drawer {
            footer = footer.rounded_bl(self.theme.modal_radius());
        } else {
            footer = footer.rounded_b_2xl();
        }

        match action {
            StickyFooterAction::OverrideApproval {
                action_sid,
                report_id,
                artifact_path,
            } => {
                let ack_key = format!("{report_id}:{artifact_path}");
                let ack = self.board_inspector.override_ack_key.as_deref()
                    == Some(ack_key.as_str());
                let overriding = self.overriding_reports.contains(&report_id);
                let confirm_label = self.t("verify.override.confirm");
                let toggle_key = ack_key;

                footer
                    .child(
                        theme::interactive(
                            "bi-override-ack",
                            confirm_label.clone(),
                            &self.theme,
                        )
                        .role(gpui::Role::CheckBox)
                        .aria_toggled(ack.into())
                        .flex()
                        .items_center()
                        .gap(self.theme.space(6.))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(self.theme.ink())
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            if panel.board_inspector.override_ack_key.as_deref()
                                == Some(toggle_key.as_str())
                            {
                                panel.board_inspector.override_ack_key = None;
                            } else {
                                panel.board_inspector.override_ack_key =
                                    Some(toggle_key.clone());
                            }
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
                                    self.theme.canvas()
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(ack, |box_el| {
                                    box_el.child(
                                        theme::icons::check()
                                            .size(px(10.))
                                            .text_color(self.theme.on_ink()),
                                    )
                                }),
                        )
                        .child(confirm_label),
                    )
                    .child(
                        theme::Button::new(
                            "bi-override-submit",
                            self.t("verify.override.action"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .size(theme::ControlSize::Small)
                        .disabled(!ack || overriding)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                panel.confirm_override_for_session(
                                    &action_sid,
                                    &report_id,
                                    &artifact_path,
                                    cx,
                                );
                            }),
                        ),
                    )
                    .into_any_element()
            }
            StickyFooterAction::DeliveryFailed { retry_sid } => {
                let retrying = self.retrying_delivery.as_deref()
                    == Some(retry_sid.as_str());
                footer
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.status_attention())
                            .child(self.t("chat.status_delivery_failed")),
                    )
                    .child(
                        theme::Button::new(
                            "bi-retry-delivery",
                            self.t("chat.retry_delivery"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .size(theme::ControlSize::Small)
                        .disabled(retrying)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                panel
                                    .retry_delivery_for_session(&retry_sid, cx);
                            }),
                        ),
                    )
                    .into_any_element()
            }
            StickyFooterAction::OpenApprovalChat { open_sid } => footer
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.status_attention())
                        .child(self.t("board.state_needs_approval")),
                )
                .child(
                    theme::Button::new(
                        "bi-open-approval-chat",
                        self.t("drawer.work_open_session"),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .size(theme::ControlSize::Small)
                    .trailing_icon(theme::icons::arrow_up_right().size(px(12.)))
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            panel.close_board_drawer(cx);
                            panel.open_session(&open_sid, cx);
                        }),
                    ),
                )
                .into_any_element(),
        }
    }
}
