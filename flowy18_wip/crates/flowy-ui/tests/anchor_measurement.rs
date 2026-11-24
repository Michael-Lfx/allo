//! Popover anchors are measured from the trigger's own prepaint pass.
//!
//! `gpui-pre` dropped `InteractiveElement::on_prepaint`, so `anchor_trigger`
//! records bounds through [`flowy_ui::PrepaintBounds`]. The regression guarded
//! here: without that measurement `OverlayHost::anchor_bounds` stays `None` and
//! every popover renders as an empty element, so nothing ever shows up.

use flowy_ui::{ThemeOptions, UiContext, anchor_trigger, popover};
use gpui::{
    Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, Styled, TestAppContext, VisualTestContext, Window, div, px,
};

struct AnchorHarness {
    ui: UiContext,
    show_popover: bool,
}

impl Render for AnchorHarness {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        self.ui
            .overlays
            .update(cx, |host, _| host.begin_frame(window));
        let root = div().id("root").size_full().child(
            anchor_trigger(div().id("trigger"), "trigger-anchor", &self.ui)
                .w(px(120.))
                .h(px(32.)),
        );
        if !self.show_popover {
            return root;
        }
        root.child(popover(
            "anchor-popover",
            "trigger-anchor",
            // Only laid out when the popover actually renders, so these bounds
            // are the observable proof that the floating content reached the
            // element tree.
            anchor_trigger(
                div().id("popover-probe"),
                "popover-probe-anchor",
                &self.ui,
            ),
            &self.ui,
            window,
            cx,
            false,
        ))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(Window::simulate_next_frame);
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn anchor_bounds(
    cx: &mut VisualTestContext,
    view: &Entity<AnchorHarness>,
    anchor: &str,
) -> Option<Bounds<Pixels>> {
    cx.update(|_, cx| view.read(cx).ui.overlays.read(cx).anchor_bounds(anchor))
}

#[gpui::test]
fn prepaint_records_the_trigger_bounds(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| AnchorHarness {
        ui: UiContext::new(ThemeOptions::default(), false, cx),
        show_popover: false,
    });

    // Opening the window prepaints one frame, which is where the trigger's
    // bounds must be recorded. Before the fix this stayed `None` forever.
    let bounds = anchor_bounds(cx, &view, "trigger-anchor")
        .expect("anchor_trigger records the trigger bounds during prepaint");
    // Width and height are applied through the wrapper's `Styled` delegation,
    // so this also proves styling still reaches the trigger.
    assert_eq!(bounds.size.width, px(120.));
    assert_eq!(bounds.size.height, px(32.));
}

#[gpui::test]
fn popover_content_renders_once_the_anchor_is_measured(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|_, cx| AnchorHarness {
        ui: UiContext::new(ThemeOptions::default(), false, cx),
        show_popover: true,
    });
    assert!(
        anchor_bounds(cx, &view, "popover-probe-anchor").is_none(),
        "the first frame only measures the trigger, so the popover is empty"
    );

    draw(cx);

    assert!(
        anchor_bounds(cx, &view, "trigger-anchor").is_some(),
        "the trigger has bounds after prepaint"
    );
    assert!(
        anchor_bounds(cx, &view, "popover-probe-anchor").is_some(),
        "popover content renders once the anchor has bounds"
    );
}
