//! Zero-global GPU visualization primitives built on `gpui_kit::base::plot::shape`
//! and `gpui::canvas`.
//!
//! Unlike `gpui_kit::component::chart::*`, these primitives never read a process-global
//! `Theme`; every color comes explicitly from [`ResolvedTheme`], matching Flowy's
//! window-scoped theme contract and headless `#[gpui::test]` requirements.

use std::f32::consts::TAU;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Bounds, ElementId, FontWeight, Hsla,
    IntoElement, PathBuilder, Pixels, Rgba, SharedString, Window, canvas, div,
    point, prelude::*, px,
};
use gpui_kit::base::plot::shape::{Arc, ArcData, Pie};

use crate::{MotionPolicy, ResolvedTheme, motion};

/// One weighted slice inside [`donut_chart`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DonutSegment {
    pub value: f32,
    pub color: Rgba,
}

impl DonutSegment {
    #[must_use]
    pub const fn new(value: f32, color: Rgba) -> Self {
        Self { value, color }
    }
}

/// Visual state of a connector between nodes inside the flowchart modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowEdgeStyle {
    /// Solid line + filled arrow in verified/active tint.
    Completed,
    /// Animated dashed flow line when work is actively executing.
    Flowing,
    /// Subtle static dashed line for pending downstream steps.
    Pending,
}

/// Compact GPU hollow donut ring with a centered headline metric.
pub fn donut_chart(
    id: impl Into<ElementId>,
    size_px: f32,
    segments: Vec<DonutSegment>,
    center_label: impl Into<SharedString>,
    theme: &ResolvedTheme,
) -> AnyElement {
    let track_color = Hsla::from(theme.soft());
    let ink = theme.ink();
    let center_label = center_label.into();
    let thickness = (size_px * 0.13).clamp(4.0, 8.0);
    let has_data = segments.iter().any(|segment| segment.value > 0.0);

    let ring = canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, _cx| bounds,
        move |bounds, _, window, _cx| {
            let side =
                f32::from(bounds.size.width).min(f32::from(bounds.size.height));
            let outer = side.mul_add(0.5, -1.0).max(2.0);
            let inner = (outer - thickness).max(1.0);
            let arc = Arc::new().inner_radius(inner).outer_radius(outer);

            // Always draw full track ring so empty or sparse states read clearly.
            let full = ArcData::new(&(), 0, 1.0, 0.0, TAU - 0.002);
            arc.paint(&full, track_color, &bounds, window);

            if !has_data {
                return;
            }
            let non_zero: Vec<DonutSegment> = segments
                .iter()
                .copied()
                .filter(|seg| seg.value > 0.0)
                .collect();
            let pad = if non_zero.len() > 1 { 0.06 } else { 0.0 };
            let slices = Pie::new()
                .value(|seg: &DonutSegment| Some(seg.value))
                .pad_angle(pad)
                .arcs(&non_zero);
            for slice in &slices {
                arc.paint(slice, Hsla::from(slice.data.color), &bounds, window);
            }
        },
    )
    .size_full();

    let elem_id: ElementId = id.into();
    let shell = div()
        .id(elem_id.clone())
        .relative()
        .size(px(size_px))
        .flex_none()
        .child(ring)
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(ink)
                .child(center_label),
        );

    if motion::travel_scale(theme.motion_policy()) > 0.0 {
        shell
            .with_animation(
                (elem_id, "donut"),
                Animation::new(motion::MODERATE)
                    .with_easing(motion::out_quint()),
                |el, delta| el.opacity(0.35_f32.mul_add(delta, 0.65)),
            )
            .into_any_element()
    } else {
        shell.into_any_element()
    }
}

/// Compact circular progress indicator for flowchart skill/work nodes.
pub fn progress_ring(
    id: impl Into<ElementId>,
    size_px: f32,
    percent: f32,
    color: Rgba,
    running: bool,
    theme: &ResolvedTheme,
) -> AnyElement {
    let clamped = percent.clamp(0.0, 100.0);
    let track = Hsla::from(theme.soft());
    let stroke = Hsla::from(color);
    let thickness = (size_px * 0.14).clamp(2.2, 4.0);
    let policy = theme.motion_policy();

    let ring_canvas = canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, _cx| bounds,
        move |bounds, _, window, _cx| {
            let side =
                f32::from(bounds.size.width).min(f32::from(bounds.size.height));
            let outer = side.mul_add(0.5, -0.5).max(2.0);
            let inner = (outer - thickness).max(1.0);
            let arc = Arc::new().inner_radius(inner).outer_radius(outer);

            let track_slice = ArcData::new(&(), 0, 1.0, 0.0, TAU - 0.002);
            arc.paint(&track_slice, track, &bounds, window);

            let sweep = ((clamped / 100.0) * (TAU - 0.004)).max(0.22);
            let slice = ArcData::new(&(), 0, clamped, 0.0, sweep);
            arc.paint(&slice, stroke, &bounds, window);
        },
    )
    .size(px(size_px))
    .flex_none();

    let elem_id: ElementId = id.into();
    if running && motion::travel_scale(policy) > 0.0 {
        div()
            .id(elem_id.clone())
            .size(px(size_px))
            .flex_none()
            .child(ring_canvas)
            .with_animation(
                (elem_id, "ring"),
                Animation::new(Duration::from_millis(1600))
                    .repeat()
                    .with_easing(motion::pulsing_sine()),
                |el, t| el.opacity(0.25_f32.mul_add(t, 0.75)),
            )
            .into_any_element()
    } else {
        div()
            .id(elem_id)
            .size(px(size_px))
            .flex_none()
            .child(ring_canvas)
            .into_any_element()
    }
}

