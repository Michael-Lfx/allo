//! Deliverables drawer: the files one turn produced, with a light preview.
//!
//! Kept apart from `starter` and `feed` so the drawer can be replaced without
//! touching the chat projection or the home stage.

use std::path::Path;
use std::sync::Arc;

use flowy_agent::{ChatLine, FileEntry, ReceiptState};
use flowy_ui as theme;
use gpui::{
    AnimationExt, AnyElement, Context, FontWeight, Image, ImageFormat,
    IntoElement, SharedString, SpringAnimation, Window, div, img, prelude::*,
    px,
};

use crate::panel::StudioPanel;
use flowy_ui::status_pill;

/// Active tab inside the unified right-hand sheet drawer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RightDrawerTab {
    #[default]
    Deliverables,
    Steps,
    Trace,
}

/// Source scope for the files listed inside the Deliverables drawer so
/// background refreshes update live latest results without overwriting a user's
/// pinned historical turn snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliverableScope {
    Latest(Vec<FileEntry>),
    TurnSnapshot(Vec<FileEntry>),
}

impl DeliverableScope {
    #[must_use]
    pub const fn files(&self) -> &[FileEntry] {
        match self {
            Self::Latest(files) | Self::TurnSnapshot(files) => files.as_slice(),
        }
    }
}

/// Truncated parent directory path, keeping the tail legible in a narrow drawer.
fn short_parent_path(path: &str) -> String {
    let cleaned = crate::ports::display_path_str(path);
    let parent = Path::new(&cleaned)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default();
    const MAX: usize = 42;
    let count = parent.chars().count();
    if count <= MAX {
        return parent.to_owned();
    }
    let tail: String = parent.chars().skip(count - (MAX - 1)).collect();
    format!("\u{2026}{tail}")
}

/// Reading more than this on the UI path would stall the frame. Matches the
/// localized `"1MB"` copy in `chat.preview_too_large` across all 23 locales.
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// What the right-hand inspector sheet shows when opened from the background
/// works popover (Plan §3.2 #2: single-work step trace without interrupting the
/// active main conversation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectedWork {
    Session { id: String, title: String },
    Receipt(flowy_agent::AcceptedMessage),
}

