//! HTTP surface for insights contribution management.

pub mod local_analytics;
pub mod routes;
pub mod service;
pub mod state;

pub use local_analytics::LocalAnalytics;
pub use routes::insights_routes;
pub use service::InsightsService;
pub use state::InsightsRouterState;
