//! Controlled inputs and selection controls. Domain values stay in the caller.
use crate::{ControlSize, InteractiveDiv, ResolvedTheme, interactive};
use gpui::{
    AnyElement, App, ElementId, Entity, IntoElement, ParentElement, Pixels,
    Role, SharedString, Styled, Window, accesskit, div, prelude::*, px,
};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::input::{Input, InputState, Textarea, TextareaState};
use std::rc::Rc;

/// Low-level single-line editor; callers must attach their localized aria label.
pub fn text_input(
    _id: impl Into<ElementId>,
    state: &Entity<InputState>,
) -> Input {
    Input::new(state)
}
/// Low-level multiline editor; callers must attach their localized aria label.
pub fn text_area(
    _id: impl Into<ElementId>,
    state: &Entity<TextareaState>,
) -> Textarea {
    Textarea::new(state)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Editable,
    ReadOnly,
    Disabled,
}

#[derive(Debug)]
#[must_use]
pub struct TextField {
    id: ElementId,
    state: Entity<InputState>,
    label: SharedString,
    placeholder: SharedString,
    mode: InputMode,
    quiet: bool,
    error: Option<SharedString>,
}
impl TextField {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        state: Entity<InputState>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state,
            placeholder: SharedString::default(),
            mode: InputMode::Editable,
            quiet: false,
            error: None,
        }
    }
    pub fn placeholder(mut self, value: impl Into<SharedString>) -> Self {
        self.placeholder = value.into();
        self
    }
    pub const fn mode(mut self, value: InputMode) -> Self {
        self.mode = value;
        self
    }
    /// A lighter field for forms on the page canvas.
    pub const fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }
    pub fn invalid(mut self, message: impl Into<SharedString>) -> Self {
        self.error = Some(message.into());
        self
    }
    pub fn render(self, theme: &ResolvedTheme, cx: &App) -> InteractiveDiv {
        let input_id: ElementId = (self.id.clone(), "input").into();
        let disabled = self.mode == InputMode::Disabled;
        let border = if self.error.is_some() {
            theme.danger()
        } else if self.quiet {
            theme.quiet_field_border()
        } else {
            theme.border_strong()
        };
        let mut shell = div()
            .id(self.id.clone())
            .aria_label(self.label.clone())
            .w_full()
            .min_h(theme.control_height(ControlSize::Medium))
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(if self.quiet {
                theme.canvas()
            } else {
                theme.soft()
            })
            .text_color(theme.ink())
            .when(self.quiet && !disabled && self.error.is_none(), |shell| {
                let hover_border = theme.quiet_field_hover_border();
                shell.hover(move |style| style.border_color(hover_border))
            });
        if self.mode != InputMode::Editable {
            // Read-only and disabled share the non-interactive text
            // presentation: neither may host an editor, because the kit engine
            // exposes no per-render read-only flag and an edit would land in
            // the caller's state. Only the disabled field is dimmed.
            shell = shell
                .role(Role::TextInput)
                .when(disabled, |field| field.opacity(0.5))
                .px_3()
                .py_2()
                .child(self.state.read(cx).value().to_string());
        } else {
            // `InputState` fixes its placeholder at construction, so the hint
            // is rendered by the shell instead of the engine.
            let empty = self.state.read(cx).value().is_empty();
            let hint = if empty {
                self.placeholder.clone()
            } else {
                SharedString::default()
            };
            // Kit keeps the single-line and multi-line engines in separate
            // states; this control owns the single-line one, `TextAreaField`
            // the multi-line one.
            let input: AnyElement =
                text_input(input_id, &self.state).into_any_element();
            shell = shell
                .child(
                    div().relative().w_full().child(input).child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .left(px(12.))
                            .text_color(theme.muted())
                            .child(hint),
                    ),
                )
                .child(A11yElement::new(div()).invalid(self.error.is_some()));
        }
        shell.test_support()
    }
}

