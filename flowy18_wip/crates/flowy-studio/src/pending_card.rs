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
    AnyElement, Context, FontWeight, SharedString, Stateful, div, prelude::*,
    px,
};

use crate::panel::StudioPanel;

const MAX_COMMAND_PREVIEW_CHARS: usize = 600;

/// Deterministic 64-bit FNV-1a hash so element IDs derive from stable
/// `(session_id, card_id, subkey)` tuples instead of positional list indices.
fn stable_card_hash(parts: &[&str]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn clamp_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}\u{2026}")
    } else {
        head
    }
}

/// Formats the current action of a live [`BranchHeartbeat`] into one localized
/// status phrase using existing locale keys (never leaking raw English
/// observer literals like `"thinking"` or `"evaluating tool evidence"`).
#[must_use]
pub fn localize_heartbeat_action(
    heartbeat: &BranchHeartbeat,
    t: &dyn Fn(&str) -> String,
) -> Option<String> {
    let action = heartbeat.current_action.trim();
    match action {
        "" => {
            if heartbeat.awaiting_evidence {
                let waiting = t("card.heartbeat_waiting");
                let waiting = waiting.trim();
                (!waiting.is_empty()).then(|| waiting.to_owned())
            } else {
                None
            }
        }
        "thinking" | "reasoning" => Some(t("chat.thinking")),
        "saving_files" => Some(t("chat.status_saving_files")),
        "waiting_for_user" => Some(t("chat.status_waiting_for_user")),
        "stopping" => Some(t("chat.status_stopping")),
        "evaluating tool evidence" => {
            let status = t("chat.status_running");
            if let Some(last) = heartbeat
                .last_completed
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let tool = last.strip_suffix(" (error)").unwrap_or(last);
                let safe = crate::ports::redact_trace_text(tool);
                let safe = safe.trim();
                if !safe.is_empty() {
                    return Some(format!("{safe} \u{00b7} {status}"));
                }
            }
            Some(status)
        }
        other => {
            let safe = crate::ports::redact_trace_text(other);
            let safe = safe.trim();
            if safe.is_empty() {
                None
            } else {
                Some(format!("{safe} \u{00b7} {}", t("chat.status_running")))
            }
        }
    }
}

