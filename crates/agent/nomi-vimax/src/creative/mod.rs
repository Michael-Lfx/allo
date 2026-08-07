//! Creative IR — high-fidelity bridge between ViMax Agent artifacts and Canvas.

mod canvas_doc;
mod ir;
mod scan;

#[cfg(test)]
mod tests;

pub use canvas_doc::{build_canvas_document, MediaIdMap};
pub use ir::{
    CreativeCharacter, CreativeFilm, CreativeMediaFile, CreativeMediaKind, CreativeScene,
    CreativeShot, CreativeWorldAsset, CreativeWorldKind, CREATIVE_IR_VERSION,
};
pub use scan::scan_session_film;
