//! Shared redacted Trace panel, `WorkContractProgress` block, `ReceiptState`
//! visuals, and compact status pill shared by the Board, Modal, and Right Drawer.

use flowy_agent::{ReceiptState, SubmissionOutcome};
use flowy_ui as theme;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, div,
    prelude::*, px,
};

use crate::panel::StudioPanel;
use crate::ports::{
    RedactedTraceEntry, TraceLevel, WorkContractProgress, format_traces_jsonl,
};

/// Trace filter level shared between Flowchart Modal and Right Drawer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TraceFilter {
    #[default]
    All,
    WarnAndError,
}

impl StudioPanel {
    pub(crate) fn receipt_state_visual(
        &self,
        state: ReceiptState,
    ) -> (gpui::Rgba, String, bool) {
        match state {
            ReceiptState::Queued => {
                (self.theme.muted(), self.t("chat.bg_task_queued"), false)
            }
            ReceiptState::Running => (
                self.theme.status_active(),
                self.t("chat.bg_task_running"),
                true,
            ),
            ReceiptState::Retrying => (
                self.theme.status_attention(),
                self.t("board.receipt_retrying"),
                true,
            ),
            ReceiptState::Dead => {
                (self.theme.danger(), self.t("board.receipt_dead"), false)
            }
            ReceiptState::Quarantined => (
                self.theme.status_attention(),
                self.t("board.receipt_quarantined"),
                false,
            ),
            ReceiptState::Cancelled => {
                (self.theme.muted(), self.t("board.receipt_cancelled"), false)
            }
            ReceiptState::Completed => (
                self.theme.status_verified(),
                self.t("verify.card.passed"),
                false,
            ),
        }
    }

    pub(crate) fn contract_progress_checklist(
        &self,
        progress: &WorkContractProgress,
    ) -> AnyElement {
        let total = progress
            .required_labels
            .len()
            .max(progress.delivered_count + progress.missing_items.len());
        let summary = self.tf(
            "board.contract_progress",
            &[
                ("done", &progress.delivered_count.to_string()),
                ("total", &total.to_string()),
            ],
        );
        let outcome_color = match &progress.outcome {
            Some(SubmissionOutcome::Completed) => self.theme.status_verified(),
            Some(SubmissionOutcome::Partial { .. }) => {
                self.theme.status_attention()
            }
            Some(SubmissionOutcome::Failed { .. }) => self.theme.danger(),
            Some(SubmissionOutcome::Aborted) | None => self.theme.muted(),
        };
        let mut missing_pool: Vec<String> = progress
            .missing_items
            .iter()
            .map(|item| item.trim().to_ascii_lowercase())
            .collect();
        let checklist_rows: Vec<(String, bool, bool)> =
            if progress.required_labels.is_empty() {
                Vec::new()
            } else {
                let settled_known = progress.outcome.is_some()
                    || progress.delivered_count > 0
                    || !progress.missing_items.is_empty();
                progress
                    .required_labels
                    .iter()
                    .map(|label| {
                        let norm = label.trim().to_ascii_lowercase();
                        let is_missing = if let Some(pos) = missing_pool
                            .iter()
                            .position(|candidate| *candidate == norm)
                        {
                            missing_pool.remove(pos);
                            true
                        } else {
                            false
                        };
                        let verified = !is_missing && settled_known;
                        (label.clone(), verified, is_missing)
                    })
                    .collect()
            };
        let remaining_missing: Vec<String> = progress
            .missing_items
            .iter()
            .filter(|item| {
                let norm = item.trim().to_ascii_lowercase();
                missing_pool.contains(&norm)
            })
            .cloned()
            .collect();
        let missing_line = (!remaining_missing.is_empty()).then(|| {
            self.tf(
                "board.contract_missing",
                &[("items", &remaining_missing.join(", "))],
            )
        });
        div()
            .w_full()
            .p(self.theme.space(8.))
            .rounded_lg()
            .bg(self.theme.soft().opacity(0.6))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(6.))
                    .text_xs()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(outcome_color)
                            .child(summary),
                    )
                    .when_some(progress.contract_revision, |row, rev| {
                        row.child(
                            div()
                                .text_color(self.theme.muted())
                                .child(format!("rev {rev}")),
                        )
                    }),
            )
            .when(!checklist_rows.is_empty(), |col| {
                let mut items =
                    div().w_full().flex().flex_col().gap(px(3.)).pt(px(2.));
                for (label, verified, is_missing) in checklist_rows {
                    let (icon, color) = if is_missing {
                        (
                            theme::icons::circle_alert(),
                            self.theme.status_attention(),
                        )
                    } else if verified {
                        (theme::icons::check(), self.theme.status_verified())
                    } else {
                        (theme::icons::clock(), self.theme.muted())
                    };
                    items = items.child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_xs()
                            .child(
                                icon.flex_none()
                                    .size(px(12.))
                                    .text_color(color),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .text_color(if is_missing {
                                        self.theme.status_attention()
                                    } else {
                                        self.theme.ink()
                                    })
                                    .child(label),
                            ),
                    );
                }
                col.child(items)
            })
            .when_some(missing_line, |col, text| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.status_attention())
                        .whitespace_normal()
                        .child(text),
                )
            })
            .into_any_element()
    }

    pub(crate) fn modal_trace_panel(
        &self,
        session_id: &str,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let entries: &[RedactedTraceEntry] = self
            .cached_trace
            .as_ref()
            .filter(|(cached_sid, _)| cached_sid == session_id)
            .map_or(&[], |(_, rows)| rows.as_slice());
        let filtered: Vec<_> = entries
            .iter()
            .filter(|entry| match self.trace_filter {
                TraceFilter::All => true,
                TraceFilter::WarnAndError => {
                    matches!(entry.level, TraceLevel::Warn | TraceLevel::Error)
                }
            })
            .collect();
        let copy_sid = session_id.to_owned();

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
                        let jsonl = panel
                            .cached_trace
                            .as_ref()
                            .filter(|(cached_sid, _)| cached_sid == &copy_sid)
                            .map_or_else(String::new, |(_, rows)| {
                                format_traces_jsonl(rows)
                            });
                        cx.write_to_clipboard(ClipboardItem::new_string(jsonl));
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
