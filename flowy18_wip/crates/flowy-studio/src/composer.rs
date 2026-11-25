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
    WorkspaceMenu,
    VerifyToggle,
    SkillMenu,
    CloudNotReady,
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
    workspace_label: SharedString,
    workspace_bound: bool,
    workspace_menu_open: bool,
    semantic_verify_enabled: bool,
    skill_label: Option<SharedString>,
    skill_menu_open: bool,
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
            workspace_label: SharedString::default(),
            workspace_bound: false,
            workspace_menu_open: false,
            semantic_verify_enabled: false,
            skill_label: None,
            skill_menu_open: false,
            cloud_ready: false,
            model_menu_open: false,
            running: false,
            _theme_sub: theme_sub,
            _prompt_sub: prompt_sub,
        }
    }

    #[expect(
        clippy::too_many_arguments,
        clippy::fn_params_excessive_bools,
        reason = "pushed snapshot fields avoid exposing StudioPanel to composer"
    )]
    pub(crate) fn sync_state(
        &mut self,
        model: Model,
        menu_open: bool,
        running: bool,
        model_label: String,
        cloud_ready: bool,
        workspace_label: String,
        workspace_bound: bool,
        workspace_menu_open: bool,
        semantic_verify_enabled: bool,
        skill_label: Option<String>,
        skill_menu_open: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let model_label = SharedString::from(model_label);
        let workspace_label = SharedString::from(workspace_label);
        let skill_label = skill_label.map(SharedString::from);
        if self.model != model
            || self.model_menu_open != menu_open
            || self.running != running
            || self.model_label != model_label
            || self.cloud_ready != cloud_ready
            || self.workspace_label != workspace_label
            || self.workspace_bound != workspace_bound
            || self.workspace_menu_open != workspace_menu_open
            || self.semantic_verify_enabled != semantic_verify_enabled
            || self.skill_label != skill_label
            || self.skill_menu_open != skill_menu_open
        {
            self.model = model;
            self.model_menu_open = menu_open;
            self.running = running;
            self.model_label = model_label;
            self.cloud_ready = cloud_ready;
            self.workspace_label = workspace_label;
            self.workspace_bound = workspace_bound;
            self.workspace_menu_open = workspace_menu_open;
            self.semantic_verify_enabled = semantic_verify_enabled;
            self.skill_label = skill_label;
            self.skill_menu_open = skill_menu_open;
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
                    if !draft {
                        return;
                    }
                    if !this.cloud_ready {
                        cx.emit(ComposerEvent::CloudNotReady);
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
                    .flex_1()
                    .min_w_0()
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
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(self.model_chip(cx))
                                    .child(self.workspace_chip(cx))
                                    .child(self.verify_chip(cx))
                                    .child(self.skill_chip(cx)),
                            )
                            .child(div().flex_none().child(
                                self.composer_submit(running, has_draft, cx),
                            )),
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

    fn cockpit_chip(
        &self,
        id: &'static str,
        label: String,
        colors: (gpui::Rgba, gpui::Rgba),
        icon: theme::icons::Icon,
        layout: (Option<f32>, Option<bool>),
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> impl gpui::StatefulInteractiveElement + gpui::Element {
        let (bg, fg) = colors;
        let (max_w, expanded) = layout;
        let label_el = if max_w.is_some() {
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .truncate()
                .child(label.clone())
                .into_any_element()
        } else {
            label.clone().into_any_element()
        };
        theme::interactive(id, label, &self.theme)
            .focus_visible(|style| style.border_color(self.theme.focus_ring()))
            .min_h(self.theme.control_height(theme::ControlSize::Small))
            .when_some(max_w, |chip, width| chip.max_w(px(width)))
            .px(self.theme.space(8.))
            .rounded_full()
            .bg(bg)
            .flex()
            .items_center()
            .gap(px(4.))
            .text_xs()
            .text_color(fg)
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.dock_hover()))
            .when_some(
                expanded,
                gpui::StatefulInteractiveElement::aria_expanded,
            )
            .on_click(on_click)
            .child(icon)
            .child(label_el)
    }

    fn model_chip(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let label = self.composer_model_label();
        let (bg, fg) = if self.cloud_ready {
            (self.theme.soft(), self.theme.ink())
        } else {
            (
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
            )
        };
        theme::anchor_trigger(
            self.cockpit_chip(
                "composer-model",
                label,
                (bg, fg),
                theme::icons::sparkles().size(px(12.)),
                (None, Some(self.model_menu_open)),
                cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::ModelMenu("composer-model-trigger"));
                }),
            ),
            "composer-model-trigger",
            &self.ui,
        )
    }

    fn workspace_chip(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let label = if self.workspace_label.is_empty() {
            self.t("composer.workspace_default")
        } else {
            self.workspace_label.to_string()
        };
        let (bg, fg) = if self.workspace_bound {
            (self.theme.status_active_bg(), self.theme.status_active())
        } else {
            (self.theme.soft(), self.theme.ink())
        };
        theme::anchor_trigger(
            self.cockpit_chip(
                "composer-workspace",
                label,
                (bg, fg),
                theme::icons::folder().size(px(12.)),
                (Some(148.0), Some(self.workspace_menu_open)),
                cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::WorkspaceMenu);
                }),
            ),
            "composer-workspace-trigger",
            &self.ui,
        )
    }

    fn verify_chip(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let (label, bg, fg) = if self.semantic_verify_enabled {
            (
                self.t("composer.verify_l3_on"),
                self.theme.status_verified_bg(),
                self.theme.status_verified(),
            )
        } else {
            (
                self.t("composer.verify_ask"),
                self.theme.soft(),
                self.theme.muted(),
            )
        };
        self.cockpit_chip(
            "composer-verify",
            label,
            (bg, fg),
            theme::icons::check().size(px(11.)),
            (None, None),
            cx.listener(|_, _, _, cx| {
                cx.emit(ComposerEvent::VerifyToggle);
            }),
        )
    }

    fn skill_chip(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let active = self.skill_label.is_some();
        let label = self
            .skill_label
            .as_ref()
            .map_or_else(|| self.t("composer.skill_pick"), ToString::to_string);
        let (bg, fg) = if active {
            (self.theme.status_active_bg(), self.theme.status_active())
        } else {
            (self.theme.soft(), self.theme.muted())
        };
        theme::anchor_trigger(
            self.cockpit_chip(
                "composer-skill",
                label,
                (bg, fg),
                theme::icons::sparkles().size(px(11.)),
                (Some(136.0), Some(self.skill_menu_open)),
                cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::SkillMenu);
                }),
            ),
            "composer-skill-trigger",
            &self.ui,
        )
    }

    fn composer_submit(
        &self,
        running: bool,
        has_draft: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        // Keep the button actionable whenever a draft exists: if cloud routing
        // is not ready, clicking guides the user to the model menu just like
        // pressing Enter instead of silently ignoring clicks.
        let needs_setup = has_draft && !self.cloud_ready;
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
        let btn_bg = if needs_setup {
            self.theme.status_attention()
        } else {
            send_colors.background
        };
        let btn_fg = if needs_setup {
            self.theme.on_ink()
        } else {
            send_colors.foreground
        };
        let btn_hover = if needs_setup {
            self.theme.status_attention().opacity(0.88)
        } else {
            send_colors.hover
        };
        row.child(
            theme::button(
                "chat-send",
                self.t("studio.send"),
                !has_draft,
                &self.theme,
                cx.listener(|this, _, _, cx| {
                    if !this.cloud_ready {
                        cx.emit(ComposerEvent::CloudNotReady);
                        return;
                    }
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
            .bg(btn_bg)
            .text_color(btn_fg)
            .when(!has_draft, |el| el.opacity(0.45))
            .when(has_draft, move |el| {
                el.hover(move |style| style.bg(btn_hover))
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
