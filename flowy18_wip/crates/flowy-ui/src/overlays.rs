//! Window-scoped overlay presentation and focus lifetime.
//!
//! The host stores presentation metadata only. Its close events are requests:
//! the application owns the business state and confirms closure by rendering
//! the next scope (or `None`). Popovers use measured window coordinates.

use crate::{ResolvedTheme, UiContext};
use gpui::{
    Anchor, AnyElement, App, Bounds, Context, Div, ElementId, Entity,
    EventEmitter, FocusHandle, InputPreference, IntoElement, KeyDownEvent,
    KeystrokeEvent, ParentElement, Pixels, Role, SharedString, Stateful,
    Subscription, WeakEntity, WeakFocusHandle, Window, WindowId, anchored,
    deferred, div, point, prelude::*, px,
};
use std::collections::HashMap;
use std::rc::Rc;

type TooltipDismiss = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Escape,
    Outside,
    AnchorRemoved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    CloseRequested {
        scope: SharedString,
        reason: CloseReason,
    },
}

#[derive(Debug)]
struct FocusScope {
    id: SharedString,
    root: FocusHandle,
    restore: Option<WeakFocusHandle>,
}

#[derive(Clone, Copy, Debug)]
struct MeasuredAnchor {
    bounds: Bounds<Pixels>,
    frame: u64,
}

/// One instance belongs to one window through [`UiContext`].
pub struct OverlayHost {
    window: Option<WindowId>,
    modal: Option<FocusScope>,
    popup: Option<FocusScope>,
    fallback: Option<WeakFocusHandle>,
    anchors: HashMap<SharedString, MeasuredAnchor>,
    frame: u64,
    key_subscription: Option<Subscription>,
    tooltip_dismiss: Option<(u64, TooltipDismiss)>,
}

impl std::fmt::Debug for OverlayHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayHost")
            .field("window", &self.window)
            .field("modal", &self.modal)
            .field("popup", &self.popup)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<OverlayEvent> for OverlayHost {}

impl OverlayHost {
    pub fn new(_: &mut Context<'_, Self>) -> Self {
        Self {
            window: None,
            modal: None,
            popup: None,
            fallback: None,
            anchors: HashMap::new(),
            frame: 0,
            key_subscription: None,
            tooltip_dismiss: None,
        }
    }

    pub fn install(entity: &Entity<Self>, cx: &mut App) {
        let weak = entity.downgrade();
        let subscription = cx.intercept_keystrokes(move |event, window, cx| {
            let _ =
                weak.update(cx, |host, cx| host.intercept(event, window, cx));
        });
        entity.update(cx, |host, _| host.key_subscription = Some(subscription));
    }

    /// Call once at the window root before rendering triggers and floating
    /// surfaces. A one-frame grace period lets an opening surface use the
    /// trigger's last measurement before this frame has been prepainted.
    pub fn begin_frame(&mut self, window: &Window) {
        self.window = Some(window.window_handle().window_id());
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        self.anchors
            .retain(|_, anchor| frame.wrapping_sub(anchor.frame) <= 2);
        self.tooltip_dismiss = None;
    }

    /// A visible information tooltip consumes Escape before its parent scope.
    pub(crate) fn register_tooltip(&mut self, dismiss: TooltipDismiss) {
        self.tooltip_dismiss = Some((self.frame, dismiss));
    }

    pub fn set_fallback_focus(&mut self, focus: &FocusHandle) {
        self.fallback = Some(focus.downgrade());
    }

