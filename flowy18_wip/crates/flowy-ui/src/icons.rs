//! Local Lucide icons used by the GPUI renderer.
//!
//! The SVG sources are vendored under `assets/icons` so icon rendering stays
//! deterministic and does not depend on a network request at runtime.

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Image, ImageFormat, Img,
    InspectorElementId, InteractiveElement, Interactivity, LayoutId, Pixels,
    StyleRefinement, Styled, Svg, TimedTransformation, Transformation, Window,
    img, px, svg,
};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// A local SVG mask whose default color follows the surrounding text.
///
/// GPUI's raw SVG renderer requires an explicit local color to paint; resolving
/// that color at paint time preserves both inherited and caller-specified colors.
#[must_use]
pub struct Icon {
    svg: Svg,
}

impl Icon {
    /// Applies a static GPU transformation to the underlying SVG without
    /// affecting layout or hit-testing.
    pub fn with_transformation(
        mut self,
        transformation: Transformation,
    ) -> Self {
        self.svg = self.svg.with_transformation(transformation);
        self
    }

    /// Applies a continuous GPU timed transformation to the underlying SVG
    /// with zero view re-renders.
    pub fn with_timed_transformation(
        mut self,
        transformation: TimedTransformation,
    ) -> Self {
        self.svg = self.svg.with_timed_transformation(transformation);
        self
    }

    /// Rotates the icon continuously on the GPU clock when motion is active;
    /// stays static under [`crate::MotionPolicy::Reduced`].
    pub fn spinning(self, policy: crate::MotionPolicy) -> Self {
        if policy == crate::MotionPolicy::Reduced {
            self
        } else {
            self.with_timed_transformation(TimedTransformation::rotation(
                Duration::from_millis(900),
                Transformation::default(),
            ))
        }
    }
}

impl std::fmt::Debug for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icon").finish_non_exhaustive()
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
    arrow_up => "arrow-up.svg",
    arrow_up_right => "arrow-up-right.svg",
    bell => "bell.svg",
    book => "book.svg",
    briefcase_business => "briefcase-business.svg",
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
    download => "download.svg",
    ellipsis => "ellipsis.svg",
    eye => "eye.svg",
    eye_off => "eye-off.svg",
    external_link => "external-link.svg",
    file_plus => "file-plus.svg",
    file_text => "file-text.svg",
    folder => "folder.svg",
    folder_open => "folder-open.svg",
    folder_pen => "folder-pen.svg",
    folder_plus => "folder-plus.svg",
    hash => "hash.svg",
    house => "house.svg",
    infinity => "infinity.svg",
    info => "info.svg",
    layout_grid => "layout-grid.svg",
    list => "list.svg",
    sort_desc => "sort-desc.svg",
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
    panel_right => "panel-right.svg",
    pin => "pin.svg",
    play => "play.svg",
    plug => "plug.svg",
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
    trash => "trash.svg",
    user => "user.svg",
    volume_2 => "volume-2.svg",
    wand_sparkles => "wand-sparkles.svg",
    workflow => "workflow.svg",
    x => "x.svg",
}

/// Full-color file-type artwork. Unlike [`Icon`], these SVGs must be decoded as
/// images so their authored fills survive instead of becoming a text-color mask.
macro_rules! file_type_icons {
    ($($name:ident => $file:literal),+ $(,)?) => {
        $(
            pub fn $name() -> Img {
                static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
                img(IMAGE.get_or_init(|| {
                    Arc::new(Image::from_bytes(
                        ImageFormat::Svg,
                        include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/assets/file-types/",
                            $file
                        )).to_vec(),
                    ))
                }).clone())
                .size(px(16.))
            }
        )+
    };
}

file_type_icons! {
    code_file => "CODE.svg",
    docx_file => "DOCX.svg",
    java_file => "JAVA.svg",
    jpg_file => "JPG.svg",
    mp3_file => "MP3.svg",
    mp4_file => "MP4.svg",
    pdf_file => "PDF.svg",
    ppt_file => "PPT.svg",
    xls_file => "XLS.svg",
    zip_file => "ZIP.svg",
}
