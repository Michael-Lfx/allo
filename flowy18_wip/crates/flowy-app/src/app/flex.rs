use super::FlowyApp;
use crate::prefs::FlexProvider;
use flowy_agent::{LlmConfig, Model};
use flowy_ui as theme;
use gpui::{ColorExt, Context, SharedString, div, prelude::*, px};
use gpui_ce_elements::editable_text::text_input;

impl FlowyApp {
    pub(super) fn open_flex(&mut self, cx: &mut Context<'_, Self>) {
        self.flex.open = true;
        self.flex.provider_menu = false;
        self.flex.draft_provider = self.prefs.flex_provider;
        self.flex.secret = self.prefs.flex_api_key.clone();
        self.flex.key_visible = false;
        let mask = "*".repeat(self.flex.secret.chars().count());
        self.flex
            .api_key
            .update(cx, |state, cx| state.emplace(&mask, cx));
        self.studio.update(cx, |panel, cx| {
            panel.model_menu_open = false;
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn close_flex(&mut self, cx: &mut Context<'_, Self>) {
        self.flex.open = false;
        self.flex.provider_menu = false;
        cx.notify();
    }

    pub(super) fn flex_layer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("flex-layer")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::ink().opacity(0.18))
            .backdrop_blur(px(14.))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.close_flex(cx);
            }))
            .child(self.flex_dialog(cx))
    }

    fn flex_dialog(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        // Menu is appended last so it paints above later form rows (api key).
        let mut card = div()
            .id("flex-dialog")
            .relative()
            .w(px(440.))
            .rounded(px(20.))
            .bg(theme::card())
            .shadow_lg()
            .px_8()
            .pt_6()
            .pb_6()
            .flex()
            .flex_col()
            .gap_4()
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                if app.flex.provider_menu {
                    app.flex.provider_menu = false;
                    cx.notify();
                }
            }))
            .child(self.flex_header(cx))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t("flex.body")),
            )
            .child(self.flex_provider_block(cx))
            .child(self.flex_key_block(cx))
            .child(self.flex_actions(cx));
        if self.flex.provider_menu {
            card = card.child(self.flex_provider_menu(cx));
        }
        card
    }

    fn flex_header(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(
                div()
                    .id("flex-close")
                    .absolute()
                    .top_0()
                    .right_0()
                    .size(px(28.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .child("×")
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.close_flex(cx);
                    })),
            )
            .child(
                div()
                    .size(px(56.))
                    .rounded_full()
                    .bg(theme::soft())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_2xl()
                    .child("∞"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t("flex.title")),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0()
                            .rounded_full()
                            .bg(theme::line())
                            .text_xs()
                            .text_color(theme::muted())
                            .child(self.t("flex.beta")),
                    ),
            )
    }

    fn flex_provider_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let label = self
            .flex
            .draft_provider
            .map(|provider| self.t(provider.name_key()))
            .unwrap_or_else(|| self.t("flex.provider_placeholder"));
        let muted = self.flex.draft_provider.is_none();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child(self.t("flex.provider")))
            .child(
                div()
                    .id("flex-provider")
                    .w_full()
                    .h(px(40.))
                    .px_3()
                    .rounded_lg()
                    .bg(theme::soft())
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::line()))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.flex.provider_menu = !app.flex.provider_menu;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(if muted {
                                theme::muted()
                            } else {
                                theme::ink()
                            })
                            .child(label),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::muted())
                            .child(if self.flex.provider_menu {
                                "▴"
                            } else {
                                "▾"
                            }),
                    ),
            )
    }

    fn flex_provider_menu(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        // Anchored to the dialog (last paint child), not the provider row.
        let mut menu = div()
            .id("flex-provider-menu")
            .occlude()
            .absolute()
            .left(px(32.))
            .right(px(32.))
            .top(px(260.))
            .p_1()
            .rounded_lg()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap_1();
        for provider in FlexProvider::ALL {
            let selected = self.flex.draft_provider == Some(provider);
            let id = SharedString::from(format!("flex-provider-{}", provider.id()));
            menu = menu.child(
                div()
                    .id(id)
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .bg(if selected {
                        theme::soft()
                    } else {
                        theme::card()
                    })
                    .hover(|row| row.bg(theme::soft()))
                    .child(self.t(provider.name_key()))
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.flex.draft_provider = Some(provider);
                        app.flex.provider_menu = false;
                        cx.notify();
                    })),
            );
        }
        menu.on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
    }

    fn flex_key_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let hidden = !self.flex.key_visible;
        // Field holds real text when visible and `*` when hidden, so the caret
        // tracks the same glyphs the user sees (no overlay width mismatch).
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child(self.t("flex.api_key")))
            .child(
                div()
                    .w_full()
                    .h(px(40.))
                    .px_3()
                    .rounded_lg()
                    .bg(theme::soft())
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        text_input("flex-api-key")
                            .state(self.flex.api_key.downgrade())
                            .placeholder(self.t("flex.api_key_placeholder"))
                            .placeholder_color(theme::muted())
                            .caret_color(gpui::rgb_to_hsla(theme::ink()))
                            .caret_blink_interval_500ms()
                            .flex_1()
                            .min_w_0()
                            .min_h_auto()
                            .whitespace_nowrap()
                            .text_color(theme::ink()),
                    )
                    .child(
                        div()
                            .id("flex-toggle-key")
                            .size(px(24.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::line()))
                            .child(if hidden { "◌" } else { "●" })
                            .on_click(cx.listener(|app, _event, _window, cx| {
                                cx.stop_propagation();
                                app.toggle_flex_key_visible(cx);
                            })),
                    ),
            )
    }

    pub(super) fn touch_flex_key(&mut self, cx: &mut Context<'_, Self>) {
        if self.flex.key_visible {
            self.flex.secret = self.flex.api_key.read(cx).as_str().to_string();
            cx.notify();
            return;
        }
        let shown = self.flex.api_key.read(cx).as_str().to_string();
        let mask = "*".repeat(self.flex.secret.chars().count());
        if shown == mask {
            return;
        }
        Self::apply_masked_edit(&mut self.flex.secret, &shown);
        let next = "*".repeat(self.flex.secret.chars().count());
        if shown != next {
            self.flex
                .api_key
                .update(cx, |state, cx| state.emplace(&next, cx));
        }
        cx.notify();
    }

    fn apply_masked_edit(secret: &mut String, shown: &str) {
        let old_len = secret.chars().count();
        let new_len = shown.chars().count();
        let plain: String = shown.chars().filter(|&ch| ch != '*').collect();
        if plain.is_empty() {
            *secret = secret.chars().take(new_len).collect();
            return;
        }
        if !shown.contains('*') {
            *secret = plain;
            return;
        }
        let leading = shown.chars().take_while(|&ch| ch == '*').count();
        let trailing = shown.chars().rev().take_while(|&ch| ch == '*').count();
        let prefix: String = secret.chars().take(leading).collect();
        let suffix = if trailing == 0 || leading + trailing > old_len {
            String::new()
        } else {
            secret
                .chars()
                .skip(old_len.saturating_sub(trailing))
                .collect()
        };
        *secret = prefix + &plain + &suffix;
    }

    fn toggle_flex_key_visible(&mut self, cx: &mut Context<'_, Self>) {
        if self.flex.key_visible {
            self.flex.secret = self.flex.api_key.read(cx).as_str().to_string();
            let mask = "*".repeat(self.flex.secret.chars().count());
            self.flex
                .api_key
                .update(cx, |state, cx| state.emplace(&mask, cx));
            self.flex.key_visible = false;
        } else {
            let secret = self.flex.secret.clone();
            self.flex
                .api_key
                .update(cx, |state, cx| state.emplace(&secret, cx));
            self.flex.key_visible = true;
        }
        cx.notify();
    }

    fn flex_actions(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .pt_2()
            .flex()
            .gap_3()
            .child(
                div()
                    .id("flex-cancel")
                    .flex_1()
                    .h(px(40.))
                    .rounded_lg()
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::card())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::soft()))
                    .child(self.t("flex.cancel"))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.close_flex(cx);
                    })),
            )
            .child(
                div()
                    .id("flex-connect")
                    .flex_1()
                    .h(px(40.))
                    .rounded_lg()
                    .bg(theme::fill())
                    .text_color(theme::on_fill())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.9))
                    .child(self.t("flex.connect"))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.connect_flex(cx);
                    })),
            )
    }

    fn connect_flex(&mut self, cx: &mut Context<'_, Self>) {
        let Some(provider) = self.flex.draft_provider else {
            self.notice = Some("flex.need_provider");
            cx.notify();
            return;
        };
        if self.flex.key_visible {
            self.flex.secret = self.flex.api_key.read(cx).as_str().to_string();
        }
        let api_key = self.flex.secret.trim().to_string();
        if api_key.is_empty() {
            self.notice = Some("flex.need_key");
            cx.notify();
            return;
        }
        let config = LlmConfig::from_flex(provider, api_key.clone());
        if let Err(key) = self.studio.read(cx).agent.apply_config(config) {
            self.notice = Some(key);
            cx.notify();
            return;
        }
        self.prefs.flex_provider = Some(provider);
        self.prefs.flex_api_key = api_key;
        self.prefs.model = Model::Flex;
        self.prefs.save(self.store.root());
        let plan = self.prefs.plan;
        let version = self.prefs.model_info_version.clone();
        self.studio.update(cx, |panel, cx| {
            panel.sync_model(Model::Flex, plan, version, cx);
        });
        self.notice = None;
        self.close_flex(cx);
    }

    pub(super) fn sync_agent_llm(&self, cx: &Context<'_, Self>) {
        let config = self.prefs.llm_config();
        let _ = self.studio.read(cx).agent.apply_config(config);
    }
}
