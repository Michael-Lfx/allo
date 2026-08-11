//! Read-only Creative IR projection for Canvas materialization.

pub mod ir;
pub mod scan;

pub use ir::{CreativeFilm, CreativeMediaKind, CreativeMediaRef, CreativeScene, CreativeShot, CREATIVE_IR_VERSION};
pub use scan::scan_project;
