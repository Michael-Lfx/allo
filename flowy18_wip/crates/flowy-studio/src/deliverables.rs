//! Unified right-hand workbench drawer: Deliverables (verification failure
//! banner, workspace files with inline actions/preview, published artifacts),
//! Steps (turn-grouped timeline & per-item checklist), and quiet Trace log.

use std::path::Path;

use flowy_agent::{FileEntry, ReceiptState};
use flowy_ui as theme;
use gpui::{
    AnimationExt, AnyElement, Context, FontWeight, IntoElement, SharedString,
    SpringAnimation, Window, div, prelude::*, px,
};

use crate::drawer_preview::format_file_size;
use crate::feed::{
    DeliverableItem, DeliverableVerifyBadge, FailedVerificationItem,
};
use crate::library::{ArtifactAvailability, LibraryItem};
use crate::panel::{StudioEvent, StudioPanel};
use flowy_ui::status_pill;

const COMPACT_DRAWER_W: f32 = 360.0;
const WIDE_MIN_W: f32 = 480.0;
const WIDE_MAX_W: f32 = 620.0;
const MIN_CHAT_W: f32 = 420.0;
const MIN_DRAWER_W: f32 = 280.0;

/// Active tab inside the unified right-hand sheet drawer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RightDrawerTab {
    #[default]
    Deliverables,
    Steps,
    Trace,
}

/// Active view scope inside the Workspace Files section of Deliverables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeliverablesViewScope {
    #[default]
    Latest,
    All,
}

/// Source scope for the files listed inside the Deliverables drawer so
/// background refreshes update live latest results without overwriting a user's
/// pinned historical turn snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliverableScope {
    Latest(Vec<FileEntry>),
    TurnSnapshot(Vec<FileEntry>),
}

#[cfg(test)]
impl DeliverableScope {
    #[must_use]
    pub const fn files(&self) -> &[FileEntry] {
        match self {
            Self::Latest(files) | Self::TurnSnapshot(files) => files.as_slice(),
        }
    }
}

/// Computes adaptive right-drawer width `(width_px, can_toggle_wide)` clamped
/// against viewport width and sidebar visibility so the chat canvas stays at
/// least `MIN_CHAT_W` (`420px`) wide.
#[must_use]
pub fn compute_drawer_width(
    viewport_w: f32,
    dock_visible: bool,
    wide: bool,
) -> (f32, bool) {
    if viewport_w <= 0.0 {
        return (if wide { 520.0 } else { COMPACT_DRAWER_W }, true);
    }
    let dock_w = if dock_visible { theme::DOCK_WIDTH } else { 0.0 };
    let available = (viewport_w - dock_w - MIN_CHAT_W).max(MIN_DRAWER_W);
    let can_toggle_wide = available >= WIDE_MIN_W;
    let target = if wide && can_toggle_wide {
        (viewport_w * 0.44)
            .clamp(WIDE_MIN_W, WIDE_MAX_W)
            .min(available)
    } else {
        COMPACT_DRAWER_W.min(available)
    };
    (target, can_toggle_wide)
}

/// Truncated parent directory path, keeping the tail legible in a narrow drawer.
#[must_use]
pub fn short_parent_path(path: &str) -> String {
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

/// What the right-hand inspector sheet shows when opened from the background
/// works popover (Plan §3.2 #2: single-work step trace without interrupting the
/// active main conversation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectedWork {
    Session { id: String, title: String },
    Receipt(flowy_agent::AcceptedMessage),
}

