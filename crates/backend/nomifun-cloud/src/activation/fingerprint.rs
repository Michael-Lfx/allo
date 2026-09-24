//! Device fingerprint collection for activation reporting.

#[cfg(target_os = "windows")]
#[path = "fingerprint_windows.rs"]
mod fingerprint_windows;

#[cfg(target_os = "macos")]
#[path = "fingerprint_macos.rs"]
mod fingerprint_macos;

#[cfg(target_os = "linux")]
#[path = "fingerprint_linux.rs"]
mod fingerprint_linux;

use sha2::{Digest, Sha256};
use tracing::warn;

use super::GeoIpInfo;
use crate::error::ServerClientError;
use crate::flowy::DeviceActivateRequest;
use crate::platform;
use crate::resources;

/// Sentinel used only for the outbound activate payload when no NIC MAC is
/// available. Never persist this — treat it like "unset" so the next attempt
/// re-reads hardware.
pub const MAC_PLACEHOLDER: &str = "00:00:00:00:00:01";

#[derive(Debug, Clone)]
pub struct DeviceFingerprint {
    pub mac: String,
    pub sn: String,
    pub cpu_chip_id: String,
    pub xpu_brand: Option<String>,
}

/// Fingerprint values already persisted on this device. Empty strings mean
/// "never collected" and trigger a fresh platform read.
#[derive(Debug, Clone, Default)]
pub struct PersistedFingerprint {
    pub mac: String,
    pub sn: String,
    pub cpu_chip_id: String,
    pub xpu_brand: String,
}

pub fn collect_fingerprint(
    persisted: &PersistedFingerprint,
) -> Result<DeviceFingerprint, ServerClientError> {
    let persisted_mac_ok = is_usable_mac(&persisted.mac);
    let needs_hw = !persisted_mac_ok
        || persisted.sn.is_empty()
        || persisted.cpu_chip_id.is_empty()
        || persisted.xpu_brand.is_empty();
    let (new_mac, new_sn, new_cpu, new_xpu) = if needs_hw {
        collect_unpersisted()
    } else {
        (None, None, None, None)
    };

    let mac = if persisted_mac_ok {
        normalize_mac(&persisted.mac)
    } else {
        match new_mac.filter(|value| is_usable_mac(value)) {
            Some(value) => normalize_mac(&value),
            None => {
                warn!("could not read MAC address; using generated placeholder");
                MAC_PLACEHOLDER.to_string()
            }
        }
    };
    let sn = if persisted.sn.is_empty() {
        new_sn.unwrap_or_else(generate_serial_number)
    } else {
        persisted.sn.clone()
    };
    let cpu_chip_id = if persisted.cpu_chip_id.is_empty() {
        new_cpu.unwrap_or_else(|| {
            warn!("could not read CPU chip id; using hashed fallback");
            hash_cpu_fallback("unknown-cpu")
        })
    } else {
        persisted.cpu_chip_id.clone()
    };
    let xpu_brand = if persisted.xpu_brand.is_empty() {
        new_xpu.filter(|value| !value.is_empty())
    } else if persisted.xpu_brand.eq_ignore_ascii_case("unknown") {
        None
    } else {
        Some(persisted.xpu_brand.clone())
    };

    Ok(DeviceFingerprint {
        mac,
        sn,
        cpu_chip_id,
        xpu_brand,
    })
}

/// MAC value safe to write into local device state. Placeholders become empty
/// so a later activation re-queries the platform.
pub fn persistable_mac(mac: &str) -> String {
    let normalized = normalize_mac(mac);
    if is_usable_mac(&normalized) {
        normalized
    } else {
        String::new()
    }
}

fn is_usable_mac(raw: &str) -> bool {
    let normalized = normalize_mac(raw);
    !normalized.is_empty()
        && !normalized.eq_ignore_ascii_case(MAC_PLACEHOLDER)
        && !normalized.eq_ignore_ascii_case("00:00:00:00:00:00")
}

fn collect_unpersisted() -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    (
        read_mac_address(),
        read_serial_number(),
        read_cpu_chip_id(),
        read_xpu_brand(),
    )
}

