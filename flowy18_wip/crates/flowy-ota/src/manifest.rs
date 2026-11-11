use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
    pub platforms: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub rollout: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformArtifact {
    pub url: String,
    pub signature: String,
}

pub fn platform_key() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-x86_64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-aarch64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-aarch64"
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
    )))]
    {
        "unknown"
    }
}

pub fn artifact_for_platform(manifest: &Manifest) -> Option<PlatformArtifact> {
    let key = platform_key();
    let value = manifest.platforms.get(key)?;
    serde_json::from_value(value.clone()).ok()
}

/// True when `candidate` is a strictly greater dotted version than `current`.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    let current = parse_version(current);
    let candidate = parse_version(candidate);
    candidate > current
}

fn parse_version(raw: &str) -> [u64; 3] {
    let trimmed = raw.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|p| {
            let digits: String =
                p.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .unwrap_or(0);
    [major, minor, patch]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_compare_by_semver_fields() {
        assert!(is_newer("1.8.0", "1.8.1"));
        assert!(is_newer("1.8.0", "1.9.0"));
        assert!(!is_newer("1.8.0", "1.8.0"));
        assert!(!is_newer("1.8.0", "1.7.9"));
    }
}
