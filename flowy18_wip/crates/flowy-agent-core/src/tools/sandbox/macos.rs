//! Seatbelt launch arguments and readiness probe for macOS Shell tools.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use rpi_tools::ExecutionError;
use rpi_tools::env::ShellOverride;
use tempfile::NamedTempFile;

use super::{
    SandboxAvailability, SandboxBackend, SandboxPolicy, escape_sbpl_literal,
    expand_canonical_variants, macos_seatbelt_profile, unix,
};

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub(super) fn availability() -> &'static SandboxAvailability {
    static AVAILABILITY: OnceLock<SandboxAvailability> = OnceLock::new();
    AVAILABILITY.get_or_init(|| {
        let binary_detected = Path::new(SANDBOX_EXEC).is_file();
        if binary_detected && probe() {
            SandboxAvailability::Enforced {
                backend: SandboxBackend::MacOsSeatbelt,
            }
        } else {
            SandboxAvailability::Degraded {
                platform: "macos",
                planned_backend: SandboxBackend::MacOsSeatbelt,
                binary_detected,
                reason: "sandbox-exec unavailable or profile setup failed; active layers: path_guard + shell_policy + process_group_reaping",
            }
        }
    })
}

fn probe() -> bool {
    let policy = SandboxPolicy {
        writable_roots: Vec::new(),
        protected_paths: Vec::new(),
        allow_network: false,
    };
    let Ok((shell, _profile)) = shell_override(&policy) else {
        return false;
    };
    let mut command = Command::new(shell.program);
    command.args(shell.args).arg("exit 0");
    unix::probe(command)
}

pub(super) fn shell_override(
    policy: &SandboxPolicy,
) -> Result<(ShellOverride, NamedTempFile), ExecutionError> {
    if policy
        .writable_roots
        .iter()
        .chain(&policy.protected_paths)
        .flat_map(|path| expand_canonical_variants(path))
        .any(|path| escape_sbpl_literal(&path).is_none())
    {
        return Err(unix::setup_error(
            "sandbox path cannot be represented in a Seatbelt profile",
        ));
    }

    let mut profile = NamedTempFile::new().map_err(|error| {
        unix::setup_error(format!("create Seatbelt profile: {error}"))
    })?;
    profile
        .write_all(macos_seatbelt_profile(policy).as_bytes())
        .map_err(|error| {
            unix::setup_error(format!("write Seatbelt profile: {error}"))
        })?;
    let profile_path = profile.path().to_str().ok_or_else(|| {
        unix::setup_error("Seatbelt profile path must be valid UTF-8")
    })?;
    let shell = ShellOverride {
        program: PathBuf::from(SANDBOX_EXEC),
        args: vec![
            "-f".to_owned(),
            profile_path.to_owned(),
            "/bin/bash".to_owned(),
            "-c".to_owned(),
        ],
    };
    Ok((shell, profile))
}

#[cfg(test)]
mod tests;
