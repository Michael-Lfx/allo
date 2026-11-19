use flowy_ui as theme;
use flowy_ui::text_area;
use gpui::{Context, SharedString, Window, div, prelude::*, px};
use std::time::Duration;

impl crate::panel::StudioPanel {
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
            .relative()
            .flex()
            .flex_col();
        let chrome = self.surface != crate::panel::StudioSurface::Automation;
        if chrome {
            column = column.child(self.top_bar(cx));
        }
        if let Some(notice) = self.notice {
            column = column.child(
                div()
                    .px(self.theme.space(32.))
                    .py(self.theme.space(8.))
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(self.t(notice)),
            );
        }
        column = if self.surface == crate::panel::StudioSurface::Home {
            column.child(self.studio(cx))
        } else if self.surface == crate::panel::StudioSurface::Chat {
            column.child(self.chat_canvas(cx))
        } else if self.surface == crate::panel::StudioSurface::Automation {
            column.child(self.automation_canvas(cx))
        } else {
            column.child(self.section_page())
        };
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
                    .aria_disabled(locked)
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

    fn studio(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("studio-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.studio_body(cx)),
            )
            .child(self.composer(cx))
    }

    fn studio_body(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let doc = self.document.document();
        div()
            .w_full()
            .max_w(px(theme::PAGE_MAX_WIDTH))
            .mx_auto()
            .px(self.theme.space(32.))
            .pt(self.theme.space(40.))
            .pb(self.theme.space(24.))
            .flex()
            .flex_col()
            .gap(self.theme.space(24.))
            .child(self.title_row(cx))
            .child(div().text_xs().text_color(self.theme.muted()).child(
                self.tf(
                    "ui.document_nodes",
                    &[
                        ("title", &doc.title),
                        ("count", &doc.nodes.len().to_string()),
                    ],
                ),
            ))
            .child(self.feature_row(cx))
            .child(self.ideas_entry(cx))
    }

    fn title_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(self.theme.space(24.))
            .child(
                div()
                    .font_family("Georgia")
                    .font_weight(gpui::FontWeight::LIGHT)
                    .text_size(px(56.))
                    .child(self.t("studio.brand")),
            )
            .child(
                theme::interactive(
                    "creative-video",
                    self.t("studio.creative_title"),
                    &self.theme,
                )
                .w(px(220.))
                .h(px(92.))
                .rounded_xl()
                .bg(self.theme.ink())
                .text_color(self.theme.on_ink())
                .p(self.theme.space(16.))
                .flex()
                .flex_col()
                .justify_center()
                .gap(self.theme.space(4.))
                .cursor_pointer()
                .hover(|style| style.opacity(0.92))
                .on_click(cx.listener(|app, _event, _window, cx| {
                    app.fill_prompt_key("prompt.creative", cx);
                }))
                .child(div().text_sm().child(self.t("studio.creative_title")))
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.on_ink())
                        .child(self.t("studio.creative_hint")),
                ),
            )
    }

    fn feature_row(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .gap(self.theme.space(16.))
            .child(self.build_card(cx))
            .child(self.create_card(cx))
    }

    fn build_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::interactive("build-card", self.t("studio.build"), &self.theme)
            .flex_1()
            .h(px(156.))
            .rounded_xl()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .p(self.theme.space(20.))
            .flex()
            .flex_col()
            .justify_between()
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.fill_prompt_key("prompt.build", cx);
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(self.t("studio.build")),
            )
            .child(div().text_lg().child(self.t("studio.build_body")))
    }

    fn create_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex_1()
            .h(px(156.))
            .rounded_xl()
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .flex()
            .overflow_hidden()
            .child(
                theme::interactive(
                    "create-card",
                    self.t("studio.create"),
                    &self.theme,
                )
                .flex_1()
                .p(self.theme.space(20.))
                .flex()
                .flex_col()
                .justify_between()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .on_click(cx.listener(|app, _event, _window, cx| {
                    app.fill_prompt_key("prompt.create", cx);
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(self.theme.muted())
                        .child(self.t("studio.create")),
                )
                .child(div().text_lg().child(self.t("studio.create_body"))),
            )
            .child(div().w(px(1.)).h_full().bg(self.theme.line()))
            .child(
                theme::interactive(
                    "local-files",
                    self.t("studio.local_title"),
                    &self.theme,
                )
                .flex_1()
                .p(self.theme.space(20.))
                .flex()
                .flex_col()
                .justify_between()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .on_click(cx.listener(|app, _event, _window, cx| {
                    app.note("notice.local_later", cx);
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(self.theme.muted())
                        .child(self.t("studio.local_title")),
                )
                .child(div().text_sm().child(self.t("studio.local_body"))),
            )
    }

    fn composer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .on_action(cx.listener(
                |panel, _: &crate::SubmitComposer, _window, cx| {
                    // The home surface has its own composer; Enter submits here
                    // too, matching the chat composer's binding.
                    let draft =
                        !panel.prompt.read(cx).as_str().trim().is_empty();
                    if panel.status.is_busy() || !draft || !panel.cloud_ready {
                        return;
                    }
                    panel.send_prompt(cx);
                },
            ))
            .px(self.theme.space(32.))
            .pb(self.theme.space(16.))
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .bg(self.theme.card())
                    .border_1()
                    .border_color(self.theme.line())
                    .rounded_xl()
                    .shadow_sm()
                    .px(self.theme.space(16.))
                    .pt(self.theme.space(12.))
                    .pb(self.theme.space(8.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(8.))
                    .child(self.prompt_field())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                theme::Button::new(
                                    "attach",
                                    self.t("studio.attach"),
                                )
                                .variant(theme::ControlVariant::Ghost)
                                .size(theme::ControlSize::Small)
                                .render(
                                    &self.theme,
                                    cx.listener(|app, _event, _window, cx| {
                                        app.note("notice.attach_later", cx);
                                    }),
                                ),
                            )
                            .child(
                                theme::Button::new(
                                    "send-prompt",
                                    self.t("studio.send"),
                                )
                                .variant(theme::ControlVariant::Primary)
                                .size(theme::ControlSize::Small)
                                .render(
                                    &self.theme,
                                    cx.listener(|app, _event, _window, cx| {
                                        app.send_prompt(cx);
                                    }),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .pt(self.theme.space(8.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t("studio.disclaimer")),
            )
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

    fn prompt_field(&self) -> impl gpui::IntoElement {
        text_area("prompt")
            .aria_label(self.t("studio.prompt_placeholder"))
            .state(self.prompt.downgrade())
            .placeholder(self.t("studio.prompt_placeholder"))
            .placeholder_color(self.theme.muted())
            .caret_color(gpui::rgb_to_hsla(self.theme.ink()))
            .caret_blink_interval_500ms()
            .w_full()
            .h(px(72.))
            .whitespace_normal()
            .overflow_y_scroll()
            .text_color(self.theme.ink())
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
