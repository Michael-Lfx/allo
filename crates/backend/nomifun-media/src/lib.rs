//! HTTP surface for Flowy media settings, credits, and workflow history.

pub mod routes;
pub mod service;
pub mod state;

pub use routes::media_routes;
pub use service::MediaApiService;
pub use state::MediaRouterState;
