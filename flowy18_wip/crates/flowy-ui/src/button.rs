//! Interactive primitives. IDs preserve GPUI focus state across frames.
use crate::{ControlSize, ControlVariant, InteractiveDiv, ResolvedTheme};
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Role, SharedString, Window, div,
    prelude::*, px,
};
use gpui_kit::base::TestSupportExt as _;

/// The unobserved interaction surface shared by [`interactive`] and [`button`].
// The theme is taken by reference on purpose: every control receives the same
// theme for the whole frame, and copying it per control buys nothing.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "callers share one theme across many controls"
)]
fn interaction_surface(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &ResolvedTheme,
) -> gpui::Stateful<gpui::Div> {
    let focus = theme.focus_ring();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .cursor_pointer()
        .font(crate::ui_font())
        .border_2()
        .border_color(theme.line().opacity(0.))
        .focus_visible(move |style| style.border_color(focus))
}

/// Semantic, keyboard-accessible interaction surface for custom content.
/// Keep its id stable and provide a localized accessible name.
pub fn interactive(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &ResolvedTheme,
) -> InteractiveDiv {
    // Registers the node for `find_all_by_label`/`find_all_by_role` in UI
    // tests. Identity in builds without `test-support`.
    interaction_surface(id, label, theme).test_support()
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    theme: &ResolvedTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> InteractiveDiv {
    let id = id.into();
    let label = label.into();
    if disabled {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .font(crate::ui_font())
            .test_support()
    } else {
        interaction_surface(id, label, theme)
            .on_click(on_click)
            .test_support()
    }
}

/// Standard visual and behavior recipe for buttons. Loading retains the tab stop.
#[must_use]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ControlVariant,
    size: ControlSize,
    disabled: bool,
    loading: bool,
    icon: Option<AnyElement>,
    trailing_icon: Option<AnyElement>,
    framed: bool,
    full_width: bool,
}
impl Button {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ControlVariant::Secondary,
            size: ControlSize::Medium,
            disabled: false,
            loading: false,
            icon: None,
            trailing_icon: None,
            framed: false,
            full_width: false,
        }
    }
    pub const fn variant(mut self, value: ControlVariant) -> Self {
        self.variant = value;
        self
    }
    pub const fn size(mut self, value: ControlSize) -> Self {
        self.size = value;
        self
    }
    pub const fn disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self
    }
    pub const fn loading(mut self, value: bool) -> Self {
        self.loading = value;
        self
    }
    pub fn icon(mut self, value: impl IntoElement) -> Self {
        self.icon = Some(value.into_any_element());
        self
    }
    pub fn trailing_icon(mut self, value: impl IntoElement) -> Self {
        self.trailing_icon = Some(value.into_any_element());
        self
    }
    /// Uses the light frame shared by supporting toolbar controls.
    pub const fn framed(mut self) -> Self {
        self.framed = true;
        self
    }
    pub const fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    pub fn render(
        self,
        theme: &ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let loading_id: ElementId = (self.id.clone(), "loading").into();
        let icon_id: ElementId = (self.id.clone(), "icon").into();
        let trailing_id: ElementId = (self.id.clone(), "trailing-icon").into();
        let colors = theme.control_colors(self.variant);
        let hover = if self.framed {
            theme.soft()
        } else {
            colors.hover
        };
        let loading = self.loading;
        let label = self.label.clone();
        let leading = if loading {
            Some(
                crate::icons::loader_circle()
                    .spinning(theme.motion_policy())
                    .into_any_element(),
            )
        } else {
            self.icon
        };
        let leading_id = if loading { loading_id } else { icon_id };
        let element = button(
            self.id,
            self.label,
            self.disabled,
            theme,
            move |event, window, cx| {
                if !loading {
                    on_click(event, window, cx);
                }
            },
        )
        .border_1()
        .border_color(colors.border)
        .min_h(theme.control_height(self.size))
        .when(self.full_width, Styled::w_full)
        .px(theme.control_padding(self.size))
        .py(px(4.))
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(colors.background)
        .text_color(colors.foreground)
        .text_size(theme.label_size())
        .font_weight(if self.variant == ControlVariant::Primary {
            gpui::FontWeight::SEMIBOLD
        } else {
            gpui::FontWeight::MEDIUM
        })
        .when(self.framed, |el| toolbar_frame(el, *theme))
        .when(self.disabled, |el| el.opacity(0.5))
        .when(!self.disabled && !loading, move |el| {
            el.hover(move |s| s.bg(hover))
        })
        .when(loading, Styled::cursor_default)
        .children(leading.map(|icon| div().id(leading_id).child(icon)))
        .child(label)
        .children(
            self.trailing_icon
                .filter(|_| !loading)
                .map(|icon| div().id(trailing_id).child(icon)),
        );
        crate::A11yElement::new(element.into_element())
            .busy(loading)
            .disabled(self.disabled)
            .into_any_element()
            .into_any_element()
    }
}

