//! Persistent interactive decision cards (`PendingCard::Disambiguation` and
//! `PendingCard::ToolApproval`) and honest stop-handoff summary rendered above
//! the composer.
//!
//! All state mutations go strictly through `AgentHandle::apply_card_action`
//! (`CardAction::ResolveCard`); this module never touches kernel storage.

use flowy_agent::{
    BranchHeartbeat, CardAction, CardSelection, DisambiguationOption,
    PendingCard, StopHandoffManifest, TurnStatus,
};
use flowy_ui as theme;
use gpui::{
    AnyElement, Context, FontWeight, SharedString, div, prelude::*, px,
};

use crate::panel::StudioPanel;

/// Formats a live [`BranchHeartbeat`] into one concise, honest progress line
/// without fabricating percentage progress when no new evidence has arrived.
#[must_use]
pub fn format_heartbeat_line(
    heartbeat: &BranchHeartbeat,
    waiting_label: &str,
) -> Option<String> {
    let mut parts: Vec<&str> = Vec::with_capacity(3);
    if let Some(last) = heartbeat
        .last_completed
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(last);
    }
    let current = heartbeat.current_action.trim();
    if !current.is_empty() {
        parts.push(current);
    } else if heartbeat.awaiting_evidence && !waiting_label.trim().is_empty() {
        parts.push(waiting_label.trim());
    }
    if let Some(next) = heartbeat
        .next_check_hint
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(next);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

