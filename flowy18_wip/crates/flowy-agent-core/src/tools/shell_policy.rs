//! Cross-platform static fail-closed safety filter for shell tool commands.
//!
//! Runs as a host-owned pre-execution guard on `powershell` and `bash` tool
//! calls across Windows, macOS, and Linux before any subprocess is spawned.
//! This layer complements (and does not claim to replace) OS-level sandboxing
//! (`sandbox.rs`): it blocks known high-impact host tampering, credential store
//! access, macOS privacy/TCC bypasses, and host-wide destructive operations
//! while keeping normal workspace scripting frictionless.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

/// Blocked attempt to tamper with OS privacy/permission dialogs (e.g. macOS TCC).
pub const SHELL_PRIVACY_TAMPER: &str = "SHELL_PRIVACY_TAMPER";

/// Blocked attempt to read or modify sensitive host credential stores.
pub const SHELL_CREDENTIAL_ACCESS: &str = "SHELL_CREDENTIAL_ACCESS";

/// Blocked destructive host-wide wipe or raw disk/volume operation.
pub const SHELL_DESTRUCTIVE_HOST_OP: &str = "SHELL_DESTRUCTIVE_HOST_OP";

/// Blocked OS-level privilege escalation or system configuration tampering.
pub const SHELL_SYSTEM_TAMPER: &str = "SHELL_SYSTEM_TAMPER";

/// Decision returned by [`evaluate_shell_command`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellPolicyDecision {
    Allow,
    Deny { code: &'static str, reason: String },
}

impl ShellPolicyDecision {
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    #[must_use]
    pub fn denial_message(&self) -> Option<String> {
        match self {
            Self::Allow => None,
            Self::Deny { code, reason } => Some(format!("{code}: {reason}")),
        }
    }
}

struct CompiledRules {
    tccutil: Regex,
    osascript_privacy: Regex,
    unix_rm_host: Regex,
    win_delete_host: Regex,
    disk_format: Regex,
    reg_hklm: Regex,
    exec_policy_machine: Regex,
    sudo_exec: Regex,
    unix_root_perm: Regex,
    system_service: Regex,
}

fn rules() -> &'static CompiledRules {
    static RULES: OnceLock<CompiledRules> = OnceLock::new();
    RULES.get_or_init(|| CompiledRules {
        tccutil: Regex::new(r"(?i)(?:^|[\s;&|(`])tccutil(?:\s|$|[;&|)])")
            .expect("tccutil regex"),
        osascript_privacy: Regex::new(
            r"(?i)\bosascript\b[^\n]*(?:system\s+(?:settings|preferences|events)|com\.apple\.preference\.security|accessibility|privacy)",
        )
        .expect("osascript privacy regex"),
        unix_rm_host: Regex::new(
            r#"(?i)(?:^|[;&|(`]\s*)rm\s+(?:-[a-z]*r[a-z]*|--recursive)\b[^\n;&|]*(?:\s+["']?(?:/\*?|~(?:/\*)?|\$home(?:/\*)?|/(?:users|home|etc|system|library|windows|usr|var|bin|sbin)(?:/\*?)?)["']?(?:\s|$|[;&|)]))"#,
        )
        .expect("unix rm regex"),
        win_delete_host: Regex::new(
            r#"(?i)(?:^|[;&|(`]\s*)(?:remove-item|ri|del|erase|rd|rmdir)\b[^\n;&|]*(?:["']?(?:[a-z]:/(?:windows|users|program files|programdata)?|(?:\$env:userprofile|%userprofile%|\$home))/?\*?["']?(?:\s|$|[;&|)]))"#,
        )
        .expect("win delete regex"),
        disk_format: Regex::new(
            r"(?i)(?:(?:^|[\s;&|(`])(?:mkfs(?:\.[a-z0-9]+)?|diskpart|format-volume|clear-disk|bcdedit)\b|\bdd\b[^\n;&|]*\bof=/dev/)",
        )
        .expect("disk format regex"),
        reg_hklm: Regex::new(
            r"(?i)\breg(?:\.exe)?\s+(?:delete|add)\s+hklm(?:\\|/|[\s])",
        )
        .expect("reg hklm regex"),
        exec_policy_machine: Regex::new(
            r"(?i)\bset-executionpolicy\b[^\n;&|]*\blocalmachine\b",
        )
        .expect("execution policy regex"),
        sudo_exec: Regex::new(r"(?i)(?:^|[;&|(`]\s*)sudo\s+")
            .expect("sudo regex"),
        unix_root_perm: Regex::new(
            r"(?i)(?:^|[;&|(`]\s*)(?:chmod|chown)\b[^\n;&|]*\s+/(?:\s|$|[;&|)]|(?:etc|usr|system|library|bin|sbin)\b)",
        )
        .expect("unix root perm regex"),
        system_service: Regex::new(
            r"(?i)(?:\blaunchctl\s+(?:load|bootstrap)\s+/library/launchdaemons\b|\bsystemctl\s+(?:disable|stop|mask)\b)",
        )
        .expect("system service regex"),
    })
}

