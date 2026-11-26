//! Composer keeps its own input/visual updates while the feed stays cached.
use std::f32::consts::FRAC_PI_4;

use flowy_agent::{Model, TurnStatus};
use flowy_i18n::I18n;
use flowy_ui as theme;
use flowy_ui::text_area;
use gpui::{
    AnimationExt, Context, Entity, EventEmitter, FontWeight, Render,
    SharedString, SpringAnimation, Subscription, Transformation, Window, div,
    prelude::*, px, radians,
};
use gpui_kit::base::input::TextareaState;

/// Enter pressed while the composer owns focus.
#[derive(Clone, Debug, Default, PartialEq, Eq, gpui::Action)]
#[action(namespace = flowy_studio)]
pub struct SubmitComposer;

const QUOTE_PREFIX: &str = "> \u{275d} ";
const MAX_QUOTE_CHARS: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuoteRole {
    User,
    Assistant,
}

impl QuoteRole {
    pub(crate) const fn protocol_tag(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Assistant => "Flowy",
        }
    }

    fn parse_tag(tag: &str) -> Option<Self> {
        match tag.trim() {
            "User" | "You" => Some(Self::User),
            "Flowy" | "Assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveQuote {
    pub role: QuoteRole,
    pub excerpt: String,
}

pub(crate) fn make_quote_excerpt(raw: &str) -> String {
    let (_, source) = split_quoted_prompt(raw);
    let compact: String =
        source.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let head: String = chars.by_ref().take(MAX_QUOTE_CHARS).collect();
    if chars.next().is_some() {
        let clamped: String = head.chars().take(MAX_QUOTE_CHARS - 1).collect();
        format!("{clamped}\u{2026}")
    } else {
        head
    }
}

pub(crate) fn compose_quoted_prompt(
    quote: Option<&ActiveQuote>,
    prompt: &str,
) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Some(quote) = quote.filter(|q| !q.excerpt.trim().is_empty()) else {
        return prompt.to_owned();
    };
    let excerpt = make_quote_excerpt(&quote.excerpt);
    let header =
        format!("{QUOTE_PREFIX}{}: {excerpt}", quote.role.protocol_tag());
    if trimmed.starts_with("/skill:") {
        let (skill_cmd, rest) = match prompt.trim_start().split_once(' ') {
            Some((head, tail)) => (head, tail.trim_start()),
            None => (trimmed, ""),
        };
        if rest.is_empty() {
            format!("{skill_cmd} {header}")
        } else {
            format!("{skill_cmd} {header}\n\n{rest}")
        }
    } else {
        format!("{header}\n\n{prompt}")
    }
}

pub(crate) fn split_quoted_prompt(text: &str) -> (Option<ActiveQuote>, String) {
    let (skill_prefix, rest) = if let Some(after_slash) =
        text.strip_prefix("/skill:")
        && let Some((cmd, tail)) = after_slash.split_once(' ')
        && !cmd.is_empty()
        && tail.starts_with(QUOTE_PREFIX)
    {
        (Some(format!("/skill:{cmd}")), tail)
    } else {
        (None, text)
    };
    let Some(after_marker) = rest.strip_prefix(QUOTE_PREFIX) else {
        return (None, text.to_owned());
    };
    let (header_line, body) =
        if let Some((h, b)) = after_marker.split_once("\n\n") {
            (h, b)
        } else if skill_prefix.is_some() && !after_marker.contains('\n') {
            (after_marker, "")
        } else {
            return (None, text.to_owned());
        };
    let Some((role_raw, excerpt_raw)) = header_line.split_once(": ") else {
        return (None, text.to_owned());
    };
    let Some(role) = QuoteRole::parse_tag(role_raw) else {
        return (None, text.to_owned());
    };
    let excerpt = excerpt_raw.trim();
    if excerpt.is_empty() {
        return (None, text.to_owned());
    }
    let display_body = match skill_prefix {
        Some(skill) if body.trim().is_empty() => skill,
        Some(skill) => format!("{skill} {body}"),
        None => body.to_owned(),
    };
    (
        Some(ActiveQuote {
            role,
            excerpt: excerpt.to_owned(),
        }),
        display_body,
    )
}

#[derive(Clone, Copy, Debug)]
pub enum ComposerEvent {
    Send,
    Stop,
    ModelMenu,
    AddMenu,
    OpenAddMenu,
    ClearWorkspace,
    ClearSkill,
    ClearQuote,
    CloudNotReady,
}

