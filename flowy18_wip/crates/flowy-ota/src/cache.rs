//! Single-slot download cache: download and install stay separate.
//!
//! Ported from allo's `DownloadedUpdateCache`. Only a package that reached
//! [`DownloadedUpdateSlot::Ready`] may be claimed for install.

#![allow(clippy::significant_drop_tightening)]

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Snapshot of what the cache holds. Only `ready` means installable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStatus {
    /// `empty` | `downloading` | `ready` | `installing`
    pub state: &'static str,
    pub version: Option<String>,
    pub transferred: u64,
    pub total: u64,
}

/// Byte counters for the active download; safe to share with a worker thread.
#[derive(Debug, Default)]
pub struct DownloadProgressCounters {
    transferred: AtomicU64,
    total: AtomicU64,
}

impl DownloadProgressCounters {
    pub fn reset(&self) {
        self.transferred.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }

    pub fn record(&self, chunk_length: u64, content_length: Option<u64>) {
        self.transferred.fetch_add(chunk_length, Ordering::Relaxed);
        if let Some(total) = content_length {
            self.total.store(total, Ordering::Relaxed);
        }
    }

    pub fn set(&self, transferred: u64, content_length: Option<u64>) {
        self.transferred.store(transferred, Ordering::Relaxed);
        if let Some(total) = content_length {
            self.total.store(total, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.transferred.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }
}

#[derive(Debug)]
enum DownloadedUpdateSlot<T> {
    Empty,
    Downloading { version: String },
    Ready { version: String, payload: T },
    Installing { version: String },
}

/// One verified package at a time. Generic so tests can use plain payloads.
pub struct DownloadedUpdateCache<T> {
    slot: Mutex<DownloadedUpdateSlot<T>>,
    progress: Arc<DownloadProgressCounters>,
}

impl<T> fmt::Debug for DownloadedUpdateCache<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DownloadedUpdateCache")
            .field("status", &self.status())
            .finish()
    }
}

impl<T> Default for DownloadedUpdateCache<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(DownloadedUpdateSlot::Empty),
            progress: Arc::new(DownloadProgressCounters::default()),
        }
    }
}

#[derive(Debug)]
pub enum BeginUpdateDownload {
    Start,
    AlreadyReady { retained_len: u64 },
    StartReplacingReady { replaced: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum TakeReadyError {
    NotRetained { version: String },
    StillDownloading { version: String },
    AlreadyInstalling { version: String },
}

impl TakeReadyError {
    pub fn message(&self) -> String {
        match self {
            Self::NotRetained { version } => {
                format!("update {version} has not been downloaded")
            }
            Self::StillDownloading { version } => {
                format!("update {version} is still downloading")
            }
            Self::AlreadyInstalling { version } => {
                format!("update {version} is already installing")
            }
        }
    }

    pub const fn handoff_may_have_started(&self) -> bool {
        matches!(self, Self::AlreadyInstalling { .. })
    }
}

impl<T> DownloadedUpdateCache<T> {
    pub fn begin_download(
        &self,
        version: &str,
        measure: impl FnOnce(&T) -> u64,
    ) -> Result<BeginUpdateDownload, String> {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*slot {
            DownloadedUpdateSlot::Downloading { version: active }
            | DownloadedUpdateSlot::Installing { version: active } => {
                return Err(format!(
                    "update {active} is already being processed"
                ));
            }
            DownloadedUpdateSlot::Ready {
                version: ready,
                payload,
            } if ready == version => {
                return Ok(BeginUpdateDownload::AlreadyReady {
                    retained_len: measure(payload),
                });
            }
            DownloadedUpdateSlot::Empty
            | DownloadedUpdateSlot::Ready { .. } => {}
        }
        let replaced = match &*slot {
            DownloadedUpdateSlot::Ready { version: ready, .. } => {
                Some(ready.clone())
            }
            _ => None,
        };
        self.progress.reset();
        *slot = DownloadedUpdateSlot::Downloading {
            version: version.to_owned(),
        };
        Ok(match replaced {
            Some(replaced) => {
                BeginUpdateDownload::StartReplacingReady { replaced }
            }
            None => BeginUpdateDownload::Start,
        })
    }

    pub fn status(&self) -> PackageStatus {
        let slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let idle =
            |state: &'static str, version: Option<String>| PackageStatus {
                state,
                version,
                transferred: 0,
                total: 0,
            };
        match &*slot {
            DownloadedUpdateSlot::Empty => idle("empty", None),
            DownloadedUpdateSlot::Downloading { version } => {
                let (transferred, total) = self.progress.snapshot();
                PackageStatus {
                    state: "downloading",
                    version: Some(version.clone()),
                    transferred,
                    total,
                }
            }
            DownloadedUpdateSlot::Ready { version, .. } => {
                idle("ready", Some(version.clone()))
            }
            DownloadedUpdateSlot::Installing { version } => {
                idle("installing", Some(version.clone()))
            }
        }
    }

    pub fn progress_counters(&self) -> Arc<DownloadProgressCounters> {
        Arc::clone(&self.progress)
    }

    pub fn cancel_download(&self, version: &str) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(
            &*slot,
            DownloadedUpdateSlot::Downloading { version: active } if active == version
        ) {
            *slot = DownloadedUpdateSlot::Empty;
        }
    }

