//! Conversation artifact browsing. Demo fixtures belong to examples.

pub mod model;
mod preview;
mod preview_view;
pub(crate) mod view;

use gpui::{IntoElement, div, prelude::*};

pub use model::{
    ArtifactAvailability, Category, LibraryItem, LibraryModel, LibrarySort,
    LibraryUsage, LibraryView,
};
pub use preview::MAX_LOCAL_PREVIEW_BYTES;
pub(crate) use preview::{
    TablePreview, is_text_preview_ext, parse_delimited_table,
    pretty_json_if_valid,
};
pub(crate) use preview_view::render_table_preview;
pub use preview_view::{PreviewEvent, PreviewPanel};
pub(crate) use view::artifact_file_icon;
pub use view::{LibraryEvent, LibraryPanel};

/// Names a composed control for UI tests without changing layout, semantics or
/// paint order: the wrapper only forwards its single child.
///
/// `.debug_selector` records bounds only in debug builds with `test-support`
/// and is a no-op everywhere else.
pub(crate) fn test_control(
    selector: &'static str,
    element: impl IntoElement,
) -> impl IntoElement {
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .child(element)
}
