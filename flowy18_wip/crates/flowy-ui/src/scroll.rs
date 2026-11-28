//! Unified visual thin scrollbar and AccessKit `ScrollView` + `ScrollBar` host.
//!
//! `gpui-pre` reserves `.scrollbar_width(...)` space and handles wheel input on
//! overflow containers, but paints no scrollbar chrome and emits no AccessKit
//! scroll metadata or action handlers. This module pairs
//! `gpui_kit::base::Scrollbar` (explicitly themed from [`ResolvedTheme`]) with
//! an [`Element`] host that keeps the accessibility tree and Windows UIA
//! `IScrollProvider` properties synchronized on the exact same prepaint frame.

use std::fmt;

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId,
    Hsla, InspectorElementId, InteractiveElement, IntoElement, ParentElement,
    Pixels, Point, ScrollEvent, ScrollHandle, SharedString, Size, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, accesskit,
    div, point, prelude::*, px,
};
use gpui_kit::base::{Scrollbar, ScrollbarStyles};

pub use gpui_kit::base::{ScrollbarAxis, ScrollbarHandle, ScrollbarMode};

use crate::ResolvedTheme;

/// Logical pixel step used for small/line accessibility scroll increments.
pub const A11Y_SCROLL_ITEM_PX: f32 = 72.0;

/// Fraction of the viewport scrolled per page-step accessibility increment.
pub const A11Y_SCROLL_PAGE_RATIO: f32 = 0.85;

const MIN_SCROLL_OVERFLOW_PX: f32 = 0.5;
const TRACK_WIDTH_PX: f32 = 10.0;
const THUMB_IDLE_WIDTH_PX: f32 = 4.0;
const THUMB_IDLE_INSET_PX: f32 = 3.0;
const THUMB_ACTIVE_WIDTH_PX: f32 = 6.0;
const THUMB_ACTIVE_INSET_PX: f32 = 2.0;
const THUMB_MIN_LENGTH_PX: f32 = 36.0;

type ViewportModifier = Box<dyn FnOnce(gpui::Div) -> gpui::Div>;
type ScrollListener =
    Box<dyn Fn(&ScrollEvent, &mut Window, &mut App) + 'static>;
type ScrollSelector = Box<dyn FnOnce() -> String + 'static>;

/// Converts a GPUI scroll offset (`0.0` down/right to `-max_px`) and extent
/// into non-negative AccessKit physical pixel `(min, current, max)` values.
#[must_use]
pub fn gpui_offset_to_accesskit(
    offset_px: f32,
    max_px: f32,
    scale_factor: f32,
) -> (f64, f64, f64) {
    let scale = f64::from(scale_factor.max(0.1));
    let safe_max = f64::from(max_px.max(0.0)) * scale;
    let clamped = f64::from((-offset_px).max(0.0)) * scale;
    (0.0, clamped.clamp(0.0, safe_max), safe_max)
}

/// Converts a target AccessKit physical scroll position back into a clamped
/// non-positive GPUI logical pixel offset (`0.0` ..= `-max_px`).
#[must_use]
pub fn accesskit_offset_to_gpui(
    target_phys: f64,
    max_px: f32,
    scale_factor: f32,
) -> f32 {
    if !target_phys.is_finite() || max_px <= 0.0 {
        return 0.0;
    }
    let scale = f64::from(scale_factor.max(0.1));
    let logical = (target_phys / scale) as f32;
    -logical.clamp(0.0, max_px.max(0.0))
}

/// Converts a GPUI negative offset (`0.0` ..= `-max_px`) into `0.0..=100.0` %.
#[must_use]
pub fn gpui_offset_to_percent(offset_px: f32, max_px: f32) -> f64 {
    if max_px <= 0.0 {
        return 0.0;
    }
    ((f64::from(-offset_px) / f64::from(max_px)) * 100.0).clamp(0.0, 100.0)
}

/// Converts a `0.0..=100.0` percentage into a clamped non-positive GPUI offset.
#[must_use]
pub fn percent_to_gpui_offset(pct: f64, max_px: f32) -> f32 {
    if !pct.is_finite() || max_px <= 0.0 {
        return 0.0;
    }
    let ratio = (pct.clamp(0.0, 100.0) / 100.0) as f32;
    -(max_px * ratio).clamp(0.0, max_px)
}

/// Computes the next clamped GPUI scroll offset for an AccessKit scroll action.
#[must_use]
pub fn step_scroll_offset(
    current: Point<f32>,
    max_offset: Size<f32>,
    viewport: Size<f32>,
    action: accesskit::Action,
    unit: Option<accesskit::ScrollUnit>,
) -> Point<f32> {
    let step_for = |vp_extent: f32| match unit {
        Some(accesskit::ScrollUnit::Page) => {
            (vp_extent * A11Y_SCROLL_PAGE_RATIO).max(A11Y_SCROLL_ITEM_PX)
        }
        Some(accesskit::ScrollUnit::Item) | None => A11Y_SCROLL_ITEM_PX,
    };

    let mut next_x = current.x.clamp(-max_offset.width.max(0.0), 0.0);
    let mut next_y = current.y.clamp(-max_offset.height.max(0.0), 0.0);
    match action {
        accesskit::Action::ScrollUp => {
            next_y = (next_y + step_for(viewport.height))
                .clamp(-max_offset.height.max(0.0), 0.0);
        }
        accesskit::Action::ScrollDown => {
            next_y = (next_y - step_for(viewport.height))
                .clamp(-max_offset.height.max(0.0), 0.0);
        }
        accesskit::Action::ScrollLeft => {
            next_x = (next_x + step_for(viewport.width))
                .clamp(-max_offset.width.max(0.0), 0.0);
        }
        accesskit::Action::ScrollRight => {
            next_x = (next_x - step_for(viewport.width))
                .clamp(-max_offset.width.max(0.0), 0.0);
        }
        _ => {}
    }
    point(next_x, next_y)
}

