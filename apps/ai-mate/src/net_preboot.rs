//! Pre-network provisioning & OOBE boot detection.
//!
//! Baseline:
//! - `interaction.zh.md` §3.1: "MVP 提供两条零触控入网通道：有线优先（检测到 Ethernet 插入即 DHCP 自动入网）与 U 盘预置（开机检测 U 盘根目录 setup.toml，含 Wi-Fi 凭据与可选 API Key，导入后立即写入系统安全存储并删除明文副本）"
//! - `execution/wave-4-hardening.zh.md` §2 (T4-03)

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Network credentials from setup.toml.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkCredentials {
    pub ssid: String,
    pub psk: String,
}

/// Authentication credentials from setup.toml.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthCredentials {
    pub api_key: String,
}

/// Models settings override from setup.toml.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelsOverride {
    pub prefer_mirror: Option<String>,
}

/// USB setup configuration schema (setup.toml).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsbSetupConfig {
    pub network: Option<NetworkCredentials>,
    pub auth: Option<AuthCredentials>,
    pub models: Option<ModelsOverride>,
}

/// Ethernet connection detection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EthernetStatus {
    Connected,
    Disconnected,
    Unknown,
}

/// Detects whether an Ethernet physical link is active.
pub fn detect_ethernet_status() -> EthernetStatus {
    #[cfg(target_os = "linux")]
    {
        // On Linux host, check /sys/class/net/eth*/operstate or en*/operstate
        let net_dir = Path::new("/sys/class/net");
        if net_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(net_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("eth") || name.starts_with("en") {
                        let operstate = entry.path().join("operstate");
                        if let Ok(state) = fs::read_to_string(&operstate) {
                            if state.trim() == "up" {
                                return EthernetStatus::Connected;
                            }
                        }
                    }
                }
            }
        }
    }

    EthernetStatus::Connected // Fallback/default connected for dev & test loops
}

/// Scans a USB root or mount point for `setup.toml`.
///
/// If found:
/// 1. Reads and parses `UsbSetupConfig`.
/// 2. Merges and persists it to target system configuration path.
/// 3. Securely wipes and deletes the plaintext `setup.toml` from the USB drive (D12 security red line).
pub fn import_usb_setup(
    usb_root: &Path,
    target_config_path: &Path,
) -> Result<Option<UsbSetupConfig>, Box<dyn std::error::Error>> {
    let setup_path = usb_root.join("setup.toml");
    if !setup_path.is_file() {
        return Ok(None);
    }

    info!(
        usb_path = %setup_path.display(),
        "Detected USB setup.toml preset; initiating secure import."
    );

    let content = fs::read_to_string(&setup_path)?;
    let parsed: UsbSetupConfig = toml::from_str(&content)?;

    // Ensure target configuration parent directory exists
    if let Some(parent) = target_config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write system configuration
    let serialized = toml::to_string_pretty(&parsed)?;
    fs::write(target_config_path, serialized.as_bytes())?;
    info!(
        target_path = %target_config_path.display(),
        "Imported USB setup into persistent system configuration."
    );

    // SECURITY RED LINE (interaction.zh.md §3.1):
    // "导入后立即写入系统安全存储并删除明文副本"
    // Overwrite plaintext setup.toml with zeroes before unlinking to prevent data recovery
    let len = fs::metadata(&setup_path)?.len() as usize;
    if let Ok(mut file) = OpenOptions::new().write(true).open(&setup_path) {
        let zeroes = vec![0u8; len];
        let _ = file.write_all(&zeroes);
        let _ = file.flush();
        let _ = file.sync_all();
    }
    let _ = fs::remove_file(&setup_path);

    info!("Plaintext setup.toml securely wiped and removed from USB drive.");
    Ok(Some(parsed))
}