/// Formats a live [`BranchHeartbeat`] for multi-work pills and rows using
/// localized observer actions (and omitting raw English `next_check_hint`
/// strings).
#[must_use]
pub fn format_heartbeat_line_localized(
    heartbeat: &BranchHeartbeat,
    t: &dyn Fn(&str) -> String,
) -> Option<String> {
    let action = localize_heartbeat_action(heartbeat, t);
    let last = (heartbeat.current_action.trim() != "evaluating tool evidence")
        .then(|| {
            heartbeat
                .last_completed
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(crate::ports::redact_trace_text)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .flatten();
    match (last, action) {
        (Some(last), Some(action)) if last != action => {
            Some(format!("{last} \u{00b7} {action}"))
        }
        (Some(last), _) => Some(last),
        (None, action) => action,
    }
}

impl StudioPanel {
    fn is_session_resolving(&self, session_id: &str) -> bool {
        self.resolving_cards
            .iter()
            .any(|(sid, _)| sid == session_id)
    }

    fn pending_card_shell(
        &self,
        id: (&'static str, u64),
        debug_sel: &'static str,
        title: String,
        badge: Option<String>,
    ) -> Stateful<gpui::Div> {
        let header_title = div()
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
                    .child(title.clone()),
            );
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(header_title)
            .when_some(badge, |row, tag| {
                row.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded_md()
                        .bg(self.theme.soft())
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(tag),
                )
            });
        div()
            .id(id)
            .debug_selector(move || debug_sel.to_owned())
            .role(gpui::Role::Alert)
            .aria_label(title)
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
            .child(header)
    }

    /// Renders persisted pending decision cards for the active conversation
    /// (both Router's own choice cards and routed child Worker approval cards).
    pub(crate) fn pending_cards_slot(
        &self,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let Some(session_id) = self.agent.active_session_id() else {
            return div().into_any_element();
        };
        let routed_cards = self.agent.root_pending_cards(&session_id);
        if routed_cards.is_empty() {
            return div().into_any_element();
        }
        self.render_pending_cards_stack(&session_id, routed_cards, cx)
    }

    pub(crate) fn render_pending_cards_stack(
        &self,
        session_id: &str,
        routed_cards: Vec<flowy_agent::RoutedPendingCard>,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let child_titles: std::collections::HashMap<String, String> =
            if self.agent.session_role(session_id)
                == flowy_agent::SessionRole::Router
            {
                self.agent
                    .child_works(session_id)
                    .into_iter()
                    .map(|w| (w.work_id, w.title))
                    .collect()
            } else {
                std::collections::HashMap::new()
            };
        let pad_x = self.theme.space(32.);
        let pad_b = self.theme.space(8.);
        let gap = self.theme.space(8.);
        let mut stack = theme::scroll_area("pending-cards-stack", &self.theme)
            .a11y_label(self.t("a11y.scroll_pending_cards"))
            .debug_selector(|| "pending-cards-stack".to_owned())
            .flex_1()
            .min_w_0()
            .max_w(px(crate::STAGE_MAX_W))
            .max_h_viewport(px(320.))
            .scrollbar_inset(px(6.))
            .viewport_style(move |s| {
                s.px(pad_x).pb(pad_b).flex().flex_col().gap(gap)
            });
        for routed in &routed_cards {
            let owner_id = routed.owner_session_id.as_str();
            let root_id = routed.root_session_id.as_str();
            let source_label = (routed.owner_role
                == flowy_agent::SessionRole::Worker)
                .then(|| {
                    let raw_title = child_titles
                        .get(owner_id)
                        .filter(|t| !t.trim().is_empty())
                        .map_or_else(
                            || crate::ports::short_id(owner_id),
                            |t| crate::ports::redact_trace_text(t),
                        );
                    self.tf("card.from_work", &[("title", &raw_title)])
                });
            stack = stack.child(match &routed.card {
                PendingCard::Disambiguation {
                    card_id,
                    prompt,
                    impact_boundary,
                    options,
                    allow_multi,
                    ..
                } => self.render_disambiguation_card(
                    owner_id,
                    root_id,
                    source_label.as_deref(),
                    card_id,
                    prompt,
                    impact_boundary,
                    options,
                    *allow_multi,
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
                    owner_id,
                    root_id,
                    source_label.as_deref(),
                    card_id,
                    tool_name,
                    target_summary,
                    args_hash,
                    risk_reason,
                    command_preview,
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
        reason = "maps 1:1 from PendingCard::Disambiguation fields plus routed owner"
    )]
    fn render_disambiguation_card(
        &self,
        session_id: &str,
        root_session_id: &str,
        source_label: Option<&str>,
        card_id: &str,
        prompt: &str,
        impact_boundary: &str,
        options: &[DisambiguationOption],
        allow_multi: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let card_hash = stable_card_hash(&[session_id, card_id]);
        let resolving = self.is_session_resolving(session_id);
        let card_key = (session_id.to_owned(), card_id.to_owned());
        let selected_ids: Vec<String> = self
            .pending_card_selected
            .get(&card_key)
            .map_or_default(|ids| {
                ids.iter()
                    .filter(|oid| {
                        options.iter().any(|opt| &opt.object_id == *oid)
                    })
                    .cloned()
                    .collect()
            });

        let mut option_list = div().w_full().flex().flex_col().gap(px(6.));
        if options.is_empty() {
            option_list = option_list.child(
                div()
                    .w_full()
                    .px(self.theme.space(10.))
                    .py(self.theme.space(8.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.5))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("card.disambig_empty_hint")),
            );
        }
        for option in options {
            let is_selected = selected_ids.contains(&option.object_id);
            let sid = session_id.to_owned();
            let root_session_id_opt = root_session_id.to_owned();
            let cid = card_id.to_owned();
            let oid = option.object_id.clone();
            let opt_hash = stable_card_hash(&[session_id, card_id, &oid]);
            let ext_tag = option.extension.trim().to_ascii_uppercase();
            let label = crate::ports::redact_trace_text(option.label.trim());
            let anchor =
                crate::ports::redact_trace_text(option.anchor_summary.trim());
            option_list = option_list.child(
                theme::interactive(
                    ("disambig-opt", opt_hash),
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
                .when(resolving, |row| row.opacity(0.6))
                .when(!resolving, |row| {
                    row.cursor_pointer()
                        .hover(|style| style.bg(self.theme.soft()))
                })
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
                        if panel.is_session_resolving(&sid) {
                            return;
                        }
                        if allow_multi {
                            let entry = panel
                                .pending_card_selected
                                .entry((sid.clone(), cid.clone()))
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
                            let root_opt = root_session_id_opt.clone();
                            panel.resolve_pending_card(
                                sid.clone(),
                                root_opt,
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
        let root_confirm = root_session_id.to_owned();
        let cid_confirm = card_id.to_owned();
        let confirm_ids = selected_ids;
        let boundary = crate::ports::redact_trace_text(impact_boundary.trim());
        let prompt_line = crate::ports::redact_trace_text(prompt.trim());

        self.pending_card_shell(
            ("pending-disambig-card", card_hash),
            "chat-pending-disambig-card",
            self.t("card.disambig_title"),
            source_label.map(str::to_owned),
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
        .when(allow_multi && !options.is_empty(), |card| {
            card.child(
                div().w_full().flex().justify_end().child(
                    theme::Button::new(
                        ("disambig-confirm", card_hash),
                        self.t("card.disambig_confirm"),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .size(theme::ControlSize::Small)
                    .disabled(confirm_ids.is_empty())
                    .loading(resolving)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            if panel.is_session_resolving(&sid_confirm)
                                || confirm_ids.is_empty()
                            {
                                return;
                            }
                            panel.resolve_pending_card(
                                sid_confirm.clone(),
                                root_confirm.clone(),
                                cid_confirm.clone(),
                                CardSelection::Disambiguate {
                                    object_ids: confirm_ids.clone(),
                                },
                                cx,
                            );
                        }),
                    ),
                ),
            )
        })
        .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "maps 1:1 from PendingCard::ToolApproval fields plus routed owner"
    )]
    fn render_tool_approval_card(
        &self,
        session_id: &str,
        root_session_id: &str,
        source_label: Option<&str>,
        card_id: &str,
        tool_name: &str,
        target_summary: &str,
        args_hash: &str,
        risk_reason: &str,
        command_preview: &str,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let card_hash = stable_card_hash(&[session_id, card_id]);
        let resolving = self.is_session_resolving(session_id);
        let reason = crate::ports::redact_trace_text(risk_reason.trim());
        let target = crate::ports::redact_trace_text(target_summary.trim());
        let preview = clamp_chars(
            &crate::ports::redact_trace_text(command_preview.trim()),
            MAX_COMMAND_PREVIEW_CHARS,
        );
        let sid_ok = session_id.to_owned();
        let root_ok = root_session_id.to_owned();
        let cid_ok = card_id.to_owned();
        let hash_ok = args_hash.to_owned();
        let sid_no = session_id.to_owned();
        let root_no = root_session_id.to_owned();
        let cid_no = card_id.to_owned();
        let badge = match source_label {
            Some(src) => Some(format!("{src} \u{00b7} {tool_name}")),
            None => Some(tool_name.to_owned()),
        };

        self.pending_card_shell(
            ("pending-tool-approval-card", card_hash),
            "chat-pending-tool-approval-card",
            self.t("card.tool_approval_title"),
            badge,
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
            let pad_x = self.theme.space(10.);
            let pad_y = self.theme.space(6.);
            let ink = self.theme.ink();
            card.child(
                theme::scroll_area(
                    ("tool-approval-preview", card_hash),
                    &self.theme,
                )
                .a11y_label(self.t("a11y.scroll_tool_command_preview"))
                .debug_selector(|| "tool-approval-preview".to_owned())
                .w_full()
                .max_h_viewport(px(140.))
                .scrollbar_inset(px(6.))
                .rounded_lg()
                .bg(self.theme.soft())
                .overflow_hidden()
                .viewport_style(move |s| {
                    s.px(pad_x)
                        .py(pad_y)
                        .text_xs()
                        .text_color(ink)
                        .whitespace_normal()
                })
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
                        ("tool-approval-deny", card_hash),
                        self.t("card.tool_deny"),
                    )
                    .variant(theme::ControlVariant::Secondary)
                    .size(theme::ControlSize::Small)
                    .disabled(resolving)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            if panel.is_session_resolving(&sid_no) {
                                return;
                            }
                            panel.resolve_pending_card(
                                sid_no.clone(),
                                root_no.clone(),
                                cid_no.clone(),
                                CardSelection::DenyTool { reason: None },
                                cx,
                            );
                        }),
                    ),
                )
                .child(
                    theme::Button::new(
                        ("tool-approval-approve", card_hash),
                        self.t("card.tool_approve"),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .size(theme::ControlSize::Small)
                    .loading(resolving)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            if panel.is_session_resolving(&sid_ok) {
                                return;
                            }
                            panel.resolve_pending_card(
                                sid_ok.clone(),
                                root_ok.clone(),
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
    /// modified files, verified effects, pending checks, or rollback advice.
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
                .map(|p| {
                    crate::ports::redact_trace_text(&crate::ports::basename(p))
                })
                .collect();
            self.tf(
                "card.stop_handoff_modified",
                &[("files", &names.join(", "))],
            )
        });
        let verified_line =
            (!manifest.verified_effects.is_empty()).then(|| {
                let items: Vec<String> = manifest
                    .verified_effects
                    .iter()
                    .map(|item| crate::ports::redact_trace_text(item))
                    .collect();
                self.tf(
                    "card.stop_handoff_verified",
                    &[("items", &items.join(", "))],
                )
            });
        let pending_line = (!manifest.pending_checks.is_empty()).then(|| {
            let items: Vec<String> = manifest
                .pending_checks
                .iter()
                .map(|item| crate::ports::redact_trace_text(item))
                .collect();
            self.tf(
                "card.stop_handoff_pending",
                &[("items", &items.join(", "))],
            )
        });
        let rollback =
            localize_rollback_hint(&manifest.rollback_hint, &|k| self.t(k));

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
                    .when_some(verified_line, |col, text| {
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
        root_session_id: String,
        card_id: String,
        selection: CardSelection,
        cx: &mut Context<'_, Self>,
    ) {
        if self.is_session_resolving(&session_id) {
            return;
        }
        let guard_key = (session_id.clone(), card_id.clone());
        if !self.resolving_cards.insert(guard_key.clone()) {
            return;
        }
        let agent = self.agent.clone();
        cx.notify();
        cx.spawn(async move |panel, cx| {
            let sid_for_job = session_id.clone();
            let (result, updates) = cx
                .background_executor()
                .spawn(async move {
                    let outcome = std::panic::catch_unwind(
                        std::panic::AssertUnwindSafe(|| {
                            agent.apply_card_action(CardAction::ResolveCard {
                                session_id: sid_for_job,
                                card_id,
                                selection,
                            })
                        }),
                    )
                    .unwrap_or(Err("notice.agent_failed"));
                    let updates = agent.drain_updates();
                    (outcome, updates)
                })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                panel.resolving_cards.remove(&guard_key);
                panel.process_agent_updates(updates, cx);
                match result {
                    Ok(_) => {
                        panel.pending_card_selected.remove(&guard_key);
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                    }
                    Err(notice) => {
                        if notice == "notice.card_not_found" {
                            panel.pending_card_selected.remove(&guard_key);
                        }
                        let active = panel.agent.active_session_id();
                        if active.as_deref() == Some(guard_key.0.as_str())
                            || active.as_deref()
                                == Some(root_session_id.as_str())
                        {
                            panel.emit_notice(Some(notice), cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn localize_rollback_hint(raw: &str, t: &dyn Fn(&str) -> String) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("card.stop_rollback_") {
        t(trimmed)
    } else {
        crate::ports::redact_trace_text(trimmed)
    }
}

pub fn stop_handoff_has_details(manifest: &StopHandoffManifest) -> bool {
    !manifest.modified_files.is_empty()
        || !manifest.verified_effects.is_empty()
        || !manifest.pending_checks.is_empty()
        || !manifest.rollback_hint.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_heartbeat_covers_all_observer_actions_and_redaction_edges() {
        let fake_t = |key: &str| format!("<{key}>");

        let with_secret = BranchHeartbeat {
            current_action:
                "curl -H 'Authorization: Bearer sk-1234567890abcdef1234567890'"
                    .into(),
            ..BranchHeartbeat::default()
        };
        let rendered =
            format_heartbeat_line_localized(&with_secret, &fake_t).unwrap();
        assert!(!rendered.contains("sk-1234567890abcdef1234567890"));
        assert_eq!(
            format_heartbeat_line_localized(
                &BranchHeartbeat::default(),
                &fake_t
            ),
            None
        );

        let cases = [
            ("thinking", "<chat.thinking>"),
            ("reasoning", "<chat.thinking>"),
            ("saving_files", "<chat.status_saving_files>"),
            ("waiting_for_user", "<chat.status_waiting_for_user>"),
            ("stopping", "<chat.status_stopping>"),
        ];
        for (raw, expected) in cases {
            let hb = BranchHeartbeat {
                current_action: raw.into(),
                next_check_hint: Some("raw english hint".into()),
                awaiting_evidence: true,
                ..BranchHeartbeat::default()
            };
            assert_eq!(
                localize_heartbeat_action(&hb, &fake_t).as_deref(),
                Some(expected),
                "observer action {raw:?} should localize"
            );
            assert_eq!(
                format_heartbeat_line_localized(&hb, &fake_t).as_deref(),
                Some(expected),
                "localized line should not leak raw English next_check_hint"
            );
        }

        let evaluating = BranchHeartbeat {
            last_completed: Some("powershell (error)".into()),
            current_action: "evaluating tool evidence".into(),
            next_check_hint: Some("awaiting next model step".into()),
            ..BranchHeartbeat::default()
        };
        assert_eq!(
            localize_heartbeat_action(&evaluating, &fake_t).as_deref(),
            Some("powershell \u{00b7} <chat.status_running>")
        );
        // Should not duplicate `last_completed` when evaluating tool evidence.
        assert_eq!(
            format_heartbeat_line_localized(&evaluating, &fake_t).as_deref(),
            Some("powershell \u{00b7} <chat.status_running>")
        );

        let evaluating_blank = BranchHeartbeat {
            last_completed: Some("   ".into()),
            current_action: "evaluating tool evidence".into(),
            ..BranchHeartbeat::default()
        };
        assert_eq!(
            localize_heartbeat_action(&evaluating_blank, &fake_t).as_deref(),
            Some("<chat.status_running>")
        );

        let waiting_with_last = BranchHeartbeat {
            last_completed: Some("已读取报价表".into()),
            current_action: String::new(),
            awaiting_evidence: true,
            ..BranchHeartbeat::default()
        };
        assert_eq!(
            format_heartbeat_line_localized(&waiting_with_last, &fake_t)
                .as_deref(),
            Some("已读取报价表 \u{00b7} <card.heartbeat_waiting>")
        );
    }

    #[test]
    fn stop_handoff_visibility_and_rollback_localization_work() {
        assert!(!stop_handoff_has_details(&StopHandoffManifest::default()));
        let verified_only = StopHandoffManifest {
            verified_effects: vec!["verify_deck (deck.pptx)".into()],
            ..StopHandoffManifest::default()
        };
        assert!(stop_handoff_has_details(&verified_only));
        assert_eq!(
            localize_rollback_hint("card.stop_rollback_none", &|k| format!(
                "<{k}>"
            )),
            "<card.stop_rollback_none>"
        );
        assert_eq!(
            localize_rollback_hint("legacy hint", &|k| format!("<{k}>")),
            "legacy hint"
        );
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
                        "s-test",
                        None,
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
                        cx,
                    )
                });
                let empty_disambig = self.panel.update(cx, |panel, cx| {
                    panel.render_disambiguation_card(
                        "s-test",
                        "s-test",
                        None,
                        "card-disambig-empty",
                        "empty options guard",
                        "",
                        &[],
                        false,
                        cx,
                    )
                });
                let multiline_preview = (0..18)
                    .map(|i| format!("echo step_{i} ./dist/asset_{i}.txt"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let approval = self.panel.update(cx, |panel, cx| {
                    panel.render_tool_approval_card(
                        "s-test",
                        "s-test",
                        None,
                        "card-tool",
                        "powershell",
                        "workspace/build",
                        "abc123",
                        "将修改工作区文件",
                        &multiline_preview,
                        cx,
                    )
                });
                let routed: Vec<flowy_agent::RoutedPendingCard> = (0..4)
                    .map(|i| flowy_agent::RoutedPendingCard {
                        root_session_id: "s-test".into(),
                        owner_session_id: "s-test".into(),
                        owner_role: flowy_agent::SessionRole::Standard,
                        card: PendingCard::ToolApproval {
                            card_id: format!("stack-card-{i}"),
                            tool_name: "powershell".into(),
                            command_preview: multiline_preview.clone(),
                            args_hash: format!("hash-{i}"),
                            risk_reason: "修改工作区构建产物".into(),
                            target_summary: "workspace/build".into(),
                            created_at_ms: 1_760_000_000_000 + i,
                        },
                    })
                    .collect();
                let cards_stack = self.panel.update(cx, |panel, cx| {
                    panel.render_pending_cards_stack("s-test", routed, cx)
                });
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(disambig)
                    .child(empty_disambig)
                    .child(approval)
                    .child(cards_stack)
            }
        }
        let panel_clone = panel.clone();
        let (_preview, preview_cx) =
            cx.add_window_view(|_, _| CardPreview { panel: panel_clone });
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
        let cmd_bounds = preview_cx
            .debug_bounds("tool-approval-preview")
            .expect("tool-approval-preview bounds");
        assert!(
            (cmd_bounds.size.height - px(140.)).abs() < px(1.)
                && cmd_bounds.size.width > px(100.),
            "multiline command preview ScrollArea must cap at 140px; got {:?}",
            cmd_bounds.size
        );
        let stack_bounds = preview_cx
            .debug_bounds("pending-cards-stack")
            .expect("pending-cards-stack bounds");
        assert!(
            (stack_bounds.size.height - px(320.)).abs() < px(1.)
                && stack_bounds.size.width > px(200.),
            "pending-cards-stack ScrollArea must cap at 320px; got {:?}",
            stack_bounds.size
        );
        drop(panel);
        let _ = std::fs::remove_dir_all(&root);
    }
}
