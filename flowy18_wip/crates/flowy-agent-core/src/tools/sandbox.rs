//! Platform-neutral OS sandbox policy contract and honest availability probe.
//!
//! Architecture follows a clean separation between a declarative
//! [`SandboxPolicy`] (writable roots, protected host paths, network egress) and
//! per-OS enforcement backends:
//! - **macOS**: `/usr/bin/sandbox-exec` (Seatbelt SBPL profile)
//! - **Linux**: `bwrap` (Bubblewrap mount, PID, IPC, and network namespaces)
//! - **Windows**: `AppContainer` / restricted token (requiring classic
//!   `WindowsPowerShell\v1.0\powershell.exe`, never MSIX-packaged shells)
//!
//! Linux and macOS select their backends after a successful startup probe.
//! Reserved or unavailable backends report [`SandboxAvailability::Degraded`].

use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use unix::execution_env as unix_execution_env;

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
        macos::availability().clone()
    }
    #[cfg(target_os = "linux")]
    {
        linux::availability().clone()
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

fn default_protected_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let xdg_data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    default_protected_paths_with(
        home.as_deref(),
        xdg_data.as_deref(),
        appdata.as_deref(),
    )
}

fn default_protected_paths_with(
    home: Option<&Path>,
    xdg_data: Option<&Path>,
    appdata: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join(".ssh"));
        paths.push(home.join(".aws"));
        paths.push(home.join(".gnupg"));
        paths.push(home.join(".kube").join("config"));
        paths.push(home.join(".docker").join("config.json"));
        paths.push(home.join(".npmrc"));
        paths.push(home.join(".pypirc"));
        paths.push(home.join(".git-credentials"));
        // macOS Flowy auth store (`ProjectDirs::from("com", "Flowy", "Flowy")`
        // resolves to `com.Flowy.Flowy`, plus legacy `Flowy` paths).
        paths.push(
            home.join("Library")
                .join("Application Support")
                .join("com.Flowy.Flowy")
                .join("auth"),
        );
        paths.push(
            home.join("Library")
                .join("Application Support")
                .join("Flowy")
                .join("auth"),
        );
        paths.push(
            home.join("Library")
                .join("Application Support")
                .join("Flowy")
                .join("data")
                .join("auth"),
        );
        // Linux Flowy auth/config stores
        paths
            .push(home.join(".local").join("share").join("flowy").join("auth"));
        paths.push(home.join(".config").join("flowy").join("auth"));
    }
    if let Some(xdg_data) = xdg_data {
        paths.push(xdg_data.join("flowy").join("auth"));
    }
    if let Some(appdata) = appdata {
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

fn escape_sbpl_literal(path: &Path) -> Option<String> {
    let raw = path.to_str()?;
    if raw.contains('\0') || raw.contains('\n') || raw.contains('\r') {
        return None;
    }
    Some(raw.replace('\\', "\\\\").replace('"', "\\\""))
}

fn expand_canonical_variants(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    if let Ok(canonical) = std::fs::canonicalize(path)
        && canonical != path
    {
        out.push(canonical);
    }
    out
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
        for variant in expand_canonical_variants(root) {
            if let Some(escaped) = escape_sbpl_literal(&variant) {
                out.push_str("(allow file-write* (subpath \"");
                out.push_str(&escaped);
                out.push_str("\"))\n");
            }
        }
    }
    for protected in &policy.protected_paths {
        for variant in expand_canonical_variants(protected) {
            if let Some(escaped) = escape_sbpl_literal(&variant) {
                out.push_str("(deny file-read* file-write* (subpath \"");
                out.push_str(&escaped);
                out.push_str("\"))\n");
            }
        }
    }
    if !policy.allow_network {
        out.push_str("(deny network-outbound)\n");
    }
    out
}