fn scrollbar_styles_for(theme: ResolvedTheme) -> ScrollbarStyles {
    let thumb = Hsla::from(theme.scrollbar_thumb());
    let hover = Hsla::from(theme.scrollbar_thumb_hover());
    let active = Hsla::from(theme.scrollbar_thumb_active());
    let transparent = gpui::transparent_black();

    ScrollbarStyles::default()
        .track(move |t| t.bg(transparent).width(px(TRACK_WIDTH_PX)))
        .track_hover(move |t| t.bg(transparent).width(px(TRACK_WIDTH_PX)))
        .track_active(move |t| t.bg(transparent).width(px(TRACK_WIDTH_PX)))
        .thumb(move |b| {
            b.bg(thumb)
                .width(px(THUMB_IDLE_WIDTH_PX))
                .inset(px(THUMB_IDLE_INSET_PX))
                .radius(px(999.))
                .min_length(px(THUMB_MIN_LENGTH_PX))
        })
        .thumb_hover(move |b| {
            b.bg(hover)
                .width(px(THUMB_ACTIVE_WIDTH_PX))
                .inset(px(THUMB_ACTIVE_INSET_PX))
                .radius(px(999.))
                .min_length(px(THUMB_MIN_LENGTH_PX))
        })
        .thumb_active(move |b| {
            b.bg(active)
                .width(px(THUMB_ACTIVE_WIDTH_PX))
                .inset(px(THUMB_ACTIVE_INSET_PX))
                .radius(px(999.))
                .min_length(px(THUMB_MIN_LENGTH_PX))
        })
}

fn scaled_accesskit_rect(
    bounds: Bounds<Pixels>,
    scale: f32,
) -> accesskit::Rect {
    let s = f64::from(scale.max(0.1));
    accesskit::Rect {
        x0: f64::from(f32::from(bounds.origin.x)) * s,
        y0: f64::from(f32::from(bounds.origin.y)) * s,
        x1: f64::from(f32::from(bounds.origin.x + bounds.size.width)) * s,
        y1: f64::from(f32::from(bounds.origin.y + bounds.size.height)) * s,
    }
}

fn compute_overflow_max<H: ScrollbarHandle>(
    handle: &H,
) -> (Size<f32>, Size<f32>, Point<f32>) {
    let vp = handle.viewport_bounds();
    let content = handle.content_size();
    let offset = handle.offset();
    let vp_size = Size {
        width: f32::from(vp.size.width).max(0.0),
        height: f32::from(vp.size.height).max(0.0),
    };
    let max_size = Size {
        width: (f32::from(content.width) - vp_size.width).max(0.0),
        height: (f32::from(content.height) - vp_size.height).max(0.0),
    };
    let offset_px = point(f32::from(offset.x), f32::from(offset.y));
    (vp_size, max_size, offset_px)
}

struct ScrollA11yPrepaint<P> {
    inner: P,
    scale_factor: f32,
    view_id: EntityId,
    v_bar_node_id: Option<accesskit::NodeId>,
    h_bar_node_id: Option<accesskit::NodeId>,
}

fn apply_scroll_step<H: ScrollbarHandle>(
    handle: &H,
    action: accesskit::Action,
    data: Option<&accesskit::ActionData>,
) {
    let (vp, max, current) = compute_overflow_max(handle);
    let unit = match data {
        Some(accesskit::ActionData::ScrollUnit(u)) => Some(*u),
        _ => None,
    };
    let next = step_scroll_offset(current, max, vp, action, unit);
    handle.set_offset(point(px(next.x), px(next.y)));
}

fn attach_parent_scroll_actions<H: ScrollbarHandle + Clone + 'static>(
    mut el: Stateful<gpui::Div>,
    handle: H,
    axis: ScrollbarAxis,
    view_id: EntityId,
) -> Stateful<gpui::Div> {
    if axis.has_vertical() {
        for action in
            [accesskit::Action::ScrollUp, accesskit::Action::ScrollDown]
        {
            let handle = handle.clone();
            el = el.on_a11y_action(action, move |data, win, cx| {
                apply_scroll_step(&handle, action, data);
                cx.notify(view_id);
                win.refresh();
            });
        }
    }
    if axis.has_horizontal() {
        for action in [
            accesskit::Action::ScrollLeft,
            accesskit::Action::ScrollRight,
        ] {
            let handle = handle.clone();
            el = el.on_a11y_action(action, move |data, win, cx| {
                apply_scroll_step(&handle, action, data);
                cx.notify(view_id);
                win.refresh();
            });
        }
    }
    el.on_a11y_action(
        accesskit::Action::SetScrollOffset,
        move |data, win, cx| {
            let Some(accesskit::ActionData::SetScrollOffset(target)) = data
            else {
                return;
            };
            let scale = win.scale_factor();
            let (_, max, mut next) = compute_overflow_max(&handle);
            if axis.has_horizontal() {
                next.x = accesskit_offset_to_gpui(target.x, max.width, scale);
            }
            if axis.has_vertical() {
                next.y = accesskit_offset_to_gpui(target.y, max.height, scale);
            }
            handle.set_offset(point(px(next.x), px(next.y)));
            cx.notify(view_id);
            win.refresh();
        },
    )
}

/// Element adapter that exposes AccessKit `Role::ScrollView` + scroll metrics +
/// synthetic `Role::ScrollBar` nodes for any GPUI element backed by a
/// [`ScrollbarHandle`] (`ScrollHandle`, `ListState`, or `UniformListScrollHandle`).
struct ScrollA11yHost<H, E> {
    a11y_id: ElementId,
    handle: H,
    axis: ScrollbarAxis,
    label: Option<SharedString>,
    element: E,
}