/// Pushed read-only snapshot from `StudioPanel` so `ChatComposer` stays decoupled
/// without positional boolean flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerViewState {
    pub model: Model,
    pub model_menu_open: bool,
    pub running: bool,
    pub stopping: bool,
    pub model_label: SharedString,
    pub cloud_ready: bool,
    pub workspace_label: SharedString,
    pub workspace_bound: bool,
    pub skill_label: Option<SharedString>,
    pub(crate) active_quote: Option<ActiveQuote>,
    pub add_menu_open: bool,
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
    skill_label: Option<SharedString>,
    active_quote: Option<ActiveQuote>,
    add_menu_open: bool,
    cloud_ready: bool,
    model_menu_open: bool,
    running: bool,
    stopping: bool,
    _theme_sub: Subscription,
    _prompt_sub: Subscription,
}

struct ContextChipSpec {
    pill_id: &'static str,
    clear_id: &'static str,
    max_w: f32,
    aria_label: SharedString,
    label: SharedString,
    clear_label: String,
    icon: theme::icons::Icon,
    clear_event: ComposerEvent,
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
            skill_label: None,
            active_quote: None,
            add_menu_open: false,
            cloud_ready: false,
            model_menu_open: false,
            running: false,
            stopping: false,
            _theme_sub: theme_sub,
            _prompt_sub: prompt_sub,
        }
    }

    pub(crate) fn sync_state(
        &mut self,
        next: ComposerViewState,
        cx: &mut Context<'_, Self>,
    ) {
        if self.model != next.model
            || self.model_menu_open != next.model_menu_open
            || self.running != next.running
            || self.stopping != next.stopping
            || self.model_label != next.model_label
            || self.cloud_ready != next.cloud_ready
            || self.workspace_label != next.workspace_label
            || self.workspace_bound != next.workspace_bound
            || self.skill_label != next.skill_label
            || self.active_quote != next.active_quote
            || self.add_menu_open != next.add_menu_open
        {
            self.model = next.model;
            self.model_menu_open = next.model_menu_open;
            self.running = next.running;
            self.stopping = next.stopping;
            self.model_label = next.model_label;
            self.cloud_ready = next.cloud_ready;
            self.workspace_label = next.workspace_label;
            self.workspace_bound = next.workspace_bound;
            self.skill_label = next.skill_label;
            self.active_quote = next.active_quote;
            self.add_menu_open = next.add_menu_open;
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

    /// Calm personal-assistant composer: single `+` menu + active-only context
    /// chips on the left; single model picker + stop/send on the right.
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
                    .max_w(px(crate::STAGE_MAX_W))
                    .rounded(self.theme.modal_radius())
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
                    .when_some(self.active_quote.as_ref(), |card, quote| {
                        card.child(self.quote_banner(quote, cx))
                    })
                    .child(
                        // The kit multi-line editor is `h_full` inside, so it
                        // needs a bounded parent or it collapses to one row.
                        div()
                            .w_full()
                            .min_h(px(48.))
                            .max_h(px(160.))
                            .pb(px(2.))
                            .child(
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
                                    .gap(px(6.))
                                    .child(self.add_menu_trigger(cx))
                                    .when(self.workspace_bound, |row| {
                                        row.child(
                                            self.active_workspace_chip(cx),
                                        )
                                    })
                                    .when_some(
                                        self.skill_label.as_ref(),
                                        |row, label| {
                                            row.child(self.active_skill_chip(
                                                label.clone(),
                                                cx,
                                            ))
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(self.model_selector(cx))
                                    .child(self.composer_submit(
                                        running, has_draft, cx,
                                    )),
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

    fn add_menu_trigger(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let side = self.theme.control_height(theme::ControlSize::Small);
        let open = self.add_menu_open;
        let plus_icon =
            if crate::motion::travel_scale(self.theme.motion_policy()) > 0.0 {
                let target = if open { FRAC_PI_4 } else { 0.0_f32 };
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .with_spring(
                        "composer-add-spin",
                        SpringAnimation::new(crate::motion::spring_snappy())
                            .to(target),
                        |el, angle| {
                            el.child(
                                theme::icons::plus()
                                    .size(px(14.))
                                    .with_transformation(
                                        Transformation::rotate(radians(angle)),
                                    ),
                            )
                        },
                    )
                    .into_any_element()
            } else {
                let icon = theme::icons::plus().size(px(14.));
                if open {
                    icon.with_transformation(Transformation::rotate(radians(
                        FRAC_PI_4,
                    )))
                    .into_any_element()
                } else {
                    icon.into_any_element()
                }
            };
        // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
        // `Action::Expand`, so keeping a plain button stays clickable via UIA.
        theme::anchor_trigger(
            theme::interactive(
                "composer-add",
                self.t("composer.add_label"),
                &self.theme,
            )
            .w(side)
            .h(side)
            .rounded_full()
            .border_1()
            .border_color(if open {
                self.theme.border_strong()
            } else {
                self.theme.line()
            })
            .bg(if open {
                self.theme.soft()
            } else {
                self.theme.card()
            })
            .text_color(self.theme.ink())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .child(plus_icon)
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(ComposerEvent::AddMenu);
            })),
            "composer-add-trigger",
            &self.ui,
        )
    }

    fn quote_banner(
        &self,
        quote: &ActiveQuote,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("composer-quote-banner")
            .debug_selector(|| "composer-quote-banner".to_owned())
            .w_full()
            .px(self.theme.space(10.))
            .py(px(6.))
            .rounded_md()
            .border_l_2()
            .border_color(self.theme.accent())
            .bg(self.theme.soft())
            .flex()
            .items_center()
            .justify_between()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        theme::icons::quote()
                            .size(px(12.))
                            .flex_none()
                            .text_color(self.theme.muted()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.muted())
                            .child(format!("{}:", self.t("chat.quoted_label"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .text_xs()
                            .text_color(self.theme.ink())
                            .child(quote.excerpt.clone()),
                    ),
            )
            .child(
                theme::interactive(
                    "composer-quote-clear",
                    self.t("composer.quote_clear"),
                    &self.theme,
                )
                .debug_selector(|| "composer-quote-clear".to_owned())
                .size(px(20.))
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(self.theme.muted())
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(self.theme.dock_hover())
                        .text_color(self.theme.ink())
                })
                .child(theme::icons::x().size(px(11.)))
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::ClearQuote);
                })),
            )
    }

    fn active_context_chip(
        &self,
        spec: ContextChipSpec,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let ContextChipSpec {
            pill_id,
            clear_id,
            max_w,
            aria_label,
            label,
            clear_label,
            icon,
            clear_event,
        } = spec;
        let bg = self.theme.status_active_bg();
        let fg = self.theme.status_active();
        div()
            .min_h(self.theme.control_height(theme::ControlSize::Small))
            .max_w(px(max_w))
            .pl(self.theme.space(8.))
            .pr(px(4.))
            .rounded_full()
            .bg(bg)
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                theme::interactive(pill_id, aria_label, &self.theme)
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_xs()
                    .text_color(fg)
                    .cursor_pointer()
                    .child(icon.text_color(fg))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(label),
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(ComposerEvent::OpenAddMenu);
                    })),
            )
            .child(
                theme::interactive(clear_id, clear_label, &self.theme)
                    .size(px(18.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(fg)
                    .cursor_pointer()
                    .hover(|style| style.bg(self.theme.dock_hover()))
                    .child(theme::icons::x().size(px(11.)).text_color(fg))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(clear_event);
                    })),
            )
    }

    fn active_workspace_chip(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let label = if self.workspace_label.is_empty() {
            self.t("composer.workspace_default")
        } else {
            self.workspace_label.to_string()
        };
        let aria_label =
            SharedString::from(format!("{}: {label}", self.t("dock.projects")));
        self.active_context_chip(
            ContextChipSpec {
                pill_id: "composer-workspace-pill",
                clear_id: "composer-clear-workspace",
                max_w: 184.0,
                aria_label,
                label: SharedString::from(label),
                clear_label: self.t("composer.clear_workspace"),
                icon: theme::icons::folder().size(px(12.)),
                clear_event: ComposerEvent::ClearWorkspace,
            },
            cx,
        )
    }

    fn active_skill_chip(
        &self,
        label: SharedString,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        self.active_context_chip(
            ContextChipSpec {
                pill_id: "composer-skill-pill",
                clear_id: "composer-clear-skill",
                max_w: 172.0,
                aria_label: label.clone(),
                label,
                clear_label: self.t("composer.skill_clear"),
                icon: theme::icons::sparkles().size(px(11.)),
                clear_event: ComposerEvent::ClearSkill,
            },
            cx,
        )
    }

    fn model_selector(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let label = self.composer_model_label();
        let open = self.model_menu_open;
        let (bg, fg) = if !self.cloud_ready {
            (
                self.theme.status_attention_bg(),
                self.theme.status_attention(),
            )
        } else if open {
            (self.theme.soft(), self.theme.ink())
        } else {
            (self.theme.card(), self.theme.muted())
        };
        let chevron = if open {
            theme::icons::chevron_up().size(px(12.))
        } else {
            theme::icons::chevron_down().size(px(12.))
        };
        // Omit `.aria_expanded(...)`: gpui-pre 0.3.14 drops AccessKit
        // `Action::Expand`, so keeping a plain button stays clickable via UIA.
        theme::anchor_trigger(
            theme::interactive("composer-model", label.clone(), &self.theme)
                .min_h(self.theme.control_height(theme::ControlSize::Small))
                .px(self.theme.space(8.))
                .rounded_full()
                .bg(bg)
                .text_xs()
                .text_color(fg)
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .hover(|style| {
                    style.bg(self.theme.soft()).text_color(self.theme.ink())
                })
                .child(label)
                .child(chevron)
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(ComposerEvent::ModelMenu);
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
        let send_size = self.theme.control_height(theme::ControlSize::Small);
        if running {
            let stopping = self.stopping;
            let danger_bg = self.theme.danger_bg();
            let danger_fg = self.theme.danger();
            let stop_label = if stopping {
                self.t("chat.status_stopping")
            } else {
                self.t("chat.stop")
            };
            row = row.child(
                theme::button(
                    "chat-stop",
                    stop_label,
                    stopping,
                    &self.theme,
                    cx.listener(|this, _, _, cx| {
                        if !this.stopping {
                            cx.emit(ComposerEvent::Stop);
                        }
                    }),
                )
                .w(send_size)
                .h(send_size)
                .rounded_full()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.soft())
                .text_color(self.theme.ink())
                .flex()
                .items_center()
                .justify_center()
                .when(stopping, |el| el.opacity(0.5))
                .when(!stopping, move |el| {
                    el.hover(move |style| {
                        style
                            .bg(danger_bg)
                            .border_color(danger_fg.opacity(0.45))
                            .text_color(danger_fg)
                    })
                })
                .child(theme::icons::square().size(px(11.))),
            );
        }
        let send_colors =
            self.theme.control_colors(theme::ControlVariant::Primary);
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
            .when(!has_draft, |el| el.opacity(0.4))
            .when(has_draft, move |el| {
                el.hover(move |style| style.bg(btn_hover))
            })
            .child(theme::icons::arrow_up().size(px(14.))),
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
                if let Some(session_id) = self.agent.active_session_id()
                    && let Some(heartbeat) =
                        self.agent.session_heartbeat(&session_id)
                    && let Some(label) =
                        crate::pending_card::localize_heartbeat_action(
                            &heartbeat,
                            &|key: &str| self.t(key),
                        )
                {
                    return format!("{label} \u{00b7} {duration}");
                }
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

    #[test]
    fn quote_excerpt_truncates_multibyte_chinese_without_panic() {
        let long_zh = "深圳天气预报：今天晴转多云，气温二十四度；😀".repeat(10);
        let excerpt =
            super::make_quote_excerpt(&format!("第一行\n\n{long_zh}"));
        assert!(excerpt.chars().count() <= 160);
        assert!(excerpt.ends_with('\u{2026}'));
        assert!(!excerpt.contains('\n'));
        let wrapped = format!("> \u{275d} Flowy: 旧摘要\n\n用户后续提问内容");
        assert_eq!(super::make_quote_excerpt(&wrapped), "用户后续提问内容");
    }

    #[test]
    fn quote_compose_and_split_preserve_skill_command_and_chinese_body() {
        let quote = super::ActiveQuote {
            role: super::QuoteRole::Assistant,
            excerpt: "深圳今日晴朗，气温 24°C".into(),
        };
        let plain = super::compose_quoted_prompt(Some(&quote), "明天降温吗？");
        assert!(plain.starts_with("> \u{275d} Flowy: "));
        let (parsed, body) = super::split_quoted_prompt(&plain);
        assert_eq!(parsed.as_ref(), Some(&quote));
        assert_eq!(body, "明天降温吗？");

        let skill_packed = super::compose_quoted_prompt(
            Some(&quote),
            "/skill:flowy/xlsx-summary 整理第三季度表",
        );
        assert!(
            skill_packed
                .starts_with("/skill:flowy/xlsx-summary > \u{275d} Flowy: "),
            "skill protocol prefix must remain first for prepare_skill_prompt: {skill_packed}"
        );
        let (parsed_skill, body_skill) =
            super::split_quoted_prompt(&skill_packed);
        assert_eq!(parsed_skill.as_ref(), Some(&quote));
        assert_eq!(body_skill, "/skill:flowy/xlsx-summary 整理第三季度表");

        let ordinary_md = "> 普通引文\n\n正文提问";
        assert_eq!(
            super::split_quoted_prompt(ordinary_md),
            (None, ordinary_md.to_owned())
        );
    }
}
