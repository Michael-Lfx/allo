//! HTTP surface for insights contribution management.

pub mod routes;
pub mod service;
pub mod state;

pub use routes::insights_routes;
pub use service::InsightsService;
pub use state::InsightsRouterState;
