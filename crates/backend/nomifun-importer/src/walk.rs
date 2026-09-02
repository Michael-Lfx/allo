//! Safe tree copy for the immutable materialized snapshot
//! (docs/agent-store/02 §2 step 5, §7, §10).
//!
//! V1 policy:
//! - copies every regular file under the source root into the versioned
//!   snapshot directory;
//! - **rejects all symbolic links** (98 §7: external symlinks are refused;
//!   in-market symlink resolution is deferred — nothing in V1 follows links),
//! - never touches files outside the source root (walkdir cannot escape it;
//!   manifest paths are validated separately in `manifest.rs`).

use std::path::Path;

use crate::digest::sha256_hex;

#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error("import copy failed: {0}")]
    Io(String),
    #[error("symbolic links are not allowed inside imported content: {0}")]
    Symlink(String),
    #[error("unsafe path component: {0}")]
    UnsafePath(String),
}

/// One copied file: snapshot-relative path (forward slashes) + content digest.
#[derive(Debug, Clone)]
pub struct CopiedFile {
    pub relative: String,
    pub digest_hex: String,
}

/// Copy `source` into `dest` (which must not exist yet) and hash every file.
///
/// The caller is responsible for cleaning up `dest` when a later stage blocks
/// the import; this function only guarantees a fully copied tree on success.
pub fn copy_tree(source: &Path, dest: &Path) -> Result<Vec<CopiedFile>, WalkError> {
    let source_abs = source
        .canonicalize()
        .map_err(|error| WalkError::Io(format!("{}: {error}", source.display())))?;
    let mut files: Vec<CopiedFile> = Vec::new();

    for entry in walkdir::WalkDir::new(&source_abs).follow_links(false) {
        let entry = entry.map_err(|error| WalkError::Io(error.to_string()))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| WalkError::Io(format!("{}: {error}", path.display())))?;
        if metadata.file_type().is_symlink() {
            return Err(WalkError::Symlink(path.display().to_string()));
        }
        let relative = path
            .strip_prefix(&source_abs)
            .map_err(|_| WalkError::UnsafePath(path.display().to_string()))?;
        if relative.as_os_str().is_empty() {
            continue; // the root itself
        }
        let rel_str = relative
            .to_str()
            .ok_or_else(|| WalkError::UnsafePath(path.display().to_string()))?;
        if rel_str.split(['\\', '/']).any(|component| component == "..") {
            return Err(WalkError::UnsafePath(path.display().to_string()));
        }

        if metadata.is_dir() {
            let target = dest.join(relative);
            std::fs::create_dir_all(&target)
                .map_err(|error| WalkError::Io(format!("{}: {error}", target.display())))?;
            continue;
        }
        if !metadata.is_file() {
            // Sockets / fifos / devices inside a plugin root are treated as
            // hostile content: refuse the whole import.
            return Err(WalkError::UnsafePath(path.display().to_string()));
        }

        let data = std::fs::read(path)
            .map_err(|error| WalkError::Io(format!("{}: {error}", path.display())))?;
        let digest_hex = sha256_hex(&data);
        let target = dest.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| WalkError::Io(format!("{}: {error}", parent.display())))?;
        }
        std::fs::write(&target, &data)
            .map_err(|error| WalkError::Io(format!("{}: {error}", target.display())))?;
        files.push(CopiedFile {
            relative: rel_str.replace('\\', "/"),
            digest_hex,
        });
    }

    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_tree(root: &Path, entries: &[(&str, &str)]) {
        for (rel, content) in entries {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn copies_sorted_files_and_hashes_content() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("plugin");
        write_tree(
            &source,
            &[
                (".codebuddy-plugin/plugin.json", r#"{"name":"p"}"#),
                ("agents/lead.md", "---\nname: lead\n---\nbody"),
                ("skills/hello/SKILL.md", "# hello"),
                ("bin/run.sh", "#!/bin/sh\necho hi"),
            ],
        );
        let dest = temp.path().join("snap");
        let files = copy_tree(&source, &dest).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].relative, ".codebuddy-plugin/plugin.json");
        assert!(dest.join("bin/run.sh").exists());
        // digest matches the content
        assert_eq!(
            files.iter().find(|f| f.relative == "bin/run.sh").unwrap().digest_hex,
            sha256_hex(b"#!/bin/sh\necho hi")
        );
    }

    #[test]
    fn rejects_symlinks_inside_the_tree() {
        #[cfg(unix)]
        {
            let temp = tempfile::tempdir().unwrap();
            let source = temp.path().join("plugin");
            fs::create_dir_all(source.join("agents")).unwrap();
            fs::write(source.join("agents/real.md"), "---\nname: x\n---\n").unwrap();
            std::os::unix::fs::symlink(temp.path().join("outside.txt"), source.join("agents/link.md"))
                .unwrap();
            let dest = temp.path().join("snap");
            let error = copy_tree(&source, &dest).unwrap_err();
            assert!(matches!(error, WalkError::Symlink(_)));
            assert!(!dest.exists(), "failed copy must not leave a partial tree");
        }
    }
}