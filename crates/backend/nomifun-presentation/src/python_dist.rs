//! Private CPython from python-build-standalone: download one pinned archive,
//! verify size + SHA-256, extract it safely, and publish it atomically under
//! `<capability>/python/<id>/`.
//!
//! The archive is trusted only after the digest matches, but extraction is
//! still defensive (no absolute paths, no `..`, no writes through symlinks, no
//! links that resolve outside the root, entry/byte budgets) so a bad mirror
//! override or a future archive layout change cannot write outside `home`.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::manifest;
use crate::progress::{CANCELLED, InstallProgress, is_cancelled};

const READY_MARKER: &str = ".flowy-python-ready";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// A download that receives nothing for this long fails over to the next URL.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// What to download and where it may come from. Owned so tests can point it at
/// a local server with a synthetic archive.
#[derive(Debug, Clone)]
pub struct PythonSource {
    /// Directory name under `python/`.
    pub id: String,
    pub urls: Vec<String>,
    pub sha256: String,
    pub size: u64,
    /// Top-level directory inside the archive.
    pub archive_root: String,
}

impl PythonSource {
    /// Pinned build for this host, honouring [`manifest::PYTHON_BASE_URLS_ENV`].
    pub fn pinned() -> Result<Self, String> {
        let build = manifest::python_build_for_host().ok_or_else(|| {
            format!(
                "no portable Python build for {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })?;
        let bases = std::env::var(manifest::PYTHON_BASE_URLS_ENV)
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(|base| base.trim().trim_end_matches('/').to_owned())
                    .filter(|base| !base.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|bases| !bases.is_empty())
            .unwrap_or_else(|| manifest::PYTHON_BASE_URLS.iter().map(|base| (*base).to_owned()).collect());
        // `+` must be percent-encoded: GitHub's release CDN treats it as a space.
        let file = build.file_name().replace('+', "%2B");
        Ok(Self {
            id: build.id(),
            urls: bases
                .iter()
                .map(|base| format!("{base}/{}/{file}", manifest::PYTHON_RELEASE))
                .collect(),
            sha256: build.sha256.to_owned(),
            size: build.size,
            archive_root: "python".to_owned(),
        })
    }
}

/// Interpreter inside an extracted python-build-standalone home.
pub fn interpreter(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("python.exe")
    } else {
        home.join("bin").join("python3")
    }
}

/// `home` was fully extracted and published by [`ensure`].
pub fn is_ready(home: &Path) -> bool {
    home.join(READY_MARKER).is_file() && interpreter(home).is_file()
}

/// Make `<pythons_dir>/<source.id>/` a ready interpreter home, downloading and
/// extracting only when needed. Callers hold the capability install lease, so
/// partial `.download-*` / `.staging-*` leftovers belong to dead attempts.
pub async fn ensure(
    pythons_dir: &Path,
    source: &PythonSource,
    progress: &InstallProgress,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let home = crate::source::child_dir(pythons_dir, &source.id)?;
    if is_ready(&home) {
        return Ok(home);
    }
    fs::create_dir_all(pythons_dir).map_err(|error| format!("create {}: {error}", pythons_dir.display()))?;
    remove_partials(pythons_dir);
    // A home without the marker is an interrupted publish; rebuild it.
    if home.exists() {
        fs::remove_dir_all(&home).map_err(|error| format!("remove stale {}: {error}", home.display()))?;
    }

    let nonce = Uuid::now_v7().simple().to_string();
    let nonce = &nonce[nonce.len() - 8..];
    let archive = pythons_dir.join(format!(".download-{nonce}.tar.gz"));
    let staging = pythons_dir.join(format!(".staging-{nonce}"));
    let result = async {
        download(source, &archive, progress, cancel).await?;
        if is_cancelled(cancel) {
            return Err(CANCELLED.to_owned());
        }
        let (archive_path, staging_path, root) = (archive.clone(), staging.clone(), source.archive_root.clone());
        tokio::task::spawn_blocking(move || extract(&archive_path, &staging_path, &root))
            .await
            .map_err(|error| format!("extract task failed: {error}"))??;
        if !interpreter(&staging).is_file() {
            return Err(format!("archive has no {}", interpreter(Path::new(&source.archive_root)).display()));
        }
        fs::write(staging.join(READY_MARKER), source.sha256.as_bytes())
            .map_err(|error| format!("mark Python ready: {error}"))?;
        fs::rename(&staging, &home).map_err(|error| format!("publish {}: {error}", home.display()))
    }
    .await;
    let _ = fs::remove_file(&archive);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| home)
}

/// Drop every interpreter home except `keep` (still referenced by a kept
/// snapshot's venv). Best effort: Windows may hold a running interpreter.
pub fn collect_unused(pythons_dir: &Path, keep: &[String]) {
    let Ok(entries) = fs::read_dir(pythons_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || keep.iter().any(|kept| kept == &name) {
            continue;
        }
        if let Err(error) = fs::remove_dir_all(entry.path()) {
            tracing::debug!(python = %name, error = %error, "could not remove unused Python runtime");
        }
    }
}

fn remove_partials(pythons_dir: &Path) {
    let Ok(entries) = fs::read_dir(pythons_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".download-") {
            let _ = fs::remove_file(entry.path());
        } else if name.starts_with(".staging-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

// ------------------------------------------------------------------ download

async fn download(
    source: &PythonSource,
    dest: &Path,
    progress: &InstallProgress,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if source.urls.is_empty() {
        return Err("no Python download source configured".into());
    }
    let client = nomifun_net::proxy::apply_detected_proxy(reqwest::Client::builder())
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(concat!("Flowy/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("build HTTP client: {error}"))?;
    let mut errors = Vec::new();
    for url in &source.urls {
        if is_cancelled(cancel) {
            return Err(CANCELLED.into());
        }
        progress.reset();
        progress.total_bytes.store(source.size, Ordering::Relaxed);
        match download_one(&client, url, source, dest, progress, cancel).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                let _ = fs::remove_file(dest);
                if is_cancelled(cancel) {
                    return Err(CANCELLED.into());
                }
                tracing::warn!(url = %url, error = %error, "Python download failed; trying next source");
                errors.push(format!("{url}: {error}"));
            }
        }
    }
    Err(format!("all Python download sources failed: {}", errors.join("; ")))
}

async fn download_one(
    client: &reqwest::Client,
    url: &str,
    source: &PythonSource,
    dest: &Path,
    progress: &InstallProgress,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let response = cancellable(client.get(url).send(), cancel)
        .await?
        .map_err(|error| format!("request: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    if let Some(length) = response.content_length()
        && length != source.size
    {
        return Err(format!("expected {} bytes, server announced {length}", source.size));
    }

    let mut response = response;
    let mut file = fs::File::create(dest).map_err(|error| format!("create {}: {error}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    while let Some(chunk) = cancellable(response.chunk(), cancel)
        .await?
        .map_err(|error| format!("read body: {error}"))?
    {
        received += chunk.len() as u64;
        if received > source.size {
            return Err(format!("body is larger than the pinned {} bytes", source.size));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|error| format!("write {}: {error}", dest.display()))?;
        progress.received_bytes.store(received, Ordering::Relaxed);
    }
    file.sync_all().map_err(|error| format!("sync {}: {error}", dest.display()))?;
    if received != source.size {
        return Err(format!("received {received} of {} bytes", source.size));
    }
    let digest = format!("{:x}", hasher.finalize());
    if !digest.eq_ignore_ascii_case(&source.sha256) {
        return Err(format!("SHA-256 mismatch: expected {}, got {digest}", source.sha256));
    }
    Ok(())
}

/// Await `future`, giving up on cancel or after [`STALL_TIMEOUT`] of silence.
/// Dropping a pending `send`/`chunk` future is safe: no data is lost that the
/// next attempt would need, because every attempt restarts from zero.
async fn cancellable<T>(future: impl std::future::Future<Output = T>, cancel: &AtomicBool) -> Result<T, String> {
    tokio::pin!(future);
    let deadline = tokio::time::Instant::now() + STALL_TIMEOUT;
    loop {
        if is_cancelled(cancel) {
            return Err(CANCELLED.into());
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(format!("no data for {}s", STALL_TIMEOUT.as_secs()));
        }
        match tokio::time::timeout(CANCEL_POLL.min(deadline - now), &mut future).await {
            Ok(value) => return Ok(value),
            Err(_) => continue,
        }
    }
}

// ------------------------------------------------------------------- extract

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Dir,
    File,
    Symlink,
    /// pax / GNU metadata records.
    Skip,
}

#[derive(Debug, PartialEq, Eq)]
enum Planned {
    Dir(PathBuf),
    File { path: PathBuf, executable: bool },
    Symlink { path: PathBuf, target: PathBuf },
    Skip,
}

/// Validates entries in archive order. Pure, so the rules are unit-testable
/// without writing files (or creating symlinks on Windows).
struct ExtractPlan<'a> {
    root: &'a str,
    links: HashSet<PathBuf>,
    entries: usize,
    bytes: u64,
}

impl<'a> ExtractPlan<'a> {
    fn new(root: &'a str) -> Self {
        Self { root, links: HashSet::new(), entries: 0, bytes: 0 }
    }

    fn admit(
        &mut self,
        kind: EntryKind,
        path: &Path,
        link: Option<&Path>,
        size: u64,
        mode: u32,
    ) -> Result<Planned, String> {
        if kind == EntryKind::Skip {
            return Ok(Planned::Skip);
        }
        let rel = self.relative(path)?;
        self.entries += 1;
        if self.entries > manifest::MAX_PYTHON_ENTRIES {
            return Err(format!("Python archive has more than {} entries", manifest::MAX_PYTHON_ENTRIES));
        }
        if rel.as_os_str().is_empty() {
            return match kind {
                EntryKind::Dir => Ok(Planned::Skip),
                _ => Err(format!("archive root {}/ is not a directory", self.root)),
            };
        }
        // Ancestors of every earlier entry exist as real directories, so the
        // only way to write outside is through a link we created.
        if self.links.contains(&rel) || rel.ancestors().skip(1).any(|ancestor| self.links.contains(ancestor)) {
            return Err(format!("archive entry {} goes through a symlink", rel.display()));
        }
        match kind {
            EntryKind::Dir => Ok(Planned::Dir(rel)),
            EntryKind::File => {
                self.bytes = self.bytes.saturating_add(size);
                if self.bytes > manifest::MAX_PYTHON_BYTES {
                    return Err(format!("Python archive is larger than {} bytes", manifest::MAX_PYTHON_BYTES));
                }
                Ok(Planned::File { path: rel, executable: mode & 0o111 != 0 })
            }
            EntryKind::Symlink => {
                let target = link.ok_or_else(|| format!("symlink {} has no target", rel.display()))?;
                check_link_target(&rel, target)?;
                self.links.insert(rel.clone());
                Ok(Planned::Symlink { path: rel, target: target.to_path_buf() })
            }
            EntryKind::Skip => Ok(Planned::Skip),
        }
    }

    /// `python/lib/x.py` → `lib/x.py`; anything not under the root, absolute,
    /// or containing `..` is rejected.
    fn relative(&self, path: &Path) -> Result<PathBuf, String> {
        let mut components = path.components().filter(|component| !matches!(component, Component::CurDir));
        match components.next() {
            Some(Component::Normal(first)) if first == std::ffi::OsStr::new(self.root) => {}
            _ => return Err(format!("archive entry {} is outside {}/", path.display(), self.root)),
        }
        let mut rel = PathBuf::new();
        for component in components {
            match component {
                Component::Normal(name) => {
                    check_archive_name(name, path)?;
                    rel.push(name);
                }
                _ => return Err(format!("unsafe archive path {}", path.display())),
            }
        }
        Ok(rel)
    }
}

/// Targets must be `../../dir/file`: leading `..` (no deeper than the link's
/// own directory), then plain names. Forbidding `..` after a name means a
/// link can never climb out through another link, whatever the entry order.
fn check_link_target(link: &Path, target: &Path) -> Result<(), String> {
    let depth = link.parent().map_or(0, |parent| parent.components().count());
    let mut ups = 0;
    let mut descended = false;
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if !descended => {
                ups += 1;
                if ups > depth {
                    return Err(format!("symlink {} -> {} escapes the runtime", link.display(), target.display()));
                }
            }
            Component::Normal(name) => {
                check_archive_name(name, target)?;
                descended = true;
            }
            _ => {
                return Err(format!(
                    "symlink {} -> {} is not a plain relative link",
                    link.display(),
                    target.display()
                ));
            }
        }
    }
    Ok(())
}

fn check_archive_name(name: &std::ffi::OsStr, context: &Path) -> Result<(), String> {
    let name = name.to_str().ok_or_else(|| format!("non-UTF-8 name in {}", context.display()))?;
    if name.contains(['\\', ':', '\0']) {
        return Err(format!("unsafe name {name:?} in {}", context.display()));
    }
    Ok(())
}

fn entry_kind(kind: tar::EntryType) -> Result<EntryKind, String> {
    use tar::EntryType as T;
    Ok(match kind {
        T::Regular | T::Continuous => EntryKind::File,
        T::Directory => EntryKind::Dir,
        T::Symlink => EntryKind::Symlink,
        T::XGlobalHeader | T::XHeader | T::GNULongName | T::GNULongLink => EntryKind::Skip,
        other => return Err(format!("unsupported archive entry type {other:?}")),
    })
}

fn extract(archive: &Path, dest: &Path, root: &str) -> Result<(), String> {
    fs::create_dir(dest).map_err(|error| format!("create {}: {error}", dest.display()))?;
    walk_archive(archive, root, Some(dest)).map(|_| ())
}

/// Walk `archive`, validating every entry; write it under `dest` when given.
/// Returns `(files, symlinks)`.
fn walk_archive(archive: &Path, root: &str, dest: Option<&Path>) -> Result<(usize, usize), String> {
    let file = fs::File::open(archive).map_err(|error| format!("open {}: {error}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)));
    let mut plan = ExtractPlan::new(root);
    let (mut files, mut links) = (0, 0);
    for entry in tar.entries().map_err(|error| format!("read archive: {error}"))? {
        let mut entry = entry.map_err(|error| format!("read archive: {error}"))?;
        let kind = entry_kind(entry.header().entry_type())?;
        let path = entry.path().map_err(|error| format!("archive path: {error}"))?.into_owned();
        let link = entry
            .link_name()
            .map_err(|error| format!("archive link: {error}"))?
            .map(|link| link.into_owned());
        let size = entry.header().size().map_err(|error| format!("archive size: {error}"))?;
        let mode = entry.header().mode().unwrap_or(0o644);
        let planned = plan.admit(kind, &path, link.as_deref(), size, mode)?;
        match &planned {
            Planned::File { .. } => files += 1,
            Planned::Symlink { .. } => links += 1,
            _ => {}
        }
        let Some(dest) = dest else { continue };
        match planned {
            Planned::Skip => {}
            Planned::Dir(rel) => {
                let dir = dest.join(rel);
                fs::create_dir_all(&dir).map_err(|error| format!("create {}: {error}", dir.display()))?;
            }
            Planned::File { path: rel, executable } => {
                let target = dest.join(rel);
                create_parent(&target)?;
                // `create_new` refuses to open through an existing (even dangling) link.
                let mut out = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|error| format!("create {}: {error}", target.display()))?;
                std::io::copy(&mut entry, &mut out).map_err(|error| format!("write {}: {error}", target.display()))?;
                crate::source::set_executable(&target, executable)?;
            }
            Planned::Symlink { path: rel, target } => {
                let at = dest.join(rel);
                create_parent(&at)?;
                make_symlink(&target, &at)?;
            }
        }
    }
    Ok((files, links))
}

fn create_parent(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display())),
        None => Ok(()),
    }
}

#[cfg(unix)]
fn make_symlink(target: &Path, at: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, at).map_err(|error| format!("symlink {}: {error}", at.display()))
}