    pub fn finish_download(
        &self,
        version: &str,
        payload: T,
    ) -> Result<(), String> {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !matches!(
            &*slot,
            DownloadedUpdateSlot::Downloading { version: active } if active == version
        ) {
            return Err(format!(
                "download state for update {version} changed before completion"
            ));
        }
        *slot = DownloadedUpdateSlot::Ready {
            version: version.to_owned(),
            payload,
        };
        Ok(())
    }

    pub fn take_ready(&self, version: &str) -> Result<T, TakeReadyError> {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match std::mem::replace(&mut *slot, DownloadedUpdateSlot::Empty) {
            DownloadedUpdateSlot::Ready {
                version: ready,
                payload,
            } if ready == version => {
                *slot = DownloadedUpdateSlot::Installing { version: ready };
                Ok(payload)
            }
            other @ DownloadedUpdateSlot::Ready { .. } => {
                *slot = other;
                Err(TakeReadyError::NotRetained {
                    version: version.to_owned(),
                })
            }
            other @ DownloadedUpdateSlot::Downloading { .. } => {
                *slot = other;
                Err(TakeReadyError::StillDownloading {
                    version: version.to_owned(),
                })
            }
            other @ DownloadedUpdateSlot::Installing { .. } => {
                *slot = other;
                Err(TakeReadyError::AlreadyInstalling {
                    version: version.to_owned(),
                })
            }
            DownloadedUpdateSlot::Empty => Err(TakeReadyError::NotRetained {
                version: version.to_owned(),
            }),
        }
    }

    pub fn restore_ready(&self, version: String, payload: T) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(
            &*slot,
            DownloadedUpdateSlot::Installing { version: active } if active == &version
        ) {
            *slot = DownloadedUpdateSlot::Ready { version, payload };
        }
    }

    pub fn finish_install(&self, version: &str) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(
            &*slot,
            DownloadedUpdateSlot::Installing { version: active } if active == version
        ) {
            *slot = DownloadedUpdateSlot::Empty;
        }
    }
}