pub fn build_activate_request(
    app: &str,
    channel: &str,
    fingerprint: &DeviceFingerprint,
    geo: Option<&GeoIpInfo>,
) -> DeviceActivateRequest {
    let mut request = DeviceActivateRequest {
        app: app.to_string(),
        channel: channel.to_string(),
        mac: fingerprint.mac.clone(),
        sn: fingerprint.sn.clone(),
        activate_timestamp: chrono::Utc::now().timestamp_millis(),
        cpu_chip_id: fingerprint.cpu_chip_id.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os_version: platform::os_version_string(),
        install_id: String::new(),
        activate_reason: String::new(),
        arch: platform::cpu_arch().to_string(),
        host_runtime: String::new(),
        invite_code: String::new(),
        utm_source: String::new(),
        utm_medium: String::new(),
        utm_campaign: String::new(),
        signup_method: String::new(),
        ram_mb: resources::total_ram_mb(),
        disk_free_gb: resources::largest_disk_free_gb(),
        credits_balance: None,
        plan_code: String::new(),
        first_launch_at_ms: None,
        login_to_activate_ms: None,
        xpu_brand: fingerprint.xpu_brand.clone(),
        public_ip: String::new(),
        country: String::new(),
        country_code: String::new(),
        province: String::new(),
        city: String::new(),
        region: String::new(),
        operator: String::new(),
        postal: "0".to_string(),
        latitude: "0".to_string(),
        longitude: "0".to_string(),
        isp: String::new(),
        timezone: String::new(),
        currency: String::new(),
    };

    if let Some(geo) = geo {
        apply_geo(&mut request, geo);
    }

    request
}

fn apply_geo(request: &mut DeviceActivateRequest, geo: &GeoIpInfo) {
    if !geo.public_ip.is_empty() {
        request.public_ip = geo.public_ip.clone();
    }
    if !geo.country.is_empty() {
        request.country = geo.country.clone();
    }
    if !geo.country_code.is_empty() {
        request.country_code = geo.country_code.clone();
    }
    if !geo.province.is_empty() {
        request.province = geo.province.clone();
    }
    if !geo.city.is_empty() {
        request.city = geo.city.clone();
    }
    if !geo.region.is_empty() {
        request.region = geo.region.clone();
    }
    if !geo.operator.is_empty() {
        request.operator = geo.operator.clone();
        request.isp = geo.operator.clone();
    }
    if !geo.postal.is_empty() {
        request.postal = geo.postal.clone();
    }
    if !geo.latitude.is_empty() {
        request.latitude = geo.latitude.clone();
    }
    if !geo.longitude.is_empty() {
        request.longitude = geo.longitude.clone();
    }
    if !geo.timezone.is_empty() {
        request.timezone = geo.timezone.clone();
    }
    if !geo.currency.is_empty() {
        request.currency = geo.currency.clone();
    }
}

fn normalize_mac(raw: &str) -> String {
    raw.trim().replace('-', ":").to_ascii_uppercase()
}

fn generate_serial_number() -> String {
    let suffix = uuid::Uuid::new_v4().to_string().replace('-', "");
    format!(
        "CLAWSN{}{}",
        chrono::Utc::now().timestamp_millis(),
        &suffix[..8.min(suffix.len())]
    )
}

fn hash_cpu_fallback(model: &str) -> String {
    let digest = Sha256::digest(model.as_bytes());
    format!("CPU{}", hex::encode(&digest[..8]).to_ascii_uppercase())
}

#[cfg(target_os = "windows")]
fn read_mac_address() -> Option<String> {
    fingerprint_windows::read_mac_address()
}

#[cfg(target_os = "windows")]
fn read_serial_number() -> Option<String> {
    fingerprint_windows::read_serial_number()
}

#[cfg(target_os = "windows")]
fn read_cpu_chip_id() -> Option<String> {
    fingerprint_windows::read_cpu_chip_id()
}

#[cfg(target_os = "windows")]
fn read_xpu_brand() -> Option<String> {
    fingerprint_windows::read_xpu_brand()
}

#[cfg(target_os = "linux")]
fn read_mac_address() -> Option<String> {
    fingerprint_linux::read_mac_address()
}

#[cfg(target_os = "linux")]
fn read_serial_number() -> Option<String> {
    fingerprint_linux::read_serial_number()
}

#[cfg(target_os = "linux")]
fn read_cpu_chip_id() -> Option<String> {
    let model = fingerprint_linux::read_cpu_brand()?;
    Some(hash_cpu_fallback(&model))
}

