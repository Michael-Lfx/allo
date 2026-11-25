//! Composer keeps its own input/visual updates while the feed stays cached.
use flowy_agent::{Model, TurnStatus};
use flowy_i18n::I18n;
use flowy_ui as theme;
use flowy_ui::text_area;
use gpui::{
    Context, Entity, EventEmitter, Render, SharedString, Subscription, Window,
    div, prelude::*, px,
};
use gpui_kit::base::input::TextareaState;

/// Enter pressed while the composer owns focus.
#[derive(Clone, Debug, Default, PartialEq, Eq, gpui::Action)]
#[action(namespace = flowy_studio)]
pub struct SubmitComposer;

#[derive(Clone, Copy, Debug)]
pub enum ComposerEvent {
    Send,
    Stop,
    ModelMenu(&'static str),
}

pub struct ChatComposer {
    ui: theme::UiContext,
    theme: theme::ResolvedTheme,
    prompt: Entity<TextareaState>,
    i18n: I18n,
    model: Model,
    /// Live cloud-catalog label. The catalog lives on `StudioPanel`, so the
    /// selected model name is pushed down while the composer stays cached.
    model_label: SharedString,
    cloud_ready: bool,
    model_menu_open: bool,
    running: bool,
    _theme_sub: Subscription,
    _prompt_sub: Subscription,
}

impl EventEmitter<ComposerEvent> for ChatComposer {}

impl ChatComposer {
    pub(crate) fn new(
        ui: theme::UiContext,
        prompt: Entity<TextareaState>,
        i18n: I18n,
        model: Model,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let theme = ui.snapshot(cx);
        let theme_sub = cx.observe(&ui.theme, |composer, entity, cx| {
            composer.theme = entity.read(cx).resolved();
            cx.notify();
        });
        let prompt_sub = cx.subscribe(
            &prompt,
            |_, _, _: &gpui_kit::base::input::InputEvent, cx| cx.notify(),
        );
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

    fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    /// Cursor-style floating composer: dark card, model/access/mode chips, send/stop.
    pub(crate) fn chat_composer(
        &self,
        running: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let has_draft = !self.prompt.read(cx).value().trim().is_empty();
        div()
            .id("chat-composer")
            .on_action(cx.listener(
                move |this, _: &SubmitComposer, _window, cx| {
                    // Enter submits; Shift+Enter keeps inserting newlines via
                    // the editable-text binding. Empty/running drafts swallow
                    // Enter so it cannot leave a stray blank line.
                    // gpui-pre reads the draft through `value()`, not
                    // `as_str()`.
                    let draft = !this.prompt.read(cx).value().trim().is_empty();
                    // A running turn no longer blocks Enter: the host accepts
                    // the message into its durable queue instead of rejecting
                    // it, so "accepted" and "answered" are separate events.
                    if !draft || !this.cloud_ready {
                        return;
                    }
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "ui.composer.SubmitComposer",
                        "submit"
                    );
                    cx.emit(ComposerEvent::Send);
                },
            ))
            .px(self.theme.space(32.))
            .pb(self.theme.space(20.))
            .pt(self.theme.space(4.))
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .rounded(px(20.))
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.card())
                    .shadow_sm()
                    .px(self.theme.space(16.))
                    .pt(self.theme.space(12.))
                    .pb(self.theme.space(8.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(8.))
                    .child(
                        // The kit multi-line editor is `h_full` inside, so it
                        // needs a bounded parent or it collapses to one row.
                        div().w_full().min_h(px(44.)).max_h(px(160.)).child(
                            text_area("chat-prompt", &self.prompt)
                                .into_any_element(),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .child(div().flex_1())
                            .child(self.model_chip(cx))
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

    fn model_chip(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let label = self.composer_model_label();
        theme::anchor_trigger(
            theme::interactive("composer-model", label.clone(), &self.theme)
                .focus_visible(|style| {
                    style.border_color(self.theme.focus_ring())
                })
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(10.))
                .rounded_full()
                .flex()
                .items_center()
                .gap(self.theme.space(4.))
                .text_xs()
                .text_color(self.theme.ink())
                .child(theme::icons::sparkles().size(px(12.)))
                .child(label)
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft().opacity(0.45)))
                .aria_expanded(self.model_menu_open)
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::ModelMenu("composer-model-trigger"));
                })),
            "composer-model-trigger",
            &self.ui,
        )
    }

    fn composer_submit(
        &self,
        running: bool,
        has_draft: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        // A missing cloud model means no live routing is configured; keep the
        // send affordance disabled instead of failing after the click.
        let has_draft = has_draft && self.cloud_ready;
        // While a turn is running the stop affordance must stay reachable, and
        // a new message must still be submittable: the host accepts it into its
        // durable queue (`AgentHandle::accept_deferred`) instead of dropping it.
        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(self.theme.space(4.));
        if running {
            row = row.child(
                theme::IconButton::new(
                    "chat-stop",
                    self.t("chat.stop"),
                    theme::icons::square(),
                )
                .size(theme::ControlSize::Small)
                .variant(theme::ControlVariant::Ghost)
                .render(
                    &self.theme,
                    cx.listener(|_, _, _, cx| {
                        cx.emit(ComposerEvent::Stop);
                    }),
                ),
            );
        }
        let send_colors =
            self.theme.control_colors(theme::ControlVariant::Primary);
        let send_size = self.theme.control_height(theme::ControlSize::Small);
        row.child(
            theme::button(
                "chat-send",
                self.t("studio.send"),
                !has_draft,
                &self.theme,
                cx.listener(|_, _, _, cx| {
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "ui.composer.chat-send",
                        "click"
                    );
                    cx.emit(ComposerEvent::Send);
                }),
            )
            .w(send_size)
            .h(send_size)
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(send_colors.background)
            .text_color(send_colors.foreground)
            .when(!has_draft, |el| el.opacity(0.45))
            .when(has_draft, move |el| {
                el.hover(move |style| style.bg(send_colors.hover))
            })
            .child(theme::icons::arrow_right().size(px(14.))),
        )
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