impl<H, E> ScrollA11yHost<H, E>
where
    H: ScrollbarHandle + Clone + 'static,
    E: Element,
{
    const fn new(
        a11y_id: ElementId,
        handle: H,
        axis: ScrollbarAxis,
        label: Option<SharedString>,
        element: E,
    ) -> Self {
        Self {
            a11y_id,
            handle,
            axis,
            label,
            element,
        }
    }
}

impl<H, E> Element for ScrollA11yHost<H, E>
where
    H: ScrollbarHandle + Clone + 'static,
    E: Element + 'static,
{
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = ScrollA11yPrepaint<E::PrepaintState>;

    fn id(&self) -> Option<ElementId> {
        self.element.id().or_else(|| Some(self.a11y_id.clone()))
    }

    fn source_location(
        &self,
    ) -> Option<&'static std::panic::Location<'static>> {
        self.element.source_location()
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        Some(accesskit::Role::ScrollView)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.element.write_a11y_info(node);
        node.set_clips_children();
        if let Some(label) = &self.label {
            node.set_label(label.as_ref());
        }
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        self.element.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let inner =
            self.element
                .prepaint(id, inspector_id, bounds, state, window, cx);
        ScrollA11yPrepaint {
            inner,
            scale_factor: window.scale_factor(),
            view_id: window.current_view(),
            v_bar_node_id: None,
            h_bar_node_id: None,
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        state: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder<'_>,
    ) {
        self.element
            .a11y_synthetic_children(&mut state.inner, builder);
        let vp = self.handle.viewport_bounds();
        let (vp_size, max, offset_px) = compute_overflow_max(&self.handle);
        let v_scrollable =
            self.axis.has_vertical() && max.height > MIN_SCROLL_OVERFLOW_PX;
        let h_scrollable =
            self.axis.has_horizontal() && max.width > MIN_SCROLL_OVERFLOW_PX;

        if v_scrollable {
            let (y_min, y_pos, y_max) = gpui_offset_to_accesskit(
                offset_px.y,
                max.height,
                state.scale_factor,
            );
            let parent = builder.parent_node();
            parent.set_scroll_y_min(y_min);
            parent.set_scroll_y(y_pos);
            parent.set_scroll_y_max(y_max);
            parent.add_action(accesskit::Action::ScrollUp);
            parent.add_action(accesskit::Action::ScrollDown);
            parent.add_action(accesskit::Action::SetScrollOffset);

            let mut vbar = accesskit::Node::new(accesskit::Role::ScrollBar);
            vbar.set_orientation(accesskit::Orientation::Vertical);
            let bar_w = px(TRACK_WIDTH_PX.min(vp_size.width));
            let bar_bounds = Bounds {
                origin: point(vp.origin.x + vp.size.width - bar_w, vp.origin.y),
                size: gpui::size(bar_w, vp.size.height),
            };
            vbar.set_bounds(scaled_accesskit_rect(
                bar_bounds,
                state.scale_factor,
            ));
            vbar.set_min_numeric_value(0.0);
            vbar.set_max_numeric_value(100.0);
            vbar.set_numeric_value(gpui_offset_to_percent(
                offset_px.y,
                max.height,
            ));
            let item_pct = ((f64::from(A11Y_SCROLL_ITEM_PX)
                / f64::from(max.height))
                * 100.0)
                .clamp(1.0, 100.0);
            let page_pct =
                ((f64::from(vp_size.height * A11Y_SCROLL_PAGE_RATIO)
                    / f64::from(max.height))
                    * 100.0)
                    .clamp(item_pct, 100.0);
            vbar.set_numeric_value_step(item_pct);
            vbar.set_numeric_value_jump(page_pct);
            vbar.add_action(accesskit::Action::SetValue);

            let v_id = builder.synthetic_node_id("vertical-scrollbar");
            if builder.push_child(v_id, vbar) {
                state.v_bar_node_id = Some(v_id);
            }
        }

        if h_scrollable {
            let (x_min, x_pos, x_max) = gpui_offset_to_accesskit(
                offset_px.x,
                max.width,
                state.scale_factor,
            );
            let parent = builder.parent_node();
            parent.set_scroll_x_min(x_min);
            parent.set_scroll_x(x_pos);
            parent.set_scroll_x_max(x_max);
            parent.add_action(accesskit::Action::ScrollLeft);
            parent.add_action(accesskit::Action::ScrollRight);
            parent.add_action(accesskit::Action::SetScrollOffset);

            let mut hbar = accesskit::Node::new(accesskit::Role::ScrollBar);
            hbar.set_orientation(accesskit::Orientation::Horizontal);
            let bar_h = px(TRACK_WIDTH_PX.min(vp_size.height));
            let bar_bounds = Bounds {
                origin: point(
                    vp.origin.x,
                    vp.origin.y + vp.size.height - bar_h,
                ),
                size: gpui::size(vp.size.width, bar_h),
            };
            hbar.set_bounds(scaled_accesskit_rect(
                bar_bounds,
                state.scale_factor,
            ));
            hbar.set_min_numeric_value(0.0);
            hbar.set_max_numeric_value(100.0);
            hbar.set_numeric_value(gpui_offset_to_percent(
                offset_px.x,
                max.width,
            ));
            let item_pct = ((f64::from(A11Y_SCROLL_ITEM_PX)
                / f64::from(max.width))
                * 100.0)
                .clamp(1.0, 100.0);
            let page_pct =
                ((f64::from(vp_size.width * A11Y_SCROLL_PAGE_RATIO)
                    / f64::from(max.width))
                    * 100.0)
                    .clamp(item_pct, 100.0);
            hbar.set_numeric_value_step(item_pct);
            hbar.set_numeric_value_jump(page_pct);
            hbar.add_action(accesskit::Action::SetValue);

            let h_id = builder.synthetic_node_id("horizontal-scrollbar");
            if builder.push_child(h_id, hbar) {
                state.h_bar_node_id = Some(h_id);
            }
        }
        if !v_scrollable {
            let parent = builder.parent_node();
            parent.remove_action(accesskit::Action::ScrollUp);
            parent.remove_action(accesskit::Action::ScrollDown);
        }
        if !h_scrollable {
            let parent = builder.parent_node();
            parent.remove_action(accesskit::Action::ScrollLeft);
            parent.remove_action(accesskit::Action::ScrollRight);
        }
        if !v_scrollable && !h_scrollable {
            builder
                .parent_node()
                .remove_action(accesskit::Action::SetScrollOffset);
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(
            id,
            inspector_id,
            bounds,
            layout,
            &mut prepaint.inner,
            window,
            cx,
        );

        if !window.is_a11y_active() {
            return;
        }

        let view_id = prepaint.view_id;

        if let Some(v_id) = prepaint.v_bar_node_id {
            let handle = self.handle.clone();
            window.on_a11y_action(
                v_id,
                accesskit::Action::SetValue,
                move |data, win, cx| {
                    let Some(accesskit::ActionData::NumericValue(pct)) = data
                    else {
                        return;
                    };
                    let (_, max, current) = compute_overflow_max(&handle);
                    let next_y = percent_to_gpui_offset(*pct, max.height);
                    handle.set_offset(point(px(current.x), px(next_y)));
                    cx.notify(view_id);
                    win.refresh();
                },
            );
        }

        if let Some(h_id) = prepaint.h_bar_node_id {
            let handle = self.handle.clone();
            window.on_a11y_action(
                h_id,
                accesskit::Action::SetValue,
                move |data, win, cx| {
                    let Some(accesskit::ActionData::NumericValue(pct)) = data
                    else {
                        return;
                    };
                    let (_, max, current) = compute_overflow_max(&handle);
                    let next_x = percent_to_gpui_offset(*pct, max.width);
                    handle.set_offset(point(px(next_x), px(current.y)));
                    cx.notify(view_id);
                    win.refresh();
                },
            );
        }
    }
}

fn inset_axis_span(origin: &mut Pixels, span: &mut Pixels, inset: Pixels) {
    if inset <= px(0.) || *span <= px(0.) {
        return;
    }
    let shrink = inset * 2.;
    if *span <= shrink {
        *origin += *span / 2.;
        *span = px(0.);
    } else {
        *origin += inset;
        *span -= shrink;
    }
}

fn inset_content_span(
    vp_span: Pixels,
    content_span: &mut Pixels,
    inset: Pixels,
) {
    if inset <= px(0.) || vp_span <= px(0.) {
        return;
    }
    let shrink = inset * 2.;
    *content_span = if vp_span <= shrink {
        px(0.)
    } else {
        (*content_span - shrink).max(px(0.))
    };
}

#[derive(Clone)]
struct InsetScrollbarHandle<H> {
    inner: H,
    axis: ScrollbarAxis,
    inset: Pixels,
}

impl<H: ScrollbarHandle + Clone + 'static> ScrollbarHandle
    for InsetScrollbarHandle<H>
{
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        let mut bounds = self.inner.viewport_bounds();
        if matches!(self.axis, ScrollbarAxis::Vertical | ScrollbarAxis::Both) {
            inset_axis_span(
                &mut bounds.origin.y,
                &mut bounds.size.height,
                self.inset,
            );
        }
        if matches!(self.axis, ScrollbarAxis::Horizontal | ScrollbarAxis::Both)
        {
            inset_axis_span(
                &mut bounds.origin.x,
                &mut bounds.size.width,
                self.inset,
            );
        }
        bounds
    }

    fn offset(&self) -> Point<Pixels> {
        self.inner.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.inner.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        let mut content = self.inner.content_size();
        let vp_size = self.inner.viewport_bounds().size;
        if matches!(self.axis, ScrollbarAxis::Vertical | ScrollbarAxis::Both) {
            // Preserve exact overflow (`content - viewport`) after insetting
            // the track bounds so non-overflowing panes stay hidden and thumb
            // drag reaches the true edge.
            inset_content_span(vp_size.height, &mut content.height, self.inset);
        }
        if matches!(self.axis, ScrollbarAxis::Horizontal | ScrollbarAxis::Both)
        {
            inset_content_span(vp_size.width, &mut content.width, self.inset);
        }
        content
    }

    fn start_drag(&self) {
        self.inner.start_drag();
    }

    fn end_drag(&self) {
        self.inner.end_drag();
    }
}

fn flowy_scrollbar<H: ScrollbarHandle + Clone + 'static>(
    id: ElementId,
    handle: &H,
    axis: ScrollbarAxis,
    theme: ResolvedTheme,
    mode: ScrollbarMode,
    inset: Pixels,
) -> AnyElement {
    let styles = scrollbar_styles_for(theme);
    if inset > px(0.) {
        let inset_handle = InsetScrollbarHandle {
            inner: handle.clone(),
            axis,
            inset,
        };
        Scrollbar::new(&inset_handle)
            .id(id)
            .axis(axis)
            .mode(mode)
            .styles(move |_| styles)
            .into_any_element()
    } else {
        Scrollbar::new(handle)
            .id(id)
            .axis(axis)
            .mode(mode)
            .styles(move |_| styles)
            .into_any_element()
    }
}