const SENSITIVE_PATH_MARKERS: &[(&str, &str)] = &[
    ("/.ssh", "SSH key store"),
    ("~/.ssh", "SSH key store"),
    ("/.aws", "AWS credential store"),
    ("~/.aws", "AWS credential store"),
    ("/.gnupg", "GPG keyring"),
    ("~/.gnupg", "GPG keyring"),
    ("flowy/flowy/data/auth", "Flowy account auth store"),
    ("/etc/shadow", "Unix shadow password file"),
    ("/etc/sudoers", "Unix sudoers policy"),
    (
        "windows/system32/config",
        "Windows SAM/SYSTEM registry hive",
    ),
];

/// Evaluate a shell command string against the host static safety policy.
///
/// The check is OS-agnostic so Windows, macOS, and Linux policy rules can all
/// be validated uniformly in unit tests and enforced regardless of shell flavor.
#[must_use]
pub fn evaluate_shell_command(
    command: &str,
    _cwd: &Path,
) -> ShellPolicyDecision {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return ShellPolicyDecision::Allow;
    }
    let normalized = trimmed.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let compiled = rules();

    if compiled.tccutil.is_match(trimmed)
        || compiled.osascript_privacy.is_match(trimmed)
    {
        return ShellPolicyDecision::Deny {
            code: SHELL_PRIVACY_TAMPER,
            reason: "modifying OS privacy permissions or driving privilege dialogs via shell is blocked".to_owned(),
        };
    }

    for (marker, label) in SENSITIVE_PATH_MARKERS {
        if lower.contains(marker) {
            return ShellPolicyDecision::Deny {
                code: SHELL_CREDENTIAL_ACCESS,
                reason: format!(
                    "access to sensitive host path ({label}) is blocked"
                ),
            };
        }
    }

    if compiled.unix_rm_host.is_match(&normalized)
        || compiled.win_delete_host.is_match(&normalized)
        || compiled.disk_format.is_match(&normalized)
    {
        return ShellPolicyDecision::Deny {
            code: SHELL_DESTRUCTIVE_HOST_OP,
            reason:
                "destructive host-wide deletion or disk formatting is blocked"
                    .to_owned(),
        };
    }

    if compiled.reg_hklm.is_match(trimmed)
        || compiled.exec_policy_machine.is_match(trimmed)
        || compiled.sudo_exec.is_match(trimmed)
        || compiled.unix_root_perm.is_match(&normalized)
        || compiled.system_service.is_match(&normalized)
    {
        return ShellPolicyDecision::Deny {
            code: SHELL_SYSTEM_TAMPER,
            reason:
                "privileged system or service modification via shell is blocked"
                    .to_owned(),
        };
    }

    ShellPolicyDecision::Allow
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        SHELL_CREDENTIAL_ACCESS, SHELL_DESTRUCTIVE_HOST_OP,
        SHELL_PRIVACY_TAMPER, SHELL_SYSTEM_TAMPER, ShellPolicyDecision,
        evaluate_shell_command,
    };

    #[test]
    fn benign_workspace_commands_are_allowed() {
        let cwd = Path::new("workspace");
        for cmd in [
            "Write-Output 'hello'",
            "Get-ChildItem -Path ./outputs",
            "python scripts/build_deck.py --out outputs/q3.pptx",
            "Remove-Item -Recurse -Force ./tmp_build",
            "rm -rf ./build/cache",
            "git status --short",
            "ls -la outputs/",
        ] {
            assert_eq!(
                evaluate_shell_command(cmd, cwd),
                ShellPolicyDecision::Allow,
                "benign command must be allowed: {cmd}"
            );
        }
    }

    #[test]
    fn macos_privacy_tampering_is_blocked() {
        let cwd = Path::new("workspace");
        for cmd in [
            "tccutil reset All com.flowy.app",
            "osascript -e 'tell application \"System Events\" to click button 1'",
            "osascript -e 'open location \"x-apple.systempreferences:com.apple.preference.security\"'",
        ] {
            let decision = evaluate_shell_command(cmd, cwd);
            assert!(
                matches!(
                    decision,
                    ShellPolicyDecision::Deny {
                        code: SHELL_PRIVACY_TAMPER,
                        ..
                    }
                ),
                "expected privacy tamper block for {cmd:?}, got {decision:?}"
            );
        }
    }

    #[test]
    fn credential_store_access_is_blocked_on_all_platforms() {
        let cwd = Path::new("workspace");
        for cmd in [
            "cat ~/.ssh/id_rsa",
            "Get-Content $env:USERPROFILE\\.ssh\\config",
            "type %APPDATA%\\Flowy\\Flowy\\data\\auth\\session.json",
            "cat /etc/shadow",
            "ls ~/.aws/credentials",
            "dir C:\\Windows\\System32\\config\\SAM",
        ] {
            let decision = evaluate_shell_command(cmd, cwd);
            assert!(
                matches!(
                    decision,
                    ShellPolicyDecision::Deny {
                        code: SHELL_CREDENTIAL_ACCESS,
                        ..
                    }
                ),
                "expected credential access block for {cmd:?}, got {decision:?}"
            );
        }
    }

    #[test]
    fn host_wide_destructive_operations_are_blocked() {
        let cwd = Path::new("workspace");
        for cmd in [
            "rm -rf /",
            "rm -rf ~/*",
            "Remove-Item -Recurse -Force C:\\Windows",
            "Remove-Item -Recurse -Force C:\\",
            "dd if=/dev/zero of=/dev/sda bs=1M",
            "Format-Volume -DriveLetter C",
        ] {
            let decision = evaluate_shell_command(cmd, cwd);
            assert!(
                matches!(
                    decision,
                    ShellPolicyDecision::Deny {
                        code: SHELL_DESTRUCTIVE_HOST_OP,
                        ..
                    }
                ),
                "expected destructive block for {cmd:?}, got {decision:?}"
            );
        }
    }

    #[test]
    fn system_tampering_and_privilege_escalation_are_blocked() {
        let cwd = Path::new("workspace");
        for cmd in [
            "sudo rm -rf ./something",
            "reg delete HKLM\\Software\\Policies /f",
            "Set-ExecutionPolicy Unrestricted -Scope LocalMachine",
            "chmod -R 777 /etc",
            "systemctl disable sshd",
        ] {
            let decision = evaluate_shell_command(cmd, cwd);
            assert!(
                matches!(
                    decision,
                    ShellPolicyDecision::Deny {
                        code: SHELL_SYSTEM_TAMPER,
                        ..
                    }
                ),
                "expected system tamper block for {cmd:?}, got {decision:?}"
            );
        }
    }
}
