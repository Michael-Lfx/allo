//! Centered responsive Flowchart Modal rendered in `FlowyApp`'s top-level slot.

use flowy_agent::{ReceiptState, VerificationIssue};
use flowy_ui as theme;
use flowy_ui::charts::{FlowEdgeStyle, flow_connector, progress_ring};
use flowy_ui::status_pill;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div,
    prelude::*, px,
};

use super::FlowModalTab;
use crate::panel::StudioPanel;
use crate::ports::{
    BranchPhase, SessionBoardGroup, WorkBranchSummary, display_path_str,
};

impl StudioPanel {
    /// Centered wide Flowchart Modal opened when clicking any session on the board.
    /// Rendered at the top-level `FlowyApp` modal slot so the scrim covers the
    /// entire window (including dock and titlebar) with generous horizontal room.
    pub fn flow_modal_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Option<AnyElement> {
        let (group, selected_branch) = self.resolve_flow_modal()?;
        let group = group.clone();
        let selected_branch = selected_branch.clone();

        let open_sid = group.session_id.clone();
        let can_open_session = !group.is_synthetic();
        let close_button = self.icon_tooltip_button(
            "flow-modal-close",
            "flow-modal-close-tip",
            self.t("chat.close"),
            theme::icons::x().size(px(14.)),
            theme::ControlVariant::Ghost,
            |panel, _, cx| {
                panel.close_flow_modal(cx);
            },
            window,
            cx,
        );

        let header = div()
            .w_full()
            .rounded_t(self.theme.modal_radius())
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
                                    row.child(status_pill(
                                        self.theme.card(),
                                        self.theme.muted(),
                                        name,
                                        None,
                                    ))
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
                            .trailing_icon(
                                theme::icons::arrow_up_right().size(px(12.)),
                            )
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.close_flow_modal(cx);
                                    panel.open_session(&open_sid, cx);
                                }),
                            ),
                        )
                    })
                    .child(close_button),
            );

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

        let hidden_branches = group
            .total_branch_count
            .saturating_sub(group.branches.len());
        let branch_count_label = if hidden_branches > 0 {
            format!(
                "{} (+{hidden_branches})",
                self.tf(
                    "board.branch_count",
                    &[("count", &group.total_branch_count.to_string())],
                )
            )
        } else {
            self.tf(
                "board.branch_count",
                &[("count", &group.total_branch_count.to_string())],
            )
        };
        let flow_canvas = div()
            .id("flow-modal-canvas")
            .w_full()
            .max_h(px(250.))
            .overflow_scroll()
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
                            .child(
                                div()
                                    .text_xs()
                                    .opacity(0.8)
                                    .child(branch_count_label),
                            ),
                    ),
            )
            .child(branch_rows);

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

        let detail_body = match self.flow_modal.tab {
            FlowModalTab::Action => {
                self.modal_action_panel(&group, &selected_branch, window, cx)
            }
            FlowModalTab::Trace => {
                self.modal_trace_panel(&group.session_id, cx)
            }
        };

        let footer = div()
            .id("flow-modal-detail")
            .flex_1()
            .min_h(px(180.))
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

        Some(theme::centered_modal_sheet(
            theme::ModalSheetConfig {
                layer_id: "flow-modal-layer".into(),
                panel_id: "flow-modal-panel".into(),
                label: SharedString::from(group.title.clone()),
                max_width_px: 1040.0,
            },
            &self.theme,
            &self.ui,
            cx,
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
        let is_app = branch.phase.is_blocker();
        let is_run = branch.phase.is_running();
        let is_done = !is_app && !is_run;
        let pct_rounded = branch.progress_pct.clamp(0.0, 100.0).round() as u32;
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
            BranchPhase::DeliveryFailed => {
                self.t("chat.status_delivery_failed")
            }
            BranchPhase::Running { step_label, .. } => {
                if step_label.is_empty() {
                    self.t("chat.status_running")
                } else {
                    step_label.clone()
                }
            }
            BranchPhase::Queued { state, .. } => {
                self.receipt_state_visual(*state).1
            }
            BranchPhase::Delivered { override_used, .. } => {
                if *override_used {
                    self.t("verify.override.badge")
                } else {
                    self.t("verify.card.passed")
                }
            }
            BranchPhase::Idle => self.t("chat.status_idle"),
        };

        let skill_text = branch.skill_label.resolve(|k| self.t(k));
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
            .px(px(6.))
            .py(px(4.))
            .rounded_xl()
            .when(selected, |row| row.bg(self.theme.soft().opacity(0.6)))
            .flex()
            .items_center()
            .child(flow_connector(
                ("fm-e1", idx),
                22.0,
                e1_color,
                e1_style,
                policy,
            ))
            .child(
                theme::interactive(
                    ("fm-n1", idx),
                    skill_text.clone(),
                    &self.theme,
                )
                .w(px(148.))
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
                    panel.flow_modal.branch = Some(bid1.clone());
                    panel.flow_modal.override_ack_key = None;
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
                                .child(skill_text),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .child(format!("{pct_rounded}%")),
                        ),
                ),
            )
            .child(flow_connector(
                ("fm-e2", idx),
                22.0,
                e2_color,
                e2_style,
                policy,
            ))
            .child(
                theme::interactive(
                    ("fm-n2", idx),
                    gate_title.clone(),
                    &self.theme,
                )
                .w(px(156.))
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
                    panel.flow_modal.branch = Some(bid2.clone());
                    panel.flow_modal.override_ack_key = None;
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
                            div().child("!").into_any_element()
                        } else if is_done {
                            theme::icons::check()
                                .size(px(12.))
                                .text_color(self.theme.status_verified())
                                .into_any_element()
                        } else {
                            div()
                                .size(px(7.))
                                .rounded_full()
                                .bg(self.theme.status_active())
                                .into_any_element()
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
                22.0,
                e3_color,
                e3_style,
                policy,
            ))
            .child({
                let has_file_deliverable = branch.deliverable_path.is_some();
                let badge = if has_file_deliverable {
                    div()
                        .flex_none()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(format_color)
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(self.theme.on_ink())
                        .child(self.format_tag_label(branch.format))
                        .into_any_element()
                } else {
                    div()
                        .size(px(22.))
                        .flex_none()
                        .rounded_md()
                        .bg(self.theme.soft())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            theme::icons::sparkles()
                                .size(px(11.))
                                .text_color(self.theme.muted()),
                        )
                        .into_any_element()
                };
                let subtitle = match &branch.phase {
                    BranchPhase::Delivered { .. } => {
                        self.t("verify.card.passed")
                    }
                    BranchPhase::Idle if !has_file_deliverable => {
                        self.t("chat.status_idle")
                    }
                    BranchPhase::Queued { state, .. }
                        if !has_file_deliverable =>
                    {
                        self.receipt_state_visual(*state).1
                    }
                    BranchPhase::Running { .. } if !has_file_deliverable => {
                        self.t("chat.status_running")
                    }
                    BranchPhase::DeliveryFailed => {
                        self.t("chat.status_delivery_failed")
                    }
                    _ => self.t("chat.deliverables"),
                };
                theme::interactive(
                    ("fm-n3", idx),
                    branch.deliverable_name.clone(),
                    &self.theme,
                )
                .w(px(172.))
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
                    panel.flow_modal.branch = Some(bid3.clone());
                    panel.flow_modal.override_ack_key = None;
                    cx.notify();
                }))
                .child(badge)
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
                                .child(subtitle),
                        ),
                )
            })
            .into_any_element()
    }

    fn modal_tab_button(
        &self,
        tab: FlowModalTab,
        label: String,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let selected = self.flow_modal.tab == tab;
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
            panel.flow_modal.tab = tab;
            panel.refresh_trace_cache();
            cx.notify();
        }))
        .into_any_element()
    }

    fn modal_action_panel(
        &self,
        group: &SessionBoardGroup,
        branch: &WorkBranchSummary,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let left_card = match &branch.phase {
            BranchPhase::NeedsApproval {
                report_id,
                artifact_path,
                summary,
                issues,
                semantic_hint,
                provenance_warnings,
            } => self.modal_approval_card(
                &group.session_id,
                report_id,
                artifact_path,
                summary,
                issues,
                semantic_hint.as_deref(),
                provenance_warnings,
                cx,
            ),
            BranchPhase::DeliveryFailed => {
                self.modal_delivery_failed_card(group, branch, cx)
            }
            BranchPhase::Running { .. } => {
                self.modal_running_card(group, branch, cx)
            }
            BranchPhase::Queued { receipt_id, state } => {
                self.modal_queued_card(branch, receipt_id, *state, cx)
            }
            BranchPhase::Delivered {
                provenance_warnings,
                ..
            } => div()
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
                        .child(branch.skill_label.resolve(|k| self.t(k))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(branch.detail.clone()),
                )
                .when(!provenance_warnings.is_empty(), |card| {
                    card.child(
                        div()
                            .px(self.theme.space(8.))
                            .py(px(4.))
                            .rounded_md()
                            .bg(self.theme.status_attention_bg())
                            .text_xs()
                            .text_color(self.theme.status_attention())
                            .whitespace_normal()
                            .child(format!(
                                "{}: {}",
                                self.t("verify.provenance_invalid"),
                                provenance_warnings.join(", ")
                            )),
                    )
                })
                .when_some(branch.work_progress.as_ref(), |card, prog| {
                    card.child(self.contract_progress_block(prog))
                })
                .into_any_element(),
            BranchPhase::Idle => div()
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
                        .child(branch.skill_label.resolve(|k| self.t(k))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(branch.detail.clone()),
                )
                .when_some(branch.work_progress.as_ref(), |card, prog| {
                    card.child(self.contract_progress_block(prog))
                })
                .into_any_element(),
        };

        let file_path = branch.deliverable_path.clone();
        let file_actions = file_path.as_ref().map(|path| {
            let can_open = flowy_agent::can_open_with_default_app(path);
            let open_path = path.clone();
            let reveal_path = path.clone();
            let open_btn = can_open.then(|| {
                self.icon_tooltip_button(
                    "fm-open-file",
                    "fm-open-file-tip",
                    self.t("deliverables.open_external"),
                    theme::icons::external_link().size(px(14.)),
                    theme::ControlVariant::Secondary,
                    move |panel, _, cx| {
                        panel.open_file_externally(&open_path, cx);
                    },
                    window,
                    cx,
                )
            });
            let reveal_btn = self.icon_tooltip_button(
                "fm-reveal-folder",
                "fm-reveal-folder-tip",
                self.t("board.reveal_folder"),
                theme::icons::folder_open().size(px(14.)),
                theme::ControlVariant::Ghost,
                move |panel, _, cx| {
                    panel.reveal_file(&reveal_path, cx);
                },
                window,
                cx,
            );
            div()
                .flex()
                .items_center()
                .gap(self.theme.space(6.))
                .children(open_btn)
                .child(reveal_btn)
        });
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
            .children(file_actions);

        div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(self.theme.space(12.))
            .child(left_card)
            .child(file_card)
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "modal approval card surfaces full verification evidence for explicit override"
    )]
    fn modal_approval_card(
        &self,
        session_id: &str,
        report_id: &str,
        artifact_path: &str,
        summary: &str,
        issues: &[VerificationIssue],
        semantic_hint: Option<&str>,
        provenance_warnings: &[String],
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let ack_key = format!("{report_id}:{artifact_path}");
        let ack = self.flow_modal.override_ack_key.as_deref()
            == Some(ack_key.as_str());
        let has_report = !report_id.trim().is_empty();
        let overriding = self.overriding_reports.contains(report_id);
        let sid = session_id.to_owned();
        let rid = report_id.to_owned();
        let apath = artifact_path.to_owned();
        let toggle_key = ack_key;
        let confirm_label = self.t("verify.override.confirm");

        let mut body = div()
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
                    .child(summary.to_owned()),
            );

        if !issues.is_empty() {
            let mut issue_list = div().flex().flex_col().gap(px(4.));
            for issue in issues.iter().take(6) {
                let kind = self.t(flowy_agent::verify_ui_key(&issue.code));
                let row_text = if issue.field.trim().is_empty() {
                    kind
                } else {
                    format!("{kind} \u{00b7} {}", issue.field)
                };
                issue_list = issue_list.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.status_attention())
                        .whitespace_normal()
                        .child(row_text),
                );
            }
            body = body.child(issue_list);
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

        body = body
            .child(
                theme::interactive(
                    "fm-override-ack",
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
                .text_color(self.theme.muted())
                .on_click(cx.listener(move |panel, _, _, cx| {
                    if panel.flow_modal.override_ack_key.as_deref()
                        == Some(toggle_key.as_str())
                    {
                        panel.flow_modal.override_ack_key = None;
                    } else {
                        panel.flow_modal.override_ack_key =
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
                            self.theme.card()
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
                    "fm-override-submit",
                    self.t("verify.override.action"),
                )
                .variant(theme::ControlVariant::Primary)
                .size(theme::ControlSize::Small)
                .disabled(!(ack && has_report) || overriding)
                .render(
                    &self.theme,
                    cx.listener(move |panel, _, _, cx| {
                        panel.confirm_override_for_session(
                            &sid, &rid, &apath, cx,
                        );
                    }),
                ),
            );

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
            .child(body)
            .into_any_element()
    }

    fn modal_delivery_failed_card(
        &self,
        group: &SessionBoardGroup,
        branch: &WorkBranchSummary,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let retry_sid = group.session_id.clone();
        let retrying = self.retrying_delivery.as_deref()
            == Some(group.session_id.as_str());
        div()
            .flex_1()
            .min_w(px(240.))
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.status_attention().opacity(0.5))
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
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
            )
            .when_some(branch.work_progress.as_ref(), |card, prog| {
                card.child(self.contract_progress_block(prog))
            })
            .child(
                div().flex().items_center().child(
                    theme::Button::new(
                        "fm-retry-delivery",
                        self.t("chat.retry_delivery"),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .size(theme::ControlSize::Small)
                    .disabled(retrying)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            panel.retry_delivery_for_session(&retry_sid, cx);
                        }),
                    ),
                ),
            )
            .into_any_element()
    }

    fn modal_running_card(
        &self,
        group: &SessionBoardGroup,
        branch: &WorkBranchSummary,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let stop_sid = group.session_id.clone();
        let can_stop = !group.is_synthetic();
        div()
            .flex_1()
            .min_w(px(240.))
            .p(self.theme.space(12.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.ink())
                    .child(branch.skill_label.resolve(|k| self.t(k))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(branch.detail.clone()),
            )
            .when_some(branch.work_progress.as_ref(), |card, prog| {
                card.child(self.contract_progress_block(prog))
            })
            .when(can_stop, |card| {
                card.child(
                    div().flex().items_center().child(
                        theme::Button::new(
                            "fm-stop-session",
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
                    ),
                )
            })
            .into_any_element()
    }

    fn modal_queued_card(
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
            .flex_1()
            .min_w(px(240.))
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
                card.child(self.contract_progress_block(prog))
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
                                    "fm-retry-dead-receipt",
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
                                    "fm-cancel-receipt",
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
}
