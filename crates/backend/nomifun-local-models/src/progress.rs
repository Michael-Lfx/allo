//! Lock-free download progress + sliding-window speed.
//!
//! The byte counter is a plain atomic so every chunk update lands; the previous
//! `try_lock` design silently dropped updates whenever the status snapshot held
//! the map lock. Speed is derived from a ~2s sliding window of samples instead
//! of a whole-download average, so a large file shows a real rate from the
//! first samples rather than creeping up from zero.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use nomifun_api_types::LocalModelDownloadPhase;

/// How far back samples contribute to the current speed.
pub(crate) const SPEED_WINDOW: Duration = Duration::from_secs(2);

/// Minimum gap between recorded samples; keeps the mutex cold under a fast
/// connection while still yielding a fresh rate several times a second.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(200);

/// Shared download progress for one in-flight model install.
pub(crate) struct DownloadProgress {
    total: AtomicU64,
    downloaded: AtomicU64,
    samples: Mutex<VecDeque<(Instant, u64)>>,
    /// In-flight file downloads in the pipeline (0 or 1 today, but counted).
    downloading: AtomicUsize,
    /// In-flight background SHA-256 verifications.
    verifying: AtomicUsize,
}

impl DownloadProgress {
    /// Progress for a download of `total` bytes.
    pub(crate) fn new(total: u64) -> Self {
        Self {
            total: AtomicU64::new(total),
            downloaded: AtomicU64::new(0),
            samples: Mutex::new(VecDeque::new()),
            downloading: AtomicUsize::new(0),
            verifying: AtomicUsize::new(0),
        }
    }

    /// Fully-complete progress for `total` bytes (used once a model is installed).
    pub(crate) fn completed(total: u64) -> Self {
        let progress = Self::new(total);
        progress.downloaded.store(total, Ordering::Relaxed);
        progress
    }

    /// Expected byte count, once known. Zero means the size is still unknown.
    pub(crate) fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Replace the expected size after a content-length probe.
    pub(crate) fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// Bytes accounted for so far.
    pub(crate) fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    /// Set the absolute byte count and record a sample. Used at file/segment
    /// boundaries where a concurrent `add` would double-count.
    ///
    /// The speed samples are deliberately preserved across a file boundary so a
    /// brief pause between files does not blank the rate. They are only dropped
    /// when the byte count moves backwards (a retry that rolls the offset back),
    /// where a stale forward sample would corrupt the window.
    pub(crate) fn reset(&self, downloaded: u64, now: Instant) {
        self.downloaded.store(downloaded, Ordering::Relaxed);
        {
            let mut samples = self
                .samples
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if samples
                .back()
                .is_some_and(|(_, bytes)| downloaded < *bytes)
            {
                samples.clear();
            }
        }
        self.sample(downloaded, now);
    }

    /// Add `delta` freshly-written bytes and record a sample.
    pub(crate) fn add(&self, delta: u64, now: Instant) {
        let downloaded = self.downloaded.fetch_add(delta, Ordering::Relaxed) + delta;
        self.sample(downloaded, now);
    }