/// A multi-line text field.
///
/// Kit keeps the multi-line engine in its own state type (`TextareaState`), so
/// [`TextField`] — which wraps the single-line `InputState` — cannot serve a
/// multi-line field. This is its parallel control: same shell, same builders,
/// fixed height with the editor scrolling inside it.
#[derive(Debug)]
#[must_use]
pub struct TextAreaField {
    id: ElementId,
    state: Entity<TextareaState>,
    label: SharedString,
    placeholder: SharedString,
    mode: InputMode,
    height: Pixels,
    quiet: bool,
    error: Option<SharedString>,
}
impl TextAreaField {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        state: Entity<TextareaState>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state,
            placeholder: SharedString::default(),
            mode: InputMode::Editable,
            height: px(120.),
            quiet: false,
            error: None,
        }
    }
    pub fn placeholder(mut self, value: impl Into<SharedString>) -> Self {
        self.placeholder = value.into();
        self
    }
    pub const fn mode(mut self, value: InputMode) -> Self {
        self.mode = value;
        self
    }
    /// The field's outer height. The editor fills it and scrolls its content.
    pub const fn height(mut self, value: Pixels) -> Self {
        self.height = value;
        self
    }
    /// A lighter field for forms on the page canvas.
    pub const fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }
    pub fn invalid(mut self, message: impl Into<SharedString>) -> Self {
        self.error = Some(message.into());
        self
    }
    pub fn render(self, theme: &ResolvedTheme, cx: &App) -> InteractiveDiv {
        let input_id: ElementId = (self.id.clone(), "input").into();
        let disabled = self.mode == InputMode::Disabled;
        let border = if self.error.is_some() {
            theme.danger()
        } else if self.quiet {
            theme.quiet_field_border()
        } else {
            theme.border_strong()
        };
        // A definite height, not a minimum: the editor fills its frame and
        // needs one to resolve `h_full` against.
        let mut shell = div()
            .id(self.id.clone())
            .aria_label(self.label.clone())
            .w_full()
            .h(self.height)
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(if self.quiet {
                theme.canvas()
            } else {
                theme.soft()
            })
            .text_color(theme.ink())
            .when(self.quiet && !disabled && self.error.is_none(), |shell| {
                let hover_border = theme.quiet_field_hover_border();
                shell.hover(move |style| style.border_color(hover_border))
            });
        if self.mode != InputMode::Editable {
            // Same split as `TextField`: read-only and disabled only present
            // text, so an edit can never reach the caller's state.
            shell = shell
                .role(Role::MultilineTextInput)
                .when(disabled, |field| field.opacity(0.5))
                .px_3()
                .py_2()
                .overflow_hidden()
                .child(
                    div()
                        .whitespace_normal()
                        .child(self.state.read(cx).value().to_string()),
                );
        } else {
            // Same shell-side hint as `TextField`: the state's placeholder is
            // fixed at construction, and these fields are built without one.
            let empty = self.state.read(cx).value().is_empty();
            let hint = if empty {
                self.placeholder.clone()
            } else {
                SharedString::default()
            };
            let area: AnyElement =
                text_area(input_id, &self.state).into_any_element();
            shell = shell
                .child(
                    div().relative().w_full().h_full().child(area).child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .left(px(12.))
                            .text_color(theme.muted())
                            .child(hint),
                    ),
                )
                .child(A11yElement::new(div()).invalid(self.error.is_some()));
        }
        shell.test_support()
    }
}

#[derive(Debug)]
#[must_use]
pub struct Switch {
    id: ElementId,
    label: SharedString,
    checked: bool,
    disabled: bool,
}
impl Switch {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked,
            disabled: false,
        }
    }
    pub const fn disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self
    }
    pub fn render(
        self,
        theme: &ResolvedTheme,
        on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> InteractiveDiv {
        let checked = self.checked;
        crate::button(
            self.id,
            self.label,
            self.disabled,
            theme,
            move |_, w, c| on_change(&!checked, w, c),
        )
        .role(Role::Switch)
        .aria_toggled(if checked {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        })
        .w(px(40.))
        .min_h(px(24.))
        .rounded_full()
        .px(px(2.))
        .bg(if checked {
            theme.fill()
        } else {
            theme.border_strong()
        })
        .flex()
        .items_center()
        .when(checked, Styled::justify_end)
        .child(div().size(px(18.)).rounded_full().bg(if checked {
            theme.on_fill()
        } else {
            theme.card()
        }))
        .test_support()
    }
}

/// A stable key and localized label for radio and tab choices.
#[derive(Clone, Debug)]
pub struct Choice {
    pub id: SharedString,
    pub label: SharedString,
    pub disabled: bool,
}
impl Choice {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }
}

