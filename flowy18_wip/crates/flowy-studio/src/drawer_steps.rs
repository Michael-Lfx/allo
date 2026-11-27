//! Structured, turn-grouped step timeline for the right-hand Steps drawer tab.
//!
//! Replaces flat numbered text rows with grouped turns (latest expanded, earlier
//! turns collapsible), expandable step details with redacted content, clickable
//! deliverable chips that open inline preview, and safe error-action buttons.

use flowy_agent::{ChatLine, FileEntry, TurnStatus, VerificationIssue};
use flowy_ui as theme;
use gpui::{
    AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px,
};

use crate::panel::{StudioEvent, StudioPanel};

const STEP_SNIPPET_CHARS: usize = 56;
const STEP_DETAIL_MAX_CHARS: usize = 1600;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepKind {
    Thinking {
        duration_secs: u64,
        live: bool,
        text: String,
    },
    ToolCall {
        name: String,
        target: String,
        call_id: String,
        running: bool,
        result: Option<(bool, String)>,
    },
    ToolResult {
        text: String,
        is_error: bool,
    },
    LegacyTool {
        text: String,
    },
    Status {
        text: String,
    },
    FileChanges {
        files: Vec<FileEntry>,
    },
    Verification {
        report_id: String,
        artifact_path: String,
        ok: bool,
        override_used: bool,
        issues: Vec<VerificationIssue>,
    },
    Artifact {
        artifact_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepItem {
    pub id: String,
    pub kind: StepKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepTurnGroup {
    pub turn_ix: usize,
    pub turn_number: usize,
    pub user_prompt: String,
    pub steps: Vec<StepItem>,
    pub has_error: bool,
    pub running: bool,
    pub file_count: usize,
}

fn bounded_redacted_text(raw: &str, max_chars: usize) -> String {
    let redacted = crate::ports::redact_trace_text(raw);
    let mut chars = redacted.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}\u{2026}")
    } else {
        head
    }
}

/// Projects chat history lines into turn-grouped structured steps, folding each
/// `ToolCall` with its immediately following `ToolResult` inside the same turn.
#[must_use]
pub fn collect_step_turns(
    lines: &[ChatLine],
    status: &TurnStatus,
) -> Vec<StepTurnGroup> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut turn_starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(ix, line)| {
            matches!(line, ChatLine::User { .. }).then_some(ix)
        })
        .collect();
    if turn_starts.first().copied() != Some(0) {
        turn_starts.insert(0, 0);
    }
    turn_starts.push(lines.len());

    let last_slice_ix = turn_starts.len().saturating_sub(2);
    let mut groups = Vec::new();
    for (turn_ix, bounds) in turn_starts.windows(2).enumerate() {
        let mut user_prompt = String::new();
        let mut steps: Vec<StepItem> = Vec::new();
        let mut has_error = false;
        let mut file_count = 0usize;
        let mut last_status: Option<String> = None;

        for line in &lines[bounds[0]..bounds[1]] {
            match line {
                ChatLine::User { text } => {
                    if user_prompt.is_empty() {
                        user_prompt = crate::composer::truncate_quote_excerpt(
                            &crate::ports::redact_trace_text(text),
                            72,
                        );
                    }
                }
                ChatLine::Assistant { .. } | ChatLine::TaskReceipt { .. } => {}
                ChatLine::Reasoning {
                    text,
                    duration_secs,
                    live,
                } => {
                    if text.trim().is_empty() && !*live {
                        continue;
                    }
                    last_status = None;
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::Thinking {
                            duration_secs: *duration_secs,
                            live: *live,
                            text: text.clone(),
                        },
                    });
                }
                ChatLine::ToolCall {
                    name,
                    target,
                    call_id,
                    running,
                } => {
                    last_status = None;
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::ToolCall {
                            name: name.clone(),
                            target: target.clone(),
                            call_id: call_id.clone(),
                            running: *running,
                            result: None,
                        },
                    });
                }
                ChatLine::ToolResult { text, is_error } => {
                    last_status = None;
                    if *is_error {
                        has_error = true;
                    }
                    if let Some(StepItem {
                        kind:
                            StepKind::ToolCall {
                                running,
                                result: slot @ None,
                                ..
                            },
                        ..
                    }) = steps.last_mut()
                    {
                        *running = false;
                        *slot = Some((*is_error, text.clone()));
                    } else {
                        let id = format!("t{turn_ix}-s{}", steps.len());
                        steps.push(StepItem {
                            id,
                            kind: StepKind::ToolResult {
                                text: text.clone(),
                                is_error: *is_error,
                            },
                        });
                    }
                }
                ChatLine::Tool { text } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    last_status = None;
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::LegacyTool { text: text.clone() },
                    });
                }
                ChatLine::Status { text } => {
                    let trimmed = text.trim();
                    if trimmed.is_empty()
                        || last_status.as_deref() == Some(trimmed)
                    {
                        continue;
                    }
                    last_status = Some(trimmed.to_owned());
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::Status { text: text.clone() },
                    });
                }
                ChatLine::FileChange {
                    path,
                    change,
                    added,
                    removed,
                    binary,
                } => {
                    last_status = None;
                    file_count += 1;
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::FileChanges {
                            files: vec![FileEntry {
                                path: path.clone(),
                                change: *change,
                                added: *added,
                                removed: *removed,
                                binary: *binary,
                            }],
                        },
                    });
                }
                ChatLine::FileChanges { files } => {
                    if files.is_empty() {
                        continue;
                    }
                    last_status = None;
                    file_count += files.len();
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::FileChanges {
                            files: files.clone(),
                        },
                    });
                }
                ChatLine::Verification {
                    report_id,
                    artifact_path,
                    ok,
                    issues,
                    override_used,
                    ..
                } => {
                    last_status = None;
                    if !*ok && !*override_used {
                        has_error = true;
                    }
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::Verification {
                            report_id: report_id.clone(),
                            artifact_path: artifact_path.clone(),
                            ok: *ok,
                            override_used: *override_used,
                            issues: issues.clone(),
                        },
                    });
                }
                ChatLine::Artifact { artifact_id } => {
                    last_status = None;
                    let id = format!("t{turn_ix}-s{}", steps.len());
                    steps.push(StepItem {
                        id,
                        kind: StepKind::Artifact {
                            artifact_id: artifact_id.clone(),
                        },
                    });
                }
            }
        }

        let is_active_tail = turn_ix == last_slice_ix && status.is_busy();
        if steps.is_empty() && !is_active_tail {
            continue;
        }
        let turn_number = groups.len() + 1;
        groups.push(StepTurnGroup {
            turn_ix,
            turn_number,
            user_prompt,
            steps,
            has_error,
            running: is_active_tail,
            file_count,
        });
    }
    groups
}

