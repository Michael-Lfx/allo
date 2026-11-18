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

/// Enter pressed while the composer owns focus.
#[derive(Clone, Debug, Default, PartialEq, Eq, gpui::Action)]
#[action(namespace = flowy_studio)]
pub struct SubmitComposer;

#[derive(Clone, Copy, Debug)]
pub enum ComposerEvent {
    Send,
    Stop,
    ModelMenu(&'static str),
    Access,
    InventoryMode,
}

pub struct ChatComposer {
    ui: theme::UiContext,
    theme: theme::ResolvedTheme,
    prompt: Entity<EditableTextState>,
    i18n: I18n,
    model: Model,
    /// Live cloud-catalog label. The catalog lives on `StudioPanel`, so the
    /// selected model name is pushed down while the composer stays cached.
    model_label: SharedString,
    cloud_ready: bool,
    model_menu_open: bool,
    running: bool,
    inventory: bool,
    folder: Option<String>,
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
            model_label: SharedString::default(),
            cloud_ready: false,
            model_menu_open: false,
            running: false,
            inventory: false,
            folder: None,
            _theme_sub: theme_sub,
            _prompt_sub: prompt_sub,
        }
    }

    pub(crate) fn sync_state(
        &mut self,
        model: Model,
        menu_open: bool,
        running: bool,
        model_label: String,
        cloud_ready: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let model_label = SharedString::from(model_label);
        if self.model != model
            || self.model_menu_open != menu_open
            || self.running != running
            || self.model_label != model_label
            || self.cloud_ready != cloud_ready
        {
            self.model = model;
            self.model_menu_open = menu_open;
            self.running = running;
            self.model_label = model_label;
            self.cloud_ready = cloud_ready;
            cx.notify();
        }
    }

    pub(crate) fn sync_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
        cx.notify();
    }

    pub(crate) fn sync_inventory(
        &mut self,
        inventory: bool,
        folder: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.inventory = inventory;
        self.folder = folder;
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
            .on_action(cx.listener(
                move |this, _: &SubmitComposer, _window, cx| {
                    // Enter submits; Shift+Enter keeps inserting newlines via
                    // the editable-text binding. Empty/running drafts swallow
                    // Enter so it cannot leave a stray blank line.
                    let draft =
                        !this.prompt.read(cx).as_str().trim().is_empty();
                    if running || !draft || !this.cloud_ready {
                        return;
                    }
                    cx.emit(ComposerEvent::Send);
                },
            ))
            .px(self.theme.space(32.))
            .pb(self.theme.space(20.))
            .pt(self.theme.space(8.))
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
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
                    .children(self.folder.as_ref().map(|folder| {
                        div()
                            .text_xs()
                            .text_color(self.theme.composer_muted())
                            .child(folder.clone())
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(self.theme.space(4.))
                            .child(self.chat_composer_chip(
                                "composer-model",
                                self.composer_model_label(),
                                true,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-access",
                                self.t("inventory.pick_folder"),
                                false,
                                cx,
                            ))
                            .child(self.chat_composer_chip(
                                "composer-mode",
                                self.t(if self.inventory {
                                    "inventory.mode"
                                } else {
                                    "inventory.auto"
                                }),
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

    pub(crate) fn composer_model_label(&self) -> String {
        if self.model_label.is_empty() {
            self.t(self.model.name_key())
        } else {
            self.model_label.to_string()
        }
    }

    fn chat_composer_chip(
        &self,
        id: &'static str,
        label: String,
        primary: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let interactive = matches!(
            id,
            "composer-model" | "composer-access" | "composer-mode"
        );
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
            .when(primary, |el| el.child(theme::icons::sparkles()))
            .child(label)
            .when(interactive, |el| {
                el.cursor_pointer()
                    .hover(|style| style.bg(self.theme.soft().opacity(0.35)))
            })
            .when(id == "composer-model", |el| {
                el.aria_expanded(self.model_menu_open)
            })
            .when(id == "composer-mode", |el| {
                el.aria_toggled(if self.inventory {
                    gpui::accesskit::Toggled::True
                } else {
                    gpui::accesskit::Toggled::False
                })
            });
        let control = match id {
            "composer-model" => control.on_click(cx.listener(|_, _, _, cx| {
                cx.emit(ComposerEvent::ModelMenu("composer-model-trigger"));
            })),
            "composer-access" => control.on_click(
                cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Access)),
            ),
            "composer-mode" => control.on_click(cx.listener(|_, _, _, cx| {
                cx.emit(ComposerEvent::InventoryMode);
            })),
            _ => control,
        };
        match id {
            "composer-model" => theme::anchor_trigger(
                control,
                "composer-model-trigger",
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
        let composer_theme = theme::ResolvedTheme::resolve(
            theme::ThemeOptions {
                mode: theme::ThemeMode::Dark,
                ..self.theme.options
            },
            true,
        );
        // A missing cloud model means no live routing is configured; keep the
        // send affordance disabled instead of failing after the click.
        let has_draft = has_draft && self.cloud_ready;
        if running {
            theme::IconButton::new(
                "chat-stop",
                self.t("chat.stop"),
                theme::icons::square(),
            )
            .size(theme::ControlSize::Small)
            .variant(theme::ControlVariant::Ghost)
            .render(
                &composer_theme,
                cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::Stop);
                }),
            )
        } else {
            theme::IconButton::new(
                "chat-send",
                self.t("studio.send"),
                theme::icons::arrow_right(),
            )
            .size(theme::ControlSize::Small)
            .variant(theme::ControlVariant::Primary)
            .disabled(!has_draft)
            .render(
                &composer_theme,
                cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::Send);
                }),
            )
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
            TurnStatus::Error { message } => {
                status_error_label(message, &|key: &str| self.t(key))
            }
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

/// Error status stores an i18n key; older indices may still hold a raw message.
/// Never render either one verbatim: translate keys and mask raw text behind a
/// generic, localized error line.
fn status_error_label(message: &str, t: &dyn Fn(&str) -> String) -> String {
    if message.starts_with("notice.") || message.starts_with("chat.") {
        t(message)
    } else {
        t("chat.status_error")
    }
}

#[cfg(test)]
mod tests {
    use super::status_error_label;

    fn fake_t(key: &str) -> String {
        format!("<{key}>")
    }

    #[test]
    fn error_status_translates_keys_and_masks_raw_messages() {
        assert_eq!(
            status_error_label("chat.status_error", &fake_t),
            "<chat.status_error>"
        );
        assert_eq!(
            status_error_label("notice.agent_failed", &fake_t),
            "<notice.agent_failed>"
        );
        assert_eq!(
            status_error_label("model reported an error", &fake_t),
            "<chat.status_error>"
        );
    }
}
