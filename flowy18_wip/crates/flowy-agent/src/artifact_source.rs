//! Rooted file opening for the publisher. These handles are held for the
//! whole copy, so path validation and the bytes being copied cannot diverge.

use std::fs::File;
use std::io;
use std::path::{Component, Path};

/// Both the source and its path guards must survive the complete hash/copy
/// operation. On Windows the guards prevent replacing any parent directory;
/// on Unix every child is opened relative to the already-open parent handle.
pub struct PinnedSource {
    pub file: File,
    _ancestors: Vec<File>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Open one regular file relative to a raw host-assigned output directory.
/// Never canonicalize `root` before calling this function: doing so would
/// erase a junction or symlink at the root that this helper must reject.
#[cfg(windows)]
pub fn open(root: &Path, relative: &Path) -> io::Result<PinnedSource> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const REPARSE_POINT: u32 = 0x0400;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
    const SHARE_READ: u32 = 1;
    const SHARE_WRITE: u32 = 2;

    validate_windows_root(root)?;
    let absolute = root.to_path_buf();
    let mut directories = Vec::new();
    // Pin every ancestor from the volume/UNC root down. Excluding SHARE_DELETE
    // stops renames and junction swaps while a subsequent component is opened,
    // which is what actually protects the copy - the shape of the ancestors
    // does not. A host may legitimately redirect the tree above the assigned
    // root: a junctioned profile, OneDrive, or a domain folder-redirection
    // policy. Rejecting those would skip every import for that user while
    // gaining nothing, because the pinned handles block a swap either way.
    // The root itself must still be a real directory: that is the boundary
    // the caller hands us, and anything inside it stays strict below.
    for ancestor in absolute.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let handle = OpenOptions::new()
            .access_mode(0)
            .share_mode(SHARE_READ | SHARE_WRITE)
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
            .open(ancestor)?;
        // A reparse-opened junction reports `is_dir() == false` even though its
        // attributes carry FILE_ATTRIBUTE_DIRECTORY, so the attribute bit is
        // the only reliable shape test here.
        let attributes = handle.metadata()?.file_attributes();
        let is_directory = attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
        let is_root = ancestor == absolute.as_path();
        if !is_directory {
            return Err(invalid("publisher path is not a directory"));
        }
        if is_root && attributes & REPARSE_POINT != 0 {
            return Err(invalid("publisher root is a link or reparse point"));
        }
        directories.push(handle);
    }
    let components = relative_components(relative)?;
    for component in &components {
        validate_windows_name(component.as_os_str())?;
    }
    let mut current = absolute;
    for component in &components[..components.len() - 1] {
        current.push(component.as_os_str());
        let handle = OpenOptions::new()
            .access_mode(0)
            .share_mode(SHARE_READ | SHARE_WRITE)
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
            .open(&current)?;
        let attributes = handle.metadata()?.file_attributes();
        if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(invalid("publisher path is not a directory"));
        }
        if attributes & REPARSE_POINT != 0 {
            return Err(invalid(
                "publisher directory is a link or reparse point",
            ));
        }
        directories.push(handle);
    }
    current.push(
        components
            .last()
            .expect("relative components are nonempty")
            .as_os_str(),
    );
    // Do not share writes or deletes for the source file. This prevents an
    // external writer replacing or changing the source during either hash pass.
    let file = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(current)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & REPARSE_POINT != 0 {
        return Err(invalid("publisher source is not a regular file"));
    }
    Ok(PinnedSource {
        file,
        _ancestors: directories,
    })
}

/// `openat` reports a symbolic link as `ELOOP`, which reads like a malformed
/// path rather than the real cause. `ErrorKind::FilesystemLoop` would name it,
/// but that variant is still unstable, so match the normalized errno instead.
/// Name the two sites separately: a link above the root is the harmful case
/// (this platform still requires real directories there, see `BLOCKED.md`), a
/// link at or below the root is the intended rejection.
#[cfg(unix)]
fn explain_unix_link(error: rustix::io::Errno, above_root: bool) -> io::Error {
    if error != rustix::io::Errno::LOOP {
        return error.into();
    }
    invalid(if above_root {
        "publisher path above the root is a symbolic link; this platform still requires real directories above the root (see BLOCKED.md)"
    } else {
        "publisher directory is a link or reparse point"
    })
}

