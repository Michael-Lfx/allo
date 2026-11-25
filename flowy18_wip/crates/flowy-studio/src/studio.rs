use flowy_ui as theme;

/// How many receipts the strip spells out before collapsing into a count.
const PENDING_STRIP_LIMIT: usize = 3;

/// Last few characters of a receipt id, by **character**, never by byte.
///
/// `String::len()` counts bytes and `&id[i..]` panics when `i` lands inside a
/// multi-byte character. Ids come from a human-readable journal on disk, so a
/// hand-edited one must not be able to take the window down.
fn short_receipt_id(id: &str) -> String {
    let count = id.chars().count();
    id.chars().skip(count.saturating_sub(8)).collect()
}
use gpui::{Context, SharedString, Window, div, prelude::*, px};
use std::time::Duration;

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
            Some(hint) => self
                .tf("chat.pending_other", &[("id", &short_receipt_id(hint))]),
        }
    }

    pub(crate) fn workspace(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
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
            column = column.child(self.top_bar(cx));
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
        // Accepted-but-not-yet-run messages. This strip is the receipt the user
        // was promised: it is re-read from the journal, so it also reappears
        // after a restart instead of dying with the process.
        if !self.pending_receipts.is_empty() {
            let total = self.pending_receipts.len();
            let shown = total.min(PENDING_STRIP_LIMIT);
            let rows = self.pending_receipts[..shown].iter().enumerate().map(
                |(index, receipt)| {
                    let preview: String =
                        receipt.text.chars().take(40).collect();
                    let label = format!(
                        "{} · {} · {}",
                        self.tf(
                            "chat.pending_receipt",
                            &[("id", &short_receipt_id(&receipt.id))],
                        ),
                        self.receipt_owner(receipt),
                        preview
                    );
                    div()
                        .id(("pending-receipt", index))
                        .role(gpui::Role::Status)
                        .aria_label(label.clone())
                        .text_sm()
                        .text_color(self.theme.muted())
                        .child(label)
                },
            );
            // The overflow row keeps the strip a fixed-height hint instead of a
            // wall: nothing drains the journal yet, so the list only grows.
            let overflow = (total > shown).then(|| {
                let label = self.tf(
                    "chat.pending_more",
                    &[("n", &(total - shown).to_string())],
                );
                // Same treatment as the rows above: without `role` and
                // `aria_label` this line renders but never reaches assistive
                // technology, so the count would be silent.
                div()
                    .id("pending-more")
                    .role(gpui::Role::Status)
                    .aria_label(label.clone())
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(label)
            });
            column = column.child(
                div()
                    .id("studio-pending")
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px(self.theme.space(32.))
                    .py(self.theme.space(6.))
                    .children(rows)
                    .children(overflow),
            );
        }
        // The drawer sits beside whichever surface is showing, so it works in
        // the conversation too, not just on the home stage.
        let content = if self.surface == crate::panel::StudioSurface::Home {
            self.studio(window, cx).into_any_element()
        } else if self.surface == crate::panel::StudioSurface::Chat {
            self.chat_canvas(cx).into_any_element()
        } else if self.surface == crate::panel::StudioSurface::Automation {
            self.automation_canvas(cx).into_any_element()
        } else {
            self.section_page().into_any_element()
        };
        column = column.child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(content)
                .when_some(self.deliverables.as_ref(), |row, _| {
                    row.child(self.deliverables_drawer(cx))
                }),
        );
        if self.model_info_open && !self.model_menu_open && chrome {
            column = column.child(theme::popover(
                "model-info",
                "model-menu-trigger",
                self.model_info(cx),
                &self.ui,
                window,
                cx,
                self.i18n.locale().is_rtl(),
            ));
        }
        if self.model_menu_open && chrome {
            column = column.child(theme::popover(
                "model-list",
                self.model_menu_anchor,
                self.model_menu(cx),
                &self.ui,
                window,
                cx,
                self.i18n.locale().is_rtl(),
            ));
        }
        if self.credits_hover
            && !self.model_info_open
            && !self.model_menu_open
            && chrome
        {
            column = column.child(theme::popover(
                "credit-card",
                "credit-pill-trigger",
                self.credit_card(cx),
                &self.ui,
                window,
                cx,
                self.i18n.locale().is_rtl(),
            ));
        }
        column
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

    fn model_button_label(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(self.theme.space(4.))
            .child(self.model_label())
            .child(if self.model_menu_open {
                theme::icons::chevron_up()
            } else {
                theme::icons::chevron_down()
            })
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
                        panel.model_menu_open = false;
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
        self.model_info_open = false;
        cx.notify();
    }

    const fn plan_key(&self) -> &'static str {
        self.plan.label_key()
    }

    fn top_bar(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .h(px(56.))
            .px(self.theme.space(24.))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(self.theme.line())
            .child(theme::anchor_trigger(
                theme::interactive(
                    "model-menu",
                    self.model_label(),
                    &self.theme,
                )
                .px(self.theme.space(12.))
                .py(self.theme.space(4.))
                .rounded_full()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(self.model_button_label())
                .on_click(cx.listener(
                    |app, _event, _window, cx| {
                        app.model_menu_open = !app.model_menu_open
                            || app.model_menu_anchor != "model-menu-trigger";
                        app.model_menu_anchor = "model-menu-trigger";
                        cx.notify();
                    },
                )),
                "model-menu-trigger",
                &self.ui,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(12.))
                    .when(
                        self.surface == crate::panel::StudioSurface::Chat,
                        |row| {
                            row.child(
                                theme::Button::new(
                                    "deliverables",
                                    self.t("chat.deliverables"),
                                )
                                .variant(theme::ControlVariant::Ghost)
                                .size(theme::ControlSize::Small)
                                .render(
                                    &self.theme,
                                    cx.listener(|panel, _, _, cx| {
                                        panel.toggle_deliverables(cx);
                                    }),
                                ),
                            )
                        },
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(self.theme.muted())
                            .child(self.t(self.plan_key())),
                    )
                    .child(
                        theme::Button::new("upgrade", self.t("studio.upgrade"))
                            .variant(theme::ControlVariant::Secondary)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(|app, _event, _window, cx| {
                                    app.note("notice.upgrade_later", cx);
                                }),
                            ),
                    )
                    .child(self.credit_pill(cx)),
            )
    }

    fn set_credit_hover(&mut self, hovered: bool, cx: &mut Context<'_, Self>) {
        if hovered {
            self.credits_close = None;
            if !self.credits_hover {
                self.credits_hover = true;
                cx.notify();
            }
            return;
        }
        if !self.credits_hover {
            return;
        }
        self.credits_close = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.credits_hover = false;
                cx.notify();
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
                app.credits_hover = !app.credits_hover;
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
                false,
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
        help: bool,
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
                    .child(div().text_sm().child(label))
                    .when(help, |row| row.child(self.help_mark())),
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

    fn help_mark(&self) -> impl IntoElement {
        div()
            .size(px(14.))
            .rounded_full()
            .border_1()
            .border_color(self.theme.line())
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(self.theme.muted())
            .child(
                theme::icons::circle_help()
                    .size(px(14.))
                    .text_color(self.theme.muted()),
            )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_receipt_id_never_splits_a_character() {
        // The generated ids are ASCII, so the ordinary case just trims.
        assert_eq!(short_receipt_id("accepted-1a208c0bc80-0"), "c0bc80-0");
        assert_eq!(short_receipt_id("short"), "short");
        assert_eq!(short_receipt_id(""), "");

        // These are the ones that used to panic. `String::len()` counts bytes,
        // so `&id[len - 8..]` landed inside a character and took the window
        // down. The journal is a human-readable file on disk, so a hand-edited
        // id is enough to reach this.
        assert_eq!(short_receipt_id("中文中文"), "中文中文");
        // Ten characters, so the last eight survive intact.
        assert_eq!(
            short_receipt_id("排队中排队中中文测试"),
            "中排队中中文测试"
        );
        assert_eq!(short_receipt_id("短"), "短");
    }
}
