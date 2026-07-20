//! `nomifun-vimax` — HTTP surface for ViMax video generation (`/api/vimax/*`).

pub mod routes;
pub mod service;
pub mod state;

pub use routes::vimax_routes;
pub use service::VimaxApiService;
pub use state::VimaxRouterState;
