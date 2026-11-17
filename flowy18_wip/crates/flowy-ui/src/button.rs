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
    } else {
        interactive(id, label, theme).on_click(on_click)
    }
}

/// Standard visual and behavior recipe for buttons. Loading retains the tab stop.
#[derive(Debug)]
#[must_use]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ControlVariant,
    size: ControlSize,
    disabled: bool,
    loading: bool,
}
impl Button {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ControlVariant::Primary,
            size: ControlSize::Medium,
            disabled: false,
            loading: false,
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
    pub fn render(
        self,
        theme: &ResolvedTheme,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> crate::A11yElement<Div> {
        let loading_id: ElementId = (self.id.clone(), "loading").into();
        let colors = theme.control_colors(self.variant);
        let loading = self.loading;
        let label = self.label.clone();
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
        .min_h(theme.control_height(self.size))
        .px(theme.control_padding(self.size))
        .py(px(4.))
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(colors.background)
        .text_color(colors.foreground)
        .text_sm()
        .when(self.disabled, |el| el.opacity(0.5))
        .when(!self.disabled, move |el| {
            el.hover(move |s| s.bg(colors.hover))
        })
        .when(loading, |el| {
            el.child(
                div()
                    .id(loading_id)
                    .aria_hidden()
                    .child(crate::icons::loader_circle()),
            )
        })
        .child(label);
        crate::A11yElement::new(element.into_element()).busy(loading)
    }
}

#[must_use]
pub struct IconButton {
    button: Button,
    icon: AnyElement,
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
        }
    }
    pub fn size(mut self, value: ControlSize) -> Self {
        self.button = self.button.size(value);
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
    ) -> Stateful<Div> {
        let size = self.button.size;
        let icon_id: ElementId = (self.button.id.clone(), "icon").into();
        let label = self.button.label.clone();
        let colors = theme.control_colors(self.button.variant);
        button(self.button.id, label, self.button.disabled, theme, on_click)
            .min_w(theme.control_height(size))
            .min_h(theme.control_height(size))
            .rounded_lg()
            .flex()
            .items_center()
            .justify_center()
            .bg(colors.background)
            .text_color(colors.foreground)
            .when(self.button.disabled, |el| el.opacity(0.5))
            .when(!self.button.disabled, move |el| {
                el.hover(move |style| style.bg(colors.hover))
            })
            .child(div().id(icon_id).aria_hidden().child(self.icon))
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
}
