use gpui::{
    App, ClickEvent, Div, ElementId, Role, SharedString, Stateful, Window, div,
    prelude::*,
};

/// A button with keyboard activation, focus indication and an accessible name.
///
/// Supply a localized label even for icon-only buttons. Callers provide the
/// content and visual styles, and must assign a stable id within the window.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_disabled(disabled)
        .when(!disabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .focus_visible(|style| {
                    style.border_2().border_color(crate::accent())
                })
                .on_click(on_click)
        })
}

/// Lets an initial Tab enter the window's focus path. Once an element has
/// focus, ordinary action dispatch handles traversal and text input.
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
        Modifiers, Render, TestAppContext, VisualTestContext, accesskit,
        actions, point, px,
    };

    actions!(button_test, [Next, Previous]);

    fn press(cx: &mut VisualTestContext, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
    }

    #[derive(Default)]
    struct Harness {
        clicks: Vec<&'static str>,
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
                        cx.listener(|this, _, _, _| {
                            this.clicks.push("second");
                        }),
                    )
                    .absolute()
                    .left(px(110.))
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
            ])
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
}