impl StudioPanel {
    pub fn structured_steps_view(
        &self,
        session_id: &str,
        lines: &[ChatLine],
        is_running: bool,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let status = if is_running {
            TurnStatus::Running
        } else {
            TurnStatus::Completed
        };
        let turns = collect_step_turns(lines, &status);
        if turns.is_empty() {
            return div()
                .w_full()
                .px(self.theme.space(10.))
                .py(self.theme.space(12.))
                .rounded_lg()
                .bg(self.theme.soft().opacity(0.45))
                .text_xs()
                .text_color(self.theme.muted())
                .whitespace_normal()
                .child(self.t("drawer.work_no_steps"))
                .into_any_element();
        }

        let latest_turn_ix = turns.last().map(|t| t.turn_ix);
        let toggled = self.step_turn_toggled.get(session_id);
        let mut root =
            div().w_full().flex().flex_col().gap(self.theme.space(8.));

        for turn in turns.into_iter().rev() {
            let default_open = Some(turn.turn_ix) == latest_turn_ix;
            let flipped =
                toggled.is_some_and(|set| set.contains(&turn.turn_ix));
            let expanded = default_open ^ flipped;
            root =
                root.child(self.render_step_turn_card(
                    session_id, turn, expanded, window, cx,
                ));
        }
        root.into_any_element()
    }

