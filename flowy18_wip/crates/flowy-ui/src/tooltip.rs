//! Information-only tooltips opened by either pointer hover or keyboard focus.
use crate::UiContext;
use gpui::{
    App, Div, FocusHandle, SharedString, Stateful, Subscription, Window, div,
    prelude::*,
};

struct TooltipState {
    focus: FocusHandle,
    hovered: bool,
    focused: bool,
    dismissed: bool,
    subscriptions: Vec<Subscription>,
}

/// Wrap a stable, already labelled trigger. The tooltip never takes focus and
/// its text also supplies a persistent accessible description to the trigger.
pub fn tooltip_trigger<E>(
    id: impl Into<SharedString>,
    trigger: E,
    text: impl Into<SharedString>,
    ui: &UiContext,
    window: &mut Window,
    cx: &mut App,
    rtl: bool,
) -> Stateful<Div>
where
    E: gpui::StatefulInteractiveElement + IntoElement + 'static,
{
    let theme = ui.snapshot(cx);
    let id = id.into();
    let state = window.use_keyed_state(id.clone(), cx, |_, cx| TooltipState {
        focus: cx.focus_handle().tab_stop(true),
        hovered: false,
        focused: false,
        dismissed: false,
        subscriptions: Vec::new(),
    });
    let focus = state.read(cx).focus.clone();
    if state.read(cx).subscriptions.is_empty() {
        let focus_in = state.downgrade();
        let focus_out = state.downgrade();
        let entered = window.on_focus_in(&focus, cx, move |window, cx| {
            let _ = focus_in.update(cx, |state, cx| {
                if !state.focused && !state.hovered {
                    state.dismissed = false;
                }
                state.focused = true;
                cx.notify();
            });
            window.refresh();
        });
        let left = window.on_focus_out(&focus, cx, move |_, window, cx| {
            let _ = focus_out.update(cx, |state, cx| {
                state.focused = false;
                if !state.hovered {
                    state.dismissed = false;
                }
                cx.notify();
            });
            window.refresh();
        });
        state.update(cx, |state, _| state.subscriptions = vec![entered, left]);
    }
    let text = text.into();
    let hovered = state.clone();
    let dismissed = state.clone();
    let anchor = SharedString::from(format!("{id}-anchor"));
    let shown = {
        let state = state.read(cx);
        (state.hovered || focus.is_focused(window)) && !state.dismissed
    };
    let trigger = crate::anchor_trigger(
        trigger
            .track_focus(&focus)
            .tab_stop(true)
            .aria_description(text.clone())
            .on_hover(move |value, window, cx| {
                hovered.update(cx, |state, cx| {
                    if *value && !state.hovered && !state.focused {
                        state.dismissed = false;
                    }
                    state.hovered = *value;
                    cx.notify();
                });
                window.refresh();
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "escape" {
                    dismissed.update(cx, |state, cx| {
                        state.dismissed = true;
                        cx.notify();
                    });
                    window.refresh();
                    cx.stop_propagation();
                }
            }),
        anchor.clone(),
        ui,
    );
    let mut wrapper = div().id(id.clone()).child(trigger);
    if shown {
        let dismiss_state = state.downgrade();
        ui.overlays.update(cx, |host, _| {
            host.register_tooltip(std::rc::Rc::new(move |window, cx| {
                let _ = dismiss_state.update(cx, |state, cx| {
                    state.dismissed = true;
                    cx.notify();
                });
                window.refresh();
            }));
        });
        wrapper = wrapper.child(crate::popover(
            id.clone(),
            anchor,
            crate::tooltip(id, text, &theme),
            ui,
            window,
            cx,
            rtl,
        ));
    }
    wrapper
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, Role, TestAppContext};

    // GPUI's actions macro emits PartialEq without Eq.
    #[allow(clippy::derive_partial_eq_without_eq)]
    mod bindings {
        gpui::actions!(tooltip_test, [Next]);
    }

    struct Harness {
        ui: UiContext,
    }

    impl Render for Harness {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            self.ui
                .overlays
                .update(cx, |host, _| host.begin_frame(window));
            let theme = self.ui.snapshot(cx);
            let trigger = crate::Button::new("help", "Help")
                .render(&theme, |_, _, _| {})
                .absolute()
                .left(gpui::px(80.))
                .top(gpui::px(80.));
            div()
                .id("root")
                .size_full()
                .on_action(|_: &bindings::Next, window, cx| {
                    window.focus_next(cx);
                })
                .child(tooltip_trigger(
                    "help-tooltip",
                    trigger,
                    "Helpful information",
                    &self.ui,
                    window,
                    cx,
                    false,
                ))
        }
    }

    #[gpui::test]
    fn keyboard_focus_reveals_tooltip_and_escape_dismisses_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.bind_keys([gpui::KeyBinding::new("tab", bindings::Next, None)]);
        });
        let _initial_focus = cx.update(crate::install_initial_tab_focus);
        let (_, cx) = cx.add_window_view(|_, cx| Harness {
            ui: UiContext::new(crate::ThemeOptions::default(), false, cx),
        });
        let tooltip_visible = |cx: &mut gpui::VisualTestContext| {
            cx.update(Window::simulate_next_frame);
            cx.update(|window, cx| {
                window.set_a11y_forced(true);
                window.draw(cx).clear(cx);
                window
                    .a11y_tree()
                    .unwrap()
                    .nodes
                    .iter()
                    .any(|(_, node)| node.role() == Role::Tooltip)
            })
        };
        assert!(!tooltip_visible(cx));
        let press = |cx: &mut gpui::VisualTestContext, key: &str| {
            let keystroke = gpui::Keystroke::parse(key).unwrap();
            cx.simulate_event(gpui::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(gpui::KeyUpEvent { keystroke });
        };
        press(cx, "tab");
        assert!(tooltip_visible(cx));
        press(cx, "escape");
        assert!(!tooltip_visible(cx));
    }
}