fn toolbar_frame(
    element: InteractiveDiv,
    theme: ResolvedTheme,
) -> InteractiveDiv {
    element
        .border_1()
        .border_color(theme.chrome_line())
        .bg(theme.card())
        .rounded(px(10.))
}
impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("id", &self.id)
            .field("variant", &self.variant)
            .field("size", &self.size)
            .field("disabled", &self.disabled)
            .field("loading", &self.loading)
            .field("full_width", &self.full_width)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub struct IconButton {
    button: Button,
    icon: AnyElement,
    framed: bool,
    extent: Option<gpui::Pixels>,
}
impl IconButton {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        icon: impl IntoElement,
    ) -> Self {
        Self {
            button: Button::new(id, label).variant(ControlVariant::Ghost),
            icon: icon.into_any_element(),
            framed: false,
            extent: None,
        }
    }
    /// Pins both axes of the hit target.
    ///
    /// Use this when [`ControlSize`] is taller than the row the button is
    /// revealed in. A taller button grows that row on hover.
    pub const fn extent(mut self, value: gpui::Pixels) -> Self {
        self.extent = Some(value);
        self
    }
    /// Draws the control's own border and square hit target.
    pub const fn framed(mut self) -> Self {
        self.framed = true;
        self
    }
    pub fn size(mut self, value: ControlSize) -> Self {
        self.button = self.button.size(value);
        self
    }
    pub fn variant(mut self, value: ControlVariant) -> Self {
        self.button = self.button.variant(value);
        self
    }
    pub fn loading(mut self, value: bool) -> Self {
        self.button = self.button.loading(value);
        self
    }
    pub fn disabled(mut self, value: bool) -> Self {
        self.button = self.button.disabled(value);
        self
    }
    /// Renders the underlying interactive button element directly, suitable for
    /// wrapping with [`crate::tooltip_trigger`] or [`crate::anchor_trigger`]
    /// without introducing an extra focus wrapper / double tab stop.
    pub fn render_interactive(
        self,
        theme: &ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> InteractiveDiv {
        self.build_interactive(*theme, on_click).0
    }

    fn build_interactive(
        self,
        theme: ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> (InteractiveDiv, bool, bool) {
        let side = self
            .extent
            .unwrap_or_else(|| theme.control_height(self.button.size));
        let icon_id: ElementId = (self.button.id.clone(), "icon").into();
        let label = self.button.label.clone();
        let colors = theme.control_colors(self.button.variant);
        let loading = self.button.loading;
        let disabled = self.button.disabled;
        let leading = if loading {
            crate::icons::loader_circle()
                .spinning(theme.motion_policy())
                .into_any_element()
        } else {
            self.icon
        };
        let element = button(
            self.button.id,
            label,
            disabled,
            &theme,
            move |event, window, cx| {
                if !loading {
                    on_click(event, window, cx);
                }
            },
        )
        .border_1()
        .border_color(colors.border)
        .min_w(side)
        .min_h(side)
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.background)
        .text_color(colors.foreground)
        .when(disabled, |el| el.opacity(0.5))
        .when(!disabled && !loading, move |el| {
            el.hover(move |style| style.bg(colors.hover))
        })
        .when(loading, Styled::cursor_default)
        .when(self.framed, |element| {
            toolbar_frame(element, theme)
                .text_color(theme.toolbar_icon())
                .w(side)
                .h(side)
                .min_w(side)
                .min_h(side)
        })
        .child(div().id(icon_id).child(leading));
        (element, loading, disabled)
    }

    pub fn render(
        self,
        theme: &ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let (element, loading, disabled) =
            self.build_interactive(*theme, on_click);
        crate::A11yElement::new(element.into_element())
            .busy(loading)
            .disabled(disabled)
            .into_any_element()
            .into_any_element()
    }
}
impl std::fmt::Debug for IconButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconButton")
            .field("button", &self.button)
            .finish_non_exhaustive()
    }
}

