//! `nomifun-tv-show` — shared TV Show HTTP surface for video-generation modes.
//!
//! Browse / like / delete talk only to Flowy cloud. Publish-from-montage and
//! import-to-montage are optional adapters over [`nomi_montage::MontageService`].

pub mod routes;
pub mod service;
pub mod state;

pub use routes::tv_show_routes;
pub use service::TvShowService;
pub use state::TvShowRouterState;
