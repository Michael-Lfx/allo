//! Runtime dependency detection for optional tools (ffmpeg, ripgrep, etc.).

use std::ffi::OsString;
use std::path::PathBuf;

use crate::gateway::data_dir;

/// Host-provided directory for binaries bundled next to the desktop app
/// (`<resource_dir>/bin`). Set by the Tauri shell before the agent starts.
pub const BUNDLED_BIN_DIR_ENV: &str = "NOMIFUN_BUNDLED_BIN_DIR";

/// Non-Python runtime dependencies that Nomi may need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeDep {
    Node,
    Browser,
    Ripgrep,
    Ffmpeg,
}

impl std::fmt::Display for RuntimeDep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Node => "node",
            Self::Browser => "browser",
            Self::Ripgrep => "ripgrep",
            Self::Ffmpeg => "ffmpeg",
        })
    }
}

pub fn description(dep: RuntimeDep) -> &'static str {
    match dep {
        RuntimeDep::Node => "Node.js (required for browser tools and TUI)",
        RuntimeDep::Browser => "Browser engine (Chromium, for web browsing tools)",
        RuntimeDep::Ripgrep => {
            "ripgrep (fast file search — bundled or auto-installed to Flowy/Nomi/bin)"
        }
        RuntimeDep::Ffmpeg => "ffmpeg (TTS, long video concat — auto-installed to Flowy/Nomi/bin)",
    }
}

pub fn supplemental_path_entries() -> Vec<PathBuf> {
    let home = data_dir();
    let candidates = [
        home.join("bin"),
        home.join("node").join("bin"),
        home.join("tools").join("bin"),
    ];
    candidates
        .into_iter()
        .filter(|path| path.is_dir())
        .collect()
}

fn managed_binary(home: &std::path::Path, name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        home.join(format!("{name}.exe"))
    }
    #[cfg(not(windows))]
    {
        home.join(name)
    }
}

fn is_on_path_or_managed(name: &str, managed_dirs: &[PathBuf]) -> bool {
    if which::which(name).is_ok() {
        return true;
    }
    managed_dirs
        .iter()
        .any(|dir| managed_binary(dir, name).is_file())
}

pub fn is_available(dep: RuntimeDep) -> bool {
    match dep {
        RuntimeDep::Node => {
            let managed = supplemental_path_entries();
            is_on_path_or_managed("node", &managed)
        }
        RuntimeDep::Browser => which::which("agent-browser").is_ok() || has_system_browser(),
        RuntimeDep::Ripgrep => resolve_rg_executable().is_some(),
        RuntimeDep::Ffmpeg => {
            let managed = supplemental_path_entries();
            is_on_path_or_managed("ffmpeg", &managed)
        }
    }
}

pub fn resolve_ffmpeg_executable() -> Option<PathBuf> {
    if let Ok(path) = which::which("ffmpeg") {
        return Some(path);
    }
    let managed = supplemental_path_entries();
    for dir in &managed {
        let candidate = managed_binary(dir, "ffmpeg");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn rg_binary_name() -> &'static str {
    if cfg!(windows) {
        "rg.exe"
    } else {
        "rg"
    }
}

/// Locate a usable `rg` binary.
///
/// Preference order (product must not depend on Cursor/VS Code):
/// 1. Managed Flowy/Nomi `bin` (auto-installed)
/// 2. App-bundled `bin` (installer resource / Tauri resource dir)
/// 3. `PATH`
/// 4. Well-known developer locations (cargo, editors) as last resort
pub fn resolve_rg_executable() -> Option<PathBuf> {
    let managed = supplemental_path_entries();
    for dir in &managed {
        let candidate = managed_binary(dir, "rg");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    for candidate in bundled_rg_candidates(std::env::var_os(BUNDLED_BIN_DIR_ENV)) {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Ok(path) = which::which("rg") {
        return Some(path);
    }
    #[cfg(windows)]
    if let Ok(path) = which::which("rg.exe") {
        return Some(path);
    }

    for candidate in well_known_rg_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn bundled_rg_candidates(env_override: Option<OsString>) -> Vec<PathBuf> {
    let name = rg_binary_name();
    let mut out = Vec::new();

    if let Some(dir) = env_override.filter(|v| !v.is_empty()) {
        out.push(PathBuf::from(dir).join(name));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            out.push(exe_dir.join(name));
            out.push(exe_dir.join("bin").join(name));
            out.push(exe_dir.join("resources").join("bin").join(name));
            if let Some(parent) = exe_dir.parent() {
                // macOS .app: Contents/MacOS → Contents/Resources/bin
                out.push(parent.join("Resources").join("bin").join(name));
            }
        }
    }

    out
}

fn well_known_rg_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let user = std::env::var_os("USERPROFILE").map(PathBuf::from);
        if let Some(local) = local {
            out.push(
                local
                    .join("Programs")
                    .join("cursor")
                    .join("resources")
                    .join("app")
                    .join("node_modules")
                    .join("@vscode")
                    .join("ripgrep")
                    .join("bin")
                    .join("rg.exe"),
            );
            out.push(
                local
                    .join("Programs")
                    .join("Microsoft VS Code")
                    .join("resources")
                    .join("app")
                    .join("node_modules")
                    .join("@vscode")
                    .join("ripgrep")
                    .join("bin")
                    .join("rg.exe"),
            );
            out.push(
                local
                    .join("Programs")
                    .join("Microsoft VS Code Insiders")
                    .join("resources")
                    .join("app")
                    .join("node_modules")
                    .join("@vscode")
                    .join("ripgrep")
                    .join("bin")
                    .join("rg.exe"),
            );
        }
        if let Some(user) = user {
            out.push(user.join(".cargo").join("bin").join("rg.exe"));
            out.push(user.join("scoop").join("shims").join("rg.exe"));
        }
        out.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin\rg.exe"));
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            out.push(home.join(".cargo").join("bin").join("rg"));
        }
        out.push(PathBuf::from(
            "/Applications/Cursor.app/Contents/Resources/app/node_modules/@vscode/ripgrep/bin/rg",
        ));
        out.push(PathBuf::from(
            "/Applications/Visual Studio Code.app/Contents/Resources/app/node_modules/@vscode/ripgrep/bin/rg",
        ));
        out.push(PathBuf::from("/opt/homebrew/bin/rg"));
        out.push(PathBuf::from("/usr/local/bin/rg"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            out.push(home.join(".cargo").join("bin").join("rg"));
        }
        out.push(PathBuf::from("/usr/bin/rg"));
        out.push(PathBuf::from("/usr/local/bin/rg"));
    }

    out
}

fn has_system_browser() -> bool {
    let candidates: &[&str] = if cfg!(windows) {
        &["chrome", "msedge", "chromium"]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "chrome",
        ]
    };
    candidates.iter().any(|name| which::which(name).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rg_finds_path_or_well_known_binary() {
        // On developer machines with Cursor installed this should succeed even
        // when `rg` is absent from PATH (the Allo agent case).
        if resolve_rg_executable().is_none() {
            eprintln!("skip: no rg on PATH, managed, bundled, or well-known locations");
        }
    }

    #[test]
    fn bundled_candidates_include_env_override() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("packaged-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = rg_binary_name();
        let rg = bin.join(name);
        std::fs::write(&rg, b"stub").unwrap();

        let found = bundled_rg_candidates(Some(bin.as_os_str().to_os_string()));
        assert!(found.iter().any(|p| p == &rg));
        assert!(rg.is_file());
    }
}