/// What the drawer shows for the selected file.
pub enum PreviewBody {
    Text(String),
    Image(Arc<Image>),
    /// Locale key explaining why the file cannot be shown.
    Notice(&'static str),
}

/// Reads one file for preview, classifying it by extension and size.
pub fn load_preview(path: &str) -> PreviewBody {
    use std::io::Read;
    let Some(safe_path) = crate::ports::resolve_safe_local_file(path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    let Ok(file) = std::fs::File::open(&safe_path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    let Ok(meta) = file.metadata() else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    if !meta.is_file()
        || crate::ports::is_link_or_unsafe_reparse(&safe_path, &meta)
    {
        return PreviewBody::Notice("chat.preview_missing");
    }
    if meta.len() > MAX_PREVIEW_BYTES {
        return PreviewBody::Notice("chat.preview_too_large");
    }
    // Bounded read on the opened handle eliminates TOCTOU OOM if the file grows
    // between metadata check and read.
    let mut bytes = Vec::with_capacity(
        usize::try_from(meta.len().min(MAX_PREVIEW_BYTES)).unwrap_or(0),
    );
    if file
        .take(MAX_PREVIEW_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return PreviewBody::Notice("chat.preview_missing");
    }
    if bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return PreviewBody::Notice("chat.preview_too_large");
    }
    let extension = safe_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => PreviewBody::Image(Arc::new(Image::from_bytes(
            ImageFormat::Png,
            bytes,
        ))),
        Some("jpg" | "jpeg") => PreviewBody::Image(Arc::new(
            Image::from_bytes(ImageFormat::Jpeg, bytes),
        )),
        Some("webp") => PreviewBody::Image(Arc::new(Image::from_bytes(
            ImageFormat::Webp,
            bytes,
        ))),
        _ => {
            if bytes.contains(&0) {
                return PreviewBody::Notice("chat.preview_binary");
            }
            PreviewBody::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

impl StudioPanel {
    #[expect(
        clippy::too_many_arguments,
        reason = "shared tooltip-wrapped icon button used across drawer and modal toolbars"
    )]
    pub(crate) fn icon_tooltip_button(
        &self,
        id: &'static str,
        tip_id: &'static str,
        label: String,
        icon: theme::icons::Icon,
        variant: theme::ControlVariant,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<'_, Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let rtl = self.i18n.locale().is_rtl();
        let handler = cx.listener(move |panel, _event, window, cx| {
            on_click(panel, window, cx);
        });
        let button = theme::IconButton::new(id, label.clone(), icon)
            .variant(variant)
            .size(theme::ControlSize::Small)
            .render_interactive(&self.theme, handler);
        theme::tooltip_trigger(tip_id, button, label, &self.ui, window, cx, rtl)
            .into_any_element()
    }

    fn switch_to_deliverables_tab(&mut self, cx: &Context<'_, Self>) {
        self.drawer_tab = RightDrawerTab::Deliverables;
        self.inspected_work = None;
        if self.deliverables.is_none() {
            self.deliverables = self
                .feed
                .read(cx)
                .latest_deliverables()
                .map(DeliverableScope::Latest);
        }
    }

    /// Unified right-hand sheet drawer: calm 2-tab inspector (Files | Activity)
    /// plus a quiet Trace debug toggle and focused Single-Work Inspector.
    pub(crate) fn right_sheet_drawer(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let active_session = self.agent.active_session_id();
        let body = if let Some(work) = &self.inspected_work {
            match self.drawer_tab {
                RightDrawerTab::Trace => match work {
                    InspectedWork::Session { id, .. } => div()
                        .p(self.theme.space(12.))
                        .child(self.modal_trace_panel(id, cx))
                        .into_any_element(),
                    InspectedWork::Receipt(r) => {
                        let sid = r
                            .target_session_id
                            .as_deref()
                            .or(r.session_hint.as_deref())
                            .unwrap_or(
                                crate::ports::UNASSIGNED_QUEUE_SESSION_ID,
                            );
                        div()
                            .p(self.theme.space(12.))
                            .child(self.modal_trace_panel(sid, cx))
                            .into_any_element()
                    }
                },
                _ => self.work_inspector_view(work, cx),
            }
        } else {
            match self.drawer_tab {
                RightDrawerTab::Deliverables => {
                    let files = self
                        .deliverables
                        .as_ref()
                        .map_or(&[][..], DeliverableScope::files);
                    match (&self.preview_file, &self.preview_body) {
                        (Some(path), Some(preview)) => self
                            .preview_view(path, preview, window, cx)
                            .into_any_element(),
                        _ => self.deliverables_list(files, cx),
                    }
                }
                RightDrawerTab::Steps => {
                    if let Some(sid) = &active_session {
                        let child_works = if self.agent.session_role(sid)
                            == flowy_agent::SessionRole::Router
                        {
                            self.agent.child_works(sid)
                        } else {
                            Vec::new()
                        };
                        let title = self.agent.session_title();
                        let inspector = self.work_inspector_view(
                            &InspectedWork::Session {
                                id: sid.clone(),
                                title,
                            },
                            cx,
                        );
                        if child_works.is_empty() {
                            inspector
                        } else {
                            let mut col = div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(self.theme.space(8.));
                            let mut works_card = div()
                                .w_full()
                                .px(self.theme.space(12.))
                                .pt(self.theme.space(12.))
                                .flex()
                                .flex_col()
                                .gap(self.theme.space(6.))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(self.theme.muted())
                                        .child(
                                            self.t("chat.bg_tasks_this_chat"),
                                        ),
                                );
                            for (ix, work) in child_works.iter().enumerate() {
                                works_card = works_card.child(
                                    self.render_child_work_row(
                                        ("drawer-child-work", ix as u64),
                                        work,
                                        None,
                                        cx,
                                    ),
                                );
                            }
                            col = col.child(works_card).child(inspector);
                            col.into_any_element()
                        }
                    } else {
                        div()
                            .p(self.theme.space(12.))
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(self.t("drawer.work_no_steps"))
                            .into_any_element()
                    }
                }
                RightDrawerTab::Trace => {
                    if let Some(sid) = &active_session {
                        div()
                            .p(self.theme.space(12.))
                            .child(self.modal_trace_panel(sid, cx))
                            .into_any_element()
                    } else {
                        div()
                            .p(self.theme.space(12.))
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(self.t("trace.empty"))
                            .into_any_element()
                    }
                }
            }
        };

        let trace_active = self.drawer_tab == RightDrawerTab::Trace;
        let trace_toggle = self.icon_tooltip_button(
            "drawer-tab-trace",
            "drawer-tab-trace-tip",
            self.t("drawer.toggle_trace"),
            theme::icons::sliders_horizontal().size(px(14.)),
            if trace_active {
                theme::ControlVariant::Secondary
            } else {
                theme::ControlVariant::Ghost
            },
            |panel, _, cx| {
                if panel.drawer_tab == RightDrawerTab::Trace {
                    panel.switch_to_deliverables_tab(cx);
                } else {
                    panel.drawer_tab = RightDrawerTab::Trace;
                }
                panel.refresh_trace_cache();
                cx.notify();
            },
            window,
            cx,
        );
        let close_button = self.icon_tooltip_button(
            "deliverables-close",
            "deliverables-close-tip",
            self.t("chat.close"),
            theme::icons::x().size(px(14.)),
            theme::ControlVariant::Ghost,
            |panel, _, cx| {
                panel.close_right_sheet(cx);
            },
            window,
            cx,
        );

        let tab_pill =
            |tab: RightDrawerTab, key: &'static str, id: &'static str| {
                let label = self.t(key);
                let selected = self.drawer_tab == tab;
                theme::interactive(id, label.clone(), &self.theme)
                    .px(self.theme.space(8.))
                    .py(px(3.))
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
                        if matches!(tab, RightDrawerTab::Deliverables) {
                            panel.switch_to_deliverables_tab(cx);
                        } else {
                            panel.drawer_tab = tab;
                        }
                        panel.refresh_trace_cache();
                        cx.notify();
                    }))
            };

        let sheet = div()
            .id("deliverables-drawer")
            .w(px(360.))
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(8.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(10.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .child(
                        div()
                            .p(px(2.))
                            .rounded_lg()
                            .bg(self.theme.soft())
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(tab_pill(
                                RightDrawerTab::Deliverables,
                                "chat.deliverables",
                                "drawer-tab-deliverables",
                            ))
                            .child(tab_pill(
                                RightDrawerTab::Steps,
                                "drawer.tab_steps",
                                "drawer-tab-steps",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(trace_toggle)
                            .child(close_button),
                    ),
            )
            .child(
                div()
                    .id("deliverables-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            );

        let scale = crate::motion::travel_scale(self.theme.motion_policy());
        if scale <= 0.0 {
            sheet.into_any_element()
        } else {
            sheet
                .with_spring(
                    // The drawer springs in crisply from the right edge.
                    "deliverables-drawer",
                    SpringAnimation::new(crate::motion::spring_sheet())
                        .to(1.0_f32)
                        .from(0.0_f32),
                    move |drawer, phase| {
                        drawer
                            .opacity(phase.clamp(0.0, 1.0))
                            .relative()
                            .left(px(32.0 * scale * (1.0 - phase)))
                    },
                )
                .into_any_element()
        }
    }

    fn session_steps(&self, session_id: &str) -> Vec<String> {
        let lines = self.agent.session_lines(session_id);
        let last_user = lines
            .iter()
            .rposition(|line| matches!(line, ChatLine::User { .. }))
            .unwrap_or(0);
        let mut steps = Vec::new();
        for line in &lines[last_user..] {
            match line {
                ChatLine::ToolCall {
                    name,
                    target,
                    running,
                    ..
                } => {
                    let prefix =
                        if *running { "\u{25cf} " } else { "\u{2713} " };
                    let heading = if *running {
                        self.tf("chat.tool_live", &[("name", name)])
                    } else {
                        name.clone()
                    };
                    if target.trim().is_empty() {
                        steps.push(format!("{prefix}{heading}"));
                    } else {
                        let safe_target =
                            crate::ports::redact_trace_text(target);
                        let short: String =
                            safe_target.chars().take(48).collect();
                        steps.push(format!(
                            "{prefix}{heading} \u{00b7} {short}"
                        ));
                    }
                }
                ChatLine::Reasoning {
                    duration_secs,
                    live,
                    ..
                } => {
                    steps.push(if *live {
                        format!("\u{25cf} {}", self.t("chat.reasoning_live"))
                    } else {
                        let dur = self.format_duration_secs(*duration_secs);
                        format!(
                            "\u{2713} {}",
                            self.tf("chat.thought_for", &[("duration", &dur)])
                        )
                    });
                }
                ChatLine::Verification {
                    artifact_path,
                    ok,
                    override_used,
                    ..
                } => {
                    let name = crate::ports::basename(artifact_path);
                    let status = if *ok {
                        self.t("verify.card.passed")
                    } else if *override_used {
                        self.t("verify.override.badge")
                    } else {
                        self.t("verify.card.failed")
                    };
                    steps.push(format!("{status} \u{00b7} {name}"));
                }
                ChatLine::ToolResult { text, is_error } if *is_error => {
                    let safe_err = crate::ports::redact_trace_text(text);
                    let short: String = safe_err.chars().take(56).collect();
                    steps.push(format!("\u{2717} {short}"));
                }
                _ => {}
            }
        }
        steps
    }

    fn work_inspector_view(
        &self,
        work: &InspectedWork,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        match work {
            InspectedWork::Session { id, title } => {
                self.inspect_session_view(id, title, cx)
            }
            InspectedWork::Receipt(receipt) => {
                self.inspect_receipt_view(receipt, cx)
            }
        }
    }

    fn inspect_session_view(
        &self,
        id: &str,
        title: &str,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let is_running = self.agent.busy_session_ids().iter().any(|s| s == id);
        let needs_attention = self.agent.session_needs_attention(id);
        let steps = self.session_steps(id);
        let work_progress = self
            .board_snapshot
            .sessions
            .iter()
            .find(|group| group.session_id == id)
            .and_then(|group| group.session_contract.as_ref());
        let open_id = id.to_owned();
        let stop_id = id.to_owned();

        let status_badge = if is_running {
            status_pill(
                self.theme.status_active_bg(),
                self.theme.status_active(),
                self.t("chat.bg_task_running"),
                Some(crate::motion::live_dot(
                    "inspector-running-dot",
                    self.theme.status_active(),
                    self.theme.motion_policy(),
                )),
            )
        } else if needs_attention {
            status_pill(
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
                self.t("dock.attention_badge"),
                None,
            )
        } else {
            status_pill(
                self.theme.status_verified_bg(),
                self.theme.status_verified(),
                self.t("verify.card.passed"),
                None,
            )
        };

        let mut steps_col =
            div().w_full().flex().flex_col().gap(self.theme.space(6.));
        if steps.is_empty() {
            steps_col = steps_col.child(
                div()
                    .w_full()
                    .px(self.theme.space(10.))
                    .py(self.theme.space(12.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.45))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("drawer.work_no_steps")),
            );
        } else {
            for (step_ix, step_text) in steps.into_iter().enumerate() {
                steps_col = steps_col.child(
                    div()
                        .id(("work-step", step_ix as u64))
                        .w_full()
                        .px(self.theme.space(10.))
                        .py(self.theme.space(8.))
                        .rounded_lg()
                        .border_1()
                        .border_color(self.theme.line())
                        .bg(self.theme.canvas())
                        .flex()
                        .items_start()
                        .gap(self.theme.space(8.))
                        .child(
                            div()
                                .size(px(18.))
                                .flex_none()
                                .rounded_full()
                                .bg(self.theme.soft())
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(self.theme.muted())
                                .child((step_ix + 1).to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(self.theme.ink())
                                .whitespace_normal()
                                .child(step_text),
                        ),
                );
            }
        }

        div()
            .w_full()
            .p(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .w_full()
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
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(self.theme.ink())
                                    .whitespace_normal()
                                    .child(crate::ports::redact_trace_text(
                                        title,
                                    )),
                            )
                            .child(status_badge),
                    )
                    .when_some(work_progress, |card, progress| {
                        card.child(self.contract_progress_block(progress))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(8.))
                            .child(
                                theme::Button::new(
                                    "work-inspector-open",
                                    self.t("drawer.work_open_session"),
                                )
                                .variant(theme::ControlVariant::Secondary)
                                .size(theme::ControlSize::Small)
                                .trailing_icon(
                                    theme::icons::arrow_up_right()
                                        .size(px(12.)),
                                )
                                .render(
                                    &self.theme,
                                    cx.listener(move |panel, _, _, cx| {
                                        let root_id = panel
                                            .agent
                                            .root_session_id(&open_id);
                                        panel.open_session(&root_id, cx);
                                        panel.close_right_sheet(cx);
                                    }),
                                ),
                            )
                            .when(is_running, |row| {
                                row.child(
                                    theme::Button::new(
                                        "work-inspector-stop",
                                        self.t("drawer.work_stop"),
                                    )
                                    .variant(theme::ControlVariant::Ghost)
                                    .size(theme::ControlSize::Small)
                                    .render(
                                        &self.theme,
                                        cx.listener(move |panel, _, _, cx| {
                                            panel.stop_session_turn(
                                                &stop_id, cx,
                                            );
                                        }),
                                    ),
                                )
                            }),
                    ),
            )
            .child(steps_col)
            .into_any_element()
    }

    fn inspect_receipt_view(
        &self,
        receipt: &flowy_agent::AcceptedMessage,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let owner = self.receipt_owner(receipt);
        let hint = receipt
            .target_session_id
            .clone()
            .or_else(|| receipt.session_hint.clone());
        let state = self
            .visible_receipts
            .iter()
            .find(|(item, _)| item.id == receipt.id)
            .map_or(ReceiptState::Queued, |(_, state)| *state);
        let (state_fg, state_label, _) = self.receipt_state_visual(state);
        let work_progress = self
            .board_snapshot
            .sessions
            .iter()
            .flat_map(|group| group.branches.iter())
            .find(|branch| {
                matches!(
                    &branch.phase,
                    crate::ports::BranchPhase::Queued { receipt_id, .. }
                        if receipt_id == &receipt.id
                )
            })
            .and_then(|branch| branch.work_progress.as_ref());
        let cancel_id = receipt.id.clone();
        let retry_id = receipt.id.clone();
        let can_cancel = matches!(
            state,
            ReceiptState::Queued
                | ReceiptState::Running
                | ReceiptState::Retrying
                | ReceiptState::Quarantined
        );
        let is_dead = state == ReceiptState::Dead;
        let retrying = self.retrying_receipts.contains(receipt.id.as_str());
        div()
            .w_full()
            .p(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .w_full()
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
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(self.theme.muted())
                                    .child(owner),
                            )
                            .child(status_pill(
                                self.theme.soft(),
                                state_fg,
                                state_label,
                                None,
                            )),
                    )
                    .child(
                        div()
                            .w_full()
                            .p(self.theme.space(10.))
                            .rounded_lg()
                            .bg(self.theme.soft().opacity(0.45))
                            .text_xs()
                            .text_color(self.theme.ink())
                            .whitespace_normal()
                            .child(crate::ports::redact_trace_text(
                                &receipt.text,
                            )),
                    )
                    .when_some(work_progress, |card, progress| {
                        card.child(self.contract_progress_block(progress))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(self.theme.space(8.))
                            .when_some(hint, |row, session_id| {
                                row.child(
                                    theme::Button::new(
                                        "receipt-inspector-open",
                                        self.t("drawer.work_open_session"),
                                    )
                                    .variant(theme::ControlVariant::Secondary)
                                    .size(theme::ControlSize::Small)
                                    .trailing_icon(
                                        theme::icons::arrow_up_right()
                                            .size(px(12.)),
                                    )
                                    .render(
                                        &self.theme,
                                        cx.listener(move |panel, _, _, cx| {
                                            panel.open_session(&session_id, cx);
                                            panel.close_right_sheet(cx);
                                        }),
                                    ),
                                )
                            })
                            .when(is_dead, |row| {
                                row.child(
                                    theme::Button::new(
                                        "receipt-inspector-retry",
                                        self.t("board.retry_dead_receipt"),
                                    )
                                    .variant(theme::ControlVariant::Primary)
                                    .size(theme::ControlSize::Small)
                                    .disabled(retrying)
                                    .render(
                                        &self.theme,
                                        cx.listener(move |panel, _, _, cx| {
                                            panel.retry_dead_receipt(
                                                &retry_id, cx,
                                            );
                                        }),
                                    ),
                                )
                            })
                            .when(can_cancel, |row| {
                                row.child(
                                    theme::Button::new(
                                        "receipt-inspector-cancel",
                                        self.t("board.cancel_receipt"),
                                    )
                                    .variant(theme::ControlVariant::Ghost)
                                    .size(theme::ControlSize::Small)
                                    .render(
                                        &self.theme,
                                        cx.listener(move |panel, _, _, cx| {
                                            panel
                                                .cancel_receipt(&cancel_id, cx);
                                        }),
                                    ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    fn deliverables_list(
        &self,
        files: &[FileEntry],
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        if files.is_empty() {
            return div()
                .w_full()
                .px(self.theme.space(16.))
                .py(self.theme.space(32.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(self.theme.space(8.))
                .child(
                    theme::icons::file_text()
                        .size(px(24.))
                        .text_color(self.theme.muted()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .text_center()
                        .child(self.t("deliverables.empty")),
                )
                .into_any_element();
        }
        let mut column = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .p(self.theme.space(10.));
        for (index, file) in files.iter().enumerate() {
            let name = file
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&file.path)
                .to_owned();
            let parent = short_parent_path(&file.path);
            let file_kind = crate::file_kind::from_path(&file.path);
            let icon = file_kind.map_or_else(
                || {
                    theme::icons::file_text()
                        .size(px(18.))
                        .text_color(self.theme.muted())
                        .into_any_element()
                },
                |kind| kind.icon().size(px(20.)).into_any_element(),
            );
            let kind_label =
                self.t(crate::file_kind::kind_label_key(&file.path));
            let path = file.path.clone();
            column = column.child(
                theme::interactive(
                    ("deliverable", index as u64),
                    SharedString::from(name.clone()),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap(self.theme.space(10.))
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.canvas())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div()
                        .flex_none()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon),
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
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(name),
                        )
                        .when(!parent.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(parent),
                            )
                        }),
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
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.set_preview_file(Some(path.clone()), cx);
                    },
                )),
            );
        }
        if self.active_project_id.is_none()
            && let Some(first) = files.first()
            && let Some(parent_dir) =
                crate::ports::resolve_safe_parent_dir(&first.path)
            && parent_dir.parent().is_some()
            && !crate::ports::is_same_or_overlapping_dir(
                &parent_dir,
                &self.store_root,
            )
        {
            let first_path = first.path.clone();
            column = column.child(
                div().w_full().pt(self.theme.space(8.)).child(
                    theme::Button::new(
                        "deliverables-promote-project",
                        self.t("deliverables.promote_project"),
                    )
                    .variant(theme::ControlVariant::Secondary)
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            panel.register_deliverable_folder_as_project(
                                &first_path,
                                cx,
                            );
                        }),
                    ),
                ),
            );
        }
        column.into_any_element()
    }

    fn preview_view(
        &self,
        path: &str,
        preview: &PreviewBody,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let name = crate::ports::basename(path);
        let external_path = path.to_owned();
        let body = match preview {
            PreviewBody::Text(text) => div()
                .p(self.theme.space(12.))
                .text_xs()
                .child(text.clone())
                .into_any_element(),
            PreviewBody::Image(image) => div()
                .p(self.theme.space(12.))
                .child(img(image.clone()).w_full())
                .into_any_element(),
            PreviewBody::Notice(key) => div()
                .p(self.theme.space(12.))
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t(key))
                .into_any_element(),
        };
        let back_button = self.icon_tooltip_button(
            "preview-back",
            "preview-back-tip",
            self.t("chat.back"),
            theme::icons::arrow_left().size(px(14.)),
            theme::ControlVariant::Ghost,
            |panel, _, cx| {
                panel.set_preview_file(None, cx);
            },
            window,
            cx,
        );
        let reveal_path = external_path.clone();
        let reveal_button = self.icon_tooltip_button(
            "preview-reveal-folder",
            "preview-reveal-folder-tip",
            self.t("board.reveal_folder"),
            theme::icons::folder_open().size(px(14.)),
            theme::ControlVariant::Ghost,
            move |panel, _, cx| {
                panel.reveal_file(&reveal_path, cx);
            },
            window,
            cx,
        );
        let open_button =
            flowy_agent::can_open_with_default_app(&external_path).then(|| {
                self.icon_tooltip_button(
                    "preview-open-external",
                    "preview-open-external-tip",
                    self.t("deliverables.open_external"),
                    theme::icons::external_link().size(px(14.)),
                    theme::ControlVariant::Ghost,
                    move |panel, _, cx| {
                        panel.open_file_externally(&external_path, cx);
                    },
                    window,
                    cx,
                )
            });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .child(back_button)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(name),
                    )
                    .child(reveal_button)
                    .children(open_button),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PREVIEW_BYTES, PreviewBody, load_preview};
    use crate::panel::StudioPanel;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("flowy-deliverables-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_missing_file_reports_a_notice() {
        let preview = load_preview("flowy-definitely-absent-98765.txt");
        assert!(matches!(
            preview,
            PreviewBody::Notice("chat.preview_missing")
        ));
    }

    #[test]
    fn binary_content_reports_a_notice() {
        let path = scratch("binary.dat");
        std::fs::write(&path, [0u8, 1, 2, 3]).expect("write");
        assert!(matches!(
            load_preview(path.to_str().expect("utf8")),
            PreviewBody::Notice("chat.preview_binary")
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn text_content_is_returned_as_text() {
        let path = scratch("note.txt");
        std::fs::write(&path, "hello").expect("write");
        match load_preview(path.to_str().expect("utf8")) {
            PreviewBody::Text(text) => assert_eq!(text, "hello"),
            _ => panic!("expected a text preview"),
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_content_reports_a_notice() {
        let path = scratch("big.txt");
        let data = vec![b'a'; (MAX_PREVIEW_BYTES + 1) as usize];
        std::fs::write(&path, &data).expect("write");
        assert!(matches!(
            load_preview(path.to_str().expect("utf8")),
            PreviewBody::Notice("chat.preview_too_large")
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn external_open_gate_blocks_executables_and_missing_files() {
        let safe = scratch("report.pdf");
        let script = scratch("payload.ps1");
        std::fs::write(&safe, b"%PDF-1.4").expect("write safe");
        std::fs::write(&script, b"Write-Host boom").expect("write script");

        assert!(StudioPanel::can_open_file_externally(
            safe.to_str().expect("utf8")
        ));
        assert!(!StudioPanel::can_open_file_externally(
            script.to_str().expect("utf8")
        ));
        assert!(!StudioPanel::can_open_file_externally(
            "flowy-nonexistent-doc.pdf"
        ));

        let _ = std::fs::remove_file(safe);
        let _ = std::fs::remove_file(script);
    }
}
