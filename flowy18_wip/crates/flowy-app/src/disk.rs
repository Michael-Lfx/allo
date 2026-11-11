use std::io;
use std::path::Path;

pub fn commit(path: &Path, bytes: &[u8]) -> io::Result<()> {
    flowy_core::persistence::atomic_write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::commit;
    use std::fs;

    #[test]
    fn commit_replaces_an_existing_file() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir = std::env::temp_dir().join(format!("flowy-disk-{nanos}"));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("state.json");
        commit(&path, b"{\"first\":1}").unwrap();
        commit(&path, b"{\"second\":2}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"second\":2}");
        let _ = fs::remove_dir_all(dir);
    }
}
