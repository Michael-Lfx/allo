//! SHA-256 content digests (docs/agent-store/02 §2 step 5 & §11.1).

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
pub fn tree_digest(files: &[CopiedFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.relative.as_bytes());
        hasher.update(b"\n");
        hasher.update(file.digest_hex.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
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
}