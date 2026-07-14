//! `nomifun-poi` — HTTP API for local user interest (POI) topic management.

pub mod preset_starters;
pub mod routes;
pub mod service;
pub mod state;

pub use routes::poi_routes;
pub use service::PoiService;
pub use state::PoiRouterState;