/// Fluent scrollable pane wrapper combining overflow layout, themed thin
/// scrollbar thumb overlay, and AccessKit/UIA scroll semantics.
#[must_use]
#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    theme: ResolvedTheme,
    axis: ScrollbarAxis,
    mode: ScrollbarMode,
    handle: Option<ScrollHandle>,
    a11y_label: Option<SharedString>,
    max_h: Option<Pixels>,
    inset_y: Pixels,
    shell_style: StyleRefinement,
    viewport_modifier: Option<ViewportModifier>,
    on_scroll: Option<ScrollListener>,
    debug_selector: Option<ScrollSelector>,
    children: Vec<AnyElement>,
}

impl fmt::Debug for ScrollArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollArea")
            .field("id", &self.id)
            .field("axis", &self.axis)
            .field("mode", &self.mode)
            .field("a11y_label", &self.a11y_label)
            .field("max_h", &self.max_h)
            .field("inset_y", &self.inset_y)
            .finish_non_exhaustive()
    }
}

/// Builds a vertically scrollable [`ScrollArea`] styled by `theme`.
pub fn scroll_area(
    id: impl Into<ElementId>,
    theme: &ResolvedTheme,
) -> ScrollArea {
    ScrollArea {
        id: id.into(),
        theme: *theme,
        axis: ScrollbarAxis::Vertical,
        mode: ScrollbarMode::Always,
        handle: None,
        a11y_label: None,
        max_h: None,
        inset_y: px(0.),
        shell_style: StyleRefinement::default(),
        viewport_modifier: None,
        on_scroll: None,
        debug_selector: None,
        children: Vec::new(),
    }
}

