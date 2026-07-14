//! Runtime dependency detection for optional tools (ffmpeg, ripgrep, etc.).

use std::path::PathBuf;

use crate::gateway::data_dir;

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
        RuntimeDep::Ripgrep => "ripgrep (fast file search)",
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
    let managed = supplemental_path_entries();
    match dep {
        RuntimeDep::Node => is_on_path_or_managed("node", &managed),
        RuntimeDep::Browser => which::which("agent-browser").is_ok() || has_system_browser(),
        RuntimeDep::Ripgrep => is_on_path_or_managed("rg", &managed),
        RuntimeDep::Ffmpeg => is_on_path_or_managed("ffmpeg", &managed),
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
