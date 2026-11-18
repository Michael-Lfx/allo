//! Interactive primitives. IDs preserve GPUI focus state across frames.
use crate::{ControlSize, ControlVariant, ResolvedTheme};
use gpui::{
    AnyElement, App, ClickEvent, ColorExt, Div, ElementId, Role, SharedString,
    Stateful, Window, div, prelude::*, px,
};

/// Semantic, keyboard-accessible interaction surface for custom content.
/// Keep its id stable and provide a localized accessible name.
pub fn interactive(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
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

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    theme: &ResolvedTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();
    if disabled {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .aria_disabled(true)
            .font(crate::ui_font())
    } else {
        interactive(id, label, theme).on_click(on_click)
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
    pub const fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    pub fn render(
        self,
        theme: &ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> crate::A11yElement<Div> {
        let loading_id: ElementId = (self.id.clone(), "loading").into();
        let icon_id: ElementId = (self.id.clone(), "icon").into();
        let colors = theme.control_colors(self.variant);
        let loading = self.loading;
        let label = self.label.clone();
        let leading = if loading {
            Some(crate::icons::loader_circle().into_any_element())
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
        .border_2()
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
        .when(self.disabled, |el| el.opacity(0.5))
        .when(!self.disabled && !loading, move |el| {
            el.hover(move |s| s.bg(colors.hover))
        })
        .when(loading, Styled::cursor_default)
        .children(
            leading.map(|icon| div().id(leading_id).aria_hidden().child(icon)),
        )
        .child(label);
        crate::A11yElement::new(element.into_element()).busy(loading)
    }
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
        }
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
    pub fn render(
        self,
        theme: &ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> crate::A11yElement<Div> {
        let size = self.button.size;
        let icon_id: ElementId = (self.button.id.clone(), "icon").into();
        let label = self.button.label.clone();
        let colors = theme.control_colors(self.button.variant);
        let loading = self.button.loading;
        let leading = if loading {
            crate::icons::loader_circle().into_any_element()
        } else {
            self.icon
        };
        let element = button(
            self.button.id,
            label,
            self.button.disabled,
            theme,
            move |event, window, cx| {
                if !loading {
                    on_click(event, window, cx);
                }
            },
        )
        .border_2()
        .border_color(colors.border)
        .min_w(theme.control_height(size))
        .min_h(theme.control_height(size))
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.background)
        .text_color(colors.foreground)
        .when(self.button.disabled, |el| el.opacity(0.5))
        .when(!self.button.disabled && !loading, move |el| {
            el.hover(move |style| style.bg(colors.hover))
        })
        .when(loading, Styled::cursor_default)
        .when(self.framed, |element| {
            let side = theme.control_height(size);
            element
                .border_1()
                .border_color(theme.chrome_line())
                .bg(theme.card())
                .text_color(theme.toolbar_icon())
                .rounded(px(10.))
                .w(side)
                .h(side)
                .min_w(side)
                .min_h(side)
        })
        .child(div().id(icon_id).aria_hidden().child(leading));
        crate::A11yElement::new(element.into_element()).busy(loading)
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

    pub(super) fn press(cx: &mut VisualTestContext, key: &str) {
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
                    Button::new("loading", "Loading")
                        .loading(true)
                        .render(
                            &crate::ResolvedTheme::default(),
                            cx.listener(|this, _, _, _| {
                                this.clicks.push("loading");
                            }),
                        )
                        .absolute()
                        .left(px(160.))
                        .top(px(10.))
                        .size(px(40.)),
                )
                .child(
                    IconButton::new(
                        "icon-action",
                        "Icon action",
                        crate::icons::check(),
                    )
                    .variant(ControlVariant::Primary)
                    .render(
                        &crate::ResolvedTheme::default(),
                        cx.listener(|this, _, _, _| this.clicks.push("icon")),
                    )
                    .absolute()
                    .left(px(10.))
                    .top(px(60.))
                    .size(px(40.)),
                )
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
                    )
                    .absolute()
                    .left(px(60.))
                    .top(px(60.))
                    .size(px(40.)),
                )
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
                    )
                    .absolute()
                    .left(px(110.))
                    .top(px(60.))
                    .size(px(40.)),
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
        let tree = cx.update(|window, cx| {
            window.set_a11y_forced(true);
            window.draw(cx).clear(cx);
            window.a11y_tree().unwrap().clone()
        });
        let first = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("First"))
            .unwrap();
        assert_eq!(first.1.role(), accesskit::Role::Button);
        assert!(first.1.supports_action(accesskit::Action::Click));
        assert!(!first.1.is_disabled());
        let disabled = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Unavailable"))
            .unwrap();
        assert_eq!(disabled.1.role(), accesskit::Role::Button);
        assert!(disabled.1.is_disabled());
        assert!(!disabled.1.supports_action(accesskit::Action::Click));
        let first_id = first.0;
        cx.update(|window, cx| {
            window.perform_a11y_action(
                first_id,
                accesskit::Action::Click,
                None,
                cx,
            );
        });
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
        let tree = cx.update(|window, cx| {
            window.set_a11y_forced(true);
            window.draw(cx).clear(cx);
            window.a11y_tree().unwrap().clone()
        });
        let (id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Loading"))
            .unwrap();
        assert!(
            !node.is_disabled(),
            "loading must retain the keyboard focus path"
        );
        assert!(node.is_busy());
        cx.update(|window, cx| {
            window.perform_a11y_action(*id, accesskit::Action::Click, None, cx);
        });
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
        let tree = cx.update(|window, cx| {
            window.set_a11y_forced(true);
            window.draw(cx).clear(cx);
            window.a11y_tree().unwrap().clone()
        });
        for label in ["Icon action", "Disabled icon", "Loading icon"] {
            let (_, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap();
            assert_eq!(node.role(), accesskit::Role::Button);
            assert_eq!(node.is_disabled(), label == "Disabled icon");
            assert_eq!(node.is_busy(), label == "Loading icon");
        }
        let (id, _) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Loading icon"))
            .unwrap();
        cx.update(|window, cx| {
            window.perform_a11y_action(*id, accesskit::Action::Click, None, cx);
        });
        cx.read_entity(&view, |view, _| assert_eq!(view.clicks, ["icon"]));
    }
}