#[cfg(unix)]
pub fn open(root: &Path, relative: &Path) -> io::Result<PinnedSource> {
    use rustix::fs::{Mode, OFlags, openat};

    if !root.is_absolute() {
        return Err(invalid("host output root must be absolute"));
    }
    let flags =
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directories = vec![File::open("/")?];
    for component in root.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                let opened = openat(
                    directories.last().expect("root handle exists"),
                    part,
                    flags,
                    Mode::empty(),
                )
                .map_err(|error| explain_unix_link(error, true))?;
                directories.push(File::from(opened));
            }
            _ => {
                return Err(invalid(
                    "host output root contains traversal components",
                ));
            }
        }
    }
    let components = relative_components(relative)?;
    for component in &components[..components.len() - 1] {
        let opened = openat(
            directories.last().expect("root handle exists"),
            component.as_os_str(),
            flags,
            Mode::empty(),
        )
        .map_err(|error| explain_unix_link(error, false))?;
        directories.push(File::from(opened));
    }
    let opened = openat(
        directories.last().expect("root handle exists"),
        components
            .last()
            .expect("relative components are nonempty")
            .as_os_str(),
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| explain_unix_link(error, false))?;
    let file = File::from(opened);
    if !file.metadata()?.is_file() {
        return Err(invalid("publisher source is not a regular file"));
    }
    Ok(PinnedSource {
        file,
        _ancestors: directories,
    })
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn open(_root: &Path, _relative: &Path) -> io::Result<PinnedSource> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "rooted artifact publishing is unavailable on this platform",
    ))
}

fn relative_components(relative: &Path) -> io::Result<Vec<Component<'_>>> {
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid(
            "publisher requires a relative path without traversal",
        ));
    }
    Ok(components)
}

#[cfg(windows)]
fn validate_windows_root(root: &Path) -> io::Result<()> {
    use std::path::Prefix;

    if !root.is_absolute() {
        return Err(invalid("host output root must be absolute"));
    }
    let mut components = root.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => {}
            Prefix::UNC(server, share) => {
                validate_windows_name(server)?;
                validate_windows_name(share)?;
            }
            // Verbatim and device namespaces bypass Win32 path handling.
            _ => {
                return Err(invalid(
                    "publisher root uses an unsupported path namespace",
                ));
            }
        },
        _ => {
            return Err(invalid(
                "publisher root requires a drive or UNC share",
            ));
        }
    }
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(invalid("publisher root requires a fully qualified path"));
    }
    for component in components {
        let Component::Normal(name) = component else {
            return Err(invalid(
                "host output root contains traversal components",
            ));
        };
        validate_windows_name(name)?;
    }
    Ok(())
}

