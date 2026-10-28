//! Host-agnostic core for Flowy Link.
//!
//! Screens talk to [`ExecutionHost`]. The PC adapter and a future on-device
//! adapter both implement that seam. This crate does not open sockets.

mod cursor;
mod endpoint;
mod host;
mod pairing;
mod surface;
mod wake;

pub use cursor::{ReplayPlan, plan_replay};
pub use endpoint::{Endpoint, EndpointSource, Probe, choose_endpoint};
pub use host::{Capability, Command, ExecutionHost, HostKind, HostRecord, RemoteAlloHost};
pub use pairing::{
    PairingUrl, PairingUrlError, LINK_TLS_PORT, build_pairing_url, fingerprint_sha256, parse_pairing_url,
};
pub use surface::{LinkClass, classify_route};
pub use wake::{WakeKind, WakePayload, WakePayloadError};

/// Constant-time equality for equal-length secrets. Unequal lengths return false
/// without comparing bytes.
pub fn secrets_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