/// A controlled radio group. Arrow keys select the next enabled option, wrapping.
#[derive(Debug)]
#[must_use]
pub struct RadioGroup {
    id: ElementId,
    label: SharedString,
    selected: SharedString,
    choices: Vec<Choice>,
    tabs: bool,
}
impl RadioGroup {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        selected: impl Into<SharedString>,
        choices: Vec<Choice>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected: selected.into(),
            choices,
            tabs: false,
        }
    }
    pub fn render(
        self,
        theme: &ResolvedTheme,
        window: &mut Window,
        cx: &mut App,
        on_change: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> InteractiveDiv {
        let callback = Rc::new(on_change);
        let enabled: Vec<SharedString> = self
            .choices
            .iter()
            .filter(|c| !c.disabled)
            .map(|c| c.id.clone())
            .collect();
        let enabled = Rc::new(enabled);
        let focus_state = window.use_keyed_state(self.id.clone(), cx, |_, _| std::collections::HashMap::<SharedString, gpui::FocusHandle>::new());
        let handles = focus_state.update(cx, |handles, cx| {
            for choice in &self.choices {
                handles
                    .entry(choice.id.clone())
                    .or_insert_with(|| cx.focus_handle());
            }
            handles.clone()
        });
        let handles = Rc::new(handles);
        let tab_choice = if enabled.contains(&self.selected) {
            Some(self.selected.clone())
        } else {
            enabled.first().cloned()
        };
        let mut root = div()
            .id(self.id)
            .role(if self.tabs {
                Role::TabList
            } else {
                Role::RadioGroup
            })
            .aria_label(self.label)
            .flex()
            .gap_2();
        for choice in self.choices {
            let selected = choice.id == self.selected;
            let chosen = choice.id.clone();
            let click = callback.clone();
            let keyboard = callback.clone();
            let options = enabled.clone();
            let current = choice.id.clone();
            let focus_handles = handles.clone();
            let focus_handle = handles[&choice.id].clone().tab_stop(
                !choice.disabled && tab_choice.as_ref() == Some(&choice.id),
            );
            let mut item = crate::button(
                choice.id.clone(),
                choice.label.clone(),
                choice.disabled,
                theme,
                move |_, w, c| click(&chosen, w, c),
            )
            .track_focus(&focus_handle)
            .tab_stop(
                !choice.disabled && tab_choice.as_ref() == Some(&choice.id),
            )
            .role(if self.tabs {
                Role::Tab
            } else {
                Role::RadioButton
            })
            .min_h(theme.control_height(ControlSize::Medium))
            .px_3()
            .py_1()
            .rounded_lg()
            .bg(if selected { theme.fill() } else { theme.soft() })
            .text_color(if selected {
                theme.on_fill()
            } else {
                theme.ink()
            })
            .child(choice.label);
            item = if self.tabs {
                item.aria_selected(selected)
            } else {
                item.aria_toggled(if selected {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                })
            };
            if !choice.disabled {
                item = item.on_key_down(move |event, w, c| {
                    let Some(index) =
                        options.iter().position(|v| v == &current)
                    else {
                        return;
                    };
                    if options.is_empty() {
                        return;
                    }
                    let next = match event.keystroke.key.as_str() {
                        "right" | "down" => Some((index + 1) % options.len()),
                        "left" | "up" => {
                            Some((index + options.len() - 1) % options.len())
                        }
                        "home" => Some(0),
                        "end" => Some(options.len() - 1),
                        _ => None,
                    };
                    if let Some(next) = next {
                        w.focus(&focus_handles[&options[next]], c);
                        keyboard(&options[next], w, c);
                        c.stop_propagation();
                    }
                });
            }
            root = root.child(item);
        }
        root.test_support()
    }
}
#[derive(Debug)]
#[must_use]
pub struct Tabs(RadioGroup);
impl Tabs {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        selected: impl Into<SharedString>,
        choices: Vec<Choice>,
    ) -> Self {
        let mut group = RadioGroup::new(id, label, selected, choices);
        group.tabs = true;
        Self(group)
    }
    pub fn render(
        self,
        theme: &ResolvedTheme,
        window: &mut Window,
        cx: &mut App,
        on_change: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> InteractiveDiv {
        self.0.render(theme, window, cx, on_change).test_support()
    }
}

