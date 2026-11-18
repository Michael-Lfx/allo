//! Local Lucide icons used by the GPUI renderer.
//!
//! The SVG sources are vendored under `assets/icons` so icon rendering stays
//! deterministic and does not depend on a network request at runtime.

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    InteractiveElement, Interactivity, IntoElement, LayoutId, Pixels,
    StyleRefinement, Styled, Svg, Window, px, svg,
};

/// A local SVG mask whose default color follows the surrounding text.
///
/// GPUI's raw SVG renderer requires an explicit local color to paint; resolving
/// that color at paint time preserves both inherited and caller-specified colors.
#[must_use]
pub struct Icon {
    svg: Svg,
}

impl std::fmt::Debug for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icon").finish_non_exhaustive()
    }
}

impl IntoElement for Icon {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        self.svg.style()
    }
}

impl InteractiveElement for Icon {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.svg.interactivity()
    }
}

impl Element for Icon {
    type RequestLayoutState = <Svg as Element>::RequestLayoutState;
    type PrepaintState = <Svg as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.svg)
    }
    fn source_location(
        &self,
    ) -> Option<&'static std::panic::Location<'static>> {
        self.svg.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.svg.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.svg.prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let explicit = self.svg.style().text.color;
        if explicit.is_none() {
            self.svg.style().text.color = Some(window.text_style().color);
        }
        self.svg
            .paint(id, inspector, bounds, layout, state, window, cx);
        self.svg.style().text.color = explicit;
    }
}

fn icon(data: &'static [u8]) -> Icon {
    Icon {
        svg: svg().data(data).size(px(16.)),
    }
}

macro_rules! lucide_icons {
    ($($name:ident => $file:literal),+ $(,)?) => {
        $(
            pub fn $name() -> Icon {
                icon(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/icons/",
                    $file
                )))
            }
        )+
    };
}

lucide_icons! {
    arrow_left => "arrow-left.svg",
    arrow_right => "arrow-right.svg",
    arrow_up_right => "arrow-up-right.svg",
    bell => "bell.svg",
    book => "book.svg",
    check => "check.svg",
    chevron_down => "chevron-down.svg",
    chevron_left => "chevron-left.svg",
    chevron_right => "chevron-right.svg",
    chevron_up => "chevron-up.svg",
    chevrons_up_down => "chevrons-up-down.svg",
    circle_alert => "circle-exclamation-point.svg",
    circle_help => "circle-question-mark.svg",
    clock => "clock.svg",
    copy => "copy.svg",
    ellipsis => "ellipsis.svg",
    eye => "eye.svg",
    eye_off => "eye-off.svg",
    file_plus => "file-plus.svg",
    folder => "folder.svg",
    folder_plus => "folder-plus.svg",
    hash => "hash.svg",
    house => "house.svg",
    infinity => "infinity.svg",
    info => "info.svg",
    layout_grid => "layout-grid.svg",
    list => "list.svg",
    sort_desc => "sort-desc.svg",
    image_file => "image-file.svg",
    document_file => "document-file.svg",
    file_fold => "file-fold.svg",
    file_image_mark => "file-image-mark.svg",
    file_document_mark => "file-document-mark.svg",
    globe => "globe.svg",
    app_window => "app-window.svg",
    library => "library.svg",
    list_check => "list-check.svg",
    loader_circle => "loader-circle.svg",
    log_out => "log-out.svg",
    mail => "mail.svg",
    maximize => "maximize.svg",
    minus => "minus.svg",
    monitor => "monitor.svg",
    moon => "moon.svg",
    panel_left => "panel-left.svg",
    play => "play.svg",
    plus => "plus.svg",
    puzzle => "puzzle.svg",
    refresh_cw => "refresh-cw.svg",
    search => "search.svg",
    send => "send.svg",
    sliders_horizontal => "sliders-horizontal.svg",
    sparkles => "sparkles.svg",
    square => "square.svg",
    square_pen => "square-pen.svg",
    star => "star.svg",
    sun => "sun.svg",
    user => "user.svg",
    volume_2 => "volume-2.svg",
    wand_sparkles => "wand-sparkles.svg",
    workflow => "workflow.svg",
    x => "x.svg",
}