impl StudioPanel {
    #[expect(
        clippy::too_many_arguments,
        reason = "shared tooltip-wrapped icon button used across drawer and modal toolbars"
    )]
    pub(crate) fn icon_tooltip_button(
        &self,
        id: impl Into<gpui::ElementId>,
        tip_id: impl Into<gpui::SharedString>,
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
        self.refresh_drawer_metadata(cx);
    }

    /// Unified right-hand workbench drawer: Deliverables, structured Steps, and
    /// quiet Trace debug toggle.
    pub(crate) fn right_sheet_drawer(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let viewport_w = f32::from(window.viewport_size().width);
        let (drawer_w, can_toggle_wide) = compute_drawer_width(
            viewport_w,
            self.dock_visible,
            self.drawer_wide,
        );
        let active_session = self.agent.active_session_id();
        let deliverable_badge_count = self.deliverable_count(cx);

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
                _ => self.work_inspector_view(work, true, window, cx),
            }
        } else {
            match self.drawer_tab {
                RightDrawerTab::Deliverables => {
                    match (&self.drawer_preview.file, &self.drawer_preview.body)
                    {
                        (Some(path), Some(preview)) => {
                            self.drawer_preview_view(path, preview, window, cx)
                        }
                        _ => self.deliverables_list_view(window, cx),
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
                            false,
                            window,
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

        let width_toggle = can_toggle_wide.then(|| {
            let label = if self.drawer_wide {
                self.t("drawer.width_compact")
            } else {
                self.t("drawer.width_wide")
            };
            self.icon_tooltip_button(
                "drawer-width-toggle",
                "drawer-width-toggle-tip",
                label,
                theme::icons::panel_right().size(px(14.)),
                if self.drawer_wide {
                    theme::ControlVariant::Secondary
                } else {
                    theme::ControlVariant::Ghost
                },
                |panel, _, cx| {
                    panel.toggle_drawer_width(cx);
                },
                window,
                cx,
            )
        });

        let trace_active = self.drawer_tab == RightDrawerTab::Trace;
        let trace_attention =
            !trace_active && self.feed.read(cx).has_trace_attention();
        let trace_label = if trace_attention {
            self.t("drawer.trace_attention_label")
        } else {
            self.t("drawer.toggle_trace")
        };
        let trace_button = self.icon_tooltip_button(
            "drawer-tab-trace",
            "drawer-tab-trace-tip",
            trace_label,
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
        let trace_toggle = div().relative().child(trace_button).when(
            trace_attention,
            |wrap| {
                wrap.child(
                    div()
                        .id("drawer-trace-attention-dot")
                        .size(px(6.))
                        .rounded_full()
                        .bg(self.theme.status_attention())
                        .absolute()
                        .top(px(2.))
                        .right(px(2.)),
                )
            },
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

        let tab_pill = |tab: RightDrawerTab,
                        key: &'static str,
                        id: &'static str,
                        badge: Option<usize>| {
            let label = self.t(key);
            let aria = match badge.filter(|c| *c > 0) {
                Some(count) => format!("{label} ({count})"),
                None => label.clone(),
            };
            let selected = self.drawer_tab == tab;
            theme::interactive(id, SharedString::from(aria), &self.theme)
                .px(self.theme.space(8.))
                .py(px(3.))
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .flex()
                .items_center()
                .gap(px(4.))
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
                .when_some(badge.filter(|c| *c > 0), |el, count| {
                    el.child(
                        div()
                            .px(px(4.))
                            .rounded_full()
                            .bg(self.theme.soft())
                            .text_color(self.theme.muted())
                            .child(count.to_string()),
                    )
                })
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
            .w(px(drawer_w))
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
                                Some(deliverable_badge_count),
                            ))
                            .child(tab_pill(
                                RightDrawerTab::Steps,
                                "drawer.tab_steps",
                                "drawer-tab-steps",
                                None,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .children(width_toggle)
                            .child(trace_toggle)
                            .child(close_button),
                    ),
            )
            .child(
                theme::scroll_area("deliverables-scroll", &self.theme)
                    .a11y_label(self.t("a11y.scroll_deliverables_drawer"))
                    .flex_1()
                    .min_h_0()
                    .child(body),
            );

        let scale = crate::motion::travel_scale(self.theme.motion_policy());
        if scale <= 0.0 {
            sheet.into_any_element()
        } else {
            sheet
                .with_spring(
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

    fn work_inspector_view(
        &self,
        work: &InspectedWork,
        show_open_session: bool,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        match work {
            InspectedWork::Session { id, title } => self.inspect_session_view(
                id,
                title,
                show_open_session,
                window,
                cx,
            ),
            InspectedWork::Receipt(receipt) => {
                self.inspect_receipt_view(receipt, cx)
            }
        }
    }

    fn inspect_session_view(
        &self,
        id: &str,
        title: &str,
        show_open_session: bool,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let is_running = self.agent.busy_session_ids().iter().any(|s| s == id);
        let needs_attention = self.agent.session_needs_attention(id);
        let lines = self.agent.session_lines(id);
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

        let steps_col =
            self.structured_steps_view(id, &lines, is_running, window, cx);

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
                        card.child(self.contract_progress_checklist(progress))
                    })
                    .when(show_open_session || is_running, |card| {
                        card.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(self.theme.space(8.))
                                .when(show_open_session, |row| {
                                    row.child(
                                        theme::Button::new(
                                            "work-inspector-open",
                                            self.t("drawer.work_open_session"),
                                        )
                                        .variant(
                                            theme::ControlVariant::Secondary,
                                        )
                                        .size(theme::ControlSize::Small)
                                        .trailing_icon(
                                            theme::icons::arrow_up_right()
                                                .size(px(12.)),
                                        )
                                        .render(
                                            &self.theme,
                                            cx.listener(
                                                move |panel, _, _, cx| {
                                                    let root_id = panel
                                                        .agent
                                                        .root_session_id(
                                                            &open_id,
                                                        );
                                                    panel.open_session(
                                                        &root_id, cx,
                                                    );
                                                    panel.close_right_sheet(cx);
                                                },
                                            ),
                                        ),
                                    )
                                })
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
                                            cx.listener(
                                                move |panel, _, _, cx| {
                                                    panel.stop_session_turn(
                                                        &stop_id, cx,
                                                    );
                                                },
                                            ),
                                        ),
                                    )
                                }),
                        )
                    }),
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
                        card.child(self.contract_progress_checklist(progress))
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

    fn deliverables_list_view(
        &self,
        _window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let workspace_items = self.visible_workspace_deliverables(cx);
        let feed = self.feed.read(cx);
        let failed_items: Vec<FailedVerificationItem> =
            feed.failed_verifications().to_vec();
        let published_items: Vec<LibraryItem> =
            feed.session_artifacts().to_vec();
        let latest_count =
            feed.latest_deliverable_items().map_or(0, |v| v.len());
        let all_count = feed.all_deliverable_items().map_or(0, |v| v.len());
        let is_snapshot = matches!(
            self.deliverables,
            Some(DeliverableScope::TurnSnapshot(_))
        );

        if workspace_items.is_empty()
            && failed_items.is_empty()
            && published_items.is_empty()
        {
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

        let mut root = div()
            .w_full()
            .p(self.theme.space(10.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.));

        if !failed_items.is_empty() {
            root =
                root.child(self.failed_verifications_banner(&failed_items, cx));
        }

        if !workspace_items.is_empty() || is_snapshot || all_count > 0 {
            root = root.child(self.workspace_files_section(
                &workspace_items,
                latest_count,
                all_count,
                is_snapshot,
                cx,
            ));
        }

        if !published_items.is_empty() {
            root = root
                .child(self.published_artifacts_section(&published_items, cx));
        }

        root.into_any_element()
    }

    fn failed_verifications_banner(
        &self,
        failures: &[FailedVerificationItem],
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let mut banner = div()
            .id("deliverables-failed-banner")
            .w_full()
            .p(self.theme.space(10.))
            .rounded_xl()
            .border_1()
            .border_color(self.theme.danger().opacity(0.45))
            .bg(self.theme.danger().opacity(0.08))
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
                            .flex_none()
                            .size(px(14.))
                            .text_color(self.theme.danger()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.danger())
                            .child(self.t("drawer.section_needs_fix")),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("drawer.needs_fix_hint")),
            );

        for (index, item) in failures.iter().enumerate() {
            let name = crate::ports::basename(&item.artifact_path);
            let fix_summary = item.summary.clone();
            let fix_path = item.artifact_path.clone();
            let locate_report_id = item.report_id.clone();
            let locate_path = item.artifact_path.clone();

            banner = banner.child(
                div()
                    .id(("deliverables-failed-item", index as u64))
                    .w_full()
                    .p(self.theme.space(8.))
                    .rounded_lg()
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.canvas())
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
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
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(item.summary.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .child(
                                theme::Button::new(
                                    SharedString::from(format!(
                                        "deliverables-fail-fix-{index}"
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
                                            Some(fix_path.clone()),
                                            Some(window),
                                            cx,
                                        );
                                    }),
                                ),
                            )
                            .child(
                                theme::Button::new(
                                    SharedString::from(format!(
                                        "deliverables-fail-locate-{index}"
                                    )),
                                    self.t("drawer.locate_card"),
                                )
                                .variant(theme::ControlVariant::Ghost)
                                .size(theme::ControlSize::Small)
                                .render(
                                    &self.theme,
                                    cx.listener(move |panel, _, window, cx| {
                                        panel.locate_verification_card(
                                            &locate_report_id,
                                            &locate_path,
                                            Some(window),
                                            cx,
                                        );
                                    }),
                                ),
                            ),
                    ),
            );
        }

        banner.into_any_element()
    }

    fn workspace_files_section(
        &self,
        items: &[DeliverableItem],
        latest_count: usize,
        all_count: usize,
        is_snapshot: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let show_scope_toggle = all_count > latest_count || is_snapshot;
        let scope_pill = |scope: DeliverablesViewScope,
                          label: String,
                          id: &'static str| {
            let active = !is_snapshot && self.deliverables_view_scope == scope;
            theme::interactive(
                id,
                SharedString::from(label.clone()),
                &self.theme,
            )
            .px(self.theme.space(6.))
            .py(px(2.))
            .rounded_md()
            .cursor_pointer()
            .text_xs()
            .bg(if active {
                self.theme.card()
            } else {
                self.theme.soft()
            })
            .text_color(if active {
                self.theme.ink()
            } else {
                self.theme.muted()
            })
            .child(label)
            .on_click(cx.listener(move |panel, _, _, cx| {
                panel.set_deliverables_view_scope(scope, cx);
            }))
        };

        let mut section = div()
            .w_full()
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
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.muted())
                            .child(self.t("drawer.section_workspace")),
                    )
                    .when(show_scope_toggle, |row| {
                        row.child(
                            div()
                                .p(px(2.))
                                .rounded_md()
                                .bg(self.theme.soft())
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .child(scope_pill(
                                    DeliverablesViewScope::Latest,
                                    self.t("drawer.scope_latest"),
                                    "deliverables-scope-latest",
                                ))
                                .child(scope_pill(
                                    DeliverablesViewScope::All,
                                    self.tf(
                                        "drawer.scope_all",
                                        &[("count", &all_count.to_string())],
                                    ),
                                    "deliverables-scope-all",
                                )),
                        )
                    }),
            );

        for (index, item) in items.iter().enumerate() {
            let file = &item.entry;
            let name = crate::ports::basename(&file.path);
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
            let size_label = self
                .deliverable_sizes
                .get(&crate::ports::canonical_path_key(&file.path))
                .and_then(|opt| *opt)
                .map(format_file_size);
            let meta_line = match (size_label, parent.is_empty()) {
                (Some(size), false) => {
                    format!("{kind_label} · {size} · {parent}")
                }
                (Some(size), true) => format!("{kind_label} · {size}"),
                (None, false) => format!("{kind_label} · {parent}"),
                (None, true) => kind_label,
            };
            let badge_elem = item.badge.map(|badge| match badge {
                DeliverableVerifyBadge::Verified => status_pill(
                    self.theme.status_verified_bg(),
                    self.theme.status_verified(),
                    self.t("verify.card.passed"),
                    None,
                ),
                DeliverableVerifyBadge::Overridden => status_pill(
                    self.theme.status_attention_bg(),
                    self.theme.status_attention(),
                    self.t("verify.override.badge"),
                    None,
                ),
            });

            let path_preview = file.path.clone();
            let path_reveal = file.path.clone();
            let path_ext = file.path.clone();
            let path_quote = file.path.clone();
            let allow_ext = flowy_agent::can_open_with_default_app(&path_ext);

            let reveal_btn = theme::IconButton::new(
                ("deliverable-reveal", index as u64),
                format!("{} · {name}", self.t("board.reveal_folder")),
                theme::icons::folder_open(),
            )
            .variant(theme::ControlVariant::Ghost)
            .size(theme::ControlSize::Small)
            .render_interactive(
                &self.theme,
                cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.reveal_file(&path_reveal, cx);
                }),
            );

            let quote_btn = theme::IconButton::new(
                ("deliverable-quote", index as u64),
                format!("{} · {name}", self.t("drawer.quote_file")),
                theme::icons::quote(),
            )
            .variant(theme::ControlVariant::Ghost)
            .size(theme::ControlSize::Small)
            .render_interactive(
                &self.theme,
                cx.listener(move |panel, _, window, cx| {
                    cx.stop_propagation();
                    panel.quote_file_to_composer(&path_quote, Some(window), cx);
                }),
            );

            let ext_btn = allow_ext.then(|| {
                theme::IconButton::new(
                    ("deliverable-external", index as u64),
                    format!(
                        "{} · {name}",
                        self.t("deliverables.open_external")
                    ),
                    theme::icons::external_link(),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render_interactive(
                    &self.theme,
                    cx.listener(move |panel, _, _, cx| {
                        cx.stop_propagation();
                        panel.open_file_externally(&path_ext, cx);
                    }),
                )
            });

            section = section.child(
                theme::interactive(
                    ("deliverable", index as u64),
                    SharedString::from(name.clone()),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.canvas())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .flex_col()
                .gap(self.theme.space(6.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(self.theme.space(8.))
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
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(meta_line),
                                ),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(self.theme.space(6.))
                        .child(div().flex().items_center().children(badge_elem))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .child(quote_btn)
                                .child(reveal_btn)
                                .children(ext_btn),
                        ),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.set_preview_file(Some(path_preview.clone()), cx);
                    },
                )),
            );
        }

        if self.active_project_id.is_none()
            && let Some(promote_path) = self.promote_project_candidate.clone()
        {
            section = section.child(
                div().w_full().pt(self.theme.space(4.)).child(
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
                                &promote_path,
                                cx,
                            );
                        }),
                    ),
                ),
            );
        }

        section.into_any_element()
    }

    fn published_artifacts_section(
        &self,
        items: &[LibraryItem],
        cx: &Context<'_, Self>,
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
                    .child(self.t("drawer.section_published")),
            );

        for (index, item) in items.iter().enumerate() {
            let id_preview = item.id.clone();
            let id_save = item.id.clone();
            let icon = crate::library::artifact_file_icon(item).map_or_else(
                || {
                    theme::icons::library()
                        .size(px(18.))
                        .text_color(self.theme.muted())
                        .into_any_element()
                },
                |ico| ico.size(px(20.)).into_any_element(),
            );
            let ext = item.extension.to_ascii_uppercase();
            let meta = match (ext.is_empty(), item.size_bytes) {
                (false, Some(bytes)) => {
                    format!("{ext} · {}", format_file_size(bytes))
                }
                (false, None) => ext,
                (true, Some(bytes)) => format_file_size(bytes),
                (true, None) => item.delivered_date.clone(),
            };
            let unavailable =
                item.availability != ArtifactAvailability::Available;

            let save_btn = (!unavailable).then(|| {
                theme::IconButton::new(
                    ("drawer-artifact-save", index as u64),
                    format!("{} · {}", self.t("library.save_as"), item.title),
                    theme::icons::download(),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render_interactive(
                    &self.theme,
                    cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(StudioEvent::SaveArtifact(id_save.clone()));
                    }),
                )
            });

            section = section.child(
                theme::interactive(
                    ("drawer-artifact", index as u64),
                    SharedString::from(item.title.clone()),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.canvas())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
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
                                .child(item.title.clone()),
                        )
                        .when(!meta.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(meta),
                            )
                        }),
                )
                .children(save_btn)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(StudioEvent::PreviewArtifact(id_preview.clone()));
                })),
            );
        }

        section.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::compute_drawer_width;
    use crate::panel::StudioPanel;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("flowy-deliverables-{}-{name}", std::process::id()))
    }

    #[test]
    fn drawer_width_clamps_responsively_across_viewports_and_dock() {
        // 1280px with dock open: 1280 - 248 - 420 = 612px available -> compact 360, wide 563.2
        assert_eq!(compute_drawer_width(1280.0, true, false), (360.0, true));
        let (wide_1280, can_wide_1280) =
            compute_drawer_width(1280.0, true, true);
        assert!(can_wide_1280);
        assert!((wide_1280 - 563.2).abs() < 0.01);

        // 960px with dock open: 960 - 248 - 420 = 292px available -> clamps below WIDE_MIN_W
        let (narrow_open, can_wide_narrow) =
            compute_drawer_width(960.0, true, true);
        assert!(!can_wide_narrow);
        assert!((narrow_open - 292.0).abs() < 0.01);

        // 960px with dock hidden: 960 - 0 - 420 = 540px -> wide toggle unlocked
        let (narrow_dock_hidden, can_wide_hidden) =
            compute_drawer_width(960.0, false, true);
        assert!(can_wide_hidden);
        assert!((narrow_dock_hidden - 480.0).abs() < 0.01);
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