/// Cache specialized for verified on-disk packages.
pub type UpdatePackageCache =
    DownloadedUpdateCache<crate::updater::ReadyPackage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_download_from_install() {
        let cache = DownloadedUpdateCache::default();
        assert!(matches!(
            cache.begin_download("0.3.8", |_| 0),
            Ok(BeginUpdateDownload::Start)
        ));
        assert!(cache.begin_download("0.3.8", |_| 0).is_err());
        cache.finish_download("0.3.8", "verified bytes").unwrap();
        assert!(matches!(
            cache.begin_download("0.3.8", |_| 0),
            Ok(BeginUpdateDownload::AlreadyReady { .. })
        ));
        assert_eq!(cache.take_ready("0.3.8").unwrap(), "verified bytes");
        assert!(cache.begin_download("0.3.8", |_| 0).is_err());
        assert!(cache.take_ready("0.3.8").is_err());
    }

    #[test]
    fn completed_install_releases_the_slot() {
        let cache = DownloadedUpdateCache::default();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        cache.finish_download("0.4.2", "verified bytes").unwrap();
        let _ = cache.take_ready("0.4.2").unwrap();
        cache.finish_install("0.4.2");
        assert_eq!(cache.status().state, "empty");
        assert!(matches!(
            cache.begin_download("0.4.3", |_| 0),
            Ok(BeginUpdateDownload::Start)
        ));
    }

    #[test]
    fn finish_install_only_releases_claimed_version() {
        let cache = DownloadedUpdateCache::default();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        cache.finish_download("0.4.2", "verified bytes").unwrap();
        let _ = cache.take_ready("0.4.2").unwrap();
        cache.finish_install("0.4.1");
        assert_eq!(cache.status().state, "installing");
        assert_eq!(cache.status().version.as_deref(), Some("0.4.2"));
    }

    #[test]
    fn status_reports_version_while_active() {
        let cache = DownloadedUpdateCache::<&str>::default();
        assert_eq!(cache.status().state, "empty");
        cache.begin_download("0.4.2", |_| 0).unwrap();
        assert_eq!(cache.status().state, "downloading");
        assert_eq!(cache.status().version.as_deref(), Some("0.4.2"));
        cache.finish_download("0.4.2", "bytes").unwrap();
        assert_eq!(cache.status().state, "ready");
    }

    #[test]
    fn status_exposes_download_progress() {
        let cache = DownloadedUpdateCache::<&str>::default();
        let counters = cache.progress_counters();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        counters.record(400, Some(1000));
        counters.record(250, None);
        let downloading = cache.status();
        assert_eq!(downloading.transferred, 650);
        assert_eq!(downloading.total, 1000);
        cache.finish_download("0.4.2", "bytes").unwrap();
        assert_eq!((cache.status().transferred, cache.status().total), (0, 0));
    }

    #[test]
    fn new_download_starts_from_zero_progress() {
        let cache = DownloadedUpdateCache::<&str>::default();
        let counters = cache.progress_counters();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        counters.record(900, Some(1000));
        cache.cancel_download("0.4.2");
        cache.begin_download("0.4.2", |_| 0).unwrap();
        assert_eq!((cache.status().transferred, cache.status().total), (0, 0));
    }

    #[test]
    fn failed_download_releases_slot() {
        let cache = DownloadedUpdateCache::<&str>::default();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        cache.cancel_download("0.4.2");
        assert_eq!(cache.status().state, "empty");
        assert!(matches!(
            cache.begin_download("0.4.2", |_| 0),
            Ok(BeginUpdateDownload::Start)
        ));
    }

    #[test]
    fn replacing_ready_reports_discarded_version() {
        let cache = DownloadedUpdateCache::default();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        cache.finish_download("0.4.2", "verified bytes").unwrap();
        match cache.begin_download("0.4.3", |_| 0) {
            Ok(BeginUpdateDownload::StartReplacingReady { replaced }) => {
                assert_eq!(replaced, "0.4.2");
            }
            other => panic!("expected StartReplacingReady, got {other:?}"),
        }
    }

    #[test]
    fn already_ready_measures_under_same_lock() {
        let cache = DownloadedUpdateCache::default();
        cache.begin_download("0.4.2", |_| 0).unwrap();
        cache.finish_download("0.4.2", vec![0_u8; 1234]).unwrap();
        match cache
            .begin_download("0.4.2", |payload: &Vec<u8>| payload.len() as u64)
        {
            Ok(BeginUpdateDownload::AlreadyReady { retained_len }) => {
                assert_eq!(retained_len, 1234);
            }
            other => panic!("expected AlreadyReady, got {other:?}"),
        }
    }

    #[test]
    fn install_in_flight_is_not_recoverable() {
        let cache = DownloadedUpdateCache::<&str>::default();
        assert_eq!(
            cache.take_ready("0.4.2"),
            Err(TakeReadyError::NotRetained {
                version: "0.4.2".into()
            })
        );
        assert!(
            !TakeReadyError::NotRetained {
                version: "0.4.2".into()
            }
            .handoff_may_have_started()
        );
        assert!(
            TakeReadyError::AlreadyInstalling {
                version: "0.4.2".into()
            }
            .handoff_may_have_started()
        );
        cache.begin_download("0.4.2", |_| 0).unwrap();
        assert_eq!(
            cache.take_ready("0.4.2"),
            Err(TakeReadyError::StillDownloading {
                version: "0.4.2".into()
            })
        );
        cache.finish_download("0.4.2", "bytes").unwrap();
        cache.take_ready("0.4.2").unwrap();
        assert_eq!(
            cache.take_ready("0.4.2"),
            Err(TakeReadyError::AlreadyInstalling {
                version: "0.4.2".into()
            })
        );
    }

    #[test]
    fn wrong_version_does_not_lose_ready_package() {
        let cache = DownloadedUpdateCache::default();
        cache.begin_download("0.3.8", |_| 0).unwrap();
        cache.finish_download("0.3.8", vec![1, 2, 3]).unwrap();
        assert!(cache.take_ready("0.3.9").is_err());
        let payload = cache.take_ready("0.3.8").unwrap();
        cache.restore_ready("0.3.8".into(), payload);
        assert_eq!(cache.take_ready("0.3.8").unwrap(), vec![1, 2, 3]);
    }
}
