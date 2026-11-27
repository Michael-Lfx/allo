//! Bubblewrap launch arguments and readiness probe for Linux Shell tools.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use rpi_tools::env::ShellOverride;

use super::{
    SandboxAvailability, SandboxBackend, SandboxPolicy, linux_bwrap_args, unix,
};

const BWRAP: &str = "/usr/bin/bwrap";

pub(super) fn availability() -> &'static SandboxAvailability {
    static AVAILABILITY: OnceLock<SandboxAvailability> = OnceLock::new();
    AVAILABILITY.get_or_init(|| {
        let binary_detected = Path::new(BWRAP).is_file();
        if binary_detected && probe() {
            SandboxAvailability::Enforced {
                backend: SandboxBackend::LinuxBubblewrap,
            }
        } else {
            SandboxAvailability::Degraded {
                platform: "linux",
                planned_backend: SandboxBackend::LinuxBubblewrap,
                binary_detected,
                reason: "bwrap unavailable or namespace setup failed; active layers: path_guard + shell_policy + process_group_reaping",
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
    let shell = shell_override(&policy, Path::new("/"));
    let mut command = Command::new(shell.program);
    command.args(shell.args).arg("exit 0");
    unix::probe(command)
}

pub(super) fn shell_override(
    policy: &SandboxPolicy,
    cwd: &Path,
) -> ShellOverride {
    let mut args = linux_bwrap_args(policy, cwd);
    args.extend(["/bin/bash".to_owned(), "-c".to_owned()]);
    ShellOverride {
        program: PathBuf::from(BWRAP),
        args,
    }
}

#[cfg(test)]
mod tests;