#[cfg(windows)]
fn validate_windows_name(name: &std::ffi::OsStr) -> io::Result<()> {
    let name = name
        .to_str()
        .ok_or_else(|| invalid("publisher path is not valid Unicode"))?;
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    ':' | '/' | '\\' | '<' | '>' | '"' | '|' | '?' | '*'
                )
        })
    {
        return Err(invalid(
            "publisher path contains an unsafe Windows component",
        ));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(*prefix).is_some_and(|suffix| {
            suffix.chars().count() == 1
                && suffix.chars().next().is_some_and(|digit| {
                    matches!(digit, '1'..='9' | '¹' | '²' | '³')
                })
        })
    });
    if device {
        return Err(invalid("publisher path names a reserved Windows device"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::open;
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("build-test")
                .join(format!(
                    "artifact-source-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(root.join("outputs/nested")).unwrap();
            fs::write(root.join("outputs/nested/report.csv"), b"a,b\n1,2\n")
                .unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn opens_the_exact_nested_regular_file() {
        let scratch = Scratch::new();
        let mut pinned =
            open(&scratch.0.join("outputs"), Path::new("nested/report.csv"))
                .unwrap();
        let mut bytes = Vec::new();
        pinned.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"a,b\n1,2\n");
        assert!(!pinned._ancestors.is_empty());
    }

    #[test]
    fn rejects_relative_roots_and_non_relative_or_traversing_sources() {
        let scratch = Scratch::new();
        assert!(open(Path::new("outputs"), Path::new("report.csv")).is_err());
        for path in [
            "",
            ".",
            "../outside.csv",
            "nested/../nested/report.csv",
            "/outside.csv",
        ] {
            assert!(
                open(&scratch.0.join("outputs"), Path::new(path)).is_err(),
                "accepted {path}"
            );
        }
        assert!(open(&scratch.0.join("outputs"), Path::new("nested")).is_err());
    }

    #[cfg(unix)]
    fn symlink_directory(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).unwrap();
        true
    }

    #[cfg(windows)]
    fn symlink_directory(target: &Path, link: &Path) -> bool {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => true,
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                eprintln!(
                    "symlink test unavailable without Windows Developer Mode or link privilege: {error}"
                );
                false
            }
            Err(error) => {
                panic!("could not create test directory link: {error}")
            }
        }
    }

    /// Junction creation needs no privilege, so this case is verifiable on a
    /// normal developer account - unlike `symlink_dir`, which needs Developer
    /// Mode and silently skips the link tests above.
    #[cfg(windows)]
    fn junction_directory(target: &Path, link: &Path) -> bool {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output();
        match output {
            Ok(output) if output.status.success() => true,
            Ok(output) => {
                eprintln!(
                    "junction unavailable: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
                false
            }
            Err(error) => {
                eprintln!("junction creation failed: {error}");
                false
            }
        }
    }

    /// Windows only: the unix branch of `open` still walks the root components
    /// with `NOFOLLOW`, so a redirected ancestor is rejected there. Mirroring
    /// the relaxation needs a unix test run; tracked in `BLOCKED.md`.
    #[cfg(windows)]
    #[test]
    fn windows_allows_a_redirected_ancestor_but_rejects_a_link_at_or_below_the_root()
     {
        let scratch = Scratch::new();
        let redirected = scratch.0.join("redirected");
        if !junction_directory(&scratch.0.join("outputs"), &redirected) {
            return;
        }
        // A host may redirect a directory above the assigned root - OneDrive
        // or a domain policy moving the data tree, or a junctioned profile.
        // The ancestors are pinned either way, so a swap during the copy
        // still fails; only the shape of the path changes.
        assert!(
            open(&redirected.join("nested"), Path::new("report.csv")).is_ok(),
            "a redirected ancestor above the root must not block the import"
        );
        // The root itself, and anything below it, stays strict.
        assert!(open(&redirected, Path::new("nested/report.csv")).is_err());
        assert!(
            open(&scratch.0, Path::new("redirected/nested/report.csv"))
                .is_err()
        );
        // Remove the link explicitly instead of relying on `Scratch`'s
        // recursive delete to recognise the reparse point.
        let _ = fs::remove_dir(&redirected);
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn rejects_a_link_at_the_raw_output_root_and_each_ancestor() {
        let scratch = Scratch::new();
        let direct_link = scratch.0.join("output-link");
        if !symlink_directory(&scratch.0.join("outputs"), &direct_link) {
            return;
        }
        assert!(open(&direct_link, Path::new("nested/report.csv")).is_err());
        let parent_link = scratch.0.join("parent-link");
        assert!(symlink_directory(&scratch.0.join("outputs"), &parent_link));
        // Windows allows a redirected ancestor above the root; the dedicated
        // junction test covers it. Unix still follows the exact-component rule
        // here, so the assertion stays for that target only.
        #[cfg(unix)]
        assert!(
            open(&parent_link.join("nested"), Path::new("report.csv")).is_err()
        );
        assert!(
            open(&scratch.0, Path::new("output-link/nested/report.csv"))
                .is_err()
        );
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn rejects_a_link_as_the_final_source() {
        let scratch = Scratch::new();
        let original = scratch.0.join("outputs/nested/report.csv");
        let link = scratch.0.join("outputs/nested/link.csv");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&original, &link).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(&original, &link)
        {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314)
            {
                eprintln!(
                    "symlink test unavailable without Windows link privilege: {error}"
                );
                return;
            }
            panic!("could not create test file link: {error}");
        }
        assert!(
            open(&scratch.0.join("outputs"), Path::new("nested/link.csv"))
                .is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_guards_prevent_parent_or_file_replacement_and_writes() {
        let scratch = Scratch::new();
        let parent = scratch.0.join("outputs/nested");
        let source = parent.join("report.csv");
        let pinned =
            open(&scratch.0.join("outputs"), Path::new("nested/report.csv"))
                .unwrap();
        assert!(fs::OpenOptions::new().write(true).open(&source).is_err());
        assert!(fs::rename(&source, parent.join("changed.csv")).is_err());
        assert!(fs::rename(&parent, scratch.0.join("moved")).is_err());
        drop(pinned);
        fs::OpenOptions::new().write(true).open(&source).unwrap();
        fs::rename(parent, scratch.0.join("moved")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_refuses_a_source_that_another_writer_still_has_open() {
        let scratch = Scratch::new();
        let source = scratch.0.join("outputs/nested/report.csv");
        let writer = fs::OpenOptions::new().write(true).open(source).unwrap();
        assert!(
            open(&scratch.0.join("outputs"), Path::new("nested/report.csv"))
                .is_err()
        );
        drop(writer);
        assert!(
            open(&scratch.0.join("outputs"), Path::new("nested/report.csv"))
                .is_ok()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_root_validation_accepts_drive_and_unc_but_rejects_namespaces() {
        use super::validate_windows_root;
        assert!(validate_windows_root(Path::new(r"C:\host\outputs")).is_ok());
        assert!(
            validate_windows_root(Path::new(r"\\server\share\host\outputs"))
                .is_ok()
        );
        for root in [
            r"C:relative",
            r"\rooted",
            r"\\?\C:\outputs",
            r"\\?\UNC\server\share\outputs",
            r"\\.\C:\outputs",
            r"C:\host\..\outputs",
            r"C:\host:stream\outputs",
            r"C:\host.\outputs",
        ] {
            assert!(
                validate_windows_root(Path::new(root)).is_err(),
                "accepted {root}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_source_components_reject_ads_and_dos_device_aliases() {
        use super::validate_windows_name;
        for name in [
            "report.csv:secret",
            "CON",
            "NUL.txt",
            "com1.csv",
            "LPT².txt",
            "CONIN$",
            "report.csv.",
            "report.csv ",
        ] {
            assert!(
                validate_windows_name(std::ffi::OsStr::new(name)).is_err(),
                "accepted {name}"
            );
        }
        assert!(
            validate_windows_name(std::ffi::OsStr::new("销售汇总.csv")).is_ok()
        );
    }
}
