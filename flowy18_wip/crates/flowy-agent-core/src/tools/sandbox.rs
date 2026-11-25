//! Platform-neutral OS sandbox policy contract and honest availability probe.
//!
//! Architecture follows a clean separation between a declarative
//! [`SandboxPolicy`] (writable roots, protected host paths, network egress) and
//! per-OS enforcement backends:
//! - **macOS**: `/usr/bin/sandbox-exec` (Seatbelt SBPL profile)
//! - **Linux**: `bwrap` (Bubblewrap mount/PID/network namespaces + seccomp-BPF)
//! - **Windows**: `AppContainer` / restricted token (requiring classic
//!   `WindowsPowerShell\v1.0\powershell.exe`, never MSIX-packaged shells)
//!
//! Until an OS backend is live-verified and enabled on its target platform,
//! [`detect_sandbox_availability`] honestly reports [`SandboxAvailability::Degraded`]
//! rather than claiming OS-level sandboxing that is not enforced yet.

use std::path::{Path, PathBuf};

/// Declarative sandbox boundary for one workspace turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub writable_roots: Vec<PathBuf>,
    pub protected_paths: Vec<PathBuf>,
    pub allow_network: bool,
}

impl SandboxPolicy {
    /// Default policy scoped to one session workspace.
    #[must_use]
    pub fn for_workspace(workspace_root: PathBuf) -> Self {
        Self {
            writable_roots: vec![workspace_root],
            protected_paths: default_protected_paths(),
            allow_network: true,
        }
    }

    #[must_use]
    pub const fn with_network(mut self, allow_network: bool) -> Self {
        self.allow_network = allow_network;
        self
    }

    #[must_use]
    pub fn with_protected_path(mut self, path: PathBuf) -> Self {
        if !self.protected_paths.contains(&path) {
            self.protected_paths.push(path);
        }
        self
    }
}

/// Target OS-level sandbox technology for a platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxBackend {
    MacOsSeatbelt,
    LinuxBubblewrap,
    WindowsAppContainer,
}

impl SandboxBackend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOsSeatbelt => "macos_seatbelt",
            Self::LinuxBubblewrap => "linux_bubblewrap",
            Self::WindowsAppContainer => "windows_appcontainer",
        }
    }
}

/// Honest runtime status of OS-level process sandboxing for shell tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxAvailability {
    Enforced {
        backend: SandboxBackend,
    },
    Degraded {
        platform: &'static str,
        planned_backend: SandboxBackend,
        binary_detected: bool,
        reason: &'static str,
    },
}

impl SandboxAvailability {
    #[must_use]
    pub const fn is_enforced(&self) -> bool {
        matches!(self, Self::Enforced { .. })
    }

    #[must_use]
    pub const fn telemetry_label(&self) -> &'static str {
        match self {
            Self::Enforced { .. } => "enforced",
            Self::Degraded { .. } => "degraded",
        }
    }
}

/// Probe the current host's OS sandbox capability without overstating what is
/// actively enforced.
#[must_use]
#[allow(
    clippy::missing_const_for_fn,
    reason = "macOS and Linux branches inspect the filesystem / PATH at runtime"
)]
pub fn detect_sandbox_availability() -> SandboxAvailability {
    #[cfg(target_os = "windows")]
    {
        SandboxAvailability::Degraded {
            platform: "windows",
            planned_backend: SandboxBackend::WindowsAppContainer,
            binary_detected: false,
            reason: "os_sandbox_not_enabled_yet; active layers: path_guard + shell_policy + job_object_reaping",
        }
    }
    #[cfg(target_os = "macos")]
    {
        let binary_detected = Path::new("/usr/bin/sandbox-exec").is_file();
        SandboxAvailability::Degraded {
            platform: "macos",
            planned_backend: SandboxBackend::MacOsSeatbelt,
            binary_detected,
            reason: "seatbelt_adapter_reserved_pending_macos_verification; active layers: path_guard + shell_policy + process_group_reaping",
        }
    }
    #[cfg(target_os = "linux")]
    {
        let binary_detected = command_in_path("bwrap");
        SandboxAvailability::Degraded {
            platform: "linux",
            planned_backend: SandboxBackend::LinuxBubblewrap,
            binary_detected,
            reason: "bwrap_adapter_reserved_pending_linux_verification; active layers: path_guard + shell_policy + process_group_reaping",
        }
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux"
    )))]
    {
        SandboxAvailability::Degraded {
            platform: std::env::consts::OS,
            planned_backend: SandboxBackend::LinuxBubblewrap,
            binary_detected: false,
            reason: "unsupported_os_sandbox; active layers: path_guard + shell_policy",
        }
    }
}