    /// Synchronizes confirmed business state; it never closes a dialog by
    /// itself. Opening a different modal discards a child popup.
    pub fn set_modal(
        &mut self,
        scope: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.window = Some(window.window_handle().window_id());
        if self.modal.as_ref().map(|s| &s.id) == scope.as_ref() {
            return;
        }
        self.popup = None;
        let old = self.modal.take();
        if let Some(id) = scope {
            let restore = old
                .and_then(|old| old.restore)
                .or_else(|| window.focused(cx).map(|focus| focus.downgrade()));
            let root = cx.focus_handle().tab_stop(true);
            self.modal = Some(FocusScope {
                id,
                root: root.clone(),
                restore,
            });
            focus_after_paint(root, cx.weak_entity(), window, cx);
        } else if let Some(old) = old {
            restore_after_paint(
                old.restore.or_else(|| self.fallback.clone()),
                cx.weak_entity(),
                self.active_scope(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// Synchronizes a focus-taking menu/popover. Information-only hover cards
    /// need only [`popover`] and should leave this scope empty.
    pub fn set_popup(
        &mut self,
        scope: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.window = Some(window.window_handle().window_id());
        if self.popup.as_ref().map(|s| &s.id) == scope.as_ref() {
            return;
        }
        let old = self.popup.take();
        if let Some(id) = scope {
            let restore = old
                .and_then(|old| old.restore)
                .or_else(|| window.focused(cx).map(|focus| focus.downgrade()));
            let root = cx.focus_handle().tab_stop(true);
            self.popup = Some(FocusScope {
                id,
                root: root.clone(),
                restore,
            });
            focus_after_paint(root, cx.weak_entity(), window, cx);
        } else if let Some(old) = old {
            restore_after_paint(
                old.restore
                    .or_else(|| self.modal.as_ref().map(|s| s.root.downgrade()))
                    .or_else(|| self.fallback.clone()),
                cx.weak_entity(),
                self.active_scope(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    pub fn modal_focus(&self) -> Option<FocusHandle> {
        self.modal.as_ref().map(|scope| scope.root.clone())
    }

    pub fn popup_focus(&self) -> Option<FocusHandle> {
        self.popup.as_ref().map(|scope| scope.root.clone())
    }

    pub const fn modal_active(&self) -> bool {
        self.modal.is_some()
    }

    pub fn active_scope(&self) -> Option<SharedString> {
        self.popup
            .as_ref()
            .or(self.modal.as_ref())
            .map(|scope| scope.id.clone())
    }

    pub fn request_close(
        &self,
        reason: CloseReason,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(scope) = self.active_scope() {
            cx.emit(OverlayEvent::CloseRequested { scope, reason });
        }
    }

    fn validate_anchor(
        &self,
        scope: SharedString,
        anchor: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if self
            .anchors
            .get(anchor)
            .is_none_or(|anchor| anchor.frame != self.frame)
        {
            cx.emit(OverlayEvent::CloseRequested {
                scope,
                reason: CloseReason::AnchorRemoved,
            });
        }
    }

    pub fn anchor_bounds(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.anchors.get(id).map(|anchor| anchor.bounds)
    }

    fn measure_anchor(
        &mut self,
        id: SharedString,
        bounds: Bounds<Pixels>,
        cx: &mut Context<'_, Self>,
    ) {
        let changed = self
            .anchors
            .get(&id)
            .is_none_or(|anchor| anchor.bounds != bounds);
        self.anchors.insert(
            id,
            MeasuredAnchor {
                bounds,
                frame: self.frame,
            },
        );
        if changed {
            cx.notify();
        }
    }

    fn intercept(
        &self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.window != Some(window.window_handle().window_id()) {
            return;
        }
        // The platform's character preference includes IME/AltGr input. Never
        // turn it into a dismiss or navigation shortcut.
        if event.input_preference == InputPreference::CharacterInput {
            return;
        }
        let modifiers = &event.keystroke.modifiers;
        if modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return;
        }
        if event.keystroke.key == "escape"
            && let Some((frame, dismiss)) = &self.tooltip_dismiss
            && *frame == self.frame
        {
            dismiss(window, cx);
            cx.stop_propagation();
            return;
        }
        let Some(scope) = self.popup.as_ref().or(self.modal.as_ref()) else {
            return;
        };
        match event.keystroke.key.as_str() {
            "escape" => {
                self.request_close(CloseReason::Escape, cx);
                cx.stop_propagation();
            }
            "tab" => {
                cycle_within(&scope.root, modifiers.shift, window, cx);
                cx.stop_propagation();
            }
            _ => {
                // A just-opened overlay may not yet have a rendered focus
                // path. Consume that key instead of activating the old view.
                if !scope.root.contains_focused(window, cx) {
                    scope.root.focus(window, cx);
                    cx.stop_propagation();
                }
            }
        }
    }
}

fn focus_after_paint(
    root: FocusHandle,
    host: WeakEntity<OverlayHost>,
    window: &Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        let Some(entity) = host.upgrade() else {
            return;
        };
        let state = entity.read(cx);
        let active = state.popup_focus().or_else(|| state.modal_focus());
        if active.as_ref() != Some(&root) {
            return;
        }
        root.focus(window, cx);
        window.on_next_frame(move |window, cx| {
            let Some(entity) = host.upgrade() else {
                return;
            };
            let state = entity.read(cx);
            let active = state.popup_focus().or_else(|| state.modal_focus());
            if active.as_ref() == Some(&root) {
                cycle_within(&root, false, window, cx);
            }
        });
    });
}

fn restore_after_paint(
    restore: Option<WeakFocusHandle>,
    host: WeakEntity<OverlayHost>,
    expected: Option<SharedString>,
    window: &Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        let Some(entity) = host.upgrade() else {
            return;
        };
        if entity.read(cx).active_scope() != expected {
            return;
        }
        if let Some(focus) = restore.and_then(|focus| focus.upgrade()) {
            focus.focus(window, cx);
        } else {
            window.focus_next(cx);
        }
    });
}

/// Focus traversal is derived from GPUI's last painted tab stops, which also
/// includes native editable text. The container remains a safe fallback when
/// every descendant is disabled. Only the final focus receives key dispatch.
fn cycle_within(
    root: &FocusHandle,
    reverse: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if !root.contains_focused(window, cx) {
        root.focus(window, cx);
    }
    let origin = window.focused(cx);
    let mut visited = Vec::new();
    loop {
        if reverse {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        let Some(focused) = window.focused(cx) else {
            break;
        };
        if visited.contains(&focused) {
            break;
        }
        visited.push(focused.clone());
        if focused != *root && root.contains_focused(window, cx) {
            return;
        }
        if origin.as_ref() == Some(&focused) && focused != *root {
            break;
        }
    }
    root.focus(window, cx);
}

/// Full-window modal presentation: scrim, blur, centering and outside-click
/// dismissal. The dialog panel owns focus and stops events from reaching this
/// layer. Business state remains in the application.
pub fn modal_layer(
    id: impl Into<ElementId>,
    theme: &ResolvedTheme,
    ui: &UiContext,
    content: impl IntoElement,
) -> Stateful<Div> {
    let host = ui.overlays.downgrade();
    div()
        .id(id)
        .occlude()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.scrim())
        .backdrop_blur(theme.scrim_blur())
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |_: &gpui::MouseDownEvent, _: &mut Window, cx: &mut App| {
                let _ = host.update(cx, |host, cx| {
                    host.request_close(CloseReason::Outside, cx);
                });
                cx.stop_propagation();
            },
        )
        .child(content)
}

/// Base panel; callers supply header, scroll body and actions as children.
pub fn dialog(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    theme: &ResolvedTheme,
    ui: &UiContext,
    cx: &App,
) -> Stateful<Div> {
    let focus = ui.overlays.read(cx).modal_focus();
    div()
        .id(id)
        .role(Role::Dialog)
        .aria_label(label)
        .aria_modal(true)
        .key_context("FlowyDialog")
        .when_some(focus, |panel, focus| {
            panel.track_focus(&focus).tab_stop(true)
        })
        .relative()
        .bg(theme.card())
        .text_color(theme.ink())
        .rounded(theme.modal_radius())
        .occlude()
        .on_mouse_down(
            gpui::MouseButton::Left,
            |_: &gpui::MouseDownEvent, _: &mut Window, cx: &mut App| {
                cx.stop_propagation();
            },
        )
}

/// Records the trigger's actual bounds after layout, including scrolling.
pub fn anchor_trigger<E>(
    element: E,
    anchor_id: impl Into<SharedString>,
    ui: &UiContext,
) -> E
where
    E: gpui::StatefulInteractiveElement + IntoElement,
{
    let id = anchor_id.into();
    let host = ui.overlays.downgrade();
    element.on_prepaint(move |paint, _, cx| {
        let _ = host.update(cx, |host, cx| {
            host.measure_anchor(id.clone(), paint.bounds, cx);
        });
    })
}

/// Draws in the deferred overlay layer, anchored to a measured trigger.
/// Focus-taking callers synchronize `set_popup`; hover cards do not.
pub fn popover(
    id: impl Into<SharedString>,
    anchor_id: impl Into<SharedString>,
    content: impl IntoElement,
    ui: &UiContext,
    window: &Window,
    cx: &App,
    rtl: bool,
) -> AnyElement {
    let id = id.into();
    let anchor_id = anchor_id.into();
    let host = ui.overlays.read(cx);
    let Some(bounds) = host.anchor_bounds(anchor_id.as_ref()) else {
        let weak = ui.overlays.downgrade();
        return div()
            .id(id.clone())
            .on_prepaint(move |_, _, cx| {
                let _ = weak.update(cx, |host, cx| {
                    host.validate_anchor(id.clone(), anchor_id.as_ref(), cx);
                });
            })
            .into_any_element();
    };
    let focus = host
        .popup
        .as_ref()
        .filter(|scope| scope.id == id)
        .map(|scope| scope.root.clone());
    let position = if rtl {
        point(bounds.right(), bounds.bottom())
    } else {
        point(bounds.left(), bounds.bottom())
    };
    let weak = ui.overlays.downgrade();
    let outside_host = ui.overlays.downgrade();
    let scope = id.clone();
    let outside_scope = id.clone();
    let panel = div()
        .id(id)
        .occlude()
        .key_context("FlowyPopover")
        .when_some(focus, |panel, focus| {
            panel.track_focus(&focus).tab_stop(true)
        })
        .max_h((window.viewport_size().height - px(32.)).max(px(48.)))
        .overflow_y_scroll()
        .child(content)
        .on_prepaint(move |_, _, cx| {
            let _ = weak.update(cx, |host, cx| {
                host.validate_anchor(scope.clone(), anchor_id.as_ref(), cx);
            });
        })
        .on_mouse_down_out(move |event, _, cx| {
            // Let the trigger perform its normal toggle rather than closing
            // on mouse-down and reopening on its following click.
            if bounds.contains(&event.position) {
                return;
            }
            let _ = outside_host.update(cx, |_, cx| {
                cx.emit(OverlayEvent::CloseRequested {
                    scope: outside_scope.clone(),
                    reason: CloseReason::Outside,
                });
            });
            cx.stop_propagation();
        });
    deferred(
        anchored()
            .anchor(if rtl {
                Anchor::TopRight
            } else {
                Anchor::TopLeft
            })
            .position(position)
            .offset(point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(panel),
    )
    .into_any_element()
}

pub fn menu(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    theme: &ResolvedTheme,
    ui: &UiContext,
    cx: &App,
) -> Stateful<Div> {
    let root = ui.overlays.read(cx).popup_focus();
    div()
        .id(id)
        .role(Role::Menu)
        .aria_label(label)
        .bg(theme.card())
        .text_color(theme.ink())
        .font(crate::ui_font())
        .border_1()
        .border_color(theme.line())
        .rounded_lg()
        .shadow_md()
        .p_1()
        .flex()
        .flex_col()
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let Some(root) = root.as_ref() else {
                return;
            };
            match event.keystroke.key.as_str() {
                "down" | "up" => {
                    cycle_within(root, event.keystroke.key == "up", window, cx);
                    cx.stop_propagation();
                }
                "home" | "end" => {
                    root.focus(window, cx);
                    cycle_within(
                        root,
                        event.keystroke.key == "end",
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }
                _ => {}
            }
        })
}

pub fn select_trigger(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    expanded: bool,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    crate::interactive(id, label, theme).aria_expanded(expanded)
}

pub fn menu_item(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    theme: &ResolvedTheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label = label.into();
    // `button` only stores the label as the accessible name; menu rows must
    // also render it, otherwise a popover shows blank rows with just hover and
    // selection affordances. Colors come from the resolved theme so the row
    // stays readable in both light and dark mode.
    crate::button(id, label.clone(), disabled, theme, on_click)
        .role(Role::MenuItem)
        .text_color(if disabled { theme.muted() } else { theme.ink() })
        .child(label)
}

pub fn select_option(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    crate::interactive(id, label, theme)
        .role(Role::ListBoxOption)
        .aria_selected(selected)
}

pub fn tooltip(
    id: impl Into<gpui::ElementId>,
    text: impl Into<SharedString>,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Tooltip)
        .bg(theme.card())
        .text_color(theme.ink())
        .border_1()
        .border_color(theme.line())
        .rounded_md()
        .px_2()
        .py_1()
        .text_sm()
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        KeyUpEvent, Keystroke, Modifiers, Render, TestAppContext,
        VisualTestContext,
    };

