//! Process-local vault statistics for knowledge-base list/detail.
//!
//! Directory contents remain the source of truth; this cache only avoids
//! repeating a full markdown walk on every `GET /api/knowledge/bases`. Writes
//! invalidate. A cold miss returns [`VaultStats::pending`] and a single-flight
//! refresh fills the real counts.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Markdown-document counts for one knowledge base root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultStats {
    pub file_count: u64,
    pub total_size: u64,
    pub root_exists: bool,
}

impl VaultStats {
    /// Cache-miss placeholder: cards hide zero counts; `root_exists` matches
    /// the previous walk-timeout fallback (assume present).
    pub const fn pending() -> Self {
        Self {
            file_count: 0,
            total_size: 0,
            root_exists: true,
        }
    }
}

/// In-process stats keyed by knowledge-base id.
#[derive(Default)]
pub struct StatsCache {
    inner: RwLock<StatsCacheInner>,
}

#[derive(Default)]
struct StatsCacheInner {
    entries: HashMap<String, VaultStats>,
    in_flight: HashSet<String>,
}

impl StatsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, kb_id: &str) -> Option<VaultStats> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .get(kb_id)
            .copied()
    }

    pub fn insert(&self, kb_id: impl Into<String>, stats: VaultStats) {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .insert(kb_id.into(), stats);
    }

    pub fn invalidate(&self, kb_id: &str) {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .remove(kb_id);
    }

    /// Mark `kb_id` as refreshing. Returns `false` when another refresh is
    /// already in flight so callers do not start a second walk.
    pub fn begin_refresh(&self, kb_id: &str) -> bool {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.in_flight.insert(kb_id.to_owned())
    }

    pub fn finish_refresh(&self, kb_id: &str) {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .in_flight
            .remove(kb_id);
    }

    pub fn clear(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.entries.clear();
        inner.in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_stats_are_zero_counts_and_assume_root_exists() {
        assert_eq!(
            VaultStats::pending(),
            VaultStats {
                file_count: 0,
                total_size: 0,
                root_exists: true,
            }
        );
    }

    #[test]
    fn insert_get_and_invalidate() {
        let cache = StatsCache::new();
        assert_eq!(cache.get("kb"), None);
        cache.insert(
            "kb",
            VaultStats {
                file_count: 3,
                total_size: 12,
                root_exists: true,
            },
        );
        assert_eq!(cache.get("kb").map(|s| s.file_count), Some(3));
        cache.invalidate("kb");
        assert_eq!(cache.get("kb"), None);
    }

    #[test]
    fn begin_refresh_is_single_flight_until_finished() {
        let cache = StatsCache::new();
        assert!(cache.begin_refresh("kb"));
        assert!(!cache.begin_refresh("kb"));
        cache.finish_refresh("kb");
        assert!(cache.begin_refresh("kb"));
    }

    #[test]
    fn clear_drops_entries_and_in_flight() {
        let cache = StatsCache::new();
        cache.insert("kb", VaultStats::pending());
        assert!(cache.begin_refresh("kb"));
        cache.clear();
        assert_eq!(cache.get("kb"), None);
        assert!(cache.begin_refresh("kb"));
    }
}
