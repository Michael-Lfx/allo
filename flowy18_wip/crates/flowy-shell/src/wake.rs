use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WAKE_FILE: &str = "wake.request";
const WAKE_DIR: &str = "wake";
const PING_PREFIX: &str = "ping-";
const URL_PREFIX: &str = "url-";
const REQ_SUFFIX: &str = ".req";
const TMP_SUFFIX: &str = ".tmp";
const CLAIMED_SUFFIX: &str = ".claimed";
const MAX_WAKE_URLS: usize = 16;
const MAX_PING_FILES: usize = 16;
const MAX_QUEUED_URL_FILES: usize = 64;
const MAX_DRAINED_URLS: usize = MAX_WAKE_URLS * 4;
const STALE_TEMP_AGE: Duration = Duration::from_secs(60);

static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WakeSignal {
    pub urls: Vec<String>,
}

fn wake_path(data_root: &Path) -> PathBuf {
    data_root.join(WAKE_FILE)
}

fn wake_dir(data_root: &Path) -> PathBuf {
    data_root.join(WAKE_DIR)
}

fn next_request_stamp() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pid = std::process::id();
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:020}-{pid:010}-{seq:010}")
}

fn sanitize_url(raw: &str) -> String {
    raw.chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

fn is_stale(path: &Path, now: SystemTime) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    now.duration_since(modified).unwrap_or_default() >= STALE_TEMP_AGE
}

fn sweep_stale_temps(dir: &Path, legacy_only: bool, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if legacy_only && !name.starts_with("wake.request.") {
            continue;
        }
        if (name.ends_with(TMP_SUFFIX) || name.ends_with(CLAIMED_SUFFIX))
            && is_stale(&entry.path(), now)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn count_queued(dir: &Path, prefix: &str) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(prefix) && name.ends_with(REQ_SUFFIX)
            })
        })
        .count()
}

fn write_atomic_request(
    dir: &Path,
    prefix: &str,
    payload: &str,
) -> io::Result<()> {
    let stamp = next_request_stamp();
    let staging = dir.join(format!("{prefix}{stamp}{TMP_SUFFIX}"));
    let target = dir.join(format!("{prefix}{stamp}{REQ_SUFFIX}"));
    fs::write(&staging, payload.as_bytes())?;
    if let Err(error) = fs::rename(&staging, &target) {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    Ok(())
}

/// Request that the running desktop instance bring its main window forward
/// and optionally process deep-link URLs supplied on launch.
pub fn signal_wake(data_root: &Path, urls: &[String]) -> io::Result<()> {
    let sanitized: Vec<String> = urls
        .iter()
        .map(|raw| sanitize_url(raw))
        .filter(|s| !s.is_empty())
        .take(MAX_WAKE_URLS)
        .collect();
    let dir = wake_dir(data_root);
    fs::create_dir_all(&dir)?;
    sweep_stale_temps(&dir, false, SystemTime::now());
    if sanitized.is_empty() {
        if count_queued(&dir, PING_PREFIX) >= MAX_PING_FILES {
            return Ok(());
        }
        return write_atomic_request(&dir, PING_PREFIX, "");
    }
    if count_queued(&dir, URL_PREFIX) >= MAX_QUEUED_URL_FILES {
        return Ok(());
    }
    write_atomic_request(&dir, URL_PREFIX, &sanitized.join("\n"))
}

fn consume_claimed_urls(
    source: &Path,
    claimed: &Path,
    limit: usize,
    urls: &mut Vec<String>,
) -> bool {
    if fs::rename(source, claimed).is_err() {
        return false;
    }
    let text = fs::read_to_string(claimed).unwrap_or_default();
    let _ = fs::remove_file(claimed);
    for line in text.lines() {
        if urls.len() >= MAX_DRAINED_URLS {
            break;
        }
        let clean = sanitize_url(line);
        if !clean.is_empty() {
            urls.push(clean);
            if limit > 0 && urls.len() >= limit {
                break;
            }
        }
    }
    true
}

/// Atomically consume pending wake requests from other launched instances.
///
/// Plain wakes coalesce while deep-link URL payloads drain in strict FIFO order.
#[must_use]
pub fn take_wake(data_root: &Path) -> Option<WakeSignal> {
    let now = SystemTime::now();
    sweep_stale_temps(data_root, true, now);

    let mut saw_any = false;
    let mut urls = Vec::new();

    // Backward compatibility: drain any legacy single-file wake request first.
    let legacy_target = wake_path(data_root);
    let legacy_claimed = data_root.join(format!(
        "{WAKE_FILE}.{}{CLAIMED_SUFFIX}",
        next_request_stamp()
    ));
    if consume_claimed_urls(
        &legacy_target,
        &legacy_claimed,
        MAX_DRAINED_URLS,
        &mut urls,
    ) {
        saw_any = true;
    }

    let dir = wake_dir(data_root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return saw_any.then_some(WakeSignal { urls });
    };

    let mut pings: Vec<String> = Vec::new();
    let mut queued_urls: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.ends_with(TMP_SUFFIX) || name.ends_with(CLAIMED_SUFFIX) {
            if is_stale(&entry.path(), now) {
                let _ = fs::remove_file(entry.path());
            }
            continue;
        }
        if !name.ends_with(REQ_SUFFIX) {
            continue;
        }
        if name.starts_with(PING_PREFIX) {
            pings.push(name);
        } else if name.starts_with(URL_PREFIX) {
            queued_urls.push(name);
        }
    }
    pings.sort();
    queued_urls.sort();

    for name in pings {
        let source = dir.join(&name);
        let claimed = dir
            .join(format!("{name}.{}{CLAIMED_SUFFIX}", next_request_stamp()));
        if fs::rename(&source, &claimed).is_ok() {
            saw_any = true;
            let _ = fs::remove_file(&claimed);
        }
    }

    for name in queued_urls {
        if urls.len() >= MAX_DRAINED_URLS {
            break;
        }
        let remaining = MAX_DRAINED_URLS - urls.len();
        let file_limit = urls.len() + remaining.min(MAX_WAKE_URLS);
        let source = dir.join(&name);
        let claimed = dir
            .join(format!("{name}.{}{CLAIMED_SUFFIX}", next_request_stamp()));
        if consume_claimed_urls(&source, &claimed, file_limit, &mut urls) {
            saw_any = true;
        }
    }

    saw_any.then_some(WakeSignal { urls })
}

