//! File-change summaries, artifact preview cards, and verification cards with explicit override gating.

use flowy_ui as theme;
use gpui::{Context, FontWeight, SharedString, div, prelude::*, px};

use super::{ArtifactAction, ChatFeed, OpenDeliverables, RevealFile};
use crate::library::ArtifactAvailability;

/// Keeps a long path readable in a narrow card: strips Windows `\\?\` prefixes
/// and keeps the tail (folder and file name) legible.
fn display_path(path: &str) -> String {
    let cleaned = crate::ports::display_path_str(path);
    const MAX: usize = 64;
    if cleaned.chars().count() <= MAX {
        return cleaned;
    }
    let tail: String = cleaned
        .chars()
        .skip(cleaned.chars().count() - (MAX - 1))
        .collect();
    format!("\u{2026}{tail}")
}

impl ChatFeed {
    /// One changed file: type icon, file name, full path, and line delta.
    /// Clicking opens the containing folder, matching the project view action.
    #[expect(
        clippy::too_many_arguments,
        reason = "one row reads better than a struct built at each call site"
    )]
    pub(super) fn file_change_row(
        &self,
        index: usize,
        path: &str,
        kind: &flowy_agent::FileChangeKind,
        _added: u32,
        _removed: u32,
        _binary: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let icon = crate::file_kind::from_path(path).map_or_else(
            || {
                theme::icons::file_text()
                    .size(px(18.))
                    .text_color(self.theme.muted())
                    .into_any_element()
            },
            |file_kind| file_kind.icon().size(px(20.)).into_any_element(),
        );
        let kind_label = self.t(crate::file_kind::kind_label_key(path));
        let target = path.to_owned();
        let _ = kind;
        theme::interactive(
            ("chat-file-change", self.row_ids[index]),
            SharedString::from(name.to_owned()),
            &self.theme,
        )
        .w_full()
        .px(self.theme.space(12.))
        .py(self.theme.space(8.))
        .gap(self.theme.space(8.))
        .flex()
        .items_center()
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft()))
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(display_path(path)),
                ),
        )
        .child(
            div()
                .flex_none()
                .px(self.theme.space(6.))
                .py(px(2.))
                .rounded_md()
                .bg(self.theme.soft())
                .text_xs()
                .text_color(self.theme.muted())
                .child(kind_label),
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.emit(RevealFile(target.clone()));
        }))
    }

    /// One turn's file work, folded into a single summary row.
    pub(super) fn file_changes_row(
        &self,
        index: usize,
        files: &[flowy_agent::FileEntry],
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let count = files.len();
        let label =
            self.tf("chat.file_changes", &[("count", &count.to_string())]);
        let all = files.to_vec();
        theme::interactive(
            ("chat-file-changes", self.row_ids[index]),
            SharedString::from(label.clone()),
            &self.theme,
        )
        .w_full()
        .px(self.theme.space(12.))
        .py(self.theme.space(8.))
        .gap(self.theme.space(8.))
        .flex()
        .items_center()
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.soft()))
        .child(
            theme::icons::file_text()
                .size(px(18.))
                .text_color(self.theme.muted()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .px(self.theme.space(6.))
                .py(px(2.))
                .rounded_md()
                .bg(self.theme.soft())
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t("chat.deliverables")),
        )
        .child(
            theme::icons::chevron_right()
                .size(px(12.))
                .text_color(self.theme.muted()),
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.emit(OpenDeliverables(all.clone()));
        }))
    }

    pub(super) fn artifact_card(
        &self,
        index: usize,
        artifact_id: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = artifact_id.to_owned();
        let save_id = id.clone();
        let item = self.artifacts.get(artifact_id);
        let icon = item
            .and_then(crate::library::artifact_file_icon)
            .map_or_else(
                || theme::icons::file_text().size(px(24.)).into_any_element(),
                |icon| icon.size(px(24.)).into_any_element(),
            );
        let title = item
            .map_or_else(|| self.t("chat.artifact"), |item| item.title.clone());
        let unavailable = item.is_none_or(|item| {
            item.availability != ArtifactAvailability::Available
        });
        let details = item.map_or_else(
            || self.t("library.loading"),
            |item| {
                let size = item.size_bytes.unwrap_or_default();
                let format = item.extension.to_uppercase();
                let mut details = self.i18n.tf(
                    "library.preview.file_info",
                    &[
                        ("format", &format),
                        ("size", &format!("{:.1}", size as f64 / 1024.)),
                    ],
                );
                if item.availability != ArtifactAvailability::Available {
                    details.push_str(" · ");
                    details.push_str(&self.t(match item.availability {
                        ArtifactAvailability::Missing => {
                            "library.availability.missing"
                        }
                        ArtifactAvailability::Corrupt => {
                            "library.availability.corrupt"
                        }
                        ArtifactAvailability::Available => {
                            "library.availability.available"
                        }
                    }));
                }
                if item.override_used {
                    details.push_str(" · ");
                    details.push_str(&self.t("verify.override.badge"));
                }
                details
            },
        );
        let source = item
            .filter(|item| {
                !item.source_deleted
                    && !item.source_unavailable
                    && !item.source_session_id.is_empty()
            })
            .map(|item| item.source_session_id.clone());
        div()
            .id(("artifact-card", self.row_ids[index]))
            .w_full()
            .max_w(px(540.))
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .px(self.theme.space(12.))
            .py(self.theme.space(10.))
            .child(
                theme::interactive(
                    ("artifact-preview", self.row_ids[index]),
                    title.clone(),
                    &self.theme,
                )
                .debug_selector(|| "chat-artifact-preview".to_owned())
                .track_focus(&self.row_focus[&self.row_ids[index]])
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(self.theme.space(10.))
                .child(icon)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(self.theme.space(3.))
                        .child(
                            theme::Title::new(theme::TitleLevel::Item, title)
                                .truncate()
                                .render(&self.theme),
                        )
                        .child(
                            theme::Text::new(theme::TextRole::Meta, details)
                                .truncate()
                                .render(&self.theme),
                        ),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(ArtifactAction::Preview(id.clone()));
                })),
            )
            .child(crate::library::test_control(
                "chat-artifact-save-as",
                theme::IconButton::new(
                    ("artifact-save-as", self.row_ids[index]),
                    self.t("library.save_as"),
                    theme::icons::download(),
                )
                .disabled(unavailable)
                .render(
                    &self.theme,
                    cx.listener(move |_, _, _, cx| {
                        cx.emit(ArtifactAction::SaveAs(save_id.clone()));
                    }),
                ),
            ))
            .when_some(source, |card, session_id| {
                let artifact_id = artifact_id.to_owned();
                card.child(
                    theme::IconButton::new(
                        ("artifact-locate-source", self.row_ids[index]),
                        self.t("library.locate_task"),
                        theme::icons::external_link(),
                    )
                    .render(
                        &self.theme,
                        cx.listener(move |_, _, _, cx| {
                            cx.emit(ArtifactAction::LocateSource {
                                session_id: session_id.clone(),
                                artifact_id: artifact_id.clone(),
                            });
                        }),
                    ),
                )
            })
    }

    fn toggle_override_check(
        &mut self,
        key: String,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.override_checked.remove(&key) {
            self.override_checked.insert(key);
        }
        self.list.remeasure_items(0..self.lines.len());
        cx.notify();
    }

    pub(super) fn format_verification_issues(
        &self,
        issues: &[flowy_agent::VerificationIssue],
    ) -> Vec<String> {
        issues
            .iter()
            .map(|issue| {
                format!(
                    "{} · {}",
                    self.t(flowy_agent::verify_ui_key(&issue.code)),
                    issue.field
                )
            })
            .collect()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "verification card fields map 1:1 from ChatLine::Verification"
    )]
    pub(super) fn verification_card(
        &self,
        index: usize,
        source_session_id: Option<&str>,
        report_id: &str,
        artifact_path: &str,
        artifact_hash: &str,
        ok: bool,
        issues: &[flowy_agent::VerificationIssue],
        semantic_hint: Option<&str>,
        override_used: bool,
        provenance_warnings: &[String],
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let row_id = self.row_ids[index];
        let name = artifact_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(artifact_path)
            .to_owned();
        let short_hash: String = artifact_hash.chars().take(8).collect();
        let status_label = if ok {
            self.t("verify.card.passed")
        } else {
            self.t("verify.card.failed")
        };

        let policy = self.theme.motion_policy();

        if ok || override_used {
            let expanded = self.expanded_rows.contains(&row_id);
            let compact_label = if ok {
                self.tf("verify.compact.passed", &[("name", &name)])
            } else {
                self.tf("verify.compact.override", &[("name", &name)])
            };
            let deliverables_for_quick_open = (ok
                && !self.is_path_failed(artifact_path))
            .then(|| self.latest_deliverables())
            .flatten();
            let status_pill = if ok {
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_full()
                    .bg(self.theme.status_verified_bg())
                    .flex()
                    .items_center()
                    .gap(px(4.))
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
                            .child(self.t("verify.card.passed")),
                    )
                    .into_any_element()
            } else {
                let badge = self.t("verify.override.badge");
                div()
                    .id(("verify-override-badge", row_id))
                    .debug_selector(|| "chat-verify-override-badge".to_owned())
                    .role(gpui::Role::Status)
                    .aria_label(badge.clone())
                    .flex_none()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_full()
                    .bg(self.theme.status_attention_bg())
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        theme::icons::circle_alert()
                            .size(px(12.))
                            .text_color(self.theme.status_attention()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.status_attention())
                            .child(badge),
                    )
                    .into_any_element()
            };
            // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
            // `Action::Expand`, which breaks Windows UIA button clicks.
            let mut toggle = theme::interactive(
                ("verify-compact-toggle", row_id),
                compact_label,
                &self.theme,
            )
            .w_full()
            .min_w_0()
            .px(self.theme.space(10.))
            .py(self.theme.space(6.))
            .rounded_xl()
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .child(status_pill)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if ok {
                        self.theme.ink()
                    } else {
                        self.theme.muted()
                    })
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(name),
            );
            if let Some(files) = deliverables_for_quick_open {
                let open_btn = theme::Button::new(
                    ("verify-open-deliverables", row_id),
                    self.t("studio.view_deliverables"),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(OpenDeliverables(files.clone()));
                    }),
                );
                toggle = toggle.child(
                    div()
                        .id(("verify-open-deliverables-wrap", row_id))
                        .debug_selector(|| {
                            "verify-open-deliverables".to_owned()
                        })
                        .child(open_btn),
                );
            }
            toggle = toggle
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(if expanded {
                            theme::icons::chevron_down().size(px(12.))
                        } else {
                            theme::icons::chevron_right().size(px(12.))
                        }),
                )
                .on_click(cx.listener(move |feed, _, _, cx| {
                    feed.toggle_detail(row_id, cx);
                }));

            let mut card = div()
                .id(("verification-card", row_id))
                .debug_selector(|| "chat-verification-card".to_owned())
                .role(if ok {
                    gpui::Role::Status
                } else {
                    gpui::Role::Alert
                })
                .aria_label(status_label)
                .w_full()
                .max_w(px(560.))
                .min_w_0()
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.card())
                .flex()
                .flex_col()
                .child(toggle);

            if let Some(hint) = semantic_hint
                && !hint.trim().is_empty()
            {
                let hint_label =
                    self.tf("verify.semantic.hint", &[("hint", hint)]);
                card = card.child(
                    div()
                        .id(("verify-semantic-hint", row_id))
                        .debug_selector(|| {
                            "chat-verify-semantic-hint".to_owned()
                        })
                        .role(gpui::Role::Status)
                        .aria_label(hint_label.clone())
                        .w_full()
                        .px(self.theme.space(10.))
                        .py(self.theme.space(6.))
                        .border_t_1()
                        .border_color(self.theme.line())
                        .bg(self.theme.status_attention_bg())
                        .text_xs()
                        .text_color(self.theme.ink())
                        .whitespace_normal()
                        .child(hint_label),
                );
            }

            if !provenance_warnings.is_empty() {
                let prov_label = format!(
                    "{}: {}",
                    self.t("verify.provenance_invalid"),
                    provenance_warnings
                        .iter()
                        .map(|w| crate::ports::redact_trace_text(w))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                card = card.child(
                    div()
                        .id(("verify-provenance-warn", row_id))
                        .role(gpui::Role::Status)
                        .aria_label(prov_label.clone())
                        .w_full()
                        .px(self.theme.space(10.))
                        .py(self.theme.space(6.))
                        .border_t_1()
                        .border_color(self.theme.line())
                        .bg(self.theme.status_attention_bg())
                        .text_xs()
                        .text_color(self.theme.status_attention())
                        .whitespace_normal()
                        .child(prov_label),
                );
            }

            if expanded {
                let mut details = div()
                    .w_full()
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .border_t_1()
                    .border_color(self.theme.line())
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(display_path(artifact_path)),
                            )
                            .when(!short_hash.is_empty(), |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px(self.theme.space(6.))
                                        .py(px(1.))
                                        .rounded_md()
                                        .bg(self.theme.soft())
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .child(self.tf(
                                            "verify.card.hash",
                                            &[("hash", &short_hash)],
                                        )),
                                )
                            }),
                    );
                if !issues.is_empty() {
                    let mut list = div().w_full().flex().flex_col().gap(px(4.));
                    for (issue_ix, line_text) in self
                        .format_verification_issues(issues)
                        .into_iter()
                        .enumerate()
                    {
                        list = list.child(
                            div()
                                .id((
                                    "verify-issue",
                                    row_id * 100 + issue_ix as u64,
                                ))
                                .role(gpui::Role::Status)
                                .aria_label(line_text.clone())
                                .text_xs()
                                .text_color(self.theme.ink())
                                .whitespace_normal()
                                .child(line_text),
                        );
                    }
                    details = details.child(list);
                }
                details = details.child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .whitespace_normal()
                        .child(self.t("verify.visual_disclaimer")),
                );
                card = card.child(self.reveal_collapsible(
                    ("verify-detail-reveal", row_id),
                    row_id,
                    details,
                    cx,
                ));
            }

            if override_used && !ok {
                return crate::motion::settled_collapse_reveal(
                    ("verify-settled", row_id),
                    policy,
                    card,
                );
            }
            return card.into_any_element();
        }

        let check_key = format!("{report_id}:{artifact_path}");
        let checked = self.override_checked.contains(&check_key);

        let header = div()
            .w_full()
            .rounded_t_xl()
            .bg(self.theme.danger_bg())
            .border_b_1()
            .border_color(self.theme.line())
            .px(self.theme.space(12.))
            .py(self.theme.space(10.))
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .child(
                theme::icons::circle_alert()
                    .size(px(16.))
                    .text_color(self.theme.danger()),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.danger())
                    .child(status_label.clone()),
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
                    .child(name),
            );

        let mut card = div()
            .id(("verification-card", row_id))
            .debug_selector(|| "chat-verification-card".to_owned())
            .role(gpui::Role::Alert)
            .aria_label(status_label)
            .w_full()
            .max_w(px(560.))
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.danger().opacity(0.65))
            .bg(self.theme.card())
            .flex()
            .flex_col()
            .child(header);

        let mut body = div()
            .w_full()
            .px(self.theme.space(12.))
            .py(self.theme.space(10.))
            .flex()
            .flex_col()
            .gap(self.theme.space(8.));

        if !issues.is_empty() {
            let mut list = div().w_full().flex().flex_col().gap(px(5.));
            for (issue_ix, line_text) in self
                .format_verification_issues(issues)
                .into_iter()
                .enumerate()
            {
                list = list.child(
                    div()
                        .id(("verify-issue", row_id * 100 + issue_ix as u64))
                        .role(gpui::Role::Status)
                        .aria_label(line_text.clone())
                        .flex()
                        .items_start()
                        .gap(px(6.))
                        .child(
                            div()
                                .mt(px(5.))
                                .size(px(5.))
                                .flex_none()
                                .rounded_full()
                                .bg(self.theme.danger()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(self.theme.ink())
                                .whitespace_normal()
                                .child(line_text),
                        ),
                );
            }
            body = body.child(list);
        }

        if let Some(hint) = semantic_hint
            && !hint.trim().is_empty()
        {
            let hint_label = self.tf("verify.semantic.hint", &[("hint", hint)]);
            body = body.child(
                div()
                    .id(("verify-semantic-hint", row_id))
                    .role(gpui::Role::Status)
                    .aria_label(hint_label.clone())
                    .px(self.theme.space(8.))
                    .py(self.theme.space(6.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(hint_label),
            );
        }

        card = card.child(body);

        // §8.3: Never render an ordinary "Send / Deliver" primary button when
        // verification fails. Only an explicit unchecked-by-default checkbox
        // unlocks the override action, leaving a permanent audit mark.
        let confirm_label = self.t("verify.override.confirm");
        let toggle_key = check_key;
        let action_report = report_id.to_owned();
        let action_path = artifact_path.to_owned();
        let checkbox = theme::interactive(
            ("verify-override-check", row_id),
            confirm_label.clone(),
            &self.theme,
        )
        .debug_selector(|| "chat-verify-override-check".to_owned())
        .role(gpui::Role::CheckBox)
        .aria_toggled(if checked {
            gpui::accesskit::Toggled::True
        } else {
            gpui::accesskit::Toggled::False
        })
        .flex()
        .items_center()
        .gap(self.theme.space(8.))
        .cursor_pointer()
        .child(
            div()
                .size(px(16.))
                .rounded(px(4.))
                .border_1()
                .border_color(if checked {
                    self.theme.accent()
                } else {
                    self.theme.border_strong()
                })
                .bg(if checked {
                    self.theme.fill()
                } else {
                    self.theme.canvas()
                })
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |mark| {
                    mark.child(
                        theme::icons::check()
                            .size(px(12.))
                            .text_color(self.theme.on_fill()),
                    )
                }),
        )
        .child(
            div()
                .text_xs()
                .text_color(self.theme.ink())
                .child(confirm_label),
        )
        .on_click(cx.listener(move |feed, _, _, cx| {
            feed.toggle_override_check(toggle_key.clone(), cx);
        }));

        let action_owner = source_session_id.map(str::to_owned);
        let action_btn = theme::Button::new(
            ("verify-override-submit", row_id),
            self.t("verify.override.action"),
        )
        .variant(theme::ControlVariant::Secondary)
        .size(theme::ControlSize::Small)
        .disabled(!checked)
        .render(
            &self.theme,
            cx.listener(move |_, _, _, cx| {
                cx.emit(ArtifactAction::OverrideDeliver {
                    session_id: action_owner.clone(),
                    report_id: action_report.clone(),
                    artifact_path: action_path.clone(),
                });
            }),
        );

        card = card.child(
            div()
                .w_full()
                .rounded_b_xl()
                .bg(self.theme.soft().opacity(0.45))
                .border_t_1()
                .border_color(self.theme.line())
                .px(self.theme.space(12.))
                .py(self.theme.space(8.))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(checkbox)
                .child(crate::library::test_control(
                    "chat-verify-override-submit",
                    action_btn,
                )),
        );

        card.into_any_element()
    }
}
