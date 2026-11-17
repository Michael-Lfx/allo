//! Composer keeps its own input/visual updates while the feed stays cached.
use flowy_agent::{Model, TurnStatus};
use flowy_i18n::I18n;
use flowy_ui as theme;
use flowy_ui::text_area;
use gpui::{
    ColorExt, Context, Entity, EventEmitter, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use gpui_ce_elements::editable_text::{EditableTextState, TextChanged};

#[derive(Clone, Copy, Debug)]
pub(super) enum ComposerEvent {
    Send,
    Stop,
    ModelMenu(&'static str),
    Access,
}

pub(super) struct ChatComposer {
    ui: theme::UiContext,
    theme: theme::ResolvedTheme,
    prompt: Entity<EditableTextState>,
    i18n: I18n,
    model: Model,
    model_menu_open: bool,
    running: bool,
    _theme_sub: Subscription,
    _prompt_sub: Subscription,
}

impl EventEmitter<ComposerEvent> for ChatComposer {}

impl ChatComposer {
    pub(crate) fn new(
        ui: theme::UiContext,
        prompt: Entity<EditableTextState>,
        i18n: I18n,
        model: Model,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let theme = ui.snapshot(cx);
        let theme_sub = cx.observe(&ui.theme, |composer, entity, cx| {
            composer.theme = entity.read(cx).resolved();
            cx.notify();
        });
        let prompt_sub =
            cx.subscribe(&prompt, |_, _, _: &TextChanged, cx| cx.notify());
        Self {
            ui,
            theme,
            prompt,
            i18n,
            model,
            model_menu_open: false,
            running: false,
            _theme_sub: theme_sub,
            _prompt_sub: prompt_sub,
        }
    }

    pub(crate) fn sync_state(
        &mut self,
        model: Model,
        menu_open: bool,
        running: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if self.model != model
            || self.model_menu_open != menu_open
            || self.running != running
        {
            self.model = model;
            self.model_menu_open = menu_open;
            self.running = running;
            cx.notify();
        }
    }

    pub(crate) fn sync_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
        cx.notify();
    }
    fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    /// Cursor-style floating composer: dark card, model/access/mode chips, send/stop.
    pub(crate) fn chat_composer(
        &self,
        running: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let has_draft = !self.prompt.read(cx).as_str().trim().is_empty();
        div()
            .id("chat-composer")
            .px(self.theme.space(32.))
            .pb(self.theme.space(20.))
            .pt(self.theme.space(8.))
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(720.))
                    .rounded_xl()
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.composer())
                    .shadow_md()
                    .px(self.theme.space(12.))
                    .pt(self.theme.space(12.))
                    .pb(self.theme.space(8.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(8.))
                    .child(
                        text_area("chat-prompt")
                            .aria_label(self.t("chat.do_anything"))
                            .state(self.prompt.downgrade())
                            .placeholder(SharedString::from(
                                self.t("chat.do_anything"),
                            ))
                            .placeholder_color(self.theme.composer_muted())
                            .caret_color(gpui::rgb_to_hsla(
                                self.theme.on_composer(),
                            ))
                            .caret_blink_interval_500ms()
                            .w_full()
                            .min_h(px(44.))
                            .max_h(px(160.))
                            .whitespace_normal()
                            .overflow_y_scroll()
                            .text_color(self.theme.on_composer()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(4.))
                            .child(self.chat_composer_chip(
                                "composer-model",
                                self.composer_model_label(),
                                true,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-plan",
                                self.composer_plan_label(),
                                false,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-access",
                                self.t("chat.full_access"),
                                false,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-mode",
                                self.t("studio.build"),
                                false,
                                cx,
                            ))
                            .child(div().flex_1())
                            .child(
                                self.composer_submit(running, has_draft, cx),
                            ),
                    ),
            )
    }

    fn composer_model_label(&self) -> String {
        self.t(self.model.name_key())
    }

    fn composer_plan_label(&self) -> String {
        match self.model {
            Model::Max => self.t("chat.plan_max"),
            Model::Standard => self.t("chat.plan_standard"),
            Model::Lite => self.t("chat.plan_lite"),
            Model::Flex => self.t("model.flex"),
        }
    }

    fn chat_composer_chip(
        &self,
        id: &'static str,
        label: String,
        primary: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let control = theme::interactive(id, label.clone(), &self.theme)
            .focus_visible(|style| style.border_color(self.theme.on_composer()))
            .min_h(self.theme.control_height(theme::ControlSize::Small))
            .px(self.theme.space(8.))
            .rounded_md()
            .flex()
            .items_center()
            .gap(self.theme.space(4.))
            .text_xs()
            .text_color(if primary {
                self.theme.on_composer()
            } else {
                self.theme.composer_muted()
            })
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft().opacity(0.35)))
            .when(primary, |el| el.child(theme::icons::sparkles()))
            .aria_expanded(
                self.model_menu_open
                    && (id == "composer-model" || id == "composer-plan"),
            )
            .child(label)
            .on_click(cx.listener(move |_, _, _, cx| {
                if id == "composer-model" || id == "composer-plan" {
                    cx.emit(ComposerEvent::ModelMenu(
                        if id == "composer-plan" {
                            "composer-plan-trigger"
                        } else {
                            "composer-model-trigger"
                        },
                    ));
                } else {
                    cx.emit(ComposerEvent::Access);
                }
            }));
        match id {
            "composer-model" => theme::anchor_trigger(
                control,
                "composer-model-trigger",
                &self.ui,
            ),
            "composer-plan" => theme::anchor_trigger(
                control,
                "composer-plan-trigger",
                &self.ui,
            ),
            _ => control,
        }
    }

    fn composer_submit(
        &self,
        running: bool,
        has_draft: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        if running {
            theme::interactive("chat-stop", self.t("chat.stop"), &self.theme)
                .focus_visible(|style| {
                    style.border_color(self.theme.on_composer())
                })
                .w(px(30.))
                .h(px(30.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(self.theme.soft().opacity(0.55))
                .text_color(self.theme.on_composer())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.danger().opacity(0.35)))
                .child(theme::icons::square())
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::Stop);
                }))
        } else {
            theme::button(
                "chat-send",
                self.t("studio.send"),
                !has_draft,
                &self.theme,
                cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Send)),
            )
            .focus_visible(|style| style.border_color(self.theme.on_composer()))
            .w(px(30.))
            .h(px(30.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if has_draft {
                self.theme.on_composer()
            } else {
                self.theme.soft().opacity(0.45)
            })
            .text_color(if has_draft {
                self.theme.composer()
            } else {
                self.theme.composer_muted()
            })
            .cursor_pointer()
            .child(theme::icons::arrow_right())
        }
    }
}

impl Render for ChatComposer {
    fn render(
        &mut self,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        self.chat_composer(self.running, cx)
    }
}

impl crate::panel::StudioPanel {
    pub(crate) fn format_duration_secs(&self, secs: u64) -> String {
        self.tf("chat.duration_secs", &[("n", &secs.max(1).to_string())])
    }

    pub(crate) fn working_label(&self, status: &TurnStatus) -> String {
        match status {
            TurnStatus::Error { message } => message.clone(),
            TurnStatus::Running => {
                let duration = self.agent.working_elapsed_secs().map_or_else(
                    || self.format_duration_secs(1),
                    |secs| self.format_duration_secs(secs),
                );
                self.tf("chat.working_for", &[("duration", &duration)])
            }
            other => self.t(other.key()),
        }
    }
}
