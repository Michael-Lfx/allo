//! Layout patterns shared by settings and forms. Content is supplied by the caller.
use crate::ResolvedTheme;
use gpui::StatefulInteractiveElement;
use gpui::{
    Div, ElementId, InteractiveElement, IntoElement, ParentElement, Role,
    SharedString, Stateful, Styled, accesskit, div, prelude::*,
};

pub fn form_field(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    help: Option<SharedString>,
    error: Option<SharedString>,
    input: impl IntoElement,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    let id = id.into();
    let error_id: ElementId = (id.clone(), "error").into();
    let mut field = div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_sm().text_color(theme.ink()).child(label.into()))
        .child(input);
    if let Some(help) = help {
        field =
            field.child(div().text_sm().text_color(theme.muted()).child(help));
    }
    if let Some(error) = error {
        field = field.child(
            div()
                .id(error_id)
                .role(Role::Alert)
                .text_sm()
                .text_color(theme.danger())
                .child(error),
        );
    }
    field
}
pub fn nav_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    icon: impl IntoElement,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    let label = label.into();
    crate::interactive(id, label.clone(), theme)
        .mx_2()
        .px(theme.space(8.))
        .py(theme.space(4.))
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .bg(if selected { theme.card() } else { theme.dock() })
        .when(selected, Styled::shadow_sm)
        .hover(|row| row.bg(theme.card()))
        .child(icon)
        .child(
            div()
                .text_size(theme.body_size())
                .overflow_hidden()
                .whitespace_nowrap()
                .child(label),
        )
}

pub fn settings_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    theme: &ResolvedTheme,
) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap_6()
        .py(theme.space(12.))
        .border_t_1()
        .border_color(theme.line())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div().text_sm().text_color(theme.ink()).child(title.into()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted())
                        .whitespace_normal()
                        .child(description.into()),
                ),
        )
        .child(control)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoticeKind {
    #[default]
    Info,
    Error,
}
pub fn notice(
    id: impl Into<ElementId>,
    message: impl Into<SharedString>,
    kind: NoticeKind,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(if kind == NoticeKind::Error {
            Role::Alert
        } else {
            Role::Status
        })
        .aria_live(accesskit::Live::Polite)
        .aria_live_atomic(true)
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(theme.soft())
        .text_sm()
        .whitespace_normal()
        .text_color(if kind == NoticeKind::Error {
            theme.danger()
        } else {
            theme.muted()
        })
        .child(message.into())
}
pub fn empty_state(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    theme: &ResolvedTheme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .p_6()
        .child(div().text_lg().text_color(theme.ink()).child(title.into()))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted())
                .whitespace_normal()
                .child(description.into()),
        )
}
