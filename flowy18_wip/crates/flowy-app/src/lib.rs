//! Flowy desktop library: app state, screens host, and brand wiring.

pub mod app;
mod brand;
#[cfg(test)]
mod brand_manifest;
mod disk;
mod persona;
pub mod prefs;
pub mod session;

pub use flowy_agent as agent;
pub use flowy_i18n as i18n;

use flowy_obs::Telemetry;
use std::sync::{Arc, OnceLock};

static TELEMETRY: OnceLock<Arc<Telemetry>> = OnceLock::new();

pub(crate) fn telemetry() -> Option<&'static Arc<Telemetry>> {
    TELEMETRY.get()
}

pub fn set_telemetry(telemetry: Arc<Telemetry>) {
    let _ = TELEMETRY.set(telemetry);
}

pub mod brand_api {
    pub use crate::brand::*;
}