    struct Harness {
        host: Entity<OverlayHost>,
        background: FocusHandle,
        first: FocusHandle,
        second: FocusHandle,
        open: bool,
        popup: bool,
        _host_sub: Subscription,
    }

    impl Harness {
        fn new(cx: &mut Context<'_, Self>) -> Self {
            let host = cx.new(OverlayHost::new);
            OverlayHost::install(&host, cx);
            let host_sub = cx.observe(&host, |_, _, cx| cx.notify());
            Self {
                host,
                background: cx.focus_handle().tab_stop(true),
                first: cx.focus_handle().tab_stop(true),
                second: cx.focus_handle().tab_stop(true),
                open: true,
                popup: false,
                _host_sub: host_sub,
            }
        }
    }

    impl Render for Harness {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            self.host.update(cx, |host, cx| {
                host.begin_frame(window);
                host.set_fallback_focus(&self.background);
                host.set_modal(self.open.then(|| "dialog".into()), window, cx);
                host.set_popup(self.popup.then(|| "menu".into()), window, cx);
            });
            let modal_focus = self.host.read(cx).modal_focus();
            let mut root = div().id("root").size_full().child(
                div()
                    .id("background")
                    .track_focus(&self.background)
                    .tab_stop(true)
                    .child("Background"),
            );
            if let Some(modal_focus) = modal_focus {
                root = root.child(
                    div()
                        .id("modal")
                        .role(Role::Dialog)
                        .aria_modal(true)
                        .track_focus(&modal_focus)
                        .tab_stop(true)
                        .child(
                            div()
                                .id("first")
                                .track_focus(&self.first)
                                .tab_stop(true)
                                .child("First"),
                        )
                        .child(
                            div()
                                .id("second")
                                .track_focus(&self.second)
                                .tab_stop(true)
                                .child("Second"),
                        ),
                );
            }
            if let Some(popup_focus) = self.host.read(cx).popup_focus() {
                root = root.child(
                    div()
                        .id("popup")
                        .role(Role::Menu)
                        .track_focus(&popup_focus)
                        .tab_stop(true)
                        .child("Menu"),
                );
            }
            root
        }
    }

    fn press(cx: &mut VisualTestContext, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(Window::simulate_next_frame);
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    fn modal_tabs_wrap_and_skip_background(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Harness::new(cx));
        draw(cx);
        let first = cx.read_entity(&view, |view, _| view.first.clone());
        let second = cx.read_entity(&view, |view, _| view.second.clone());
        cx.update(|window, cx| first.focus(window, cx));
        press(cx, "tab");
        assert!(cx.update(|window, _| second.is_focused(window)));
        press(cx, "tab");
        assert!(cx.update(|window, _| first.is_focused(window)));
        press(cx, "shift-tab");
        assert!(cx.update(|window, _| second.is_focused(window)));
    }

    #[gpui::test]
    fn escape_requests_close_without_changing_business_scope(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, cx| Harness::new(cx));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let capture = events.clone();
        let subscription = cx.update(|_, cx| {
            let host = view.read(cx).host.clone();
            cx.subscribe(&host, move |_, event: &OverlayEvent, _| {
                capture.borrow_mut().push(event.clone());
            })
        });
        draw(cx);
        press(cx, "escape");
        assert_eq!(
            events.borrow().as_slice(),
            &[OverlayEvent::CloseRequested {
                scope: "dialog".into(),
                reason: CloseReason::Escape
            }]
        );
        assert!(
            cx.read_entity(&view, |view, cx| view.host.read(cx).modal_active())
        );
        drop(subscription);
    }

    #[gpui::test]
    fn nested_popup_closes_before_dialog_and_restores_its_trigger(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, cx| Harness::new(cx));
        draw(cx);
        let first = cx.read_entity(&view, |view, _| view.first.clone());
        cx.update(|window, cx| first.focus(window, cx));
        view.update(cx, |view, cx| {
            view.popup = true;
            cx.notify();
        });
        draw(cx);
        assert_eq!(
            cx.read_entity(&view, |view, cx| view.host.read(cx).active_scope()),
            Some("menu".into())
        );
        view.update(cx, |view, cx| {
            view.popup = false;
            cx.notify();
        });
        draw(cx);
        assert!(cx.update(|window, _| first.is_focused(window)));
        assert_eq!(
            cx.read_entity(&view, |view, cx| view.host.read(cx).active_scope()),
            Some("dialog".into())
        );
    }

    struct LayerHarness {
        ui: UiContext,
    }

    impl Render for LayerHarness {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            self.ui.overlays.update(cx, |host, cx| {
                host.begin_frame(window);
                host.set_modal(Some("layer".into()), window, cx);
            });
            let theme = self.ui.snapshot(cx);
            modal_layer(
                "layer-root",
                &theme,
                &self.ui,
                dialog("layer-dialog", "Layer dialog", &theme, &self.ui, cx)
                    .w(px(240.))
                    .h(px(160.))
                    .child("Dialog"),
            )
        }
    }

    #[gpui::test]
    fn modal_layer_closes_only_outside_the_dialog(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| LayerHarness {
            ui: UiContext::new(crate::ThemeOptions::default(), false, cx),
        });
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let capture = events.clone();
        let subscription = cx.update(|_, cx| {
            let host = view.read(cx).ui.overlays.clone();
            cx.subscribe(&host, move |_, event: &OverlayEvent, _| {
                capture.borrow_mut().push(event.clone());
            })
        });
        draw(cx);
        let size = cx.update(|window, _| window.viewport_size());
        cx.simulate_click(
            point(size.width / 2., size.height / 2.),
            Modifiers::none(),
        );
        assert!(events.borrow().is_empty());
        cx.simulate_click(point(px(1.), px(1.)), Modifiers::none());
        assert_eq!(
            events.borrow().as_slice(),
            &[OverlayEvent::CloseRequested {
                scope: "layer".into(),
                reason: CloseReason::Outside,
            }]
        );
        drop(subscription);
    }

    #[gpui::test]
    fn confirmed_close_restores_fallback_when_trigger_is_gone(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, cx| Harness::new(cx));
        draw(cx);
        let background =
            cx.read_entity(&view, |view, _| view.background.clone());
        view.update(cx, |view, cx| {
            view.open = false;
            cx.notify();
        });
        draw(cx);
        assert!(cx.update(|window, _| background.is_focused(window)));
    }
}
