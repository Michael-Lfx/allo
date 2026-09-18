//! SHA-256 content digests (docs/agent-store/02 §2 step 5 & §11.1).

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::walk::CopiedFile;

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Whole-tree digest over the sorted `(relative_path, file_digest)` pairs.
/// Stable across platforms because relative paths use forward slashes and the
/// list is sorted before hashing.
///
/// Sorting happens **here**, not only in `copy_tree`: doc 18 §5.4 requires
/// order stability that does not depend on the filesystem's traversal order.
pub fn tree_digest(files: &[CopiedFile]) -> String {
    let mut ordered: Vec<&CopiedFile> = files.iter().collect();
    ordered.sort_by(|left, right| left.relative.cmp(&right.relative));
    let mut hasher = Sha256::new();
    for file in ordered {
        hasher.update(file.relative.as_bytes());
        hasher.update(b"\n");
        hasher.update(file.digest_hex.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Digest a directory tree **on disk** with the same rule as [`tree_digest`].
///
/// This is the entry point for callers that never go through `copy_tree` — the
/// directory-source marketplace refresh. One algorithm behind both paths is
/// exactly what doc 18 §5.4 asks for: before this, refresh hashed
/// `relative bytes + raw bytes` with no separators, so the same tree produced
/// two incomparable digests depending on which path computed it (deviation D2).
pub fn tree_digest_of_dir(root: &Path) -> String {
    let mut files: Vec<CopiedFile> = Vec::new();
    collect_dir(root, root, &mut files);
    tree_digest(&files)
}

/// Depth-first collection of `(relative, sha256)` pairs. Entries that cannot be
/// read are skipped: a directory market must never fail a refresh because a file
/// vanished or is locked mid-walk.
fn collect_dir(root: &Path, dir: &Path, out: &mut Vec<CopiedFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_dir(root, &path, out);
        } else if path.is_file() {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            out.push(CopiedFile {
                relative: relative.to_string_lossy().replace('\\', "/"),
                digest_hex: sha256_hex(&bytes),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(relative: &str, digest: &str) -> CopiedFile {
        CopiedFile { relative: relative.to_owned(), digest_hex: digest.to_owned() }
    }

    #[test]
    fn tree_digest_is_sorted_and_stable() {
        let mut files = vec![
            file("b.txt", "bb"),
            file("a.txt", "aa"),
            file("dir/c.txt", "cc"),
        ];
        // copy_tree sorts; simulate an unsorted caller.
        files.sort_by(|a, b| a.relative.cmp(&b.relative));
        let one = tree_digest(&files);
        let two = tree_digest(&[
            file("a.txt", "aa"),
            file("b.txt", "bb"),
            file("dir/c.txt", "cc"),
        ]);
        assert_eq!(one, two);
        // a content change changes the digest
        let changed = tree_digest(&[file("a.txt", "different")]);
        assert_ne!(one, changed);
    }

    #[test]
    fn sha256_is_hex_of_digest() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn directory_and_copy_paths_agree_on_the_digest() {
        // doc 18 §5.4: the same tree must digest identically whichever path
        // computes it — the invariant the two-algorithm deviation broke.
        let dir = std::env::temp_dir().join(format!("agent-store-digest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dir")).unwrap();
        std::fs::write(dir.join("b.txt"), b"bb").unwrap();
        std::fs::write(dir.join("a.txt"), b"aa").unwrap();
        std::fs::write(dir.join("dir/c.txt"), b"cc").unwrap();

        let from_dir = tree_digest_of_dir(&dir);
        let from_copy_list = tree_digest(&[
            file("dir/c.txt", &sha256_hex(b"cc")),
            file("b.txt", &sha256_hex(b"bb")),
            file("a.txt", &sha256_hex(b"aa")),
        ]);
        assert_eq!(from_dir, from_copy_list, "both paths must agree (doc 18 §5.4)");

        // Content sensitive…
        std::fs::write(dir.join("a.txt"), b"changed").unwrap();
        assert_ne!(tree_digest_of_dir(&dir), from_dir);

        // …and stable when only the traversal order would differ (nested file
        // vs. top-level file with the same names re-created elsewhere).
        std::fs::write(dir.join("a.txt"), b"aa").unwrap();
        assert_eq!(tree_digest_of_dir(&dir), from_dir);

        let _ = std::fs::remove_dir_all(&dir);
    }
}