//! Toolbar chrome whose label metrics live with the control.
use crate::{ResolvedTheme, TypeMetrics, interactive, ui_font};
use gpui::{
    App, ClickEvent, ColorExt, Div, ElementId, FontWeight, IntoElement,
    ParentElement, Role, SharedString, Stateful, Window, div, prelude::*, px,
};
use gpui_ce_elements::editable_text::EditableTextState;

fn control_text(element: Div, metrics: TypeMetrics) -> Div {
    element
        .font(ui_font())
        .text_size(metrics.size)
        .line_height(metrics.line_height)
        .font_weight(metrics.weight)
}

#[must_use]
pub struct ToolbarSearch {
    id: ElementId,
    placeholder: SharedString,
    state: gpui::Entity<EditableTextState>,
}

impl ToolbarSearch {
    pub fn new(
        id: impl Into<ElementId>,
        placeholder: impl Into<SharedString>,
        state: gpui::Entity<EditableTextState>,
    ) -> Self {
        Self {
            id: id.into(),
            placeholder: placeholder.into(),
            state,
        }
    }

    pub fn render(self, theme: &ResolvedTheme) -> Div {
        let metrics = theme.control_metrics();
        div()
            .w(px(240.))
            .h(px(32.))
            .min_w(px(120.))
            .bg(theme.card())
            .border_1()
            .border_color(theme.chrome_line())
            .rounded(px(10.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                crate::icons::search()
                    .size(px(16.))
                    .text_color(theme.toolbar_icon()),
            )
            .child(
                control_text(div().min_w_0().flex_1(), metrics)
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme.ink())
                    .child(
                        crate::text_input(self.id)
                            .state(self.state.downgrade())
                            .aria_label(self.placeholder.clone())
                            .placeholder(self.placeholder)
                            .placeholder_color(theme.placeholder())
                            .caret_color(gpui::rgb_to_hsla(theme.ink()))
                            .caret_blink_interval_500ms()
                            .w_full()
                            .min_w_0()
                            .whitespace_nowrap()
                            .font(ui_font())
                            .text_size(metrics.size)
                            .line_height(metrics.line_height)
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.ink()),
                    ),
            )
    }
}

impl std::fmt::Debug for ToolbarSearch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolbarSearch")
            .field("id", &self.id)
            .field("placeholder", &self.placeholder)
            .finish_non_exhaustive()
    }
}

pub fn filter_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &ResolvedTheme,
) -> Stateful<Div> {
    let label = label.into();
    let metrics = theme.control_metrics();
    let selected_bg = theme.tab_selected_bg();
    let selected_ink = theme.tab_selected();
    let mut chip = interactive(id, label.clone(), theme)
        .role(Role::Tab)
        .aria_selected(selected)
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .font(ui_font())
        .text_size(metrics.size)
        .line_height(metrics.line_height)
        .font_weight(FontWeight::NORMAL)
        .whitespace_nowrap()
        .bg(if selected {
            selected_bg
        } else {
            theme.canvas().opacity(0.)
        })
        .text_color(if selected {
            selected_ink
        } else {
            theme.tab_idle()
        })
        .child(label);
    if !selected {
        chip = chip
            .hover(move |style| style.bg(selected_bg).text_color(selected_ink));
    }
    chip
}

/// Shared layout and accessibility semantics for page-level category tabs.
#[must_use]
pub struct FilterTabs {
    id: ElementId,
    label: SharedString,
    children: Vec<gpui::AnyElement>,
}

impl FilterTabs {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn render(self) -> Stateful<Div> {
        div()
            .id(self.id)
            .role(Role::TabList)
            .aria_label(self.label)
            .w_full()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(4.))
            .children(self.children)
    }
}

impl std::fmt::Debug for FilterTabs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FilterTabs")
            .field("id", &self.id)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

pub fn icon_segment_group(theme: &ResolvedTheme) -> Div {
    div()
        .h(px(32.))
        .flex()
        .items_center()
        .p(px(2.))
        .bg(theme.chrome_track())
        .border_1()
        .border_color(theme.chrome_line())
        .rounded(px(10.))
}

pub fn icon_segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl IntoElement,
    selected: bool,
    theme: &ResolvedTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> crate::A11yElement<Div> {
    let hover = theme.chrome_line();
    let mut segment = interactive(id, label, theme)
        .h_full()
        .w(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(if selected {
            theme.card()
        } else {
            hover.opacity(0.)
        })
        .text_color(if selected {
            theme.ink()
        } else {
            theme.toolbar_icon()
        })
        .when(selected, gpui::Styled::shadow_sm)
        .child(icon)
        .on_click(on_click);
    if !selected {
        segment = segment.hover(move |style| style.bg(hover));
    }
    crate::A11yElement::new(segment.into_element())
}