    fn render_step_turn_card(
        &self,
        session_id: &str,
        turn: StepTurnGroup,
        expanded: bool,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let sid = session_id.to_owned();
        let turn_ix = turn.turn_ix;
        let turn_tag = self.tf(
            "drawer.step_turn_badge",
            &[("n", &turn.turn_number.to_string())],
        );
        let steps_count = self.tf(
            "drawer.step_count",
            &[("count", &turn.steps.len().to_string())],
        );
        let summary_label = if turn.user_prompt.is_empty() {
            format!("{turn_tag} · {steps_count}")
        } else {
            format!("{turn_tag} · {} · {steps_count}", turn.user_prompt)
        };

        let header = theme::interactive(
            ("drawer-step-turn", turn.turn_ix as u64),
            SharedString::from(summary_label),
            &self.theme,
        )
        .w_full()
        .px(self.theme.space(10.))
        .py(self.theme.space(8.))
        .rounded_lg()
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft().opacity(0.6)))
        .flex()
        .items_center()
        .gap(self.theme.space(8.))
        .child(
            div()
                .flex_none()
                .px(px(6.))
                .py(px(1.))
                .rounded_md()
                .bg(self.theme.soft())
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(self.theme.ink())
                .child(turn_tag),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(if turn.user_prompt.is_empty() {
                    self.theme.muted()
                } else {
                    self.theme.ink()
                })
                .overflow_hidden()
                .whitespace_nowrap()
                .truncate()
                .child(if turn.user_prompt.is_empty() {
                    steps_count.clone()
                } else {
                    turn.user_prompt.clone()
                }),
        )
        .when(turn.running, |row| {
            row.child(crate::motion::live_dot(
                ("step-turn-live", turn.turn_ix as u64),
                self.theme.status_active(),
                self.theme.motion_policy(),
            ))
        })
        .when(!turn.running && turn.has_error, |row| {
            row.child(
                theme::icons::circle_alert()
                    .flex_none()
                    .size(px(12.))
                    .text_color(self.theme.status_attention()),
            )
        })
        .when(turn.file_count > 0, |row| {
            row.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(format!(
                        "{} {}",
                        turn.file_count,
                        self.t("chat.deliverables")
                    )),
            )
        })
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(self.theme.muted())
                .child(if turn.user_prompt.is_empty() {
                    String::new()
                } else {
                    steps_count
                }),
        )
        .child(
            if expanded {
                theme::icons::chevron_down()
            } else {
                theme::icons::chevron_right()
            }
            .flex_none()
            .size(px(12.))
            .text_color(self.theme.muted()),
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.toggle_step_turn(&sid, turn_ix, cx);
        }));

        let mut card = div()
            .w_full()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.canvas())
            .flex()
            .flex_col()
            .child(header);

        if expanded {
            let mut body = div()
                .w_full()
                .px(self.theme.space(8.))
                .pb(self.theme.space(8.))
                .pt(px(2.))
                .border_t_1()
                .border_color(self.theme.line())
                .flex()
                .flex_col()
                .gap(self.theme.space(6.));
            if turn.steps.is_empty() && turn.running {
                body = body.child(
                    div()
                        .px(self.theme.space(8.))
                        .py(self.theme.space(6.))
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(self.t("chat.status_running")),
                );
            } else {
                for step in turn.steps {
                    body = body.child(self.render_step_item(step, window, cx));
                }
            }
            card = card.child(body);
        }

        card.into_any_element()
    }

    fn render_step_item(
        &self,
        step: StepItem,
        _window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let item_id = step.id;
        let expanded = self.expanded_step_items.contains(&item_id);
        let copied = self
            .drawer_step_copied
            .as_ref()
            .is_some_and(|(id, _)| id == &item_id);

        match step.kind {
            StepKind::Thinking {
                duration_secs,
                live,
                text,
            } => {
                let heading = if duration_secs > 0 {
                    format!(
                        "{} · {}s",
                        self.t("drawer.step_thinking"),
                        duration_secs
                    )
                } else {
                    self.t("drawer.step_thinking")
                };
                let snippet = crate::composer::truncate_quote_excerpt(
                    &crate::ports::redact_trace_text(&text),
                    STEP_SNIPPET_CHARS,
                );
                let detail = (!text.trim().is_empty()).then(|| {
                    bounded_redacted_text(&text, STEP_DETAIL_MAX_CHARS)
                });
                self.render_expandable_step_shell(
                    &item_id,
                    theme::icons::sparkles().size(px(13.)),
                    if live {
                        self.theme.status_active()
                    } else {
                        self.theme.muted()
                    },
                    heading,
                    snippet,
                    detail,
                    None,
                    expanded,
                    copied,
                    cx,
                )
            }
            StepKind::ToolCall {
                name,
                target,
                call_id: _,
                running,
                result,
            } => {
                let redacted_name = crate::ports::redact_trace_text(&name);
                let redacted_target = crate::ports::redact_trace_text(&target);
                let is_error = result.as_ref().is_some_and(|(err, _)| *err);
                let (icon, color) = if running {
                    (
                        theme::icons::loader_circle().size(px(13.)),
                        self.theme.status_active(),
                    )
                } else if is_error {
                    (
                        theme::icons::circle_alert().size(px(13.)),
                        self.theme.danger(),
                    )
                } else {
                    (theme::icons::workflow().size(px(13.)), self.theme.ink())
                };
                let title = if redacted_target.is_empty() {
                    redacted_name.clone()
                } else {
                    format!(
                        "{redacted_name} · {}",
                        crate::ports::clip_label(&redacted_target, 32)
                    )
                };
                let subtitle = match &result {
                    Some((true, output)) => {
                        let snippet = crate::composer::truncate_quote_excerpt(
                            &crate::ports::redact_trace_text(output),
                            STEP_SNIPPET_CHARS,
                        );
                        if snippet.is_empty() {
                            self.t("chat.status_error")
                        } else {
                            snippet
                        }
                    }
                    Some((false, output)) => {
                        crate::composer::truncate_quote_excerpt(
                            &crate::ports::redact_trace_text(output),
                            STEP_SNIPPET_CHARS,
                        )
                    }
                    None if running => self.t("chat.status_running"),
                    None => String::new(),
                };
                let detail = result
                    .as_ref()
                    .map(|(_, output)| output.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        (!redacted_target.is_empty()).then_some(target.as_str())
                    })
                    .map(|s| bounded_redacted_text(s, STEP_DETAIL_MAX_CHARS));
                let fix_summary = is_error.then(|| {
                    let msg =
                        result.as_ref().map_or("", |(_, out)| out.as_str());
                    bounded_redacted_text(
                        &format!("{redacted_name}: {msg}"),
                        240,
                    )
                });
                self.render_expandable_step_shell(
                    &item_id,
                    icon,
                    color,
                    title,
                    subtitle,
                    detail,
                    fix_summary.map(|s| (s, None)),
                    expanded,
                    copied,
                    cx,
                )
            }
            StepKind::ToolResult { text, is_error } => {
                let title = if is_error {
                    self.t("chat.status_error")
                } else {
                    self.t("drawer.step_tool_output")
                };
                let snippet = crate::composer::truncate_quote_excerpt(
                    &crate::ports::redact_trace_text(&text),
                    STEP_SNIPPET_CHARS,
                );
                let detail = (!text.trim().is_empty()).then(|| {
                    bounded_redacted_text(&text, STEP_DETAIL_MAX_CHARS)
                });
                let fix =
                    is_error.then(|| (bounded_redacted_text(&text, 240), None));
                self.render_expandable_step_shell(
                    &item_id,
                    if is_error {
                        theme::icons::circle_alert().size(px(13.))
                    } else {
                        theme::icons::check().size(px(13.))
                    },
                    if is_error {
                        self.theme.danger()
                    } else {
                        self.theme.status_verified()
                    },
                    title,
                    snippet,
                    detail,
                    fix,
                    expanded,
                    copied,
                    cx,
                )
            }
            StepKind::LegacyTool { text } => {
                let redacted = crate::ports::redact_trace_text(&text);
                let title = crate::ports::clip_label(&redacted, 48);
                let detail = (redacted.chars().count() > 48).then(|| {
                    bounded_redacted_text(&text, STEP_DETAIL_MAX_CHARS)
                });
                self.render_expandable_step_shell(
                    &item_id,
                    theme::icons::workflow().size(px(13.)),
                    self.theme.muted(),
                    title,
                    String::new(),
                    detail,
                    None,
                    expanded,
                    copied,
                    cx,
                )
            }
            StepKind::Status { text } => {
                let title = crate::ports::redact_trace_text(&text);
                div()
                    .id(SharedString::from(format!("step-{item_id}")))
                    .w_full()
                    .px(self.theme.space(8.))
                    .py(px(5.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.45))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .child(
                        theme::icons::info()
                            .flex_none()
                            .size(px(12.))
                            .text_color(self.theme.muted()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(title),
                    )
                    .into_any_element()
            }
            StepKind::FileChanges { files } => {
                let label = self.tf(
                    "chat.file_changes",
                    &[("count", &files.len().to_string())],
                );
                let mut chips = div().w_full().flex().flex_wrap().gap(px(4.));
                for (ix, file) in files.iter().enumerate() {
                    let name = crate::ports::basename(&file.path);
                    let target_path = file.path.clone();
                    let failed = self.feed.read(cx).is_path_failed(&file.path);
                    chips = chips.child(
                        theme::interactive(
                            SharedString::from(format!(
                                "step-file-{item_id}-{ix}"
                            )),
                            SharedString::from(format!(
                                "{} · {name}",
                                self.t("chat.deliverables")
                            )),
                            &self.theme,
                        )
                        .px(px(6.))
                        .py(px(2.))
                        .rounded_md()
                        .border_1()
                        .border_color(if failed {
                            self.theme.status_attention()
                        } else {
                            self.theme.line()
                        })
                        .bg(self.theme.card())
                        .cursor_pointer()
                        .hover(|style| style.bg(self.theme.soft()))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_xs()
                        .text_color(if failed {
                            self.theme.status_attention()
                        } else {
                            self.theme.ink()
                        })
                        .child(
                            theme::icons::file_text()
                                .size(px(11.))
                                .text_color(self.theme.muted()),
                        )
                        .child(name)
                        .on_click(cx.listener(
                            move |panel, _, _, cx| {
                                panel.open_step_file_in_deliverables(
                                    &target_path,
                                    cx,
                                );
                            },
                        )),
                    );
                }
                div()
                    .id(SharedString::from(format!("step-{item_id}")))
                    .w_full()
                    .p(self.theme.space(8.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.45))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .child(
                                theme::icons::file_text()
                                    .flex_none()
                                    .size(px(13.))
                                    .text_color(self.theme.ink()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(self.theme.ink())
                                    .child(label),
                            ),
                    )
                    .child(chips)
                    .into_any_element()
            }
            StepKind::Verification {
                report_id,
                artifact_path,
                ok,
                override_used,
                issues,
            } => {
                let name = crate::ports::basename(&artifact_path);
                let (icon, color, badge) = if ok {
                    (
                        theme::icons::check().size(px(13.)),
                        self.theme.status_verified(),
                        self.t("verify.card.passed"),
                    )
                } else if override_used {
                    (
                        theme::icons::circle_alert().size(px(13.)),
                        self.theme.status_attention(),
                        self.t("verify.override.badge"),
                    )
                } else {
                    (
                        theme::icons::circle_alert().size(px(13.)),
                        self.theme.danger(),
                        self.t("verify.card.failed"),
                    )
                };
                let title = format!("{badge} · {name}");
                let issues_text = issues
                    .iter()
                    .map(|issue| {
                        format!(
                            "• {} · {}",
                            self.t(flowy_agent::verify_ui_key(&issue.code)),
                            crate::ports::redact_trace_text(&issue.field)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let failed_unsettled = !ok && !override_used;
                let locate_id = report_id;
                let locate_path = artifact_path.clone();
                let fix_summary = format!("{badge}: {name}");
                let open_path = artifact_path.clone();

                let mut card = div()
                    .id(SharedString::from(format!("step-{item_id}")))
                    .w_full()
                    .p(self.theme.space(8.))
                    .rounded_lg()
                    .border_1()
                    .border_color(if failed_unsettled {
                        self.theme.danger().opacity(0.45)
                    } else {
                        self.theme.line()
                    })
                    .bg(if failed_unsettled {
                        self.theme.danger().opacity(0.06)
                    } else {
                        self.theme.soft().opacity(0.45)
                    })
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .child(icon.flex_none().text_color(color))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(color)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(title),
                            ),
                    );
                if !issues_text.is_empty() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(bounded_redacted_text(
                                &issues_text,
                                STEP_DETAIL_MAX_CHARS,
                            )),
                    );
                }
                let mut actions = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(self.theme.space(6.));
                if failed_unsettled {
                    let fix_path = Some(artifact_path);
                    actions = actions
                        .child(
                            theme::Button::new(
                                SharedString::from(format!(
                                    "step-verify-fix-{item_id}"
                                )),
                                self.t("drawer.fix_in_chat"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, window, cx| {
                                    panel.request_fix_in_chat(
                                        fix_summary.clone(),
                                        fix_path.clone(),
                                        Some(window),
                                        cx,
                                    );
                                }),
                            ),
                        )
                        .child(
                            theme::Button::new(
                                SharedString::from(format!(
                                    "step-verify-locate-{item_id}"
                                )),
                                self.t("drawer.locate_card"),
                            )
                            .variant(theme::ControlVariant::Ghost)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, window, cx| {
                                    panel.locate_verification_card(
                                        &locate_id,
                                        &locate_path,
                                        Some(window),
                                        cx,
                                    );
                                }),
                            ),
                        );
                } else if !open_path.trim().is_empty() {
                    actions = actions.child(
                        theme::Button::new(
                            SharedString::from(format!(
                                "step-verify-open-{item_id}"
                            )),
                            self.t("library.preview"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                panel.open_step_file_in_deliverables(
                                    &open_path, cx,
                                );
                            }),
                        ),
                    );
                }
                card.child(actions).into_any_element()
            }
            StepKind::Artifact { artifact_id } => {
                let title = self
                    .feed
                    .read(cx)
                    .artifact_title(&artifact_id)
                    .unwrap_or_else(|| crate::ports::short_id(&artifact_id));
                let aid = artifact_id;
                theme::interactive(
                    SharedString::from(format!("step-art-{item_id}")),
                    SharedString::from(title.clone()),
                    &self.theme,
                )
                .w_full()
                .px(self.theme.space(8.))
                .py(px(6.))
                .rounded_lg()
                .bg(self.theme.soft().opacity(0.45))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .gap(self.theme.space(6.))
                .child(
                    theme::icons::library()
                        .flex_none()
                        .size(px(13.))
                        .text_color(self.theme.ink()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(title),
                )
                .child(
                    theme::icons::arrow_up_right()
                        .flex_none()
                        .size(px(12.))
                        .text_color(self.theme.muted()),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(StudioEvent::PreviewArtifact(aid.clone()));
                }))
                .into_any_element()
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared layout shell for collapsible tool/thinking step rows"
    )]
    fn render_expandable_step_shell(
        &self,
        item_id: &str,
        icon: theme::icons::Icon,
        accent_color: gpui::Rgba,
        title: String,
        subtitle: String,
        detail: Option<String>,
        fix_prompt: Option<(String, Option<String>)>,
        expanded: bool,
        copied: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let can_expand = detail.is_some();
        let toggle_id = item_id.to_owned();
        let header: AnyElement = if can_expand {
            theme::interactive(
                SharedString::from(format!("step-toggle-{item_id}")),
                SharedString::from(title.clone()),
                &self.theme,
            )
            .w_full()
            .px(self.theme.space(8.))
            .py(px(6.))
            .rounded_lg()
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft().opacity(0.6)))
            .flex()
            .items_start()
            .gap(self.theme.space(6.))
            .child(icon.flex_none().mt(px(2.)).text_color(accent_color))
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
                            .text_color(accent_color)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(title),
                    )
                    .when(!subtitle.is_empty() && !expanded, |col| {
                        col.child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(subtitle),
                        )
                    }),
            )
            .child(
                if expanded {
                    theme::icons::chevron_down()
                } else {
                    theme::icons::chevron_right()
                }
                .flex_none()
                .mt(px(2.))
                .size(px(12.))
                .text_color(self.theme.muted()),
            )
            .on_click(cx.listener(move |panel, _, _, cx| {
                panel.toggle_step_item(toggle_id.clone(), cx);
            }))
            .into_any_element()
        } else {
            div()
                .w_full()
                .px(self.theme.space(8.))
                .py(px(6.))
                .flex()
                .items_start()
                .gap(self.theme.space(6.))
                .child(icon.flex_none().mt(px(2.)).text_color(accent_color))
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
                                .text_color(accent_color)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(title),
                        )
                        .when(!subtitle.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .whitespace_normal()
                                    .child(subtitle),
                            )
                        }),
                )
                .into_any_element()
        };

        let mut card = div()
            .id(SharedString::from(format!("step-{item_id}")))
            .w_full()
            .rounded_lg()
            .bg(self.theme.soft().opacity(0.45))
            .flex()
            .flex_col()
            .child(header);

        if expanded && let Some(detail_text) = detail {
            let copy_id = item_id.to_owned();
            let copy_payload = detail_text.clone();
            let detail_box = div()
                .w_full()
                .px(self.theme.space(8.))
                .pb(self.theme.space(8.))
                .pt(self.theme.space(6.))
                .border_t_1()
                .border_color(self.theme.line())
                .flex()
                .flex_col()
                .gap(self.theme.space(6.))
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(self.theme.ink())
                        .whitespace_normal()
                        .child(detail_text),
                )
                .child({
                    let mut bar = div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(self.theme.space(6.))
                        .child(
                            theme::Button::new(
                                SharedString::from(format!(
                                    "step-copy-{item_id}"
                                )),
                                if copied {
                                    self.t("chat.copied")
                                } else {
                                    self.t("drawer.copy")
                                },
                            )
                            .variant(theme::ControlVariant::Ghost)
                            .size(theme::ControlSize::Small)
                            .icon(if copied {
                                theme::icons::check()
                            } else {
                                theme::icons::copy()
                            })
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.copy_drawer_step_text(
                                        copy_id.clone(),
                                        copy_payload.clone(),
                                        cx,
                                    );
                                }),
                            ),
                        );
                    if let Some((fix_summary, fix_path)) = fix_prompt {
                        bar = bar.child(
                            theme::Button::new(
                                SharedString::from(format!(
                                    "step-fix-{item_id}"
                                )),
                                self.t("drawer.fix_in_chat"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, window, cx| {
                                    panel.request_fix_in_chat(
                                        fix_summary.clone(),
                                        fix_path.clone(),
                                        Some(window),
                                        cx,
                                    );
                                }),
                            ),
                        );
                    }
                    bar
                });
            card = card.child(detail_box);
        }

        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{StepKind, collect_step_turns};
    use flowy_agent::{
        ChatLine, FileChangeKind, FileEntry, TurnStatus, VerificationIssue,
    };

    #[test]
    fn collect_step_turns_groups_by_user_pairs_tools_and_flags_errors() {
        let lines = vec![
            ChatLine::User {
                text: "Build release table".into(),
            },
            ChatLine::Reasoning {
                text: "Inspect input files".into(),
                duration_secs: 2,
                live: false,
            },
            ChatLine::ToolCall {
                name: "read_file".into(),
                target: "/tmp/in.csv".into(),
                call_id: "c1".into(),
                running: true,
            },
            ChatLine::ToolResult {
                text: "rows: 10".into(),
                is_error: false,
            },
            ChatLine::FileChanges {
                files: vec![FileEntry {
                    path: "/tmp/out.csv".into(),
                    change: FileChangeKind::Add,
                    added: 12,
                    removed: 0,
                    binary: false,
                }],
            },
            ChatLine::User {
                text: "Verify output".into(),
            },
            ChatLine::Verification {
                report_id: "rep-2".into(),
                artifact_path: "/tmp/out.csv".into(),
                artifact_hash: "abc".into(),
                ok: false,
                issues: vec![VerificationIssue {
                    code: "missing_total".into(),
                    field: "total".into(),
                }],
                semantic_hint: None,
                override_used: false,
                provenance_warnings: Vec::new(),
                source_session_id: None,
            },
        ];

        let turns = collect_step_turns(&lines, &TurnStatus::Completed);
        assert_eq!(turns.len(), 2);
        assert!(!turns[0].has_error);
        assert_eq!(turns[0].file_count, 1);
        assert_eq!(turns[0].steps.len(), 3);
        assert!(matches!(
            &turns[0].steps[1].kind,
            StepKind::ToolCall {
                running: false,
                result: Some((false, out)),
                ..
            } if out == "rows: 10"
        ));
        assert!(turns[1].has_error);
        assert_eq!(turns[1].turn_number, 2);
        assert!(matches!(
            &turns[1].steps[0].kind,
            StepKind::Verification { ok: false, .. }
        ));
    }
}
