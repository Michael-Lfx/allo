//! Windows agent-shell resolution: prefer a private pwsh 7 host, never PATH
//! or Microsoft Store aliases, and keep Windows PowerShell 5.1 as fallback.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use crate::SpawnFailure;

const MIN_PWSH_BYTES: u64 = 64 * 1024;
const BUNDLED_BIN_DIR_ENV: &str = "NOMIFUN_BUNDLED_BIN_DIR";
const PWSH_EXE_ENV: &str = "NOMIFUN_PWSH_EXE";
const SHELL_POLICY_ENV: &str = "NOMIFUN_WINDOWS_SHELL";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsShellEdition {
    PowerShell51,
    Pwsh7,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsShellResolution {
    pub path: PathBuf,
    pub edition: WindowsShellEdition,
}

static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn override_slot() -> &'static Mutex<Option<PathBuf>> {
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

/// Register the managed sidecar (or clear it). The next spawn reads this.
pub fn set_windows_powershell_override(path: Option<PathBuf>) {
    match override_slot().lock() {
        Ok(mut guard) => *guard = path,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = path;
        }
    }
}

pub fn windows_shell_edition() -> WindowsShellEdition {
    resolve_windows_powershell_exe()
        .map(|resolved| resolved.edition)
        .unwrap_or(WindowsShellEdition::PowerShell51)
}

pub fn resolve_windows_powershell_exe() -> Result<WindowsShellResolution, crate::ProcessError> {
    resolve_windows_powershell_exe_with(false)
}

pub(crate) fn resolve_windows_powershell_exe_with(
    force_system_51: bool,
) -> Result<WindowsShellResolution, crate::ProcessError> {
    if force_system_51 || system_51_forced() {
        return system_powershell_51_resolution();
    }

    if let Some(path) = override_path()
        && is_usable_pwsh(&path)
    {
        return Ok(WindowsShellResolution {
            path,
            edition: WindowsShellEdition::Pwsh7,
        });
    }

    if let Some(path) = env_path(PWSH_EXE_ENV)
        && is_usable_pwsh(&path)
    {
        return Ok(WindowsShellResolution {
            path,
            edition: WindowsShellEdition::Pwsh7,
        });
    }

    if let Some(dir) = env_path(BUNDLED_BIN_DIR_ENV) {
        let bundled = dir.join("pwsh").join("pwsh.exe");
        if is_usable_pwsh(&bundled) {
            return Ok(WindowsShellResolution {
                path: bundled,
                edition: WindowsShellEdition::Pwsh7,
            });
        }
    }

    for candidate in msi_pwsh_candidates() {
        if is_usable_pwsh(&candidate) {
            return Ok(WindowsShellResolution {
                path: candidate,
                edition: WindowsShellEdition::Pwsh7,
            });
        }
    }

    system_powershell_51_resolution()
}

fn system_51_forced() -> bool {
    std::env::var(SHELL_POLICY_ENV)
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("system51"))
}

fn override_path() -> Option<PathBuf> {
    match override_slot().lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn msi_pwsh_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for key in ["ProgramW6432", "ProgramFiles"] {
        if let Some(root) = env_path(key) {
            out.push(
                root.join("PowerShell")
                    .join("7")
                    .join("pwsh.exe"),
            );
        }
    }
    out.push(PathBuf::from(
        r"C:\Program Files\PowerShell\7\pwsh.exe",
    ));
    out
}

fn system_powershell_51_resolution() -> Result<WindowsShellResolution, crate::ProcessError> {
    let path = system_powershell_51_path().ok_or_else(|| crate::ProcessError::SpawnFailed {
        failure: SpawnFailure {
            code: "powershell_unavailable".to_owned(),
            message: "trusted Windows PowerShell 5.1 executable is unavailable".to_owned(),
        },
    })?;
    Ok(WindowsShellResolution {
        path,
        edition: WindowsShellEdition::PowerShell51,
    })
}

fn system_powershell_51_path() -> Option<PathBuf> {
    if let Some(path) = windows_directory().map(|root| {
        root.join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    }) {
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn windows_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;

        let mut buffer = vec![0_u16; 32_768];
        // SAFETY: `buffer` is writable for its declared length; the API writes a
        // NUL-terminated Windows directory path or returns zero on failure.
        let length = unsafe {
            GetWindowsDirectoryW(
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).expect("Windows directory buffer fits in u32"),
            )
        };
        if length > 0 {
            let length = usize::try_from(length).ok()?;
            if length < buffer.len() {
                buffer.truncate(length);
                return Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer)));
            }
        }
    }

    env_path("SystemRoot")
        .or_else(|| env_path("WINDIR"))
        .or(Some(PathBuf::from(r"C:\Windows")))
}

pub fn is_usable_pwsh(path: &Path) -> bool {
    if is_windows_apps_alias(path) {
        return false;
    }
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() || metadata.len() < MIN_PWSH_BYTES {
        return false;
    }
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0_u8; 2];
    file.read_exact(&mut magic).is_ok() && magic == *b"MZ"
}

fn is_windows_apps_alias(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;

    fn write_fake_pe(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        let mut file = File::create(path).expect("create");
        file.write_all(b"MZ").expect("mz");
        file.write_all(&vec![0_u8; bytes.saturating_sub(2)])
            .expect("pad");
    }

    #[test]
    fn windows_apps_alias_is_rejected() {
        let path = PathBuf::from(r"C:\Users\a\AppData\Local\Microsoft\WindowsApps\pwsh.exe");
        assert!(is_windows_apps_alias(&path));
        assert!(!is_usable_pwsh(&path));
    }

    #[test]
    fn tiny_or_non_pe_files_are_rejected() {
        let root = tempfile::tempdir().expect("temp");
        let tiny = root.path().join("pwsh.exe");
        std::fs::write(&tiny, b"MZ").expect("tiny");
        assert!(!is_usable_pwsh(&tiny));

        let text = root.path().join("not-pe.exe");
        write_fake_pe(&text, MIN_PWSH_BYTES as usize + 8);
        std::fs::write(&text, b"#!not pe").expect("overwrite");
        assert!(!is_usable_pwsh(&text));
    }

    struct OverrideGuard(Option<PathBuf>);
    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            set_windows_powershell_override(self.0.take());
        }
    }

    #[test]
    #[serial]
    fn override_wins_when_the_file_is_a_real_pe() {
        let root = tempfile::tempdir().expect("temp");
        let pwsh = root.path().join("pwsh.exe");
        write_fake_pe(&pwsh, MIN_PWSH_BYTES as usize + 8);
        assert!(is_usable_pwsh(&pwsh));

        let _guard = OverrideGuard(override_path());
        set_windows_powershell_override(Some(pwsh.clone()));
        let resolved = resolve_windows_powershell_exe_with(false).expect("override");
        assert_eq!(resolved.path, pwsh);
        assert_eq!(resolved.edition, WindowsShellEdition::Pwsh7);
    }

    #[test]
    #[serial]
    fn system51_policy_ignores_override() {
        let root = tempfile::tempdir().expect("temp");
        let pwsh = root.path().join("pwsh.exe");
        write_fake_pe(&pwsh, MIN_PWSH_BYTES as usize + 8);
        let _guard = OverrideGuard(override_path());
        set_windows_powershell_override(Some(pwsh));
        let resolved = resolve_windows_powershell_exe_with(true).expect("system 5.1");
        assert_eq!(resolved.edition, WindowsShellEdition::PowerShell51);
        assert!(
            resolved
                .path
                .ends_with(r"System32\WindowsPowerShell\v1.0\powershell.exe")
        );
    }
}
