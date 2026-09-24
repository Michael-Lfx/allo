//! Linux fingerprint sources via sysfs / procfs (no subprocess).

use std::fs;
use std::path::Path;

/// Prefer `eth0`/`en0` while Up; else any Up iface; else any non-loopback with a MAC.
pub(super) fn read_mac_address() -> Option<String> {
    let mut preferred_up: Option<String> = None;
    let mut any_up: Option<String> = None;
    let mut any_mac: Option<String> = None;
    let entries = fs::read_dir("/sys/class/net").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        let path = entry.path();
        let Some(mac) = read_trim(path.join("address")) else {
            continue;
        };
        if mac == "00:00:00:00:00:00" {
            continue;
        }
        let is_up = read_trim(path.join("operstate")).as_deref() == Some("up");
        if is_up && (name == "eth0" || name == "en0") {
            preferred_up = Some(mac);
            break;
        }
        if is_up && any_up.is_none() {
            any_up = Some(mac.clone());
        }
        if any_mac.is_none() {
            any_mac = Some(mac);
        }
    }
    preferred_up.or(any_up).or(any_mac)
}

/// DMI product serial, then board serial.
pub(super) fn read_serial_number() -> Option<String> {
    read_trim("/sys/class/dmi/id/product_serial")
        .or_else(|| read_trim("/sys/class/dmi/id/board_serial"))
}

/// CPU brand string from `/proc/cpuinfo` (`model name`, else ARM `Hardware`).
pub(super) fn read_cpu_brand() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("model name")
            .or_else(|| line.strip_prefix("Hardware"))
            .and_then(|rest| rest.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// NVIDIA procfs model, else DRM/PCI device resolved via pci.ids.
pub(super) fn read_xpu_brand() -> Option<String> {
    read_nvidia_model().or_else(read_drm_gpu_name)
}

fn read_nvidia_model() -> Option<String> {
    let entries = fs::read_dir("/proc/driver/nvidia/gpus").ok()?;
    for entry in entries.flatten() {
        let info = fs::read_to_string(entry.path().join("information")).ok()?;
        for line in info.lines() {
            if let Some(model) = line.strip_prefix("Model:") {
                let value = model.trim();
                if !value.is_empty() && !is_virtual_gpu(value) {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn read_drm_gpu_name() -> Option<String> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    let mut candidates: Vec<(bool, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device_dir = entry.path().join("device");
        let Some(vendor) = parse_hex_id(device_dir.join("vendor")) else {
            continue;
        };
        let Some(device) = parse_hex_id(device_dir.join("device")) else {
            continue;
        };
        let boot_vga = read_trim(device_dir.join("boot_vga")).as_deref() == Some("1");
        let label = resolve_pci_name(vendor, device)
            .unwrap_or_else(|| format!("PCI {vendor:04X}:{device:04X}"));
        if is_virtual_gpu(&label) {
            continue;
        }
        candidates.push((boot_vga, label));
    }
    candidates.sort_by_key(|(boot, _)| !(*boot));
    candidates.into_iter().map(|(_, name)| name).next()
}

fn resolve_pci_name(vendor_id: u16, device_id: u16) -> Option<String> {
    const PATHS: &[&str] = &[
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/libpci.ids",
    ];
    let path = PATHS.iter().map(Path::new).find(|p| p.is_file())?;
    let text = fs::read_to_string(path).ok()?;
    parse_pci_ids_text(&text, vendor_id, device_id)
}

fn parse_pci_ids_text(text: &str, vendor_id: u16, device_id: u16) -> Option<String> {
    let vendor_key = format!("{vendor_id:04x}");
    let device_key = format!("{device_id:04x}");
    let mut in_vendor = false;
    let mut vendor_name: Option<&str> = None;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('\t') {
            if !in_vendor || rest.starts_with('\t') {
                continue;
            }
            let rest = rest.trim_start();
            if let Some((id, name)) = rest.split_once(char::is_whitespace) {
                if id.eq_ignore_ascii_case(&device_key) {
                    let device_name = name.trim();
                    if device_name.is_empty() {
                        return vendor_name.map(str::to_string);
                    }
                    return Some(match vendor_name {
                        Some(v) => format!("{v} {device_name}"),
                        None => device_name.to_string(),
                    });
                }
            }
            continue;
        }
        if let Some((id, name)) = line.split_once(char::is_whitespace) {
            if id.eq_ignore_ascii_case(&vendor_key) {
                in_vendor = true;
                vendor_name = Some(name.trim());
            } else if in_vendor {
                break;
            }
        }
    }
    vendor_name.map(str::to_string)
}

fn parse_hex_id(path: impl AsRef<Path>) -> Option<u16> {
    let raw = read_trim(path)?;
    let hex = raw.strip_prefix("0x").unwrap_or(&raw);
    u16::from_str_radix(hex, 16).ok()
}

fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    let value = fs::read_to_string(path).ok()?.trim().to_string();
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(value)
    }
}

fn is_virtual_gpu(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("microsoft basic")
        || lower.contains("remote desktop")
        || lower.contains("virtio")
        || lower.contains("qxl")
        || lower.contains("bochs")
        || lower.contains("virtualbox")
        || lower.contains("vmware")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_pci_name_parses_vendor_and_device() {
        let text = "\
# comment
10de  NVIDIA Corporation
\t2204  GA102 [GeForce RTX 3090]
\t\t10de 1467  GeForce RTX 3090
8086  Intel Corporation
\t9a49  TigerLake-LP GT2 [Iris Xe Graphics]
";
        assert_eq!(
            parse_pci_ids_text(text, 0x10de, 0x2204).as_deref(),
            Some("NVIDIA Corporation GA102 [GeForce RTX 3090]")
        );
        assert_eq!(
            parse_pci_ids_text(text, 0x8086, 0x9a49).as_deref(),
            Some("Intel Corporation TigerLake-LP GT2 [Iris Xe Graphics]")
        );
        assert_eq!(
            parse_pci_ids_text(text, 0x10de, 0xffff).as_deref(),
            Some("NVIDIA Corporation")
        );
    }

    #[test]
    fn linux_source_avoids_subprocess() {
        let source = include_str!("fingerprint_linux.rs");
        let code = source.split("#[cfg(test)]").next().expect("prod code");
        assert!(code.contains("/sys/class/net"));
        assert!(code.contains("/sys/class/dmi"));
        assert!(code.contains("/proc/cpuinfo"));
        assert!(code.contains("/sys/class/drm"));
        assert!(!code.contains("std::process"));
        assert!(!code.contains("Command::"));
        assert!(!code.contains("\"lspci\""));
    }
}