/// Horizontal directed edge between two flowchart nodes (solid, dashed, or animated flow).
pub fn flow_connector(
    id: impl Into<ElementId>,
    width_px: f32,
    color: Rgba,
    style: FlowEdgeStyle,
    policy: MotionPolicy,
) -> AnyElement {
    let fill_color = Hsla::from(color);
    let dashed = !matches!(style, FlowEdgeStyle::Completed);
    let edge = canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, _cx| bounds,
        move |bounds, _, window, _cx| {
            let x0 = f32::from(bounds.origin.x);
            let y_mid = f32::from(bounds.size.height)
                .mul_add(0.5, f32::from(bounds.origin.y));
            let w = f32::from(bounds.size.width);
            let arrow_w = 6.0_f32;
            let line_end = (x0 + w - arrow_w).max(x0);
            let half_stroke = 1.0_f32;

            if dashed {
                for step in 0_u16..256 {
                    let cursor = f32::from(step).mul_add(9.0, x0);
                    if cursor >= line_end {
                        break;
                    }
                    let seg_end = (cursor + 5.0).min(line_end);
                    paint_rect(
                        cursor,
                        y_mid - half_stroke,
                        seg_end,
                        y_mid + half_stroke,
                        fill_color,
                        window,
                    );
                }
            } else {
                paint_rect(
                    x0,
                    y_mid - half_stroke,
                    line_end,
                    y_mid + half_stroke,
                    fill_color,
                    window,
                );
            }

            // Filled arrowhead pointing right.
            let tip_x = x0 + w;
            let base_x = (tip_x - arrow_w).max(x0);
            let mut arrow = PathBuilder::fill();
            arrow.move_to(point(px(base_x), px(y_mid - 4.0)));
            arrow.line_to(point(px(tip_x), px(y_mid)));
            arrow.line_to(point(px(base_x), px(y_mid + 4.0)));
            arrow.line_to(point(px(base_x), px(y_mid - 4.0)));
            if let Ok(path) = arrow.build() {
                window.paint_path(path, fill_color);
            }
        },
    )
    .w(px(width_px))
    .h(px(24.))
    .flex_none();

    let elem_id: ElementId = id.into();
    if matches!(style, FlowEdgeStyle::Flowing)
        && motion::travel_scale(policy) > 0.0
    {
        div()
            .id(elem_id.clone())
            .w(px(width_px))
            .h(px(24.))
            .flex_none()
            .child(edge)
            .with_animation(
                (elem_id, "flow"),
                Animation::new(Duration::from_millis(1200))
                    .repeat()
                    .with_easing(motion::pulsing_sine()),
                |el, t| el.opacity(0.45_f32.mul_add(t, 0.55)),
            )
            .into_any_element()
    } else {
        div()
            .id(elem_id)
            .w(px(width_px))
            .h(px(24.))
            .flex_none()
            .child(edge)
            .into_any_element()
    }
}

fn paint_rect(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: Hsla,
    window: &mut Window,
) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let mut builder = PathBuilder::fill();
    builder.move_to(point(px(x0), px(y0)));
    builder.line_to(point(px(x1), px(y0)));
    builder.line_to(point(px(x1), px(y1)));
    builder.line_to(point(px(x0), px(y1)));
    builder.line_to(point(px(x0), px(y0)));
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResolvedTheme;
    use gpui::{Context, Render};

    struct ChartHarness {
        theme: ResolvedTheme,
        segments: Vec<DonutSegment>,
        center_label: &'static str,
        percent: f32,
        running: bool,
        edge_style: FlowEdgeStyle,
        policy: MotionPolicy,
    }

    impl Render for ChartHarness {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            div()
                .w(px(320.))
                .h(px(160.))
                .flex()
                .items_center()
                .gap_3()
                .child(donut_chart(
                    "test-donut",
                    54.0,
                    self.segments.clone(),
                    self.center_label,
                    &self.theme,
                ))
                .child(progress_ring(
                    "test-ring",
                    26.0,
                    self.percent,
                    self.theme.status_active(),
                    self.running,
                    &self.theme,
                ))
                .child(flow_connector(
                    "test-edge",
                    32.0,
                    self.theme.status_verified(),
                    self.edge_style,
                    self.policy,
                ))
        }
    }

    #[gpui::test]
    fn charts_paint_without_global_component_theme_init(
        cx: &mut gpui::TestAppContext,
    ) {
        let theme = ResolvedTheme::default();
        let segments = vec![
            DonutSegment::new(2.0, theme.status_attention()),
            DonutSegment::new(3.0, theme.status_active()),
            DonutSegment::new(5.0, theme.status_verified()),
        ];
        let (_view, cx) = cx.add_window_view(|_, _| ChartHarness {
            theme,
            segments,
            center_label: "10",
            percent: 72.0,
            running: true,
            edge_style: FlowEdgeStyle::Flowing,
            policy: MotionPolicy::Full,
        });
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    #[gpui::test]
    fn empty_donut_and_reduced_motion_render_cleanly(
        cx: &mut gpui::TestAppContext,
    ) {
        let dark = ResolvedTheme::resolve(crate::ThemeOptions::default(), true);
        let (_view, cx) = cx.add_window_view(|_, _| ChartHarness {
            theme: dark,
            segments: Vec::new(),
            center_label: "0",
            percent: 0.0,
            running: false,
            edge_style: FlowEdgeStyle::Pending,
            policy: MotionPolicy::Reduced,
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}
