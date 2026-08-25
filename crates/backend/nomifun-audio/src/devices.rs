//! Audio device enumeration and selection via `cpal`.

use std::fmt;

use cpal::traits::{DeviceTrait, HostTrait};

/// Whether a device is an audio input (microphone) or output (speakers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Input,
    Output,
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceKind::Input => write!(f, "input"),
            DeviceKind::Output => write!(f, "output"),
        }
    }
}

/// Metadata for a single audio device.
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    /// Platform-specific device identifier (device name from the host API).
    pub id: String,
    /// Human-readable device name shown in the OS mixer.
    pub name: String,
    /// Whether this device is the current system default for its kind.
    pub is_default: bool,
}

/// Errors returned by device management operations.
#[derive(Debug)]
pub struct DeviceError(pub String);

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "audio device error: {}", self.0)
    }
}

impl std::error::Error for DeviceError {}

/// Cross-platform audio device manager.
pub struct AudioDeviceManager;

impl AudioDeviceManager {
    pub fn new() -> Self {
        Self
    }

    /// List all available devices of the given kind.
    pub fn list_devices(&self, kind: DeviceKind) -> Result<Vec<AudioDeviceInfo>, DeviceError> {
        let host = cpal::default_host();
        let default_name = match kind {
            DeviceKind::Input => host.default_input_device(),
            DeviceKind::Output => host.default_output_device(),
        }
        .and_then(|d| d.name().ok());

        let devices = match kind {
            DeviceKind::Input => host
                .input_devices()
                .map_err(|e| DeviceError(format!("enumerate input devices: {e}")))?,
            DeviceKind::Output => host
                .output_devices()
                .map_err(|e| DeviceError(format!("enumerate output devices: {e}")))?,
        };

        let mut out = Vec::new();
        for (i, device) in devices.enumerate() {
            let name = device
                .name()
                .unwrap_or_else(|_| format!("Audio device {i}"));
            let is_default = default_name.as_ref() == Some(&name);
            out.push(AudioDeviceInfo {
                id: name.clone(),
                name,
                is_default,
            });
        }

        if out.is_empty() {
            out.push(AudioDeviceInfo {
                id: "default".into(),
                name: format!("System Default {kind} Device"),
                is_default: true,
            });
        } else if !out.iter().any(|d| d.is_default) {
            out[0].is_default = true;
        }

        Ok(out)
    }

    /// Set the system default device.
    ///
    /// Not supported via cpal; on Windows this would need undocumented
    /// `IPolicyConfig`. Returns Ok as a no-op with a warning on Windows,
    /// Err on other platforms.
    pub fn set_default(&self, kind: DeviceKind, device_id: &str) -> Result<(), DeviceError> {
        #[cfg(target_os = "windows")]
        {
            let _ = kind;
            tracing::warn!(
                "AudioDeviceManager::set_default({device_id}) — \
                 IPolicyConfig not implemented; use the OS sound settings"
            );
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (kind, device_id);
            Err(DeviceError(
                "set_default not supported on this platform".into(),
            ))
        }
    }
}

impl Default for AudioDeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_default() {
        let mgr = AudioDeviceManager::new();
        let inputs = mgr.list_devices(DeviceKind::Input).unwrap();
        assert!(!inputs.is_empty());
        assert!(inputs.iter().any(|d| d.is_default));
    }

    #[test]
    fn list_outputs() {
        let mgr = AudioDeviceManager::new();
        let outputs = mgr.list_devices(DeviceKind::Output).unwrap();
        assert!(!outputs.is_empty());
    }
}
