//! Governance: quality scoring, delivery promise enforcement, slideshow-risk
//! gating, and cost accounting. All of this exists to make Rule Zero
//! ("no silent downgrade" — see `assets/CONTRACT.md`) mechanically enforceable
//! rather than merely documented.

pub mod cost;
pub mod delivery_promise;
pub mod scoring;
pub mod slideshow_risk;

pub use cost::{CostDelta, CostEstimate, CostLedger};
pub use delivery_promise::{DeliveryPromise, check_no_silent_downgrade};
pub use scoring::{PUBLISH_QUALITY_GATE, ScoreDimension, ScoringWeights, total_score};
pub use slideshow_risk::{SLIDESHOW_RISK_BLOCK_THRESHOLD, SlideshowRiskInputs, compute_slideshow_risk, is_blocked};