impl StudioPanel {
    /// Renders persisted pending decision cards for the active conversation.
    pub(crate) fn pending_cards_slot(
        &self,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let Some(session_id) = self.agent.active_session_id() else {
            return div().into_any_element();
        };
        let cards = self.agent.session_pending_cards(&session_id);
        if cards.is_empty() {
            return div().into_any_element();
        }
        let mut stack = div()
            .id("pending-cards-stack")
            .flex_1()
            .min_w_0()
            .max_w(px(crate::STAGE_MAX_W))
            .px(self.theme.space(32.))
            .pb(self.theme.space(8.))
            .flex()
            .flex_col()
            .gap(self.theme.space(8.));
        for (idx, card) in cards.iter().enumerate() {
            stack = stack.child(match card {
                PendingCard::Disambiguation {
                    card_id,
                    prompt,
                    impact_boundary,
                    options,
                    allow_multi,
                    ..
                } => self.render_disambiguation_card(
                    &session_id,
                    card_id,
                    prompt,
                    impact_boundary,
                    options,
                    *allow_multi,
                    idx as u64,
                    cx,
                ),
                PendingCard::ToolApproval {
                    card_id,
                    tool_name,
                    target_summary,
                    args_hash,
                    risk_reason,
                    command_preview,
                    ..
                } => self.render_tool_approval_card(
                    &session_id,
                    card_id,
                    tool_name,
                    target_summary,
                    args_hash,
                    risk_reason,
                    command_preview,
                    idx as u64,
                    cx,
                ),
            });
        }
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(stack)
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "maps 1:1 from PendingCard::Disambiguation fields"
    )]
    fn render_disambiguation_card(
        &self,
        session_id: &str,
        card_id: &str,
        prompt: &str,
        impact_boundary: &str,
        options: &[DisambiguationOption],
        allow_multi: bool,
        idx: u64,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let resolving = self.resolving_cards.contains(card_id);
        let selected_ids = self
            .pending_card_selected
            .get(card_id)
            .cloned()
            .unwrap_or_default();

        let mut option_list = div().w_full().flex().flex_col().gap(px(6.));
        for (opt_ix, option) in options.iter().enumerate() {
            let is_selected = selected_ids.contains(&option.object_id);
            let sid = session_id.to_owned();
            let cid = card_id.to_owned();
            let oid = option.object_id.clone();
            let ext_tag = option.extension.trim().to_ascii_uppercase();
            let label = option.label.clone();
            let anchor = option.anchor_summary.trim().to_owned();
            option_list = option_list.child(
                theme::interactive(
                    ("disambig-opt", idx * 32 + opt_ix as u64),
                    SharedString::from(label.clone()),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .px(self.theme.space(12.))
                .py(self.theme.space(8.))
                .rounded_lg()
                .border_1()
                .border_color(if is_selected {
                    self.theme.status_active()
                } else {
                    self.theme.line()
                })
                .bg(if is_selected {
                    self.theme.status_active_bg()
                } else {
                    self.theme.canvas()
                })
                .flex()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
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
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(label),
                        )
                        .when(!anchor.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(anchor),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .when(!ext_tag.is_empty(), |row| {
                            row.child(
                                div()
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded_md()
                                    .bg(self.theme.soft())
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .child(ext_tag),
                            )
                        })
                        .when(is_selected, |row| {
                            row.child(
                                theme::icons::check()
                                    .size(px(13.))
                                    .text_color(self.theme.status_active()),
                            )
                        }),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        if panel.resolving_cards.contains(&cid) {
                            return;
                        }
                        if allow_multi {
                            let entry = panel
                                .pending_card_selected
                                .entry(cid.clone())
                                .or_default();
                            if let Some(pos) =
                                entry.iter().position(|x| x == &oid)
                            {
                                entry.remove(pos);
                            } else {
                                entry.push(oid.clone());
                            }
                            cx.notify();
                        } else {
                            panel.resolve_pending_card(
                                sid.clone(),
                                cid.clone(),
                                CardSelection::Disambiguate {
                                    object_ids: vec![oid.clone()],
                                },
                                cx,
                            );
                        }
                    },
                )),
            );
        }

        let sid_confirm = session_id.to_owned();
        let cid_confirm = card_id.to_owned();
        let confirm_ids = selected_ids;
        let title = self.t("card.disambig_title");
        let boundary = impact_boundary.trim().to_owned();
        let prompt_line = prompt.trim().to_owned();

        div()
            .id(("pending-disambig-card", idx))
            .debug_selector(|| "chat-pending-disambig-card".to_owned())
            .role(gpui::Role::Alert)
            .aria_label(title.clone())
            .w_full()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.status_attention().opacity(0.45))
            .bg(self.theme.card())
            .shadow_xs()
            .p(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .child(
                        theme::icons::circle_alert()
                            .size(px(14.))
                            .text_color(self.theme.status_attention()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(title),
                    ),
            )
            .when(!boundary.is_empty(), |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(boundary),
                )
            })
            .when(!prompt_line.is_empty(), |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.ink().opacity(0.85))
                        .whitespace_normal()
                        .child(prompt_line),
                )
            })
            .child(option_list)
            .when(allow_multi, |card| {
                card.child(
                    div().w_full().flex().justify_end().child(
                        theme::Button::new(
                            ("disambig-confirm", idx),
                            self.t("card.disambig_confirm"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .size(theme::ControlSize::Small)
                        .disabled(resolving || confirm_ids.is_empty())
                        .loading(resolving)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                if !confirm_ids.is_empty() {
                                    panel.resolve_pending_card(
                                        sid_confirm.clone(),
                                        cid_confirm.clone(),
                                        CardSelection::Disambiguate {
                                            object_ids: confirm_ids.clone(),
                                        },
                                        cx,
                                    );
                                }
                            }),
                        ),
                    ),
                )
            })
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "maps 1:1 from PendingCard::ToolApproval fields"
    )]
    fn render_tool_approval_card(
        &self,
        session_id: &str,
        card_id: &str,
        tool_name: &str,
        target_summary: &str,
        args_hash: &str,
        risk_reason: &str,
        command_preview: &str,
        idx: u64,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let resolving = self.resolving_cards.contains(card_id);
        let title = self.t("card.tool_approval_title");
        let reason = crate::ports::redact_trace_text(risk_reason.trim());
        let target = crate::ports::redact_trace_text(target_summary.trim());
        let preview = crate::ports::redact_trace_text(command_preview.trim());
        let sid_ok = session_id.to_owned();
        let cid_ok = card_id.to_owned();
        let hash_ok = args_hash.to_owned();
        let sid_no = session_id.to_owned();
        let cid_no = card_id.to_owned();

        div()
            .id(("pending-tool-approval-card", idx))
            .debug_selector(|| "chat-pending-tool-approval-card".to_owned())
            .role(gpui::Role::Alert)
            .aria_label(title.clone())
            .w_full()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.status_attention().opacity(0.45))
            .bg(self.theme.card())
            .shadow_xs()
            .p(self.theme.space(12.))
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
                            .child(
                                theme::icons::circle_alert()
                                    .size(px(14.))
                                    .text_color(self.theme.status_attention()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(self.theme.ink())
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded_md()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(tool_name.to_owned()),
                    ),
            )
            .when(!reason.is_empty(), |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.ink())
                        .whitespace_normal()
                        .child(reason),
                )
            })
            .when(!target.is_empty(), |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(target),
                )
            })
            .when(!preview.is_empty(), |card| {
                card.child(
                    div()
                        .w_full()
                        .px(self.theme.space(10.))
                        .py(self.theme.space(6.))
                        .rounded_lg()
                        .bg(self.theme.soft())
                        .text_xs()
                        .text_color(self.theme.ink())
                        .whitespace_normal()
                        .child(preview),
                )
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(self.theme.space(8.))
                    .child(
                        theme::Button::new(
                            ("tool-approval-deny", idx),
                            self.t("card.tool_deny"),
                        )
                        .variant(theme::ControlVariant::Secondary)
                        .size(theme::ControlSize::Small)
                        .disabled(resolving)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                panel.resolve_pending_card(
                                    sid_no.clone(),
                                    cid_no.clone(),
                                    CardSelection::DenyTool { reason: None },
                                    cx,
                                );
                            }),
                        ),
                    )
                    .child(
                        theme::Button::new(
                            ("tool-approval-approve", idx),
                            self.t("card.tool_approve"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .size(theme::ControlSize::Small)
                        .disabled(resolving)
                        .loading(resolving)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                panel.resolve_pending_card(
                                    sid_ok.clone(),
                                    cid_ok.clone(),
                                    CardSelection::ApproveTool {
                                        args_hash: hash_ok.clone(),
                                    },
                                    cx,
                                );
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// Quiet, honest stop-handoff summary shown when a stopped turn left
    /// modified files, pending checks, or rollback advice.
    pub(crate) fn stop_handoff_slot(
        &self,
        _cx: &Context<'_, Self>,
    ) -> AnyElement {
        if !matches!(self.status, TurnStatus::Stopped) {
            return div().into_any_element();
        }
        let Some(session_id) = self.agent.active_session_id() else {
            return div().into_any_element();
        };
        let Some(manifest) = self.agent.session_stop_handoff(&session_id)
        else {
            return div().into_any_element();
        };
        if !stop_handoff_has_details(&manifest) {
            return div().into_any_element();
        }
        let title = self.t("card.stop_handoff_title");
        let modified_line = (!manifest.modified_files.is_empty()).then(|| {
            let names: Vec<String> = manifest
                .modified_files
                .iter()
                .map(|p| crate::ports::basename(p))
                .collect();
            self.tf(
                "card.stop_handoff_modified",
                &[("files", &names.join(", "))],
            )
        });
        let pending_line = (!manifest.pending_checks.is_empty()).then(|| {
            self.tf(
                "card.stop_handoff_pending",
                &[("items", &manifest.pending_checks.join(", "))],
            )
        });
        let rollback = manifest.rollback_hint.trim().to_owned();

        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("stop-handoff-card")
                    .role(gpui::Role::Status)
                    .aria_label(title.clone())
                    .flex_1()
                    .min_w_0()
                    .max_w(px(crate::STAGE_MAX_W))
                    .mx(self.theme.space(32.))
                    .mb(self.theme.space(6.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .rounded_xl()
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.soft().opacity(0.55))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.ink())
                            .child(title),
                    )
                    .when_some(modified_line, |col, text| {
                        col.child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .whitespace_normal()
                                .child(text),
                        )
                    })
                    .when_some(pending_line, |col, text| {
                        col.child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .whitespace_normal()
                                .child(text),
                        )
                    })
                    .when(!rollback.is_empty(), |col| {
                        col.child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .whitespace_normal()
                                .child(rollback),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn resolve_pending_card(
        &mut self,
        session_id: String,
        card_id: String,
        selection: CardSelection,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.resolving_cards.insert(card_id.clone()) {
            return;
        }
        let agent = self.agent.clone();
        let card_done = card_id.clone();
        cx.notify();
        cx.spawn(async move |panel, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    agent.apply_card_action(CardAction::ResolveCard {
                        session_id,
                        card_id,
                        selection,
                    })
                })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                panel.resolving_cards.remove(&card_done);
                panel.pending_card_selected.remove(&card_done);
                match result {
                    Ok(_) => {
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                    }
                    Err(notice) => {
                        panel.emit_notice(Some(notice), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn stop_handoff_has_details(manifest: &StopHandoffManifest) -> bool {
    !manifest.modified_files.is_empty()
        || !manifest.pending_checks.is_empty()
        || !manifest.rollback_hint.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_line_omits_empty_parts_and_surfaces_waiting_honestly() {
        let waiting = BranchHeartbeat {
            last_completed: Some("已读取报价表".into()),
            last_completed_at_ms: Some(1_000),
            current_action: String::new(),
            next_check_hint: Some("等待工具返回".into()),
            awaiting_evidence: true,
        };
        assert_eq!(
            format_heartbeat_line(&waiting, "正在等待，尚未确认进展")
                .as_deref(),
            Some("已读取报价表 · 正在等待，尚未确认进展 · 等待工具返回")
        );

        let empty = BranchHeartbeat::default();
        assert_eq!(format_heartbeat_line(&empty, "waiting"), None);
    }

    #[test]
    fn stop_handoff_visibility_requires_real_details() {
        assert!(!stop_handoff_has_details(&StopHandoffManifest::default()));
        let populated = StopHandoffManifest {
            modified_files: vec!["outputs/deck.pptx".into()],
            ..StopHandoffManifest::default()
        };
        assert!(stop_handoff_has_details(&populated));
    }

    #[gpui::test]
    fn stopping_and_waiting_statuses_localize_and_cards_render(
        cx: &mut gpui::TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-pending-card-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (panel, cx) = cx.add_window_view(|window, cx| {
            let ui = theme::UiContext::new(
                theme::ThemeOptions::default(),
                false,
                cx,
            );
            StudioPanel::new(
                crate::StudioConfig {
                    product: "Flowy",
                    i18n: flowy_i18n::I18n::embedded("Flowy"),
                    model: flowy_agent::Model::Lite,
                    plan: flowy_agent::Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: flowy_agent::LlmConfig::default(),
                },
                ui,
                window,
                cx,
            )
        });
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(
                panel.working_label(&TurnStatus::Stopping),
                panel.t("chat.status_stopping")
            );
            assert_eq!(
                panel.working_label(&TurnStatus::WaitingForUser),
                panel.t("chat.status_waiting_for_user")
            );
        });
        struct CardPreview {
            panel: gpui::Entity<StudioPanel>,
        }
        impl gpui::Render for CardPreview {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                cx: &mut gpui::Context<'_, Self>,
            ) -> impl IntoElement {
                let disambig = self.panel.update(cx, |panel, cx| {
                    panel.render_disambiguation_card(
                        "s-test",
                        "card-disambig",
                        "把刚才那份的报价改成 32 万元",
                        "我只改你选的那份，其余保持 30 万元",
                        &[
                            DisambiguationOption {
                                object_id: "obj-1".into(),
                                label: "青禾项目报价汇报.pptx".into(),
                                anchor_summary: "首页日期 2026-11-30".into(),
                                extension: "pptx".into(),
                            },
                            DisambiguationOption {
                                object_id: "obj-2".into(),
                                label: "青禾项目内部讨论稿.pptx".into(),
                                anchor_summary: "首页日期 2026-12-02".into(),
                                extension: "pptx".into(),
                            },
                        ],
                        true,
                        0,
                        cx,
                    )
                });
                let approval = self.panel.update(cx, |panel, cx| {
                    panel.render_tool_approval_card(
                        "s-test",
                        "card-tool",
                        "powershell",
                        "workspace/build",
                        "abc123",
                        "将修改工作区文件",
                        "Remove-Item -Recurse ./dist",
                        1,
                        cx,
                    )
                });
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(disambig)
                    .child(approval)
            }
        }
        let panel_ref = panel.clone();
        let (_preview, preview_cx) =
            cx.add_window_view(move |_, _| CardPreview { panel: panel_ref });
        preview_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            preview_cx
                .debug_bounds("chat-pending-disambig-card")
                .is_some(),
            "disambiguation card must paint"
        );
        assert!(
            preview_cx
                .debug_bounds("chat-pending-tool-approval-card")
                .is_some(),
            "tool approval card must paint"
        );
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }
}
