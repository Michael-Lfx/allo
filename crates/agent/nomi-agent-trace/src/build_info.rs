use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Identity of the binary that recorded a trace, so an event can be matched
/// back to the exact source it ran. A release version alone is not enough:
/// unreleased builds share the workspace version while their code differs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildInfo {
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_dirty: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Unix seconds, as embedded by the host build; absent for unstamped builds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_time: Option<String>,
    pub os: String,
    pub arch: String,
}

static HOST: OnceLock<BuildInfo> = OnceLock::new();

impl BuildInfo {
    /// What a host can know at compile time; `os` and `arch` are filled in.
    pub fn for_host(
        app_version: &str,
        git_sha: Option<&str>,
        git_dirty: Option<bool>,
        profile: Option<&str>,
        build_time: Option<&str>,
    ) -> Self {
        Self {
            app_version: app_version.to_owned(),
            git_sha: git_sha.map(str::to_owned),
            git_dirty,
            profile: profile.map(str::to_owned),
            build_time: build_time.map(str::to_owned),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
        }
    }

    fn fallback() -> Self {
        Self::for_host(env!("CARGO_PKG_VERSION"), None, None, None, None)
    }
}

/// Registers the host binary's build identity. The first call wins; later
/// calls are ignored so a trace never changes identity mid-process.
pub fn set_build_info(info: BuildInfo) {
    let _ = HOST.set(info);
}

/// The registered host identity, or the workspace version when no host
/// registered one (tests, standalone tools).
pub fn build_info() -> BuildInfo {
    HOST.get().cloned().unwrap_or_else(BuildInfo::fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_carries_version_os_and_arch_only() {
        let info = BuildInfo::fallback();

        assert_eq!(info.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.os, std::env::consts::OS);
        assert!(info.git_sha.is_none() && info.git_dirty.is_none());
    }

    #[test]
    fn unset_optional_fields_are_omitted_from_json() {
        let json = serde_json::to_value(BuildInfo::fallback()).unwrap();

        assert!(json.get("git_sha").is_none());
        assert!(json.get("profile").is_none());
        assert_eq!(json["app_version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn full_identity_roundtrips() {
        let info = BuildInfo::for_host("1.5.3", Some("abc1234"), Some(true), Some("release"), Some("1790000000"));

        let back: BuildInfo = serde_json::from_value(serde_json::to_value(&info).unwrap()).unwrap();

        assert_eq!(back, info);
    }
}
