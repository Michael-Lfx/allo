//! Identity of the local Herdsman (牧马人) engine provider.
//!
//! The row is a process-owned singleton. Its id is a stable UUIDv7 so it
//! satisfies the providers table check, and the platform is reserved so user
//! CRUD cannot overwrite the endpoint or the engine-issued API key.

/// Stable `providers.provider_id` for the Herdsman projection.
pub const HERDSMAN_PROVIDER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000203";

/// `providers.platform` value. Distinct from `nomifun-free-model` so chat
/// pickers that hide the free supply still list this engine.
pub const HERDSMAN_PLATFORM: &str = "herdsman";

pub fn is_herdsman_platform(platform: &str) -> bool {
    platform.trim().eq_ignore_ascii_case(HERDSMAN_PLATFORM)
}

pub fn is_herdsman_provider_id(value: &str) -> bool {
    value == HERDSMAN_PROVIDER_ID
}
