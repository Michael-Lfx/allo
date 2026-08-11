//! `nomifun-montage` — HTTP surface for Montage agent production (`/api/montage/*`).

pub mod materialize;
pub mod routes;
pub mod service;
pub mod state;

pub use routes::montage_routes;
pub use service::MontageApiService;
pub use state::MontageRouterState;
