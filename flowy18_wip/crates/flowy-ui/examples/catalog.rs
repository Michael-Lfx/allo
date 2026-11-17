//! Isolated component catalogue. Run `cargo run -p flowy-ui --example catalog`.
//! All demonstration values live here, outside production application state.
use flowy_ui::{
    Accent, Button, Choice, ControlVariant, Density, InputMode, RadioGroup,
    ResolvedTheme, Switch, Tabs, TextField, ThemeMode, ThemeOptions,
};
use gpui::{
    Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use gpui_ce_elements::editable_text::{EditableTextState, StringStorage};

#[allow(clippy::derive_partial_eq_without_eq)]
mod bindings {
    gpui::actions!(catalogue, [Next, Previous]);
}
use bindings::{Next, Previous};

struct Catalogue {
    theme: ResolvedTheme,
    input: Entity<EditableTextState>,
    readonly: Entity<EditableTextState>,
    enabled: bool,
    selected: gpui::SharedString,
}
impl Render for Catalogue {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let t = self.theme;
        let select_radio =
            cx.listener(|this, value: &gpui::SharedString, _, cx| {
                this.selected = value.clone();
                cx.notify();
            });
        let select_tab =
            cx.listener(|this, value: &gpui::SharedString, _, cx| {
                this.selected = value.clone();
                cx.notify();
            });
        div()
            .id("catalogue")
            .on_action(|_: &Next, window, cx| window.focus_next(cx))
            .on_action(|_: &Previous, window, cx| window.focus_prev(cx))
            .on_action(
                |_: &gpui_ce_elements::editable_text::actions::Tab,
                 window,
                 cx| window.focus_next(cx),
            )
            .size_full()
            .bg(t.canvas())
            .text_color(t.ink())
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_2xl().child("Flowy UI / Component catalogue"))
            .child(
                Button::new("toggle-theme", "Light / Dark")
                    .variant(ControlVariant::Secondary)
                    .render(
                        &t,
                        cx.listener(|this, _, _, cx| {
                            this.theme = ResolvedTheme::resolve(
                                ThemeOptions {
                                    mode: if this.theme.dark {
                                        ThemeMode::Light
                                    } else {
                                        ThemeMode::Dark
                                    },
                                    ..this.theme.options
                                },
                                false,
                            );
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        Button::new("primary", "Primary")
                            .render(&t, |_, _, _| {}),
                    )
                    .child(
                        Button::new("secondary", "Secondary")
                            .variant(ControlVariant::Secondary)
                            .render(&t, |_, _, _| {}),
                    )
                    .child(
                        Button::new("loading", "Loading")
                            .loading(true)
                            .render(&t, |_, _, _| {}),
                    )
                    .child(
                        Button::new("disabled", "Disabled")
                            .disabled(true)
                            .render(&t, |_, _, _| {}),
                    )
                    .child(
                        Button::new("danger", "Danger")
                            .variant(ControlVariant::Danger)
                            .render(&t, |_, _, _| {}),
                    ),
            )
            .child(
                TextField::new(
                    "editable",
                    "Editable field",
                    self.input.clone(),
                )
                .placeholder("Type or paste text")
                .render(&t, cx),
            )
            .child(
                TextField::new(
                    "readonly",
                    "Read-only field",
                    self.readonly.clone(),
                )
                .mode(InputMode::ReadOnly)
                .render(&t, cx),
            )
            .child(
                TextField::new(
                    "disabled-input",
                    "Disabled field",
                    self.readonly.clone(),
                )
                .mode(InputMode::Disabled)
                .render(&t, cx),
            )
            .child(
                Switch::new("switch", "Enable setting", self.enabled).render(
                    &t,
                    cx.listener(|this, value, _, cx| {
                        this.enabled = *value;
                        cx.notify();
                    }),
                ),
            )
            .child(
                RadioGroup::new(
                    "radios",
                    "Choice",
                    self.selected.clone(),
                    vec![
                        Choice::new("first", "First"),
                        Choice::new("second", "Second"),
                    ],
                )
                .render(&t, window, cx, select_radio),
            )
            .child(
                Tabs::new(
                    "tabs",
                    "Tab choice",
                    self.selected.clone(),
                    vec![
                        Choice::new("first", "First"),
                        Choice::new("second", "Second"),
                    ],
                )
                .render(&t, window, cx, select_tab),
            )
            .child(flowy_ui::notice(
                "catalogue-notice",
                "Component preview values are isolated from the application.",
                flowy_ui::NoticeKind::Info,
                &t,
            ))
    }
}
fn main() {
    gpui_platform::application().run(|cx| {
        cx.bind_keys([
            gpui::KeyBinding::new("tab", Next, None),
            gpui::KeyBinding::new("shift-tab", Previous, None),
        ]);
        cx.bind_keys(
            gpui_ce_elements::editable_text::actions::default_bindings()
                .as_keybindings(Some(
                gpui_ce_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT,
            )),
        );
        flowy_ui::install_initial_tab_focus(cx).detach();
        let bounds = Bounds::centered(None, size(px(960.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            |_, cx| {
                cx.new(|cx| Catalogue {
                    theme: ResolvedTheme::resolve(
                        ThemeOptions {
                            mode: ThemeMode::Light,
                            accent: Accent::Default,
                            density: Density::Regular,
                            font_size: 14,
                        },
                        false,
                    ),
                    input: cx.new(|cx| {
                        EditableTextState::new(StringStorage::default(), cx)
                    }),
                    readonly: cx.new(|cx| {
                        EditableTextState::new(
                            StringStorage::from("Read-only preview"),
                            cx,
                        )
                    }),
                    enabled: true,
                    selected: "first".into(),
                })
            },
        )
        .expect("open component catalogue");
        cx.activate(true);
    });
}