/// Lets initial Tab enter the window. Overlay focus scopes intercept it first.
pub fn install_initial_tab_focus(cx: &mut App) -> gpui::Subscription {
    cx.intercept_keystrokes(|event, window, cx| {
        let modifiers = &event.keystroke.modifiers;
        if event.keystroke.key != "tab"
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
            || window.focused(cx).is_some()
        {
            return;
        }
        if modifiers.shift {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if window.focused(cx).is_some() {
            cx.stop_propagation();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, IntoElement, KeyBinding, KeyDownEvent, KeyUpEvent, Keystroke,
        Modifiers, Render, TestAppContext, VisualTestContext, accesskit, point,
        px,
    };
    use gpui_kit::test::TestQueryExt;

    // GPUI actions macro emits PartialEq without Eq.
    #[allow(clippy::derive_partial_eq_without_eq)]
    mod bindings {
        use gpui::actions;
        actions!(button_test, [Next, Previous]);
    }
    use bindings::{Next, Previous};

    #[test]
    fn buttons_default_to_secondary_and_allow_explicit_primary() {
        assert_eq!(
            Button::new("default-button", "Default").variant,
            ControlVariant::Secondary
        );
        assert_eq!(
            Button::new("primary-button", "Primary")
                .variant(ControlVariant::Primary)
                .variant,
            ControlVariant::Primary
        );
    }

    pub(super) fn press(cx: &mut VisualTestContext, key: &'static str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
    }

    #[derive(Default)]
    pub(super) struct Harness {
        pub(super) clicks: Vec<&'static str>,
    }

    impl Render for Harness {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            div()
                .id("test-root")
                .size_full()
                .on_action(|_: &Next, window, cx| window.focus_next(cx))
                .on_action(|_: &Previous, window, cx| window.focus_prev(cx))
                .child(
                    button(
                        "first",
                        "First",
                        false,
                        &crate::ResolvedTheme::default(),
                        cx.listener(|this, _, _, _| {
                            this.clicks.push("first");
                        }),
                    )
                    .absolute()
                    .left(px(10.))
                    .top(px(10.))
                    .size(px(40.)),
                )
                .child(
                    button(
                        "disabled",
                        "Unavailable",
                        true,
                        &crate::ResolvedTheme::default(),
                        cx.listener(|this, _, _, _| {
                            this.clicks.push("disabled");
                        }),
                    )
                    .absolute()
                    .left(px(60.))
                    .top(px(10.))
                    .size(px(40.)),
                )
                .child(
                    button(
                        "second",
                        "Second",
                        false,
                        &crate::ResolvedTheme::default(),
                        cx.listener(|this, _, _, _| {
                            this.clicks.push("second");
                        }),
                    )
                    .absolute()
                    .left(px(110.))
                    .top(px(10.))
                    .size(px(40.)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(160.))
                        .top(px(10.))
                        .size(px(40.))
                        .child(
                            Button::new("loading", "Loading")
                                .loading(true)
                                .render(
                                    &crate::ResolvedTheme::default(),
                                    cx.listener(|this, _, _, _| {
                                        this.clicks.push("loading");
                                    }),
                                ),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .top(px(60.))
                        .size(px(40.))
                        .child(
                            IconButton::new(
                                "icon-action",
                                "Icon action",
                                crate::icons::check(),
                            )
                            .variant(ControlVariant::Primary)
                            .render(
                                &crate::ResolvedTheme::default(),
                                cx.listener(|this, _, _, _| {
                                    this.clicks.push("icon");
                                }),
                            ),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(60.))
                        .top(px(60.))
                        .size(px(40.))
                        .child(
                            IconButton::new(
                                "icon-disabled",
                                "Disabled icon",
                                crate::icons::x(),
                            )
                            .disabled(true)
                            .render(
                                &crate::ResolvedTheme::default(),
                                cx.listener(|this, _, _, _| {
                                    this.clicks.push("disabled-icon");
                                }),
                            ),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(110.))
                        .top(px(60.))
                        .size(px(40.))
                        .child(
                            IconButton::new(
                                "icon-loading",
                                "Loading icon",
                                crate::icons::refresh_cw(),
                            )
                            .loading(true)
                            .render(
                                &crate::ResolvedTheme::default(),
                                cx.listener(|this, _, _, _| {
                                    this.clicks.push("loading-icon");
                                }),
                            ),
                        ),
                )
        }
    }

    #[gpui::test]
    fn keyboard_pointer_and_disabled_buttons(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("tab", Next, None),
                KeyBinding::new("shift-tab", Previous, None),
            ]);
        });
        let _initial_focus = cx.update(install_initial_tab_focus);
        let (view, cx) = cx.add_window_view(|_, _| Harness::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        press(cx, "tab");
        assert!(
            cx.update(|window, cx| window.focused(cx).is_some()),
            "first Tab must focus a button"
        );
        for key in ["enter", "space", "tab", "enter", "shift-tab", "enter"] {
            press(cx, key);
        }
        cx.simulate_click(point(px(70.), px(20.)), Modifiers::none());
        cx.simulate_click(point(px(120.), px(20.)), Modifiers::none());
        cx.read_entity(&view, |view, _| {
            assert_eq!(
                view.clicks,
                ["first", "first", "second", "first", "second"]
            );
        });
    }

    #[gpui::test]
    fn accessibility_exposes_label_role_and_disabled_state(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, _| Harness::default());
        let (first, unavailable) = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let first = window
                .find_all_by_label("First")
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .expect("missing accessible First button");
            let unavailable = window
                .find_all_by_label("Unavailable")
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .expect("missing accessible Unavailable button");
            // The snapshot has no `disabled` projection for a plain div, and
            // gpui-pre exposes no aria-disabled setter, so the disabled state
            // is asserted below through the activation it must refuse. The
            // same trade replaces `supports_action(Click)`.
            (first.bounds(), unavailable.bounds())
        });
        cx.simulate_click(unavailable.center(), Modifiers::default());
        cx.read_entity(&view, |view, _| {
            assert_eq!(
                view.clicks,
                Vec::<&str>::new(),
                "a disabled button must not activate"
            );
        });
        cx.simulate_click(first.center(), Modifiers::default());
        cx.read_entity(&view, |view, _| assert_eq!(view.clicks, ["first"]));
    }

    #[gpui::test]
    fn loading_retains_focus_but_never_submits(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("tab", Next, None),
                KeyBinding::new("shift-tab", Previous, None),
            ]);
        });
        let _initial_focus = cx.update(install_initial_tab_focus);
        let (view, cx) = cx.add_window_view(|_, _| Harness::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        for key in ["tab", "tab", "tab", "enter", "space"] {
            press(cx, key);
        }
        cx.simulate_click(point(px(175.), px(25.)), Modifiers::none());
        let loading = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let loading = window
                .find_all_by_label("Loading")
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .expect("missing accessible Loading button");
            assert!(
                !loading.disabled().unwrap_or(false),
                "loading must retain the keyboard focus path"
            );
            // `A11yElement::busy` is not projected into `ElementSnapshot`, so
            // the busy flag itself is not assertable here. The activation
            // check below covers the behaviour it exists to advertise.
            loading.bounds()
        });
        cx.simulate_click(loading.center(), Modifiers::default());
        cx.read_entity(&view, |view, _| {
            assert_eq!(view.clicks, Vec::<&str>::new());
        });
    }

    #[gpui::test]
    fn icon_buttons_keep_names_and_loading_blocks_activation(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("tab", Next, None),
                KeyBinding::new("shift-tab", Previous, None),
            ]);
        });
        let _initial_focus = cx.update(install_initial_tab_focus);
        let (view, cx) = cx.add_window_view(|_, _| Harness::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        for key in
            ["tab", "tab", "tab", "tab", "enter", "tab", "enter", "space"]
        {
            press(cx, key);
        }
        cx.simulate_click(point(px(70.), px(70.)), Modifiers::none());
        cx.simulate_click(point(px(120.), px(70.)), Modifiers::none());
        let loading = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            for label in ["Icon action", "Disabled icon", "Loading icon"] {
                let node = window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| {
                        node.role() == Some(accesskit::Role::Button)
                            && node.visible()
                    })
                    .unwrap_or_else(|| {
                        panic!("missing accessible button {label:?}")
                    });
                // Neither the disabled flag nor the busy flag is projected into
                // `ElementSnapshot` for a div-based control, and gpui-pre has
                // no aria-disabled setter. The activation outcomes below carry
                // both guarantees: the disabled and the loading icon button
                // must both refuse the click.
                let _ = node;
            }
            window
                .find_all_by_label("Loading icon")
                .into_iter()
                .find(|node| {
                    node.role() == Some(accesskit::Role::Button)
                        && node.visible()
                })
                .expect("missing accessible Loading icon button")
                .bounds()
        });
        cx.simulate_click(loading.center(), Modifiers::default());
        cx.read_entity(&view, |view, _| assert_eq!(view.clicks, ["icon"]));
    }
}
