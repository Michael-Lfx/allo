//! `nomifun-appearance` — wallpaper scene library: ingest, analysis, and HTTP.
//!
//! Storage lives under `{data_dir}/wallpapers/{id}/` (original bytes, JPEG
//! display/thumb, `analysis.json`). Prefs in the client store only ids and
//! knobs; bitmaps never enter SQLite or CSS.

mod analysis;
mod fsio;
mod library;
mod media;
mod routes;
mod service;
mod state;
mod transcode;

pub use library::{WALLPAPERS_REL_DIR, WallpaperMeta};
pub use routes::{appearance_public_routes, appearance_routes};
pub use service::AppearanceService;
pub use state::AppearanceRouterState;
pub use analysis::WallpaperAnalysis;