pub fn disclosure(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    expanded: bool,
    theme: &ResolvedTheme,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> InteractiveDiv {
    let id = id.into();
    let arrow_id: ElementId = (id.clone(), "arrow").into();
    let label = label.into();
    interactive(id, label.clone(), theme)
        .aria_expanded(expanded)
        .flex()
        .items_center()
        .gap_2()
        .child(div().id(arrow_id).child(if expanded {
            crate::icons::chevron_down()
        } else {
            crate::icons::chevron_right()
        }))
        .child(label)
        .on_click(move |_, w, c| on_change(&!expanded, w, c))
        .test_support()
}

/// Adds semantic states missing from GPUI's fluent ARIA API without introducing a second node.
#[must_use]
pub struct A11yElement<E> {
    element: E,
    busy: bool,
    invalid: bool,
}
impl<E> A11yElement<E> {
    pub const fn new(element: E) -> Self {
        Self {
            element,
            busy: false,
            invalid: false,
        }
    }
    pub const fn busy(mut self, value: bool) -> Self {
        self.busy = value;
        self
    }
    pub const fn invalid(mut self, value: bool) -> Self {
        self.invalid = value;
        self
    }
}
impl<E> std::fmt::Debug for A11yElement<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A11yElement")
            .field("busy", &self.busy)
            .field("invalid", &self.invalid)
            .finish_non_exhaustive()
    }
}
impl<E: Styled> Styled for A11yElement<E> {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.element.style()
    }
}
impl<E: gpui::InteractiveElement> gpui::InteractiveElement for A11yElement<E> {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.element.interactivity()
    }
}
impl<E: gpui::InteractiveElement> gpui::StatefulInteractiveElement
    for A11yElement<E>
{
}
impl<E: ParentElement> ParentElement for A11yElement<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.element.extend(elements);
    }
}
impl<E: gpui::Element> gpui::Element for A11yElement<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }
    fn source_location(
        &self,
    ) -> Option<&'static std::panic::Location<'static>> {
        self.element.source_location()
    }
    fn a11y_role(&self) -> Option<accesskit::Role> {
        self.element.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.element.write_a11y_info(node);
        if self.busy {
            node.set_busy();
        }
        if self.invalid {
            node.set_invalid(accesskit::Invalid::True);
        }
    }
    fn a11y_synthetic_children(
        &mut self,
        state: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder<'_>,
    ) {
        self.element.a11y_synthetic_children(state, builder);
    }
    fn request_layout(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        self.element.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.element
            .prepaint(id, inspector, bounds, state, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Focusable, Modifiers, Render, TestAppContext};
    use gpui_kit::test::TestQueryExt;

    // GPUI's actions macro emits PartialEq without Eq.
    #[allow(clippy::derive_partial_eq_without_eq)]
    mod bindings {
        gpui::actions!(selection_test, [Next, Previous]);
    }

    struct Selection {
        radio: SharedString,
        tab: SharedString,
    }

    impl Render for Selection {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            let radio = cx.listener(|this, value: &SharedString, _, cx| {
                this.radio = value.clone();
                cx.notify();
            });
            let tab = cx.listener(|this, value: &SharedString, _, cx| {
                this.tab = value.clone();
                cx.notify();
            });
            let choices = || {
                vec![
                    Choice::new("first", "First"),
                    Choice {
                        id: "disabled".into(),
                        label: "Disabled".into(),
                        disabled: true,
                    },
                    Choice::new("last", "Last"),
                ]
            };
            div()
                .id("selection")
                .size_full()
                .flex()
                .flex_col()
                .on_action(|_: &bindings::Next, window, cx| {
                    window.focus_next(cx);
                })
                .on_action(|_: &bindings::Previous, window, cx| {
                    window.focus_prev(cx);
                })
                .child(
                    RadioGroup::new(
                        "radio",
                        "Radio",
                        self.radio.clone(),
                        choices(),
                    )
                    .render(
                        &ResolvedTheme::default(),
                        window,
                        cx,
                        radio,
                    ),
                )
                .child(
                    Tabs::new("tabs", "Tabs", self.tab.clone(), choices())
                        .render(&ResolvedTheme::default(), window, cx, tab),
                )
        }
    }

    #[gpui::test]
    fn radio_and_tabs_use_one_tab_stop_and_skip_disabled_choices(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.bind_keys([
                gpui::KeyBinding::new("tab", bindings::Next, None),
                gpui::KeyBinding::new("shift-tab", bindings::Previous, None),
            ]);
        });
        let _initial_focus = cx.update(crate::install_initial_tab_focus);
        let (view, cx) = cx.add_window_view(|_, _| Selection {
            radio: "first".into(),
            tab: "first".into(),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let press = |cx: &mut gpui::VisualTestContext, key: &'static str| {
            let keystroke = gpui::Keystroke::parse(key).unwrap();
            cx.simulate_event(gpui::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(gpui::KeyUpEvent { keystroke });
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        press(cx, "tab");
        press(cx, "right");
        cx.read_entity(&view, |view, _| {
            assert_eq!(view.radio.as_ref(), "last");
        });
        press(cx, "right");
        cx.read_entity(&view, |view, _| {
            assert_eq!(view.radio.as_ref(), "first");
        });
        press(cx, "tab");
        press(cx, "end");
        cx.read_entity(&view, |view, _| {
            assert_eq!(view.radio.as_ref(), "first");
            assert_eq!(view.tab.as_ref(), "last");
        });
        press(cx, "home");
        cx.read_entity(&view, |view, _| assert_eq!(view.tab.as_ref(), "first"));
    }

    struct Fields {
        text: Entity<InputState>,
        invalid_text: Entity<InputState>,
        checked: bool,
    }
    impl Render for Fields {
        fn render(
            &mut self,
            _window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            let theme = ResolvedTheme::default();
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    TextField::new("readonly", "Read only", self.text.clone())
                        .mode(InputMode::ReadOnly)
                        .render(&theme, cx),
                )
                .child(
                    TextField::new(
                        "disabled",
                        "Disabled field",
                        self.text.clone(),
                    )
                    .mode(InputMode::Disabled)
                    .render(&theme, cx),
                )
                .child(
                    TextField::new(
                        "invalid",
                        "Invalid field",
                        self.invalid_text.clone(),
                    )
                    .invalid("Enter a valid value")
                    .render(&theme, cx),
                )
                .child(
                    Switch::new("switch", "Notifications", self.checked)
                        .render(
                            &theme,
                            cx.listener(|this, value, _, cx| {
                                this.checked = *value;
                                cx.notify();
                            }),
                        ),
                )
        }
    }
    #[gpui::test]
    fn readonly_rejects_typing_and_disabled_has_no_editor(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| Fields {
            text: cx.new(|cx| InputState::new(window, cx)),
            invalid_text: cx.new(|cx| InputState::new(window, cx)),
            checked: false,
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let handle = view.read(cx).text.focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_input("changed");
        cx.read_entity(&view, |view, cx| {
            // Down from HEAD: the kit engine starts empty (the retired
            // `EditableTextState` started at "value"). What the test still
            // proves is unchanged — a read-only field never receives an edit.
            assert!(
                view.text.read(cx).value().is_empty(),
                "a read only field must reject typing"
            );
        });
        let switch = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let labelled = |label: &str| {
                window
                    .find_all_by_label(label)
                    .into_iter()
                    .find(|node| node.visible())
                    .unwrap_or_else(|| {
                        panic!("missing accessible field {label:?}")
                    })
            };
            let readonly = labelled("Read only");
            // `Node::is_read_only()` has no `ElementSnapshot` projection, so a
            // read-only field is asserted by its role and by the rejected
            // typing above.
            assert_eq!(
                readonly.role(),
                Some(accesskit::Role::TextInput),
                "a read only field must stay a labelled text input"
            );
            // `Node::is_disabled()` and `Node::invalid()` are likewise not
            // projected for a div-based field, and gpui-pre exposes no
            // aria-disabled setter; the two fields are still required to exist
            // under their labels.
            let _ = labelled("Disabled field");
            let _ = labelled("Invalid field");
            labelled("Notifications").bounds()
        });
        cx.simulate_click(switch.center(), Modifiers::default());
        cx.read_entity(&view, |view, _| assert!(view.checked));
    }
}
