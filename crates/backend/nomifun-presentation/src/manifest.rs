//! Pinned facts about the one ppt-master snapshot Flowy installs.
//!
//! Changing any value here is an upgrade: bump the commit only after the
//! AtomGit mirror has synced it, regenerate the lock file (`locks/README.md`)
//! and re-run the ignored network tests.

pub const CAPABILITY_ID: &str = "presentation.ppt-master";
pub const SKILL_NAME: &str = "ppt-master";
/// `metadata.version` in the pinned SKILL.md.
pub const SKILL_VERSION: &str = "6.8.0";
pub const LICENSE: &str = "MIT";

/// main-branch commit after the `v6.8.0` tag (`5e8856b`); SKILL.md still
/// reports 6.8.0. Verified reachable on both remotes on 2026-11-05.
pub const SOURCE_COMMIT: &str = "4a68cddaf40dc87891e6b64c6a69ef46adc1110c";

/// Tried in order. AtomGit first: reachable from mainland China, mirrors
/// GitHub with some lag. Both advertise `allow-reachable-sha1-in-want`, which
/// libgit2 needs to fetch a specific commit over protocol v0/v1.
pub const DEFAULT_SOURCES: &[&str] = &[
    "https://atomgit.com/hugohe3/ppt-master.git",
    "https://github.com/hugohe3/ppt-master.git",
];

/// Optional comma-separated override of [`DEFAULT_SOURCES`] (enterprise
/// mirrors). Safe because the fetched commit is still verified.
pub const SOURCES_ENV: &str = "FLOWY_PPT_MASTER_GIT_URLS";

/// Subtree of the repository that is the Skill.
pub const SKILL_SUBDIR: &str = "skills/ppt-master";

/// First-install download: shallow pack (measured 53 MB) + portable Python
/// (22–35 MB by platform) + locked wheels (≈ 30 MB).
pub const ESTIMATED_DOWNLOAD_BYTES: u64 = 120 * 1024 * 1024;

/// Materialized subtree limits (measured: ≈ 40 MB, ≈ 13k files).
pub const MAX_SKILL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_SKILL_FILES: usize = 50_000;

/// Integrity gate shipped with the Skill; must exit 0 on the pristine snapshot.
pub const ATTRIBUTION_GUARD: &str = "scripts/attribution_guard.py";

// ------------------------------------------------------------ portable Python

/// python-build-standalone release and the CPython it ships. The lock file
/// below is resolved for exactly this minor version.
pub const PYTHON_RELEASE: &str = "20261026";
pub const PYTHON_VERSION: &str = "3.12.16";

/// One `install_only_stripped` archive. Sizes and digests come from the
/// release's `SHA256SUMS` and were re-checked by downloading the Windows and
/// Linux x86_64 archives on 2026-11-05.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PythonBuild {
    pub target: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

pub const PYTHON_BUILDS: &[PythonBuild] = &[
    PythonBuild {
        target: "x86_64-pc-windows-msvc",
        sha256: "1f2ab53880fc67ea323398c6388e05af28addc99a8e389209f08a51fdff4b149",
        size: 22_014_416,
    },
    PythonBuild {
        target: "aarch64-apple-darwin",
        sha256: "353d5f1b4fd2715e7053f12f068435d5bbad16b91e057006b5d01e0925709a7e",
        size: 25_035_343,
    },
    PythonBuild {
        target: "x86_64-apple-darwin",
        sha256: "37c38fda76fec1fbf310adcbafb7f0d96af13b5511bf8ea954c3aa8eec185734",
        size: 24_757_272,
    },
    PythonBuild {
        target: "x86_64-unknown-linux-gnu",
        sha256: "ce4f6d0a6cfdb3bed6e44c2f9affd85943c76a42e25ad3a259002f31d6fb8a20",
        size: 34_505_639,
    },
    PythonBuild {
        target: "aarch64-unknown-linux-gnu",
        sha256: "bb4b6392670c5d3f93876996fcaa19858a4a413a0c7720144c18b0d6f1f0db2d",
        size: 29_234_064,
    },
];

