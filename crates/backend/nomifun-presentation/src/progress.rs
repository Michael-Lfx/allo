//! Counters shared between a running install step (often on a blocking
//! thread) and status readers.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nomifun_api_types::PresentationInstallProgress;
#[derive(Debug, Default)]
pub struct InstallProgress {
    pub received_bytes: AtomicU64,
    pub total_bytes: AtomicU64,
    pub received_objects: AtomicU64,
    pub total_objects: AtomicU64,
}

impl InstallProgress {
    pub fn reset(&self) {
        for counter in [&self.received_bytes, &self.total_bytes, &self.received_objects, &self.total_objects] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> PresentationInstallProgress {
        PresentationInstallProgress {
            received_bytes: self.received_bytes.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            received_objects: self.received_objects.load(Ordering::Relaxed),
            total_objects: self.total_objects.load(Ordering::Relaxed),
        }
    }
}

/// Set by the cancel endpoint and by the snapshot guard; every step polls it.
/// `SeqCst` pairs with the service storing `user_cancelled` before `cancel`.
pub fn is_cancelled(flag: &AtomicBool) -> bool {
    flag.load(Ordering::SeqCst)
}

pub const CANCELLED: &str = "install cancelled";
