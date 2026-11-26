use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const WAKE_FILE: &str = "wake.request";
const MAX_WAKE_URLS: usize = 16;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WakeSignal {
    pub urls: Vec<String>,
}

fn wake_path(data_root: &Path) -> PathBuf {
    data_root.join(WAKE_FILE)
}

fn temp_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}.{nanos}", std::process::id())
}

/// Request that the running desktop instance bring its main window forward
/// and optionally process deep-link URLs supplied on launch.
pub fn signal_wake(data_root: &Path, urls: &[String]) -> io::Result<()> {
    fs::create_dir_all(data_root)?;
    let sanitized: Vec<String> = urls
        .iter()
        .map(|raw| {
            raw.chars()
                .filter(|ch| !ch.is_control())
                .collect::<String>()
        })
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .take(MAX_WAKE_URLS)
        .collect();
    let payload = sanitized.join("\n");
    let target = wake_path(data_root);
    let staging = data_root.join(format!("{WAKE_FILE}.{}.tmp", temp_suffix()));
    fs::write(&staging, payload.as_bytes())?;
    if let Err(error) = fs::rename(&staging, &target) {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    Ok(())
}

/// Atomically consume a pending wake request from another launched instance.
#[must_use]
pub fn take_wake(data_root: &Path) -> Option<WakeSignal> {
    let target = wake_path(data_root);
    let claimed =
        data_root.join(format!("{WAKE_FILE}.{}.claimed", temp_suffix()));
    if fs::rename(&target, &claimed).is_err() {
        return None;
    }
    let text = fs::read_to_string(&claimed).unwrap_or_default();
    let _ = fs::remove_file(&claimed);
    let urls = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_WAKE_URLS)
        .map(str::to_owned)
        .collect();
    Some(WakeSignal { urls })
}

#[cfg(test)]
mod tests {
    use super::{WakeSignal, signal_wake, take_wake};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!(
            "flowy-wake-{tag}-{}-{nanos}-{id}",
            std::process::id()
        ))
    }

    #[test]
    fn take_wake_returns_none_when_absent() {
        let root = temp_root("absent");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert_eq!(take_wake(&root), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn signal_wake_without_urls_is_consumed_once() {
        let root = temp_root("empty");
        let _ = fs::remove_dir_all(&root);
        signal_wake(&root, &[]).unwrap();
        assert_eq!(take_wake(&root), Some(WakeSignal { urls: Vec::new() }));
        assert_eq!(take_wake(&root), None);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn signal_wake_preserves_clean_urls_and_bounds_count() {
        let root = temp_root("urls");
        let _ = fs::remove_dir_all(&root);
        let mut urls = vec![
            " flowy://invite/A_1 \n".to_owned(),
            "   ".to_owned(),
            "flowy://open".to_owned(),
        ];
        for index in 0..20 {
            urls.push(format!("flowy://invite/item-{index}"));
        }
        signal_wake(&root, &urls).unwrap();
        let drained = take_wake(&root).expect("wake signal should be present");
        assert_eq!(drained.urls.len(), super::MAX_WAKE_URLS);
        assert_eq!(drained.urls[0], "flowy://invite/A_1");
        assert_eq!(drained.urls[1], "flowy://open");
        assert_eq!(take_wake(&root), None);
        let _ = fs::remove_dir_all(&root);
    }
}