#[cfg(target_os = "linux")]
fn command_in_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file())
}

fn default_protected_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        paths.push(home.join(".ssh"));
        paths.push(home.join(".aws"));
        paths.push(home.join(".gnupg"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        paths.push(
            appdata
                .join("Flowy")
                .join("Flowy")
                .join("data")
                .join("auth"),
        );
    }
    paths
}

fn escape_sbpl_literal(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Pure, cross-platform testable macOS Seatbelt (SBPL) profile generator for
/// `/usr/bin/sandbox-exec -f <profile>`.
#[must_use]
pub fn macos_seatbelt_profile(policy: &SandboxPolicy) -> String {
    let mut out = String::from(
        "(version 1)\n\
         (allow default)\n\
         (deny file-write*)\n\
         (allow file-write* (subpath \"/tmp\") (subpath \"/private/tmp\") (literal \"/dev/null\"))\n",
    );
    for root in &policy.writable_roots {
        out.push_str("(allow file-write* (subpath \"");
        out.push_str(&escape_sbpl_literal(root));
        out.push_str("\"))\n");
    }
    for protected in &policy.protected_paths {
        out.push_str("(deny file-read* file-write* (subpath \"");
        out.push_str(&escape_sbpl_literal(protected));
        out.push_str("\"))\n");
    }
    if !policy.allow_network {
        out.push_str("(deny network-outbound)\n");
    }
    out
}

/// Pure, cross-platform testable Linux Bubblewrap (`bwrap`) argument builder.
#[must_use]
pub fn linux_bwrap_args(policy: &SandboxPolicy, cwd: &Path) -> Vec<String> {
    let mut args = vec![
        "--die-with-parent".to_owned(),
        "--new-session".to_owned(),
        "--ro-bind".to_owned(),
        "/".to_owned(),
        "/".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
    ];
    for root in &policy.writable_roots {
        let rendered = root.to_string_lossy().into_owned();
        args.push("--bind".to_owned());
        args.push(rendered.clone());
        args.push(rendered);
    }
    for protected in &policy.protected_paths {
        let rendered = protected.to_string_lossy().into_owned();
        args.push("--tmpfs".to_owned());
        args.push(rendered);
    }
    if !policy.allow_network {
        args.push("--unshare-net".to_owned());
    }
    args.push("--chdir".to_owned());
    args.push(cwd.to_string_lossy().into_owned());
    args.push("--".to_owned());
    args
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        SandboxAvailability, SandboxBackend, SandboxPolicy,
        detect_sandbox_availability, linux_bwrap_args, macos_seatbelt_profile,
    };

    #[test]
    fn availability_probe_reports_honest_status() {
        let status = detect_sandbox_availability();
        assert!(!status.is_enforced());
        assert_eq!(status.telemetry_label(), "degraded");
        #[cfg(target_os = "windows")]
        assert!(matches!(
            status,
            SandboxAvailability::Degraded {
                platform: "windows",
                planned_backend: SandboxBackend::WindowsAppContainer,
                ..
            }
        ));
    }

    #[test]
    fn seatbelt_profile_restricts_writes_masks_secrets_and_escapes_quotes() {
        let policy = SandboxPolicy {
            writable_roots: vec![PathBuf::from("/Users/alice/work \"1\"")],
            protected_paths: vec![PathBuf::from("/Users/alice/.ssh")],
            allow_network: false,
        };
        let sbpl = macos_seatbelt_profile(&policy);
        assert!(sbpl.contains("(deny file-write*)"));
        assert!(sbpl.contains(
            "(allow file-write* (subpath \"/Users/alice/work \\\"1\\\"\"))"
        ));
        assert!(sbpl.contains(
            "(deny file-read* file-write* (subpath \"/Users/alice/.ssh\"))"
        ));
        assert!(sbpl.contains("(deny network-outbound)"));
    }

    #[test]
    fn bwrap_args_bind_workspace_mask_secrets_and_toggle_network() {
        let policy = SandboxPolicy {
            writable_roots: vec![PathBuf::from("/home/alice/ws")],
            protected_paths: vec![PathBuf::from("/home/alice/.ssh")],
            allow_network: false,
        };
        let args = linux_bwrap_args(&policy, Path::new("/home/alice/ws"));
        assert!(args.contains(&"--die-with-parent".to_owned()));
        assert!(args.contains(&"--unshare-net".to_owned()));
        assert!(
            args.windows(3).any(|w| {
                w == ["--bind", "/home/alice/ws", "/home/alice/ws"]
            })
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--tmpfs", "/home/alice/.ssh"])
        );
        assert_eq!(args.last().map(String::as_str), Some("--"));
    }
}