#[cfg(not(unix))]
fn make_symlink(_target: &Path, at: &Path) -> Result<(), String> {
    // The Windows build ships none; creating one needs Developer Mode.
    Err(format!("unexpected symlink {} in the Windows Python archive", at.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_one(kind: EntryKind, path: &str, link: Option<&str>) -> Result<Planned, String> {
        ExtractPlan::new("python").admit(kind, Path::new(path), link.map(Path::new), 1, 0o644)
    }

    #[test]
    fn entries_must_stay_under_the_archive_root() {
        assert_eq!(
            plan_one(EntryKind::File, "python/lib/os.py", None).unwrap(),
            Planned::File { path: PathBuf::from("lib/os.py"), executable: false }
        );
        assert_eq!(
            plan_one(EntryKind::File, "./python/./bin/python3.12", None).unwrap(),
            Planned::File { path: PathBuf::from("bin/python3.12"), executable: false }
        );
        assert_eq!(plan_one(EntryKind::Dir, "python/", None).unwrap(), Planned::Skip);
        for bad in ["other/lib/os.py", "python/../evil", "/python/x", "python/a/../../evil", "python/a:b", "lib/x"] {
            assert!(plan_one(EntryKind::File, bad, None).is_err(), "{bad} should be rejected");
        }
        assert!(plan_one(EntryKind::File, "python", None).is_err(), "root must be a directory");
    }

    #[test]
    fn link_targets_may_only_climb_then_descend() {
        // Shapes found in the real Linux archive.
        assert!(plan_one(EntryKind::Symlink, "python/bin/python3", Some("python3.12")).is_ok());
        assert!(plan_one(EntryKind::Symlink, "python/share/terminfo/1/1178", Some("../a/adm1178")).is_ok());
        for (path, target) in [
            ("python/bin/python3", "/usr/bin/python3"),
            ("python/bin/python3", "../../etc/passwd"),
            ("python/top", ".."),
            ("python/lib/x", "a/../../../etc"),
            ("python/lib/x", "a/../b"),
        ] {
            assert!(plan_one(EntryKind::Symlink, path, Some(target)).is_err(), "{path} -> {target}");
        }
    }

    #[test]
    fn nothing_is_written_through_or_over_a_link() {
        let mut plan = ExtractPlan::new("python");
        plan.admit(EntryKind::Symlink, Path::new("python/lib/up"), Some(Path::new("..")), 0, 0).unwrap();
        assert!(plan.admit(EntryKind::File, Path::new("python/lib/up/x"), None, 1, 0).is_err());
        assert!(plan.admit(EntryKind::Dir, Path::new("python/lib/up/d"), None, 0, 0).is_err());
        assert!(plan.admit(EntryKind::File, Path::new("python/lib/up"), None, 1, 0).is_err());
        assert!(plan.admit(EntryKind::File, Path::new("python/lib/ok"), None, 1, 0o755).is_ok());
    }

    #[test]
    fn executable_bit_and_byte_budget_are_tracked() {
        let mut plan = ExtractPlan::new("python");
        assert_eq!(
            plan.admit(EntryKind::File, Path::new("python/bin/python3.12"), None, 10, 0o755).unwrap(),
            Planned::File { path: PathBuf::from("bin/python3.12"), executable: true }
        );
        let too_big = manifest::MAX_PYTHON_BYTES;
        assert!(plan.admit(EntryKind::File, Path::new("python/big"), None, too_big, 0o644).is_err());
    }

    /// Archive with a `..` entry written below `tar::Builder`'s own checks.
    fn tar_gz(entries: &[(&str, &[u8], u32)], raw_evil: Option<&str>) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_entry_type(tar::EntryType::Regular);
            builder.append_data(&mut header, path, *data).unwrap();
        }
        if let Some(evil) = raw_evil {
            let mut header = tar::Header::new_old();
            header.as_old_mut().name[..evil.len()].copy_from_slice(evil.as_bytes());
            header.set_size(1);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, &b"x"[..]).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extract_strips_the_root_and_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("py.tar.gz");
        let exe = if cfg!(windows) { "python/python.exe" } else { "python/bin/python3" };
        std::fs::write(&archive, tar_gz(&[(exe, b"#!", 0o755), ("python/lib/os.py", b"pass", 0o644)], None)).unwrap();
        let dest = dir.path().join("home");
        extract(&archive, &dest, "python").unwrap();
        assert!(interpreter(&dest).is_file());
        assert_eq!(std::fs::read(dest.join("lib/os.py")).unwrap(), b"pass");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(interpreter(&dest)).unwrap().permissions().mode() & 0o777, 0o755);
        }

        let evil = dir.path().join("evil.tar.gz");
        std::fs::write(&evil, tar_gz(&[("python/lib/ok.py", b"", 0o644)], Some("python/../escaped"))).unwrap();
        let error = extract(&evil, &dir.path().join("home2"), "python").unwrap_err();
        assert!(error.contains("unsafe archive path"), "{error}");
        assert!(!dir.path().join("escaped").exists());
    }

    /// Validate a real python-build-standalone archive against the rules
    /// without writing it (works for Linux/macOS archives on Windows too):
    /// `FLOWY_PBS_ARCHIVE=/path/cpython-...tar.gz cargo test -p nomifun-presentation real_archive -- --ignored`
    #[test]
    #[ignore = "needs FLOWY_PBS_ARCHIVE pointing at a downloaded archive"]
    fn real_archive_layout_passes_the_rules() {
        let archive = std::env::var("FLOWY_PBS_ARCHIVE").expect("FLOWY_PBS_ARCHIVE");
        let (files, links) = walk_archive(Path::new(&archive), "python", None).unwrap();
        eprintln!("{archive}: {files} files, {links} symlinks");
        assert!(files > 1_000);
    }
}