/// Archive base URLs tried in order: GitHub Release first, then the npmmirror
/// binary mirror (same bytes, verified by size and SHA-256 on 2026-11-05).
pub const PYTHON_BASE_URLS: &[&str] = &[
    "https://github.com/astral-sh/python-build-standalone/releases/download",
    "https://registry.npmmirror.com/-/binary/python-build-standalone",
];

/// Optional comma-separated override of [`PYTHON_BASE_URLS`]. Each base must
/// serve `<base>/<release>/<file>`; the pinned digest is still enforced.
pub const PYTHON_BASE_URLS_ENV: &str = "FLOWY_PYTHON_DIST_BASE_URLS";

/// Extracted runtime limits (measured: Linux x64 98 MB / 4.5k entries).
pub const MAX_PYTHON_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_PYTHON_ENTRIES: usize = 20_000;

/// Build for the running host. Windows on ARM uses the x64 build under
/// emulation: `skia-pathops` publishes no `win_arm64` wheel.
pub fn python_build_for_host() -> Option<&'static PythonBuild> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64" | "aarch64") => "x86_64-pc-windows-msvc",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        _ => return None,
    };
    PYTHON_BUILDS.iter().find(|build| build.target == target)
}

impl PythonBuild {
    pub fn file_name(&self) -> String {
        format!("cpython-{PYTHON_VERSION}+{PYTHON_RELEASE}-{}-install_only_stripped.tar.gz", self.target)
    }

    /// Directory name under `python/`; changes whenever the archive does.
    pub fn id(&self) -> String {
        format!("{PYTHON_VERSION}-{PYTHON_RELEASE}-{}", &self.sha256[..12])
    }
}

// ------------------------------------------------------------- dependencies

/// Core runtime group, resolved for CPython 3.12 on every supported platform
/// with exact versions and hashes (`pip --require-hashes`). Source:
/// `locks/requirements-core.in`.
pub const CORE_LOCK: &str = include_str!("../locks/requirements-core.lock.txt");

/// Tried after the user's own pip configuration fails. Hashes make any index
/// equally safe; this one is reachable from mainland China.
pub const PIP_FALLBACK_INDEX: &str = "https://mirrors.aliyun.com/pypi/simple/";

/// Import names differ from distribution names (`skia-pathops` → `pathops`).
pub const CORE_IMPORT_CHECK: &str =
    "import pptx, lxml, xlsxwriter, PIL, numpy, pathops, uharfbuzz, yaml, flask";

/// The interpreter must have what the Skill scripts and pip rely on.
pub const PYTHON_PROBE: &str = "import sys, ssl, sqlite3, zlib, venv; print('%d.%d' % sys.version_info[:2])";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_values_are_well_formed() {
        assert_eq!(SOURCE_COMMIT.len(), 40);
        assert!(SOURCE_COMMIT.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(DEFAULT_SOURCES.iter().all(|url| url.starts_with("https://")));
        assert!(PYTHON_BASE_URLS.iter().all(|url| url.starts_with("https://")));
        assert!(CORE_IMPORT_CHECK.contains("pathops"));
        assert!(!CORE_IMPORT_CHECK.contains("skia_pathops"));
        for build in PYTHON_BUILDS {
            assert_eq!(build.sha256.len(), 64, "{}", build.target);
            assert!(build.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(build.size > 10_000_000 && build.size < 100_000_000);
        }
    }

    #[test]
    fn lock_file_pins_every_core_package_with_hashes() {
        for package in ["python-pptx==", "lxml==", "flask==", "skia-pathops==", "uharfbuzz==", "numpy=="] {
            assert!(CORE_LOCK.contains(package), "{package} missing from the lock");
        }
        let pinned = CORE_LOCK.lines().filter(|line| line.contains("==") && !line.trim_start().starts_with('#')).count();
        let hashed = CORE_LOCK.matches("--hash=sha256:").count();
        assert!(pinned >= 9 && hashed >= pinned, "every pinned line needs hashes");
    }

    #[test]
    fn host_build_is_known_on_supported_platforms() {
        if cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux")) {
            let build = python_build_for_host().expect("supported host");
            assert!(build.file_name().starts_with("cpython-3.12.16+20261026-"));
            assert!(build.id().starts_with("3.12.16-20261026-"));
        }
    }
}
