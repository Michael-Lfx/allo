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
    E: gpui::StatefulInteractiveElement + gpui::Element + 'static,
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
        let keyboard_focus =
            focus.is_focused(window) && window.last_input_was_keyboard();
        (state.hovered || keyboard_focus) && !state.dismissed
    };
    let trigger = crate::anchor_trigger(
        trigger
            .track_focus(&focus)
            .tab_stop(true)
            .aria_description(text.clone())
            .on_hover(move |value, window, cx| {
                hovered.update(cx, |state, cx| {
                    let keyboard_focus =
                        state.focused && window.last_input_was_keyboard();
                    if *value && !state.hovered && !keyboard_focus {
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
        wrapper = wrapper.child(super::overlays::information_popover(
            SharedString::from(format!("{id}-layer")),
            anchor,
            crate::tooltip(
                SharedString::from(format!("{id}-tip")),
                text,
                &theme,
            ),
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
    use gpui_kit::test::TestQueryExt;

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
            // `AnyElement` carries no `Styled` in gpui-pre, so the absolute
            // placement wraps the trigger instead of being chained onto it.
            let trigger = div()
                .id("help-anchor")
                .absolute()
                .left(gpui::px(80.))
                .top(gpui::px(80.))
                .child(
                    crate::Button::new("help", "Help")
                        .render(&theme, |_, _, _| {}),
                );
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

    fn tooltip_visible(cx: &mut gpui::VisualTestContext) -> bool {
        cx.update(Window::simulate_next_frame);
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window
                .find_all_by_role(Role::Tooltip)
                .iter()
                .any(|node| node.visible())
        })
    }

    #[gpui::test]
    fn pointer_leaving_the_trigger_dismisses_tooltip_after_click(
        cx: &mut TestAppContext,
    ) {
        let (_, cx) = cx.add_window_view(|_, cx| Harness {
            ui: UiContext::new(crate::ThemeOptions::default(), false, cx),
        });
        assert!(!tooltip_visible(cx));
        let trigger = gpui::point(gpui::px(94.), gpui::px(94.));
        let outside = gpui::point(gpui::px(40.), gpui::px(40.));
        let modifiers = gpui::Modifiers::default();
        cx.simulate_mouse_move(trigger, None, modifiers);
        assert!(tooltip_visible(cx));
        cx.simulate_mouse_move(outside, None, modifiers);
        assert!(!tooltip_visible(cx));

        cx.simulate_mouse_move(trigger, None, modifiers);
        cx.simulate_mouse_down(trigger, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_up(trigger, gpui::MouseButton::Left, modifiers);
        assert!(tooltip_visible(cx));
        cx.simulate_mouse_move(outside, None, modifiers);
        assert!(!tooltip_visible(cx), "a click must not pin the tooltip");
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
        assert!(!tooltip_visible(cx));
        let press = |cx: &mut gpui::VisualTestContext, key: &'static str| {
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

    #[gpui::test]
    fn clicking_the_trigger_fires_while_the_tooltip_is_open(
        cx: &mut TestAppContext,
    ) {
        use std::cell::Cell;
        use std::rc::Rc;

        struct ClickHarness {
            ui: UiContext,
            clicks: Rc<Cell<usize>>,
        }

        impl Render for ClickHarness {
            fn render(
                &mut self,
                window: &mut Window,
                cx: &mut Context<'_, Self>,
            ) -> impl IntoElement {
                self.ui
                    .overlays
                    .update(cx, |host, _| host.begin_frame(window));
                let theme = self.ui.snapshot(cx);
                let clicks = self.clicks.clone();
                let trigger = div()
                    .id("help-anchor")
                    .absolute()
                    .left(gpui::px(80.))
                    .top(gpui::px(80.))
                    .child(crate::Button::new("help", "Help").render(
                        &theme,
                        move |_, _, _| {
                            clicks.set(clicks.get() + 1);
                        },
                    ));
                div().id("root").size_full().child(tooltip_trigger(
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

        let clicks = Rc::new(Cell::new(0));
        let clicks_for_view = clicks.clone();
        let (_, cx) = cx.add_window_view(move |_, cx| ClickHarness {
            ui: UiContext::new(crate::ThemeOptions::default(), false, cx),
            clicks: clicks_for_view,
        });
        let trigger = gpui::point(gpui::px(94.), gpui::px(94.));
        let modifiers = gpui::Modifiers::default();
        cx.simulate_mouse_move(trigger, None, modifiers);
        assert!(tooltip_visible(cx));
        cx.simulate_mouse_down(trigger, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_up(trigger, gpui::MouseButton::Left, modifiers);
        assert_eq!(clicks.get(), 1);
    }
}
