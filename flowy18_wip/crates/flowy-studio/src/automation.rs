use flowy_i18n::I18n;
use flowy_ui::{self as ui, icons};
use gpui::{
    Context, Entity, Focusable, FontWeight, Rgba, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use gpui_kit::base::input::{InputEditorStyle, InputEvent, TextareaState};

use crate::panel::StudioPanel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaunchMode {
    Schedule,
    Trigger,
}

impl LaunchMode {
    fn icon(self) -> icons::Icon {
        match self {
            Self::Schedule => icons::clock(),
            Self::Trigger => icons::workflow(),
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Schedule => "auto.schedule_title",
            Self::Trigger => "auto.trigger_title",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Schedule => "auto.schedule_body",
            Self::Trigger => "auto.trigger_body",
        }
    }
}

pub struct AutomationDraft {
    prompt: Entity<TextareaState>,
    placeholder: SharedString,
    placeholder_color: Option<Rgba>,
    _input_subscription: Subscription,
    pub(crate) started: bool,
    show_modes: bool,
    launch_mode: Option<LaunchMode>,
}

impl AutomationDraft {
    pub(crate) fn new(
        i18n: &I18n,
        window: &mut Window,
        cx: &mut Context<'_, StudioPanel>,
    ) -> Self {
        let placeholder = SharedString::from(i18n.t("auto.prompt_placeholder"));
        let prompt = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx);
            state.set_placeholder(placeholder.clone(), window, cx);
            state
        });
        let input_subscription =
            cx.subscribe(&prompt, |_, _, _: &InputEvent, cx| cx.notify());
        Self {
            prompt,
            placeholder,
            placeholder_color: None,
            _input_subscription: input_subscription,
            started: false,
            show_modes: true,
            launch_mode: None,
        }
    }

    pub(crate) fn sync_editor(
        &mut self,
        placeholder: SharedString,
        theme: ui::ResolvedTheme,
        window: &mut Window,
        cx: &mut Context<'_, StudioPanel>,
    ) {
        let muted = theme.muted();
        let placeholder_changed = self.placeholder != placeholder;
        let color_changed = self.placeholder_color != Some(muted);
        if placeholder_changed || color_changed {
            self.placeholder = placeholder.clone();
            self.placeholder_color = Some(muted);
            self.prompt.update(cx, |state, cx| {
                if placeholder_changed {
                    state.set_placeholder(placeholder, window, cx);
                }
                if color_changed {
                    state.set_editor_style(InputEditorStyle {
                        muted_foreground: muted.into(),
                        caret: theme.ink().into(),
                        selection: theme.focus_ring().opacity(0.28).into(),
                        ..InputEditorStyle::default()
                    });
                }
            });
        }
    }

    fn focus_prompt(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, StudioPanel>,
    ) {
        self.prompt.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn is_empty(&self, cx: &Context<'_, StudioPanel>) -> bool {
        self.prompt.read(cx).value().trim().is_empty()
    }
}

struct Suggestion {
    mode: LaunchMode,
    icon: fn() -> icons::Icon,
    source: &'static str,
    title: &'static str,
    body: &'static str,
}

const SUGGESTIONS: &[Suggestion] = &[
    Suggestion {
        mode: LaunchMode::Schedule,
        icon: icons::clock,
        source: "auto.schedule_title",
        title: "auto.suggest_growth_title",
        body: "auto.suggest_growth_body",
    },
    Suggestion {
        mode: LaunchMode::Trigger,
        icon: icons::mail,
        source: "auto.mail_saved",
        title: "auto.suggest_return_title",
        body: "auto.suggest_return_body",
    },
    Suggestion {
        mode: LaunchMode::Schedule,
        icon: icons::clock,
        source: "auto.schedule_title",
        title: "auto.suggest_ads_title",
        body: "auto.suggest_ads_body",
    },
];