impl ScrollArea {
    /// Configures scroll axes (default: vertical only).
    pub fn axis(mut self, axis: impl Into<ScrollbarAxis>) -> Self {
        self.axis = axis.into();
        self
    }

    /// Restricts scrolling to the horizontal axis.
    pub const fn horizontal(mut self) -> Self {
        self.axis = ScrollbarAxis::Horizontal;
        self
    }

    /// Enables both horizontal and vertical scrolling and scrollbars.
    pub const fn both_axes(mut self) -> Self {
        self.axis = ScrollbarAxis::Both;
        self
    }

    /// Attaches an external [`ScrollHandle`] instead of an auto-keyed handle.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }

    /// Sets the localized AccessKit / Windows UIA name for the scroll view.
    pub fn a11y_label(mut self, label: impl Into<SharedString>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

    /// Overrides the visibility policy (default: [`ScrollbarMode::Always`]
    /// whenever content overflows the viewport).
    pub const fn scrollbar_mode(mut self, mode: ScrollbarMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets vertical top/bottom inset for the scrollbar overlay, keeping the
    /// thumb clear of rounded container corners such as `modal_radius()`.
    pub const fn scrollbar_inset(mut self, inset_y: Pixels) -> Self {
        self.inset_y = inset_y;
        self
    }

    /// Configures a content-sized container capped at `max_h` instead of a
    /// flex-grow viewport (used for dropdowns/quick search where rows shrink-wrap).
    /// Combine with `.both_axes()` (never `.horizontal()`) when horizontal
    /// overflow also needs to scroll inside a height cap.
    pub const fn max_h_viewport(mut self, max_h: Pixels) -> Self {
        self.max_h = Some(max_h);
        self
    }

    /// Customizes styles applied directly on the inner scrolled viewport `div`
    /// (for example `.flex().flex_col().gap(px(2.))` when direct child indices
    /// are used with `ScrollHandle::scroll_to_item`).
    pub fn viewport_style(
        mut self,
        modify: impl FnOnce(gpui::Div) -> gpui::Div + 'static,
    ) -> Self {
        self.viewport_modifier = Some(Box::new(modify));
        self
    }

    /// Attaches a scroll-event listener directly to the inner scrollable viewport.
    pub fn on_scroll(
        mut self,
        listener: impl Fn(&ScrollEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_scroll = Some(Box::new(listener));
        self
    }

    /// Sets a test selector key forwarded to the scrollable viewport in
    /// `test-support` builds (no-op in release builds).
    pub fn debug_selector(
        mut self,
        selector: impl FnOnce() -> String + 'static,
    ) -> Self {
        self.debug_selector = Some(Box::new(selector));
        self
    }
}

impl Styled for ScrollArea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.shell_style
    }
}

impl ParentElement for ScrollArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = match self.handle {
            Some(existing) => existing,
            None => window
                .use_keyed_state(
                    (self.id.clone(), "scroll-handle"),
                    cx,
                    |_, _| ScrollHandle::new(),
                )
                .read(cx)
                .clone(),
        };

        let mut viewport_inner = div().w_full().min_w_0().min_h_0();
        if let Some(modify) = self.viewport_modifier {
            viewport_inner = modify(viewport_inner);
        }

        let mut viewport = viewport_inner
            .id(self.id.clone())
            .track_scroll(&handle)
            .scrollbar_width(px(6.))
            .children(self.children);
        if let Some(selector) = self.debug_selector {
            viewport = viewport.debug_selector(selector);
        }

        viewport = match self.axis {
            ScrollbarAxis::Vertical => viewport.overflow_y_scroll(),
            ScrollbarAxis::Horizontal => viewport.overflow_x_scroll(),
            ScrollbarAxis::Both => viewport.overflow_scroll(),
        };

        let mut shell = div().id((self.id.clone(), "shell"));
        *shell.style() = self.shell_style;
        shell = shell.relative().flex().flex_col();
        debug_assert!(
            !(matches!(self.axis, ScrollbarAxis::Horizontal)
                && self.max_h.is_some()),
            "ScrollArea::horizontal() cannot combine with max_h_viewport(); use both_axes()"
        );
        if let Some(max_h) = self.max_h {
            if shell.style().size.width.is_none() {
                shell = shell.w_full();
            }
            shell = shell.max_h(max_h);
            viewport = viewport.flex_shrink(1.0).max_h(max_h);
        } else if !matches!(self.axis, ScrollbarAxis::Horizontal) {
            viewport = viewport.flex_1();
        }