#[cfg(test)]
mod tests {
    use super::{
        CLAIMED_SUFFIX, MAX_PING_FILES, MAX_QUEUED_URL_FILES, MAX_WAKE_URLS,
        TMP_SUFFIX, WAKE_FILE, WakeSignal, signal_wake, take_wake, wake_dir,
    };
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
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
        assert_eq!(fs::read_dir(wake_dir(&root)).unwrap().count(), 0);
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
        assert_eq!(drained.urls.len(), MAX_WAKE_URLS);
        assert_eq!(drained.urls[0], "flowy://invite/A_1");
        assert_eq!(drained.urls[1], "flowy://open");
        assert_eq!(take_wake(&root), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn two_deep_link_wakes_preserve_both_in_fifo_order() {
        let root = temp_root("fifo");
        let _ = fs::remove_dir_all(&root);
        signal_wake(&root, &["flowy://invite/FIRST".to_owned()]).unwrap();
        signal_wake(&root, &["flowy://invite/SECOND".to_owned()]).unwrap();
        let drained = take_wake(&root).expect("queued deep links should drain");
        assert_eq!(
            drained.urls,
            vec![
                "flowy://invite/FIRST".to_owned(),
                "flowy://invite/SECOND".to_owned(),
            ]
        );
        assert_eq!(take_wake(&root), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_wake_after_deep_link_does_not_overwrite_urls() {
        let root = temp_root("plain-after-url");
        let _ = fs::remove_dir_all(&root);
        signal_wake(&root, &["flowy://invite/KEEP_ME".to_owned()]).unwrap();
        signal_wake(&root, &[]).unwrap();
        let drained = take_wake(&root).expect("url + ping should drain");
        assert_eq!(drained.urls, vec!["flowy://invite/KEEP_ME".to_owned()]);
        assert_eq!(take_wake(&root), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multiple_plain_wakes_coalesce_and_consume_once() {
        let root = temp_root("pings");
        let _ = fs::remove_dir_all(&root);
        for _ in 0..(MAX_PING_FILES + 8) {
            signal_wake(&root, &[]).unwrap();
        }
        assert_eq!(
            fs::read_dir(wake_dir(&root)).unwrap().count(),
            MAX_PING_FILES
        );
        assert_eq!(take_wake(&root), Some(WakeSignal { urls: Vec::new() }));
        assert_eq!(take_wake(&root), None);
        assert_eq!(fs::read_dir(wake_dir(&root)).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_wake_request_file_is_still_consumed() {
        let root = temp_root("legacy");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(WAKE_FILE), "flowy://invite/LEGACY_1\n").unwrap();
        signal_wake(&root, &["flowy://invite/QUEUE_2".to_owned()]).unwrap();
        let drained =
            take_wake(&root).expect("legacy and queued signals should drain");
        assert_eq!(
            drained.urls,
            vec![
                "flowy://invite/LEGACY_1".to_owned(),
                "flowy://invite/QUEUE_2".to_owned(),
            ]
        );
        assert!(!root.join(WAKE_FILE).exists());
        assert_eq!(take_wake(&root), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn queued_url_files_are_bounded_and_drain_cleanly_across_batches() {
        let root = temp_root("bounded-urls");
        let _ = fs::remove_dir_all(&root);
        let total_sent = MAX_QUEUED_URL_FILES + 10;
        for index in 0..total_sent {
            signal_wake(&root, &[format!("flowy://invite/item-{index:03}")])
                .unwrap();
        }
        assert_eq!(
            fs::read_dir(wake_dir(&root)).unwrap().count(),
            MAX_QUEUED_URL_FILES
        );
        let mut drained = Vec::new();
        while let Some(signal) = take_wake(&root) {
            drained.extend(signal.urls);
        }
        assert_eq!(drained.len(), MAX_QUEUED_URL_FILES);
        assert_eq!(drained[0], "flowy://invite/item-000");
        assert_eq!(
            drained[MAX_QUEUED_URL_FILES - 1],
            format!("flowy://invite/item-{:03}", MAX_QUEUED_URL_FILES - 1)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_tmp_and_claimed_files_are_cleaned_up() {
        let root = temp_root("stale");
        let _ = fs::remove_dir_all(&root);
        let dir = wake_dir(&root);
        fs::create_dir_all(&dir).unwrap();
        let stale_tmp = dir.join(format!("url-old{TMP_SUFFIX}"));
        let stale_claimed = dir.join(format!("url-old{CLAIMED_SUFFIX}"));
        let legacy_claimed =
            root.join(format!("{WAKE_FILE}.old{CLAIMED_SUFFIX}"));
        for path in [&stale_tmp, &stale_claimed, &legacy_claimed] {
            let file = fs::File::create(path).unwrap();
            let old = SystemTime::now() - std::time::Duration::from_secs(120);
            file.set_modified(old).unwrap();
        }
        assert_eq!(take_wake(&root), None);
        assert!(!stale_tmp.exists());
        assert!(!stale_claimed.exists());
        assert!(!legacy_claimed.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn concurrent_signals_and_takes_drop_zero_urls_and_never_fail() {
        let root = Arc::new(temp_root("concurrent"));
        let _ = fs::remove_dir_all(root.as_ref());
        fs::create_dir_all(root.as_ref()).unwrap();

        let writers_per_kind = 4;
        let url_iters = 12;
        let ping_iters = 25;
        let start = Arc::new(Barrier::new(writers_per_kind * 2 + 1));

        let mut handles = Vec::new();
        for worker in 0..writers_per_kind {
            let root = Arc::clone(&root);
            let start = Arc::clone(&start);
            handles.push(std::thread::spawn(move || {
                start.wait();
                for iter in 0..ping_iters {
                    signal_wake(&root, &[]).unwrap_or_else(|err| {
                        panic!("plain signal_wake worker {worker} iter {iter} failed: {err}");
                    });
                }
            }));
        }
        for worker in 0..writers_per_kind {
            let root = Arc::clone(&root);
            let start = Arc::clone(&start);
            handles.push(std::thread::spawn(move || {
                start.wait();
                for iter in 0..url_iters {
                    let url = format!("flowy://invite/w{worker}-{iter:02}");
                    signal_wake(&root, &[url]).unwrap_or_else(|err| {
                        panic!("url signal_wake worker {worker} iter {iter} failed: {err}");
                    });
                }
            }));
        }

        let reader_root = Arc::clone(&root);
        let reader_start = Arc::clone(&start);
        let reader = std::thread::spawn(move || {
            reader_start.wait();
            let mut collected = Vec::new();
            for _ in 0..400 {
                if let Some(signal) = take_wake(&reader_root) {
                    collected.extend(signal.urls);
                }
                std::thread::yield_now();
            }
            while let Some(signal) = take_wake(&reader_root) {
                collected.extend(signal.urls);
            }
            collected
        });

        for handle in handles {
            handle.join().unwrap();
        }
        let mut received = reader.join().unwrap();
        while let Some(signal) = take_wake(root.as_ref()) {
            received.extend(signal.urls);
        }

        let expected: BTreeSet<String> = (0..writers_per_kind)
            .flat_map(|w| {
                (0..url_iters)
                    .map(move |i| format!("flowy://invite/w{w}-{i:02}"))
            })
            .collect();
        let actual: BTreeSet<String> = received.iter().cloned().collect();
        assert_eq!(received.len(), expected.len(), "no duplicate or lost URLs");
        assert_eq!(actual, expected);
        let _ = fs::remove_dir_all(root.as_ref());
    }
}