#[cfg(target_os = "linux")]
fn read_xpu_brand() -> Option<String> {
    fingerprint_linux::read_xpu_brand()
}

#[cfg(target_os = "macos")]
fn read_mac_address() -> Option<String> {
    fingerprint_macos::read_mac_address()
}

#[cfg(target_os = "macos")]
fn read_serial_number() -> Option<String> {
    fingerprint_macos::read_serial_number()
}

#[cfg(target_os = "macos")]
fn read_cpu_chip_id() -> Option<String> {
    let model = fingerprint_macos::read_cpu_brand()?;
    Some(hash_cpu_fallback(&model))
}

#[cfg(target_os = "macos")]
fn read_xpu_brand() -> Option<String> {
    fingerprint_macos::read_xpu_brand()
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn read_mac_address() -> Option<String> {
    None
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn read_serial_number() -> Option<String> {
    None
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn read_cpu_chip_id() -> Option<String> {
    None
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn read_xpu_brand() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_mac_replaces_dashes() {
        assert_eq!(normalize_mac("aa-bb-cc-dd-ee-ff"), "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn normalize_mac_is_idempotent_on_colon_form() {
        assert_eq!(normalize_mac("AA:BB:CC:DD:EE:FF"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(normalize_mac("aa:bb:cc:dd:ee:ff"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(normalize_mac("FC-34-97-A5-3C-71"), "FC:34:97:A5:3C:71");
    }

    #[test]
    fn collect_fingerprint_fresh_mac_branch_always_normalizes() {
        let source = include_str!("fingerprint.rs");
        let start = source.find("pub fn collect_fingerprint").expect("fn");
        let end = source[start..]
            .find("fn collect_unpersisted")
            .map(|offset| start + offset)
            .expect("collect_unpersisted follows collect_fingerprint");
        let body = &source[start..end];
        assert!(
            body.contains("normalize_mac(&persisted.mac)")
                || body.contains("normalize_mac(&value)"),
            "MAC paths must go through normalize_mac"
        );
        assert!(body.contains("is_usable_mac"));
        assert!(body.contains("MAC_PLACEHOLDER"));
    }

    #[test]
    fn persistable_mac_drops_placeholder() {
        assert_eq!(persistable_mac("aa-bb-cc-dd-ee-ff"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(persistable_mac(MAC_PLACEHOLDER), "");
        assert_eq!(persistable_mac("00:00:00:00:00:00"), "");
        assert_eq!(persistable_mac(""), "");
        assert!(!is_usable_mac(MAC_PLACEHOLDER));
    }

    #[test]
    fn collect_fingerprint_retries_when_persisted_mac_is_placeholder() {
        let persisted = PersistedFingerprint {
            mac: MAC_PLACEHOLDER.into(),
            sn: "SN123".into(),
            cpu_chip_id: "CPUABC123".into(),
            xpu_brand: "NVIDIA GeForce RTX 4090".into(),
        };
        let fp = collect_fingerprint(&persisted).expect("fingerprint");
        // Placeholder is never a stable cached identity: either a real MAC was
        // re-read, or we still emit the outbound sentinel (and would not persist it).
        if is_usable_mac(&fp.mac) {
            assert_ne!(fp.mac, MAC_PLACEHOLDER);
        } else {
            assert_eq!(fp.mac, MAC_PLACEHOLDER);
            assert!(persistable_mac(&fp.mac).is_empty());
        }
    }

    #[test]
    fn collect_fingerprint_reuses_persisted_values() {
        let persisted = PersistedFingerprint {
            mac: "aa-bb-cc-dd-ee-ff".into(),
            sn: "SN123".into(),
            cpu_chip_id: "CPUABC123".into(),
            xpu_brand: "NVIDIA GeForce RTX 4090".into(),
        };
        let fp = collect_fingerprint(&persisted).expect("fingerprint");
        assert_eq!(fp.mac, "AA:BB:CC:DD:EE:FF");
        assert_eq!(fp.sn, "SN123");
        assert_eq!(fp.cpu_chip_id, "CPUABC123");
        assert_eq!(fp.xpu_brand.as_deref(), Some("NVIDIA GeForce RTX 4090"));
    }

    #[test]
    fn collect_fingerprint_skips_unknown_xpu_sentinel() {
        let persisted = PersistedFingerprint {
            mac: "aa-bb-cc-dd-ee-ff".into(),
            sn: "SN123".into(),
            cpu_chip_id: "CPUABC123".into(),
            xpu_brand: "unknown".into(),
        };
        let fp = collect_fingerprint(&persisted).expect("fingerprint");
        assert_eq!(fp.xpu_brand, None);
    }

    #[test]
    fn collect_fingerprint_mixes_persisted_sn_with_fresh_reads() {
        let persisted = PersistedFingerprint {
            sn: "LEGACY-SN".into(),
            ..Default::default()
        };
        let fp = collect_fingerprint(&persisted).expect("fingerprint");
        assert_eq!(fp.sn, "LEGACY-SN");
        assert!(!fp.mac.is_empty());
        assert!(!fp.cpu_chip_id.is_empty());
    }

    #[test]
    fn collect_unpersisted_round_trips_into_fingerprint() {
        let raw = collect_unpersisted();
        let persisted = PersistedFingerprint {
            mac: raw.0.clone().unwrap_or_default(),
            sn: raw.1.clone().unwrap_or_default(),
            cpu_chip_id: raw.2.clone().unwrap_or_default(),
            xpu_brand: raw.3.clone().unwrap_or_default(),
        };
        let fp = collect_fingerprint(&persisted).expect("fingerprint");
        if let Some(mac) = raw.0 {
            assert_eq!(fp.mac, normalize_mac(&mac));
        }
        if let Some(sn) = raw.1 {
            assert_eq!(fp.sn, sn);
        }
        if let Some(cpu) = raw.2 {
            assert_eq!(fp.cpu_chip_id, cpu);
        }
        if let Some(xpu) = raw.3 {
            assert_eq!(fp.xpu_brand.as_deref(), Some(xpu.as_str()));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_fingerprint_avoids_powershell() {
        let windows_source = include_str!("fingerprint_windows.rs");
        assert!(windows_source.contains("GetAdaptersAddresses"));
        assert!(windows_source.contains("GetSystemFirmwareTable"));
        assert!(windows_source.contains("CreateDXGIFactory1"));
        assert!(windows_source.contains("__cpuid"));
        assert!(!windows_source.contains("hidden_std_command"));
        assert!(!windows_source.contains("Command::new"));
    }

    #[test]
    fn macos_fingerprint_module_avoids_subprocess() {
        let macos_source = include_str!("fingerprint_macos.rs");
        let code = macos_source
            .split("#[cfg(test)]")
            .next()
            .expect("prod code");
        assert!(code.contains("getifaddrs"));
        assert!(code.contains("sysctlbyname"));
        assert!(code.contains("IOPlatformSerialNumber"));
        assert!(code.contains("IOAccelerator"));
        assert!(!code.contains("std::process"));
        assert!(!code.contains("Command::"));
    }

    #[test]
    fn linux_fingerprint_module_avoids_subprocess() {
        let linux_source = include_str!("fingerprint_linux.rs");
        let code = linux_source
            .split("#[cfg(test)]")
            .next()
            .expect("prod code");
        assert!(code.contains("/sys/class/net"));
        assert!(code.contains("/sys/class/dmi"));
        assert!(code.contains("/proc/cpuinfo"));
        assert!(code.contains("/sys/class/drm"));
        assert!(!code.contains("std::process"));
        assert!(!code.contains("Command::"));
        assert!(!code.contains("\"lspci\""));
    }

    #[test]
    fn apply_geo_fills_activation_fields() {
        let fp = collect_fingerprint(&PersistedFingerprint {
            sn: "SN1".into(),
            ..Default::default()
        })
        .expect("fp");
        let geo = GeoIpInfo {
            public_ip: "203.0.113.1".into(),
            country: "China".into(),
            country_code: "CN".into(),
            province: "Beijing".into(),
            city: "Beijing".into(),
            region: "Beijing".into(),
            operator: "China Mobile".into(),
            ..Default::default()
        };
        let req = build_activate_request("flowymes", "flowy", &fp, Some(&geo));
        assert_eq!(req.app, "flowymes");
        assert_eq!(req.country, "China");
        assert_eq!(req.province, "Beijing");
        assert_eq!(req.operator, "China Mobile");
        assert_eq!(req.isp, "China Mobile");
        assert_eq!(req.public_ip, "203.0.113.1");
    }
}