        if let Some(listener) = self.on_scroll {
            viewport = viewport.on_scroll(listener);
        }

        let viewport = attach_parent_scroll_actions(
            viewport,
            handle.clone(),
            self.axis,
            window.current_view(),
        );

        let a11y_host = ScrollA11yHost::new(
            self.id.clone(),
            handle.clone(),
            self.axis,
            self.a11y_label,
            viewport,
        );
        let scrollbar = flowy_scrollbar(
            (self.id, "scrollbar").into(),
            &handle,
            self.axis,
            self.theme,
            self.mode,
            self.inset_y,
        );

        shell.child(a11y_host).child(scrollbar)
    }
}

/// Element returned by [`scroll_list_host`] combining a `gpui::list` host,
/// themed vertical scrollbar overlay, and AccessKit scroll exposure.
#[derive(IntoElement)]
pub struct ScrollListHost<E: Element + 'static> {
    id: ElementId,
    list_state: gpui::ListState,
    theme: ResolvedTheme,
    a11y_label: SharedString,
    list_element: E,
}

impl<E: Element + 'static> fmt::Debug for ScrollListHost<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollListHost")
            .field("id", &self.id)
            .field("a11y_label", &self.a11y_label)
            .finish_non_exhaustive()
    }
}

impl<E: Element + 'static> RenderOnce for ScrollListHost<E> {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let container = attach_parent_scroll_actions(
            div()
                .id(self.id.clone())
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .size_full()
                .child(self.list_element),
            self.list_state.clone(),
            ScrollbarAxis::Vertical,
            window.current_view(),
        );
        let host = ScrollA11yHost::new(
            (self.id.clone(), "a11y-host").into(),
            self.list_state.clone(),
            ScrollbarAxis::Vertical,
            Some(self.a11y_label),
            container,
        );
        let scrollbar = flowy_scrollbar(
            (self.id, "scrollbar").into(),
            &self.list_state,
            ScrollbarAxis::Vertical,
            self.theme,
            ScrollbarMode::Always,
            px(0.),
        );
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .size_full()
            .child(host)
            .child(scrollbar)
    }
}

