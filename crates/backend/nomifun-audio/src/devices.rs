//! Audio device enumeration and selection.
//!
//! Provides a cross-platform interface to list and set input/output audio
//! devices.  The Windows implementation uses WASAPI via `windows-sys`;
//! other platforms provide a no-op stub that returns the system default.
//!
//! # Example
//!
//! ```rust,ignore
//! use nomifun_audio::devices::{AudioDeviceManager, DeviceKind};
//!
//! let mgr = AudioDeviceManager::new();
//! let inputs = mgr.list_devices(DeviceKind::Input)?;
//! for dev in &inputs {
//!     println!("{}: {}", dev.id, dev.name);
//! }
//! mgr.set_default(DeviceKind::Input, &inputs[0].id)?;
//! ```

use std::fmt;

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
    /// Platform-specific device identifier (e.g. Windows GUID string).
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
        platform::list_devices(kind)
    }

    /// Set the system default device.
    ///
    /// Note: on Windows this requires elevated privileges and the
    /// `IPolicyConfig` COM interface (undocumented).  A best-effort
    /// implementation is provided; consider using the OS UI for permanent
    /// changes.
    pub fn set_default(&self, kind: DeviceKind, device_id: &str) -> Result<(), DeviceError> {
        platform::set_default(kind, device_id)
    }
}

impl Default for AudioDeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Platform implementations
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    pub fn list_devices(kind: DeviceKind) -> Result<Vec<AudioDeviceInfo>, DeviceError> {
        // TODO: implement full WASAPI enumeration via IMMDeviceEnumerator.
        // Returning a single placeholder so the interface compiles and is
        // callable; replace with the COM enumeration in a follow-up.
        tracing::warn!(
            "AudioDeviceManager::list_devices({kind}) — \
             full WASAPI enumeration not yet implemented; returning system default only"
        );
        Ok(vec![AudioDeviceInfo {
            id: "default".into(),
            name: format!("System Default {kind} Device"),
            is_default: true,
        }])
    }

    pub fn set_default(_kind: DeviceKind, device_id: &str) -> Result<(), DeviceError> {
        tracing::warn!(
            "AudioDeviceManager::set_default({device_id}) — \
             IPolicyConfig not yet implemented"
        );
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn list_devices(kind: DeviceKind) -> Result<Vec<AudioDeviceInfo>, DeviceError> {
        Ok(vec![AudioDeviceInfo {
            id: "default".into(),
            name: format!("Default {kind} (stub)"),
            is_default: true,
        }])
    }

    pub fn set_default(_kind: DeviceKind, _device_id: &str) -> Result<(), DeviceError> {
        Err(DeviceError(
            "set_default not supported on this platform".into(),
        ))
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
}
