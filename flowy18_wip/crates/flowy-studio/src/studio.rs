use std::collections::HashMap;

use flowy_ui as theme;
use gpui::{Context, FontWeight, SharedString, Window, div, prelude::*, px};

use crate::panel::ActivePopover;

impl crate::panel::StudioPanel {
    /// Where a queued receipt belongs, in the user's words.
    pub(crate) fn receipt_owner(
        &self,
        receipt: &flowy_agent::AcceptedMessage,
    ) -> String {
        match receipt.session_hint.as_deref() {
            None => self.t("chat.pending_unassigned"),
            Some(hint)
                if Some(hint) == self.agent.active_session_id().as_deref() =>
            {
                self.t("chat.pending_here")
            }
            Some(hint) => self.tf(
                "chat.pending_other",
                &[("id", &crate::ports::short_id(hint))],
            ),
        }
    }

    pub(crate) fn workspace(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let mut column = div()
            .id("workspace")
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(self.theme.canvas())
            .text_color(self.theme.ink())
            .relative()
            .flex()
            .flex_col();
        let chrome = self.surface != crate::panel::StudioSurface::Automation;
        if chrome {
            column = column.child(self.top_bar(window, cx));
        }
        if let Some(notice) = self.notice {
            let message = self.t(notice);
            column = column.child(
                div()
                    .id("studio-notice")
                    .role(gpui::Role::Status)
                    .aria_label(message.clone())
                    .px(self.theme.space(32.))
                    .py(self.theme.space(8.))
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(message),
            );
        }
        let is_board = self.surface == crate::panel::StudioSurface::Board;
        // The drawer sits beside Home/Chat, never on the Global Visual Board
        // (which uses centered modal flowcharts so KPIs never squash).
        let content = if self.surface == crate::panel::StudioSurface::Home {
            self.studio(window, cx).into_any_element()
        } else if self.surface == crate::panel::StudioSurface::Chat {
            self.chat_canvas(cx).into_any_element()
        } else if is_board {
            self.board_canvas(cx)
        } else if self.surface == crate::panel::StudioSurface::Automation {
            self.automation_canvas(cx).into_any_element()
        } else {
            self.section_page().into_any_element()
        };
        let drawer = (!is_board && self.right_drawer_open)
            .then(|| self.right_sheet_drawer(window, cx).into_any_element());
        column = column.child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(content)
                .children(drawer),
        );
        if chrome && !self.has_flow_modal() {
            let rtl = self.i18n.locale().is_rtl();
            match self.popover {
                ActivePopover::None => {}
                ActivePopover::ModelInfo => {
                    column = column.child(theme::popover_above(
                        "model-info",
                        "composer-model-trigger",
                        self.model_info(cx),
                        &self.ui,
                        window,
                        cx,
                        rtl,
                    ));
                }
                ActivePopover::ModelMenu { anchor } => {
                    column = column.child(theme::popover_above(
                        "model-list",
                        anchor,
                        self.model_menu(cx),
                        &self.ui,
                        window,
                        cx,
                        rtl,
                    ));
                }
                ActivePopover::AddMenu => {
                    column = column.child(theme::popover_above(
                        "composer-add-menu",
                        "composer-add-trigger",
                        self.composer_add_menu(cx),
                        &self.ui,
                        window,
                        cx,
                        rtl,
                    ));
                }
                ActivePopover::EnvStatus => {
                    column = column.child(theme::popover(
                        "env-status-popover",
                        "env-status-trigger",
                        self.env_status_popover(cx),
                        &self.ui,
                        window,
                        cx,
                        rtl,
                    ));
                }
                ActivePopover::BgTasks => {
                    column = column.child(theme::popover_above(
                        "bg-tasks-popover",
                        "bg-tasks-trigger",
                        self.bg_tasks_popover(cx),
                        &self.ui,
                        window,
                        cx,
                        rtl,
                    ));
                }
                ActivePopover::Credits => {
                    column = column.child(theme::popover(
                        "credit-card",
                        "credit-pill-trigger",
                        self.credit_card(cx),
                        &self.ui,
                        window,
                        cx,
                        rtl,
                    ));
                }
            }
        }
        column
    }

    fn bg_tasks_popover(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let total = self.background_work_count();
        let sessions = self.agent.list_sessions();
        let titles: HashMap<&str, &str> = sessions
            .iter()
            .map(|session| (session.id.as_str(), session.title.as_str()))
            .collect();
        let running_ids = self.background_running_session_ids();
        let show_anonymous_active = self.surface
            != crate::panel::StudioSurface::Chat
            && running_ids.is_empty()
            && (self.status.is_busy() || self.submission_pending);
        let has_running = !running_ids.is_empty() || show_anonymous_active;

        let mut panel = div()
            .id("bg-tasks-panel")
            .occlude()
            .w(px(360.))
            .max_h(px(340.))
            .overflow_y_scroll()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .rounded_xl()
            .shadow_md()
            .p(self.theme.space(10.))
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .child(
                div()
                    .w_full()
                    .px(self.theme.space(4.))
                    .pb(self.theme.space(4.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(self.t("chat.bg_tasks_title")),
                    )
                    .child(
                        div()
                            .px(self.theme.space(6.))
                            .py(px(1.))
                            .rounded_full()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(total.to_string()),
                    ),
            );

        if show_anonymous_active {
            let status_text = self.t("chat.status_running");
            panel = panel.child(
                div()
                    .id("bg-task-active-status")
                    .role(gpui::Role::Status)
                    .aria_label(status_text.clone())
                    .w_full()
                    .min_w_0()
                    .px(self.theme.space(8.))
                    .py(self.theme.space(6.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.45))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(self.theme.status_active()),
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
                            .child(status_text),
                    )
                    .child(theme::status_pill(
                        self.theme.status_active_bg(),
                        self.theme.status_active(),
                        self.t("chat.bg_task_running"),
                        None,
                    )),
            );
        }

        let visible_receipts: Vec<(
            flowy_agent::AcceptedMessage,
            flowy_agent::ReceiptState,
        )> = self
            .popover_visible_receipts(&running_ids)
            .into_iter()
            .map(|(r, s)| (r.clone(), s))
            .collect();

        for (index, session_id) in running_ids.into_iter().enumerate() {
            let preview = self
                .agent
                .session_preview(&session_id)
                .map(|text| crate::ports::redact_trace_text(&text));
            let raw_title = titles
                .get(session_id.as_str())
                .copied()
                .filter(|title| !title.trim().is_empty())
                .map(crate::ports::redact_trace_text)
                .or_else(|| preview.clone())
                .unwrap_or_else(|| crate::ports::short_id(&session_id));
            let title = raw_title;
            let subtitle =
                preview.filter(|text| text.as_str() != title.as_str());
            let inspect_title = title.clone();
            panel = panel.child(
                theme::interactive(
                    ("bg-task-running", index as u64),
                    title.clone(),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .px(self.theme.space(8.))
                .py(self.theme.space(6.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div()
                        .size(px(6.))
                        .flex_none()
                        .rounded_full()
                        .bg(self.theme.status_active()),
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
                                .child(title),
                        )
                        .when_some(subtitle, |col, text| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(text),
                            )
                        }),
                )
                .child(theme::status_pill(
                    self.theme.status_active_bg(),
                    self.theme.status_active(),
                    self.t("chat.bg_task_running"),
                    None,
                ))
                .child(
                    theme::icons::chevron_right()
                        .size(px(12.))
                        .text_color(self.theme.muted()),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open_work_inspector(
                            crate::deliverables::InspectedWork::Session {
                                id: session_id.clone(),
                                title: inspect_title.clone(),
                            },
                            cx,
                        );
                    },
                )),
            );
        }

        let has_visible_receipts = !visible_receipts.is_empty();
        for (index, (receipt, state)) in
            visible_receipts.into_iter().enumerate()
        {
            let safe_text = crate::ports::redact_trace_text(&receipt.text);
            let preview: String = safe_text.chars().take(40).collect();
            let owner = self.receipt_owner(&receipt);
            let receipt_tag = self.tf(
                "chat.pending_receipt",
                &[("id", &crate::ports::short_id(&receipt.id))],
            );
            let label =
                format!("{receipt_tag} \u{00b7} {owner} \u{00b7} {preview}");
            let meta = format!("{receipt_tag} \u{00b7} {owner}");
            let heading = if preview.trim().is_empty() {
                receipt_tag
            } else {
                preview
            };
            let (state_fg, state_label, _) = self.receipt_state_visual(state);
            let inspect_receipt = receipt;
            panel = panel.child(
                theme::interactive(
                    ("pending-receipt", index as u64),
                    label,
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .px(self.theme.space(8.))
                .py(self.theme.space(6.))
                .rounded_lg()
                .bg(self.theme.soft().opacity(0.45))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
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
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(heading),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(meta),
                        ),
                )
                .child(theme::status_pill(
                    self.theme.soft(),
                    state_fg,
                    state_label,
                    None,
                ))
                .child(
                    theme::icons::chevron_right()
                        .size(px(12.))
                        .text_color(self.theme.muted()),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open_work_inspector(
                            crate::deliverables::InspectedWork::Receipt(
                                inspect_receipt.clone(),
                            ),
                            cx,
                        );
                    },
                )),
            );
        }

        if !has_running && !has_visible_receipts {
            panel = panel.child(
                div()
                    .py(self.theme.space(12.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .text_center()
                    .child(self.t("chat.bg_tasks_empty")),
            );
        }

        panel
    }

    fn model_info(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("model-info")
            .occlude()
            .w(px(320.))
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .rounded_xl()
            .shadow_md()
            .p(self.theme.space(16.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(self.theme.space(8.))
                    .child(theme::icons::sparkles())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .whitespace_normal()
                            .child(self.t("model.title")),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("model.body")),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(self.theme.space(8.))
                    .child(self.model_got_it(cx))
                    .child(self.model_upgrade(cx)),
            )
    }

    fn model_got_it(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::Button::new("model-got-it", self.t("model.got_it"))
            .variant(theme::ControlVariant::Secondary)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    app.dismiss_model_info(cx);
                }),
            )
    }

    fn model_upgrade(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::Button::new("model-upgrade-now", self.t("model.upgrade"))
            .variant(theme::ControlVariant::Primary)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    app.dismiss_model_info(cx);
                    app.note("notice.upgrade_later", cx);
                }),
            )
    }

    fn model_menu(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut menu = theme::menu(
            "model-list-content",
            self.t("model.menu_title"),
            &self.theme,
            &self.ui,
            cx,
        )
        .occlude()
        .w(px(400.))
        .bg(self.theme.card())
        .border_1()
        .border_color(self.theme.line())
        .rounded_xl()
        .shadow_md()
        .p(self.theme.space(8.))
        .flex()
        .flex_col()
        .gap(self.theme.space(4.))
        .max_h(px(420.))
        .overflow_y_scroll();
        for row in &self.tiers {
            let tier = row.tier;
            let selected = row.selected;
            let locked = row.locked || row.unavailable;
            let pro_badge = row.pro_badge;
            let unavailable = row.unavailable;
            // Labels live in the panel's i18n, so `sync_i18n` refreshes the
            // menu immediately after a locale switch.
            let label = self.t(tier.name_key());
            let detail = if unavailable {
                self.t("model.unavailable")
            } else {
                self.t(tier.detail_key())
            };
            let title = div()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if locked {
                            self.theme.muted()
                        } else {
                            self.theme.ink()
                        })
                        .child(label.clone()),
                )
                .when(pro_badge, |title| {
                    title.child(
                        div()
                            .px(self.theme.space(6.))
                            .py(px(1.))
                            .rounded_md()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(self.t("model.pro")),
                    )
                });
            let title_row = div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(title)
                .when(selected, |row| row.child(theme::icons::check()));
            let row_id = SharedString::from(format!("tier-{}", tier_key(tier)));
            menu = menu.child(
                theme::interactive(row_id, label, &self.theme)
                    .role(gpui::Role::MenuItemRadio)
                    .aria_selected(selected)
                    .w_full()
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .rounded_lg()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(!locked, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(self.theme.soft()))
                    })
                    .child(title_row)
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(detail),
                    )
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        if panel.agent.turn_status().is_busy() {
                            return;
                        }
                        cx.emit(crate::panel::StudioEvent::TierSelected(tier));
                        panel.close_all_popovers();
                        cx.notify();
                    })),
            );
        }
        // The embedded font has no "↻" glyph, so draw the refresh icon as an
        // SVG instead of relying on a Unicode character.
        menu = menu.child(
            theme::menu_item(
                "refresh-cloud-models",
                self.t("model.refresh"),
                self.catalog_loading,
                &self.theme,
                cx.listener(|_, _, _, cx| {
                    cx.emit(crate::panel::StudioEvent::RefreshModels);
                }),
            )
            .flex()
            .items_center()
            .gap(self.theme.space(6.))
            .child(theme::icons::refresh_cw()),
        );
        menu
    }

    fn dismiss_model_info(&mut self, cx: &mut Context<'_, Self>) {
        self.model_info_version = env!("CARGO_PKG_VERSION").to_string();
        self.emit_model(cx);
        self.close_all_popovers();
        cx.notify();
    }

    const fn plan_key(&self) -> &'static str {
        self.plan.label_key()
    }

    fn active_session_title(&self) -> Option<String> {
        let title = self.agent.session_title();
        let trimmed = title.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    fn space_breadcrumb(&self) -> impl IntoElement {
        if self.surface == crate::panel::StudioSurface::Board {
            return div()
                .id("studio-space-breadcrumb")
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(self.theme.ink())
                        .whitespace_nowrap()
                        .child(self.t("board.title")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(format!(
                            "\u{00b7} {}",
                            self.t("board.subtitle")
                        )),
                );
        }
        let active_project = self.active_project_id.as_deref().and_then(|id| {
            self.projects.iter().find(|project| project.id == id)
        });
        if let Some(project) = active_project {
            let chat_title = (self.surface
                == crate::panel::StudioSurface::Chat)
                .then(|| self.active_session_title())
                .flatten();
            return div()
                .id("studio-space-breadcrumb")
                .flex()
                .items_center()
                .gap(self.theme.space(6.))
                .min_w_0()
                .child(
                    theme::icons::folder()
                        .size(px(14.))
                        .text_color(self.theme.muted()),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(project.name.clone()),
                )
                .when_some(chat_title, |row, title| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(format!("\u{00b7} {title}")),
                    )
                });
        }
        if self.surface == crate::panel::StudioSurface::Chat {
            let title = self
                .active_session_title()
                .unwrap_or_else(|| self.t("dock.chief_assistant"));
            return div()
                .id("studio-space-breadcrumb")
                .flex()
                .items_center()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(title),
                );
        }
        // Blank Home stays calm and chrome-free.
        div().id("studio-space-breadcrumb")
    }

    fn top_bar(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let in_chat = self.surface == crate::panel::StudioSurface::Chat;
        let is_board = self.surface == crate::panel::StudioSurface::Board;
        let is_blank_home = self.surface == crate::panel::StudioSurface::Home
            && self.active_project_id.is_none();
        let can_return_to_chat = self.surface
            == crate::panel::StudioSurface::Home
            && self.agent.has_messages();
        let deliverable_count = self.deliverable_count(cx);
        let deliverables_label = if deliverable_count > 0 {
            let count = deliverable_count.to_string();
            self.tf("studio.deliverables_count", &[("count", count.as_str())])
        } else {
            self.t("chat.deliverables")
        };
        let drawer_open = self.right_drawer_open;
        let env_needs_attention =
            !matches!(self.local_env.overall, crate::ports::EnvSeverity::Ready);
        let rtl = self.i18n.locale().is_rtl();

        let deliverables_trigger =
            (!is_board && (in_chat || deliverable_count > 0)).then(|| {
                let button = theme::interactive(
                    "deliverables",
                    deliverables_label.clone(),
                    &self.theme,
                )
                .h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(8.))
                .rounded_lg()
                .border_1()
                .border_color(if drawer_open {
                    self.theme.border_strong()
                } else {
                    self.theme.canvas()
                })
                .bg(if drawer_open {
                    self.theme.soft()
                } else {
                    self.theme.canvas()
                })
                .flex()
                .items_center()
                .gap(px(5.))
                .text_color(if drawer_open {
                    self.theme.ink()
                } else {
                    self.theme.muted()
                })
                .cursor_pointer()
                .hover(|style| {
                    style.bg(self.theme.soft()).text_color(self.theme.ink())
                })
                .child(theme::icons::panel_right().size(px(15.)))
                .when(deliverable_count > 0, |btn| {
                    btn.child(
                        div()
                            .px(px(5.))
                            .py(px(1.))
                            .rounded_full()
                            .bg(self.theme.soft())
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.ink())
                            .child(deliverable_count.to_string()),
                    )
                })
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.toggle_deliverables(cx);
                }));
                theme::tooltip_trigger(
                    "deliverables-tip",
                    button,
                    deliverables_label,
                    &self.ui,
                    window,
                    cx,
                    rtl,
                )
            });

        div()
            .h(px(48.))
            .px(self.theme.space(20.))
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(12.))
            .when(!is_blank_home, |bar| {
                bar.border_b_1().border_color(self.theme.line())
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(self.space_breadcrumb()),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .when(can_return_to_chat, |row| {
                        row.child(
                            theme::Button::new(
                                "return-to-chat",
                                self.t("studio.return_to_chat"),
                            )
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(|panel, _, _, cx| {
                                    panel.set_surface(
                                        crate::panel::StudioSurface::Chat,
                                        cx,
                                    );
                                }),
                            ),
                        )
                    })
                    .children(deliverables_trigger)
                    .when(env_needs_attention, |row| {
                        row.child(self.env_status_pill(cx))
                    })
                    .child(self.credit_pill(cx)),
            )
    }

    fn env_status_pill(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let dot_color = self.theme.status_attention();
        let label = self.t("env.pill_attention");
        theme::anchor_trigger(
            theme::interactive("env-status-pill", label.clone(), &self.theme)
                .px(self.theme.space(10.))
                .py(px(4.))
                .rounded_full()
                .bg(self.theme.status_attention_bg())
                .flex()
                .items_center()
                .gap(px(6.))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(self.theme.status_attention())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.dock_hover()))
                // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
                // `Action::Expand`, so keeping the plain button role stays
                // clickable via Windows UIA.
                .on_click(cx.listener(|panel, _, _, cx| {
                    let opening = panel.popover != ActivePopover::EnvStatus;
                    panel.toggle_popover(ActivePopover::EnvStatus);
                    if opening {
                        panel.refresh_local_env();
                    }
                    cx.notify();
                }))
                .child(div().size(px(6.)).rounded_full().bg(dot_color))
                .child(label),
            "env-status-trigger",
            &self.ui,
        )
    }

    fn set_credit_hover(&mut self, hovered: bool, cx: &mut Context<'_, Self>) {
        if hovered {
            self.credits_close = None;
            // Hovering the credits pill opens the summary only when no other
            // click-triggered popover is open, so grazing the top bar never
            // dismisses an active Model or Env popover.
            if self.popover == ActivePopover::None {
                self.popover = ActivePopover::Credits;
                cx.notify();
            }
            return;
        }
        if self.popover != ActivePopover::Credits {
            return;
        }
        self.credits_close = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor().timer(crate::motion::FAST).await;
            let _ = weak.update(cx, |app, cx| {
                if app.popover == ActivePopover::Credits {
                    app.popover = ActivePopover::None;
                    cx.notify();
                }
            });
        }));
    }

    fn credit_pill(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::anchor_trigger(
            theme::interactive(
                "credit-pill",
                self.credits_label(),
                &self.theme,
            )
            .px(self.theme.space(12.))
            .py(self.theme.space(4.))
            .rounded_full()
            .bg(self.theme.soft())
            .flex()
            .items_center()
            .gap(self.theme.space(4.))
            .cursor_pointer()
            .hover(|pill| pill.bg(self.theme.line()))
            .on_click(cx.listener(|app, _, _, cx| {
                app.toggle_popover(ActivePopover::Credits);
                cx.notify();
            }))
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                app.set_credit_hover(*hovered, cx);
            }))
            .child(theme::icons::sparkles())
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.credits_label()),
            ),
            "credit-pill-trigger",
            &self.ui,
        )
    }

    fn credit_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let plan = self.plan_key();
        let mut card = div()
            .id("credit-card")
            .occlude()
            .w(px(280.))
            .p(self.theme.space(16.))
            .rounded_xl()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .on_hover(cx.listener(|app, hovered: &bool, _, cx| {
                app.set_credit_hover(*hovered, cx);
            }))
            .child(div().text_lg().child(self.t(plan)))
            .child(self.credit_row(
                theme::icons::sparkles(),
                self.t("credits.balance"),
                self.credits_label(),
            ));
        if let crate::CreditsViewState::Ready { free, daily, .. } =
            &self.credits
        {
            if let Some(free) = free {
                card = card.child(self.credit_sub(
                    self.t("credits.free"),
                    Some(crate::credits::grouped(*free)),
                ));
            }
            if let Some(daily) = daily {
                card = card.child(self.credit_sub(
                    self.t("credits.daily"),
                    Some(crate::credits::grouped(*daily)),
                ));
            }
        }
        card
    }

    fn section_page(&self) -> impl IntoElement {
        div().flex_1().flex().items_center().justify_center().child(
            div()
                .w(px(460.))
                .bg(self.theme.card())
                .border_1()
                .border_color(self.theme.line())
                .rounded_xl()
                .p(self.theme.space(32.))
                .flex()
                .flex_col()
                .gap(self.theme.space(12.))
                .child(div().text_2xl().child(self.placeholder_title()))
                .child(
                    div()
                        .text_sm()
                        .text_color(self.theme.muted())
                        .child(self.placeholder_detail()),
                ),
        )
    }

    fn credit_row(
        &self,
        mark: theme::icons::Icon,
        label: String,
        value: String,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(mark)
                    .child(div().text_sm().child(label)),
            )
            .child(div().text_sm().child(value))
    }

    fn credit_sub(
        &self,
        label: String,
        value: Option<String>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(22.))
            .child(div().text_xs().text_color(self.theme.muted()).child(label))
            .children(value.map(|text| div().text_xs().child(text)))
    }
}

const fn tier_key(tier: flowy_agent::Model) -> &'static str {
    match tier {
        flowy_agent::Model::Max => "max",
        flowy_agent::Model::Standard => "standard",
        flowy_agent::Model::Lite => "lite",
        flowy_agent::Model::Flex => "flex",
    }
}