impl StudioPanel {
    pub(crate) fn automation_canvas(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let started = self.automation.started;
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(!started, |page| {
                page.child(
                    ui::PageHeader::new(self.t("dock.automation"))
                        .render(&self.theme, window),
                )
            })
            .child(
                ui::scroll_area(
                    if started {
                        "automation-workflow"
                    } else {
                        "automation-welcome"
                    },
                    &self.theme,
                )
                .a11y_label(self.t("a11y.scroll_automation_canvas"))
                .debug_selector(move || {
                    if started {
                        "automation-workflow"
                    } else {
                        "automation-welcome"
                    }
                    .to_owned()
                })
                .flex_1()
                .min_h_0()
                .viewport_style({
                    let pad_x = self.theme.space(28.);
                    move |s| s.px(pad_x)
                })
                .child(
                    div()
                        .w_full()
                        .max_w(px(if started { 708. } else { 920. }))
                        .mx_auto()
                        .pt(self.theme.space(if started { 28. } else { 48. }))
                        .pb(self.theme.space(48.))
                        .flex()
                        .flex_col()
                        .child(if started {
                            self.automation_workflow(cx).into_any_element()
                        } else {
                            self.automation_welcome(cx).into_any_element()
                        }),
                ),
            )
    }

    fn automation_welcome(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(34.))
                    .line_height(px(43.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("auto.hero")),
            )
            .child(
                div()
                    .mt(self.theme.space(8.))
                    .text_base()
                    .text_color(self.theme.muted())
                    .child(self.t("auto.lead")),
            )
            .child(
                div()
                    .mt(self.theme.space(32.))
                    .flex()
                    .flex_wrap()
                    .gap(self.theme.space(16.))
                    .children(
                        [LaunchMode::Schedule, LaunchMode::Trigger]
                            .map(|mode| self.automation_overview_card(mode)),
                    ),
            )
            .child(
                div().mt(self.theme.space(28.)).flex().child(
                    ui::Button::new("automation-start", self.t("auto.start"))
                        .variant(ui::ControlVariant::Primary)
                        .size(ui::ControlSize::Large)
                        .trailing_icon(icons::arrow_right())
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, window, cx| {
                                panel.automation.started = true;
                                panel.automation.focus_prompt(window, cx);
                                cx.notify();
                            }),
                        ),
                ),
            )
    }

    fn automation_overview_card(&self, mode: LaunchMode) -> impl IntoElement {
        div()
            .flex_1()
            .flex_basis(px(280.))
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .overflow_hidden()
            .child(
                div()
                    .min_h(px(204.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .rounded_t_xl()
                    .bg(self.theme.soft())
                    .p(self.theme.space(20.))
                    .child(match mode {
                        LaunchMode::Schedule => {
                            self.calendar_preview().into_any_element()
                        }
                        LaunchMode::Trigger => {
                            self.trigger_preview().into_any_element()
                        }
                    }),
            )
            .child(
                div()
                    .p(self.theme.space(20.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t(mode.title())),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(self.theme.muted())
                            .child(self.t(mode.description())),
                    ),
            )
    }

    fn calendar_preview(&self) -> impl IntoElement {
        let colors = self.theme.control_colors(ui::ControlVariant::Primary);
        Self::automation_diagram()
            .child(
                div()
                    .rounded_lg()
                    .bg(self.theme.card())
                    .p(self.theme.space(10.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(8.))
                    .child(self.diagram_label(icons::clock(), "auto.month"))
                    .child(div().flex().gap(self.theme.space(2.)).children(
                        (23..=29).map(|day| {
                            let current = day == 25;
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .justify_center()
                                .child(
                                    div()
                                        .size(px(28.))
                                        .rounded_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_sm()
                                        .when(current, |d| {
                                            d.bg(colors.background)
                                                .text_color(colors.foreground)
                                                .font_weight(
                                                    FontWeight::SEMIBOLD,
                                                )
                                        })
                                        .when(!current, |d| {
                                            d.text_color(self.theme.muted())
                                        })
                                        .child(day.to_string()),
                                )
                        }),
                    )),
            )
            .child(self.diagram_connector())
            .child(self.diagram_node(icons::file_plus(), "auto.schedule_file"))
    }

    fn trigger_preview(&self) -> impl IntoElement {
        Self::automation_diagram()
            .child(self.diagram_node(icons::mail(), "auto.mail_saved"))
            .child(self.diagram_connector())
            .child(self.diagram_node(icons::square_pen(), "auto.mail_task"))
    }

    fn automation_diagram() -> gpui::Div {
        div().w_full().max_w(px(312.)).mx_auto().flex().flex_col()
    }

    fn diagram_connector(&self) -> impl IntoElement {
        div().h(px(22.)).pl(px(12.)).flex().items_center().child(
            icons::chevron_down()
                .size(px(16.))
                .text_color(self.theme.muted()),
        )
    }

    fn diagram_node(
        &self,
        icon: icons::Icon,
        label: &'static str,
    ) -> impl IntoElement {
        div()
            .min_h(px(38.))
            .rounded_lg()
            .bg(self.theme.card())
            .px(self.theme.space(10.))
            .py(self.theme.space(7.))
            .flex()
            .items_center()
            .child(self.diagram_label(icon, label))
    }

    fn diagram_label(
        &self,
        icon: icons::Icon,
        label: &'static str,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .min_w_0()
            .child(
                icon.size(px(15.))
                    .flex_shrink_0()
                    .text_color(self.theme.focus_ring()),
            )
            .child(div().min_w_0().text_sm().child(self.t(label)))
    }

    fn automation_workflow(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div().flex().child(
                    ui::Button::new(
                        "automation-back",
                        self.t("dock.automation"),
                    )
                    .variant(ui::ControlVariant::Ghost)
                    .size(ui::ControlSize::Small)
                    .icon(icons::arrow_left())
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.automation.started = false;
                            cx.notify();
                        }),
                    ),
                ),
            )
            .child(
                div()
                    .mt(self.theme.space(34.))
                    .text_center()
                    .text_size(px(28.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("auto.workflow_title")),
            )
            .child(
                div()
                    .mt(self.theme.space(28.))
                    .child(self.automation_composer(cx)),
            )
            .when(self.automation.show_modes, |column| {
                column.child(self.automation_launch_panel(cx))
            })
            .when(self.automation.is_empty(cx), |column| {
                column.child(self.automation_suggestions(cx))
            })
    }

    fn automation_composer(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mode = self.automation.launch_mode;
        let placeholder = self.t("auto.prompt_placeholder");
        div()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .px(self.theme.space(16.))
            .py(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(10.))
            .child(
                div()
                    .id("automation-draft")
                    .aria_label(placeholder)
                    .w_full()
                    .min_h(px(62.))
                    .max_h(px(160.))
                    .child(ui::text_area(
                        "automation-draft-input",
                        &self.automation.prompt,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        ui::Button::new(
                            "automation-mode-toggle",
                            self.t(mode
                                .map_or("dock.automation", LaunchMode::title)),
                        )
                        .variant(ui::ControlVariant::Ghost)
                        .framed()
                        .size(ui::ControlSize::Small)
                        .icon(
                            mode.map_or_else(icons::workflow, LaunchMode::icon),
                        )
                        .trailing_icon(if self.automation.show_modes {
                            icons::chevron_up()
                        } else {
                            icons::chevron_down()
                        })
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.automation.show_modes =
                                    !panel.automation.show_modes;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::IconButton::new(
                            "automation-send",
                            self.t("studio.send"),
                            icons::send(),
                        )
                        .variant(ui::ControlVariant::Primary)
                        .disabled(true)
                        .render(&self.theme, |_, _, _| {}),
                    ),
            )
            .child(
                div()
                    .pt(self.theme.space(8.))
                    .border_t_1()
                    .border_color(self.theme.line())
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t("auto.later")),
            )
    }

    fn automation_launch_panel(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .mt(self.theme.space(16.))
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("auto.mode_label")),
                    )
                    .child(
                        ui::IconButton::new(
                            "automation-mode-close",
                            self.t("ui.close"),
                            icons::x(),
                        )
                        .size(ui::ControlSize::Small)
                        .variant(ui::ControlVariant::Ghost)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, window, cx| {
                                panel.automation.show_modes = false;
                                panel.automation.focus_prompt(window, cx);
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div().flex().flex_wrap().gap(self.theme.space(8.)).children(
                    [LaunchMode::Schedule, LaunchMode::Trigger]
                        .map(|mode| self.automation_mode_option(mode, cx)),
                ),
            )
    }

    fn automation_mode_option(
        &self,
        mode: LaunchMode,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.automation.launch_mode == Some(mode);
        ui::interactive(mode.title(), self.t(mode.title()), &self.theme)
            .aria_toggled(if selected {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            })
            .flex_1()
            .flex_basis(px(160.))
            .min_w_0()
            .min_h(px(46.))
            .flex()
            .items_center()
            .gap(self.theme.space(9.))
            .px(self.theme.space(12.))
            .py(self.theme.space(6.))
            .rounded_lg()
            .when(selected, |d| {
                d.border_color(self.theme.accent()).bg(self.theme.card())
            })
            .when(!selected, |d| d.bg(self.theme.soft()))
            .hover(|s| s.bg(self.theme.card()).border_color(self.theme.line()))
            .child(
                mode.icon()
                    .size(px(16.))
                    .flex_shrink_0()
                    .text_color(self.theme.focus_ring()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.t(mode.title())),
            )
            .when(selected, |row| {
                row.child(
                    icons::check()
                        .size(px(14.))
                        .text_color(self.theme.focus_ring()),
                )
            })
            .on_click(cx.listener(move |panel, _, window, cx| {
                panel.automation.launch_mode = Some(mode);
                panel.automation.focus_prompt(window, cx);
                cx.notify();
            }))
    }

    fn automation_suggestions(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .mt(self.theme.space(30.))
            .flex()
            .flex_col()
            .gap(self.theme.space(10.))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("auto.suggestions")),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(self.theme.line())
                    .flex()
                    .flex_wrap()
                    .gap(self.theme.space(8.))
                    .children(SUGGESTIONS.iter().map(|example| {
                        self.automation_suggestion(example, cx)
                    })),
            )
    }

    fn automation_suggestion(
        &self,
        example: &'static Suggestion,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        ui::interactive(example.title, self.t(example.title), &self.theme)
            .flex_1()
            .flex_basis(px(182.))
            .min_w_0()
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .rounded_lg()
            .px(self.theme.space(10.))
            .py(self.theme.space(12.))
            .hover(|d| d.bg(self.theme.card()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(
                        (example.icon)()
                            .size(px(14.))
                            .flex_shrink_0()
                            .text_color(self.theme.focus_ring()),
                    )
                    .child(div().min_w_0().child(self.t(example.source))),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(self.theme.space(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t(example.title)),
                    )
                    .child(
                        icons::arrow_right()
                            .mt(px(3.))
                            .size(px(14.))
                            .flex_shrink_0()
                            .text_color(self.theme.muted()),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(self.theme.muted())
                    .child(self.t(example.body)),
            )
            .on_click(cx.listener(move |panel, _, window, cx| {
                if panel.automation.is_empty(cx) {
                    let text = panel.t(example.body);
                    panel.automation.launch_mode = Some(example.mode);
                    panel.automation.prompt.update(cx, |state, cx| {
                        state.set_value(text, window, cx);
                        let end = state.value().len();
                        state.set_selected_range(end..end, cx);
                    });
                }
                panel.automation.focus_prompt(window, cx);
                cx.notify();
            }))
    }
}