/// Pure, cross-platform testable Linux Bubblewrap (`bwrap`) argument builder.
///
/// Always includes `--unshare-pid` (required when mounting `--proc /proc` inside
/// an unprivileged user namespace), masks existing protected directories with
/// `--tmpfs`, masks existing protected regular files with `--ro-bind /dev/null`,
/// and omits missing protected paths so `bwrap` never fails to start when an
/// optional credential directory (e.g. `~/.aws`) does not exist on the host.
#[must_use]
pub fn linux_bwrap_args(policy: &SandboxPolicy, cwd: &Path) -> Vec<String> {
    let mut args = vec![
        "--die-with-parent".to_owned(),
        "--new-session".to_owned(),
        "--unshare-pid".to_owned(),
        "--unshare-ipc".to_owned(),
        "--ro-bind".to_owned(),
        "/".to_owned(),
        "/".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--tmpfs".to_owned(),
        "/run".to_owned(),
    ];
    // systemd-resolved may keep the resolver file behind the private /run mount.
    if policy.allow_network
        && let Ok(resolver) = std::fs::canonicalize("/etc/resolv.conf")
        && resolver.starts_with("/run")
        && resolver.is_file()
        && let Some(path) = resolver.to_str()
    {
        args.extend(["--ro-bind".to_owned(), path.to_owned(), path.to_owned()]);
    }

    for root in &policy.writable_roots {
        let rendered = root.to_string_lossy().into_owned();
        args.push("--bind".to_owned());
        args.push(rendered.clone());
        args.push(rendered);
    }
    for protected in &policy.protected_paths {
        let Ok(meta) = std::fs::symlink_metadata(protected) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            // Symlinked credential store (e.g. `~/.ssh -> /mnt/vault/.ssh`):
            // resolve and mask the canonical target so reads via either path
            // cannot expose secrets inside `bwrap`.
            if let Ok(canon) = std::fs::canonicalize(protected)
                && let Ok(target_meta) = std::fs::metadata(&canon)
            {
                let rendered = canon.to_string_lossy().into_owned();
                if target_meta.is_dir() {
                    args.push("--tmpfs".to_owned());
                    args.push(rendered);
                } else if target_meta.is_file() {
                    args.push("--ro-bind".to_owned());
                    args.push("/dev/null".to_owned());
                    args.push(rendered);
                }
            }
            continue;
        }
        let rendered = protected.to_string_lossy().into_owned();
        if meta.is_dir() {
            args.push("--tmpfs".to_owned());
            args.push(rendered);
        } else if meta.is_file() {
            args.push("--ro-bind".to_owned());
            args.push("/dev/null".to_owned());
            args.push(rendered);
        }
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
    use std::path::PathBuf;

    use super::{
        SandboxAvailability, SandboxBackend, SandboxPolicy,
        default_protected_paths_with, detect_sandbox_availability,
        linux_bwrap_args, macos_seatbelt_profile,
    };

    #[test]
    fn availability_probe_reports_honest_status() {
        let status = detect_sandbox_availability();
        if let SandboxAvailability::Enforced { backend } = &status {
            assert_eq!(status.telemetry_label(), "enforced");
            assert!(matches!(
                (std::env::consts::OS, backend),
                ("linux", SandboxBackend::LinuxBubblewrap)
                    | ("macos", SandboxBackend::MacOsSeatbelt)
            ));
        } else {
            assert_eq!(status.telemetry_label(), "degraded");
        }
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
        let root = std::env::temp_dir()
            .join(format!("flowy-bwrap-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ws = root.join("ws");
        let ssh_dir = root.join(".ssh");
        let shadow_file = root.join("shadow");
        let missing_aws = root.join(".aws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&ssh_dir).unwrap();
        std::fs::write(&shadow_file, b"secret").unwrap();

        let policy = SandboxPolicy {
            writable_roots: vec![ws.clone()],
            protected_paths: vec![
                ssh_dir.clone(),
                shadow_file.clone(),
                missing_aws.clone(),
            ],
            allow_network: false,
        };
        let args = linux_bwrap_args(&policy, &ws);
        let ws_s = ws.to_string_lossy().into_owned();
        let ssh_s = ssh_dir.to_string_lossy().into_owned();
        let shadow_s = shadow_file.to_string_lossy().into_owned();
        let aws_s = missing_aws.to_string_lossy().into_owned();

        assert!(args.contains(&"--die-with-parent".to_owned()));
        assert!(args.contains(&"--unshare-pid".to_owned()));
        assert!(args.contains(&"--unshare-net".to_owned()));
        assert!(args.windows(3).any(|w| w == ["--bind", &ws_s, &ws_s]));
        assert!(args.windows(2).any(|w| w == ["--tmpfs", &ssh_s]));
        assert!(
            args.windows(3)
                .any(|w| w == ["--ro-bind", "/dev/null", &shadow_s]),
            "existing regular protected file must be masked with /dev/null"
        );
        assert!(
            !args.contains(&aws_s),
            "non-existent protected path must be omitted so bwrap does not fail"
        );
        assert_eq!(args.last().map(String::as_str), Some("--"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_protected_paths_include_extra_credential_files_in_seatbelt_and_bwrap()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-sandbox-creds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();

        let expected_files = [
            root.join(".kube").join("config"),
            root.join(".docker").join("config.json"),
            root.join(".npmrc"),
            root.join(".pypirc"),
            root.join(".git-credentials"),
        ];
        for file in &expected_files {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(file, b"secret").unwrap();
        }

        let protected = default_protected_paths_with(Some(&root), None, None);
        for file in &expected_files {
            assert!(
                protected.contains(file),
                "expected protected paths to include {}",
                file.display()
            );
        }

        let policy = SandboxPolicy {
            writable_roots: vec![ws.clone()],
            protected_paths: protected,
            allow_network: true,
        };
        let sbpl = macos_seatbelt_profile(&policy);
        let bwrap = linux_bwrap_args(&policy, &ws);

        for file in &expected_files {
            let sbpl_escaped = file
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            assert!(
                sbpl.contains(&format!(
                    "(deny file-read* file-write* (subpath \"{sbpl_escaped}\"))"
                )),
                "expected Seatbelt profile to deny {}",
                file.display()
            );
            let file_s = file.to_string_lossy().into_owned();
            assert!(
                bwrap
                    .windows(3)
                    .any(|w| w == ["--ro-bind", "/dev/null", &file_s]),
                "expected bwrap args to mask {} with /dev/null",
                file.display()
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