/// Wraps a `gpui::list(ListState, ...)` with a themed visual vertical scrollbar
/// and AccessKit `Role::ScrollView` / synthetic `Role::ScrollBar` exposure.
pub fn scroll_list_host<E: Element + 'static>(
    id: impl Into<ElementId>,
    list_state: &gpui::ListState,
    theme: &ResolvedTheme,
    a11y_label: impl Into<SharedString>,
    list_element: E,
) -> ScrollListHost<E> {
    ScrollListHost {
        id: id.into(),
        list_state: list_state.clone(),
        theme: *theme,
        a11y_label: a11y_label.into(),
        list_element,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ThemeMode, ThemeOptions};
    use gpui::{Context, Render, TestAppContext};

    #[test]
    fn accesskit_offset_roundtrip_scales_for_dpi() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let (min, cur, max) =
                gpui_offset_to_accesskit(-120.0, 480.0, scale);
            assert_eq!(min, 0.0);
            assert!((cur - f64::from(120.0 * scale)).abs() < 1e-4);
            assert!((max - f64::from(480.0 * scale)).abs() < 1e-4);
            let roundtrip = accesskit_offset_to_gpui(cur, 480.0, scale);
            assert!((roundtrip - (-120.0)).abs() < 1e-3);
        }
        assert_eq!(gpui_offset_to_accesskit(15.0, 0.0, 1.25), (0.0, 0.0, 0.0));
        assert_eq!(accesskit_offset_to_gpui(f64::NAN, 100.0, 1.0), 0.0);
        assert_eq!(accesskit_offset_to_gpui(9999.0, 200.0, 2.0), -200.0);
    }

    #[test]
    fn step_and_percent_helpers_clamp_within_bounds() {
        let vp_f = gpui::size(300.0_f32, 200.0_f32);
        let max_f = gpui::size(100.0_f32, 500.0_f32);

        let down_item = step_scroll_offset(
            point(0.0, 0.0),
            max_f,
            vp_f,
            accesskit::Action::ScrollDown,
            Some(accesskit::ScrollUnit::Item),
        );
        assert_eq!(down_item, point(0.0, -A11Y_SCROLL_ITEM_PX));

        let down_page = step_scroll_offset(
            point(0.0, 0.0),
            max_f,
            vp_f,
            accesskit::Action::ScrollDown,
            Some(accesskit::ScrollUnit::Page),
        );
        assert_eq!(down_page, point(0.0, -(200.0 * A11Y_SCROLL_PAGE_RATIO)));

        let up_clamped = step_scroll_offset(
            point(0.0, -20.0),
            max_f,
            vp_f,
            accesskit::Action::ScrollUp,
            Some(accesskit::ScrollUnit::Page),
        );
        assert_eq!(up_clamped, point(0.0, 0.0));

        assert!((gpui_offset_to_percent(-125.0, 500.0) - 25.0).abs() < 1e-6);
        assert_eq!(percent_to_gpui_offset(50.0, 500.0), -250.0);
        assert_eq!(percent_to_gpui_offset(150.0, 500.0), -500.0);
        assert_eq!(percent_to_gpui_offset(-10.0, 500.0), 0.0);
    }

    struct ScrollAreaHarness {
        theme: ResolvedTheme,
        fixed_handle: ScrollHandle,
        max_h_handle: ScrollHandle,
        item_count: usize,
    }

    impl Render for ScrollAreaHarness {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            let mut bounded = scroll_area("test-max-h", &self.theme)
                .track_scroll(&self.max_h_handle)
                .max_h_viewport(px(240.0))
                .a11y_label("Max height scroll")
                .viewport_style(|v| v.flex().flex_col().gap(px(4.0)));
            for idx in 0..self.item_count {
                bounded = bounded.child(
                    div()
                        .id(("bounded-item", idx))
                        .h(px(40.0))
                        .flex_none()
                        .debug_selector(move || format!("bounded-{idx}")),
                );
            }

            div()
                .id("root")
                .size_full()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div().h(px(200.0)).flex().flex_col().child(
                        scroll_area("test-fixed", &self.theme)
                            .track_scroll(&self.fixed_handle)
                            .a11y_label("Fixed scroll")
                            .scrollbar_inset(px(8.0))
                            .flex_1()
                            .min_h_0()
                            .child(div().h(px(700.0)).w_full().debug_selector(
                                || "fixed-tall-content".to_owned(),
                            )),
                    ),
                )
                .child(bounded)
        }
    }

    #[gpui::test]
    fn scroll_area_preserves_bounds_max_h_shrink_wrap_and_scroll_to_item(
        cx: &mut TestAppContext,
    ) {
        let dark = ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Dark,
                ..ThemeOptions::default()
            },
            false,
        );
        let (view, vcx) = cx.add_window_view(|_, _| ScrollAreaHarness {
            theme: dark,
            fixed_handle: ScrollHandle::new(),
            max_h_handle: ScrollHandle::new(),
            item_count: 2,
        });

        vcx.update(|window, cx| window.draw(cx).clear(cx));
        let fixed_bounds = view.read_with(vcx, |h, _| h.fixed_handle.bounds());
        let fixed_max = view.read_with(vcx, |h, _| h.fixed_handle.max_offset());
        assert_eq!(fixed_bounds.size.height, px(200.0));
        assert!(fixed_max.y > px(400.0));

        // 2 items * 40px + 4px gap = 84px < 240px: shrink-wraps instead of stretching or collapsing.
        let small_bounds = view.read_with(vcx, |h, _| h.max_h_handle.bounds());
        let small_max = view.read_with(vcx, |h, _| h.max_h_handle.max_offset());
        assert!(small_bounds.size.height > px(70.0));
        assert!(small_bounds.size.height < px(120.0));
        assert_eq!(small_max.y, px(0.0));

        // Grow to 15 items (> 600px): caps at 240px and supports `scroll_to_item`.
        view.update(vcx, |h, cx| {
            h.item_count = 15;
            cx.notify();
        });
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        let large_bounds = view.read_with(vcx, |h, _| h.max_h_handle.bounds());
        let large_max = view.read_with(vcx, |h, _| h.max_h_handle.max_offset());
        assert_eq!(large_bounds.size.height, px(240.0));
        assert!(large_max.y > px(300.0));

        view.read_with(vcx, |h, _| h.max_h_handle.scroll_to_item(14));
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        let scrolled_offset =
            view.read_with(vcx, |h, _| h.max_h_handle.offset());
        assert!(scrolled_offset.y < px(-200.0));
    }

    #[gpui::test]
    fn scroll_area_forwards_on_scroll_listener(cx: &mut TestAppContext) {
        use gpui::{PlatformInput, ScrollDelta, ScrollEvent};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };

        struct OnScrollHarness {
            theme: ResolvedTheme,
            handle: ScrollHandle,
            calls: Arc<AtomicUsize>,
            offset_updated_before_cb: Arc<AtomicBool>,
        }

        impl gpui::Render for OnScrollHarness {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut gpui::Context<'_, Self>,
            ) -> impl IntoElement {
                let calls = Arc::clone(&self.calls);
                let cb_handle = self.handle.clone();
                let updated = Arc::clone(&self.offset_updated_before_cb);
                div().w(px(300.)).h(px(200.)).flex().flex_col().child(
                    scroll_area("test-on-scroll", &self.theme)
                        .track_scroll(&self.handle)
                        .a11y_label("Test scroll listener")
                        .flex_1()
                        .min_h_0()
                        .on_scroll(move |_event, _window, _cx| {
                            calls.fetch_add(1, Ordering::Relaxed);
                            if cb_handle.offset().y < px(0.) {
                                updated.store(true, Ordering::Relaxed);
                            }
                        })
                        .child(div().h(px(700.)).w_full()),
                )
            }
        }

        let dark = ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Dark,
                ..ThemeOptions::default()
            },
            false,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let offset_updated_before_cb = Arc::new(AtomicBool::new(false));
        let (view, vcx) = cx.add_window_view(|_, _| OnScrollHarness {
            theme: dark,
            handle: ScrollHandle::new(),
            calls: Arc::clone(&calls),
            offset_updated_before_cb: Arc::clone(&offset_updated_before_cb),
        });

        vcx.update(|window, cx| window.draw(cx).clear(cx));
        let center = view.read_with(vcx, |h, _| h.handle.bounds().center());
        vcx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::Scroll(ScrollEvent {
                    position: center,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
                    ..Default::default()
                }),
                cx,
            );
        });
        assert!(
            calls.load(Ordering::Relaxed) >= 1,
            "on_scroll callback must fire on viewport wheel input"
        );
        assert!(
            offset_updated_before_cb.load(Ordering::Relaxed),
            "scroller must update handle.offset() before on_scroll runs"
        );
    }

    #[test]
    fn inset_scrollbar_handle_preserves_zero_overflow_and_full_extent() {
        use gpui::size;
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Clone)]
        struct FakeHandle {
            vp: Bounds<Pixels>,
            content: Size<Pixels>,
            offset: Rc<Cell<Point<Pixels>>>,
        }

        impl ScrollbarHandle for FakeHandle {
            fn viewport_bounds(&self) -> Bounds<Pixels> {
                self.vp
            }
            fn offset(&self) -> Point<Pixels> {
                self.offset.get()
            }
            fn set_offset(&self, offset: Point<Pixels>) {
                self.offset.set(offset);
            }
            fn content_size(&self) -> Size<Pixels> {
                self.content
            }
        }

        let fitting = InsetScrollbarHandle {
            inner: FakeHandle {
                vp: Bounds::new(
                    point(px(10.), px(20.)),
                    size(px(260.), px(400.)),
                ),
                content: size(px(260.), px(400.)),
                offset: Rc::new(Cell::new(point(px(0.), px(0.)))),
            },
            axis: ScrollbarAxis::Vertical,
            inset: px(8.),
        };
        assert_eq!(fitting.viewport_bounds().origin.y, px(28.));
        assert_eq!(fitting.viewport_bounds().size.height, px(384.));
        assert!(
            fitting.content_size().height
                <= fitting.viewport_bounds().size.height
        );

        let overflowing = InsetScrollbarHandle {
            inner: FakeHandle {
                vp: Bounds::new(
                    point(px(10.), px(20.)),
                    size(px(260.), px(400.)),
                ),
                content: size(px(260.), px(650.)),
                offset: Rc::new(Cell::new(point(px(0.), px(0.)))),
            },
            axis: ScrollbarAxis::Vertical,
            inset: px(8.),
        };
        let overflow_vp = overflowing.viewport_bounds();
        let overflow_content = overflowing.content_size();
        assert_eq!(overflow_vp.size.height, px(384.));
        assert_eq!(overflow_content.height - overflow_vp.size.height, px(250.));

        let tiny = InsetScrollbarHandle {
            inner: FakeHandle {
                vp: Bounds::new(point(px(0.), px(0.)), size(px(100.), px(12.))),
                content: size(px(100.), px(12.)),
                offset: Rc::new(Cell::new(point(px(0.), px(0.)))),
            },
            axis: ScrollbarAxis::Vertical,
            inset: px(8.),
        };
        assert_eq!(tiny.viewport_bounds().size.height, px(0.));
        assert_eq!(tiny.content_size().height, px(0.));

        let horiz_short = InsetScrollbarHandle {
            inner: FakeHandle {
                vp: Bounds::new(
                    point(px(10.), px(20.)),
                    size(px(260.), px(12.)),
                ),
                content: size(px(500.), px(12.)),
                offset: Rc::new(Cell::new(point(px(0.), px(0.)))),
            },
            axis: ScrollbarAxis::Horizontal,
            inset: px(8.),
        };
        assert_eq!(horiz_short.viewport_bounds().origin.x, px(18.));
        assert_eq!(horiz_short.viewport_bounds().size.width, px(244.));
        assert_eq!(horiz_short.viewport_bounds().size.height, px(12.));
        assert_eq!(
            horiz_short.content_size().width
                - horiz_short.viewport_bounds().size.width,
            px(240.)
        );
    }

    #[gpui::test]
    fn scroll_area_max_h_viewport_preserves_explicit_width(
        cx: &mut TestAppContext,
    ) {
        struct WidthHarness {
            theme: ResolvedTheme,
            handle: ScrollHandle,
        }

        impl gpui::Render for WidthHarness {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut gpui::Context<'_, Self>,
            ) -> impl IntoElement {
                div().w(px(600.)).h(px(400.)).flex().flex_col().child(
                    scroll_area("test-explicit-width", &self.theme)
                        .track_scroll(&self.handle)
                        .w(px(280.))
                        .max_h_viewport(px(200.))
                        .child(div().h(px(500.)).w_full()),
                )
            }
        }

        let dark = ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Dark,
                ..ThemeOptions::default()
            },
            false,
        );
        let (view, vcx) = cx.add_window_view(|_, _| WidthHarness {
            theme: dark,
            handle: ScrollHandle::new(),
        });

        vcx.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = view.read_with(vcx, |h, _| h.handle.bounds());
        assert_eq!(bounds.size.width, px(280.0));
        assert_eq!(bounds.size.height, px(200.0));
    }

    #[gpui::test]
    fn scroll_area_horizontal_preserves_intrinsic_height_and_overflows(
        cx: &mut TestAppContext,
    ) {
        struct HorizontalHarness {
            theme: ResolvedTheme,
            handle: ScrollHandle,
        }

        impl gpui::Render for HorizontalHarness {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut gpui::Context<'_, Self>,
            ) -> impl IntoElement {
                div().w(px(240.)).h(px(300.)).flex().flex_col().child(
                    scroll_area("test-horizontal-scroll", &self.theme)
                        .horizontal()
                        .track_scroll(&self.handle)
                        .a11y_label("Test horizontal scroll area")
                        .w_full()
                        .viewport_style(|s| s.pb(px(14.)))
                        .child(div().w(px(640.)).h(px(56.))),
                )
            }
        }

        let dark = ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Dark,
                ..ThemeOptions::default()
            },
            false,
        );
        let (view, vcx) = cx.add_window_view(|_, _| HorizontalHarness {
            theme: dark,
            handle: ScrollHandle::new(),
        });

        vcx.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = view.read_with(vcx, |h, _| h.handle.bounds());
        let max_offset = view.read_with(vcx, |h, _| h.handle.max_offset());
        assert!(
            bounds.size.height >= px(56.0),
            "horizontal auto-height ScrollArea must not collapse in flex_col; got {:?}",
            bounds.size
        );
        assert!(
            max_offset.x > px(300.0),
            "horizontal ScrollArea must expose horizontal overflow; got {max_offset:?}",
        );
    }
}