    /// Record the start of a file download in the pipeline.
    pub(crate) fn begin_download(&self) {
        self.downloading.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the end of a file download in the pipeline.
    pub(crate) fn end_download(&self) {
        self.downloading.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record the start of a background verification.
    pub(crate) fn begin_verify(&self) {
        self.verifying.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the end of a background verification.
    pub(crate) fn end_verify(&self) {
        self.verifying.fetch_sub(1, Ordering::Relaxed);
    }

    /// What the install pipeline is doing right now. A finished file can be
    /// hashing while the next one downloads, so the caller can report both.
    pub(crate) fn phase(&self) -> LocalModelDownloadPhase {
        let downloading = self.downloading.load(Ordering::Relaxed) > 0;
        let verifying = self.verifying.load(Ordering::Relaxed) > 0;
        match (downloading, verifying) {
            (true, true) => LocalModelDownloadPhase::VerifyingDownloading,
            (false, true) => LocalModelDownloadPhase::Verifying,
            _ if self.downloaded() == 0 => LocalModelDownloadPhase::Connecting,
            _ => LocalModelDownloadPhase::Downloading,
        }
    }

    /// Percent complete in `[0, 100]`.
    pub(crate) fn percent(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            ((self.downloaded() as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
        }
    }

    /// Bytes/second over the sliding window ending at `now`.
    ///
    /// A single in-window sample does not blank the rate: the most recent
    /// anchor before the window is kept so a sparse window (a short stall at a
    /// file boundary) still yields a rate. Only when the newest byte is more
    /// than [`SPEED_WINDOW`] old — a real stall — does this return 0.
    pub(crate) fn speed_bps(&self, now: Instant) -> u64 {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune(&mut samples, now);
        let (Some((first_at, first_bytes)), Some((last_at, last_bytes))) =
            (samples.front(), samples.back())
        else {
            return 0;
        };
        if now.saturating_duration_since(*last_at) > SPEED_WINDOW {
            return 0;
        }
        let seconds = last_at.saturating_duration_since(*first_at).as_secs_f64();
        if seconds <= 0.0 {
            return 0;
        }
        let delta = last_bytes.saturating_sub(*first_bytes);
        (delta as f64 / seconds).round() as u64
    }

    /// True once every expected byte has been accounted for.
    #[cfg(test)]
    pub(crate) fn is_complete(&self) -> bool {
        self.total() > 0 && self.downloaded() >= self.total()
    }

    fn sample(&self, downloaded: u64, now: Instant) {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((last_at, last_bytes)) = samples.back_mut() {
            if now.saturating_duration_since(*last_at) < SAMPLE_INTERVAL {
                // Fold this update into the pending sample so the newest byte
                // count is never lost, without thrashing the lock.
                if downloaded > *last_bytes {
                    *last_bytes = downloaded;
                }
                return;
            }
        }
        samples.push_back((now, downloaded));
        prune(&mut samples, now);
    }
}

/// Drop samples older than [`SPEED_WINDOW`] so the rate reflects recent bytes.
///
/// When two or more samples remain inside the window, everything older is
/// dropped. When exactly one sample is inside the window, the nearest anchor
/// before it is kept so a sparse window still has an interval to measure. When
/// nothing is inside the window, only the newest sample is kept.
fn prune(samples: &mut VecDeque<(Instant, u64)>, now: Instant) {
    let in_window = |at: Instant| now.saturating_duration_since(at) <= SPEED_WINDOW;
    let keep_from = match samples.iter().position(|(at, _)| in_window(*at)) {
        Some(0) => 0,
        Some(index) => {
            if samples.len() - index >= 2 {
                index
            } else {
                index - 1
            }
        }
        None => samples.len().saturating_sub(1),
    };
    for _ in 0..keep_from {
        samples.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_tracks_total_and_clamps() {
        let progress = DownloadProgress::new(1000);
        assert_eq!(progress.percent(), 0.0);
        progress.reset(500, Instant::now());
        assert_eq!(progress.percent(), 50.0);
        progress.reset(2000, Instant::now());
        assert_eq!(progress.percent(), 100.0);
        assert!(progress.is_complete());
    }

    #[test]
    fn speed_uses_the_sliding_window_not_the_average() {
        let base = Instant::now();
        let progress = DownloadProgress::new(100_000_000);
        // Two slow seconds, then a fast burst: the window rate must reflect the
        // recent bytes, not the whole-history average.
        progress.reset(0, base);
        progress.add(10_000, base + Duration::from_secs(1));
        progress.add(10_000, base + Duration::from_secs(2));
        progress.add(5_000_000, base + Duration::from_millis(2_500));
        progress.add(5_000_000, base + Duration::from_secs(3));

        let now = base + Duration::from_secs(3);
        let speed = progress.speed_bps(now);
        let lifetime_average = progress.downloaded() as f64 / 3.0;
        // Window covers the ~1s burst (≈10 MB/s) while the lifetime average is
        // only ~3.3 MB/s.
        assert!(speed >= 4_500_000, "unexpected slow window rate: {speed}");
        assert!(speed as f64 > lifetime_average * 1.2);
    }

    #[test]
    fn a_real_stall_with_no_recent_bytes_reports_zero() {
        let base = Instant::now();
        let progress = DownloadProgress::new(1_000_000_000);
        progress.reset(0, base);
        progress.add(1_000_000, base + Duration::from_secs(1));

        // 5s with no new bytes: the newest sample is older than the window, so
        // the rate is genuinely zero.
        assert_eq!(progress.speed_bps(base + Duration::from_secs(5)), 0);
    }

    #[test]
    fn a_sparse_window_still_reports_a_rate() {
        let base = Instant::now();
        let progress = DownloadProgress::new(10_000_000_000);
        progress.reset(0, base);
        progress.add(1_000_000, base + Duration::from_secs(1));
        // The next byte only after a ~2.2s gap: the window holds one fresh
        // sample and the anchor just before it, so a rate is still derivable.
        let now = base + Duration::from_millis(3_200);
        progress.add(2_000_000, now);

        assert!(
            progress.speed_bps(now) > 0,
            "a sparse window should still yield a rate"
        );
    }

    #[test]
    fn phase_reports_overlapping_verify_and_download() {
        let progress = DownloadProgress::new(1000);
        assert_eq!(progress.phase(), LocalModelDownloadPhase::Connecting);

        progress.begin_download();
        // Connected but no bytes yet.
        assert_eq!(progress.phase(), LocalModelDownloadPhase::Connecting);

        progress.add(500, Instant::now());
        assert_eq!(progress.phase(), LocalModelDownloadPhase::Downloading);

        progress.begin_verify();
        assert_eq!(
            progress.phase(),
            LocalModelDownloadPhase::VerifyingDownloading
        );

        progress.end_download();
        assert_eq!(progress.phase(), LocalModelDownloadPhase::Verifying);

        progress.end_verify();
        assert_eq!(progress.phase(), LocalModelDownloadPhase::Downloading);
    }

    #[test]
    fn add_is_lossless_under_contention() {
        use std::sync::Arc;

        let progress = Arc::new(DownloadProgress::new(1_000_000));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let progress = Arc::clone(&progress);
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    progress.add(1, Instant::now());
                }
            }));
        }
        for handle in handles {
            handle.join().expect("worker");
        }
        assert_eq!(progress.downloaded(), 8000);
    }
}
