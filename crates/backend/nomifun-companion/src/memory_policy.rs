//! Unified memory injection / learn eligibility policy for companion surfaces.
//!
//! Injection already had a budget; write paths (chat save, learner, evolution,
//! nomi distill) historically each invented filters. Route new write/read
//! decisions through [`MemoryPolicy`] instead of opening another parallel pipe.

use crate::store::CompanionMemory;

/// Bounds and kind filters for companion memory injection and learning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryPolicy {
    pub per_kind: i64,
    pub char_budget: usize,
}

impl MemoryPolicy {
    pub const DEFAULT: Self = Self {
        per_kind: 5,
        char_budget: 6000,
    };

    /// Kinds safe to inject into a remote IM channel prompt (no stale tasks).
    pub const REMOTE_INJECTION_KINDS: &'static [&'static str] =
        &["profile", "preference", "knowledge"];

    /// Kinds eligible for automatic learning pipelines (learner / evolution).
    pub const LEARNABLE_KINDS: &'static [&'static str] =
        &["profile", "preference", "knowledge", "episode", "affective"];

    pub fn filter_for_injection(
        &self,
        memories: impl IntoIterator<Item = CompanionMemory>,
        remote_channel: bool,
    ) -> Vec<CompanionMemory> {
        let memories: Vec<CompanionMemory> = if remote_channel {
            memories
                .into_iter()
                .filter(|memory| {
                    Self::REMOTE_INJECTION_KINDS
                        .iter()
                        .any(|kind| memory.kind == *kind)
                })
                .collect()
        } else {
            memories.into_iter().collect()
        };
        let mut used = 0usize;
        memories
            .into_iter()
            .filter(|memory| {
                let len = memory.content.chars().count();
                if used.saturating_add(len) > self.char_budget {
                    return false;
                }
                used = used.saturating_add(len);
                true
            })
            .collect()
    }

    pub fn allows_learn_kind(&self, kind: &str) -> bool {
        Self::LEARNABLE_KINDS.iter().any(|allowed| *allowed == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(kind: &str, content: &str) -> CompanionMemory {
        CompanionMemory {
            id: format!("mem-{kind}"),
            kind: kind.into(),
            content: content.into(),
            tags: Vec::new(),
            importance: 1.0,
            strength: 1.0,
            pinned: false,
            source: "test".into(),
            status: "active".into(),
            created_at: 0,
            updated_at: 0,
            last_reinforced_at: 0,
            scope_kind: "user".into(),
            scope_companion_id: None,
        }
    }

    #[test]
    fn remote_drops_task_kinds() {
        let policy = MemoryPolicy::DEFAULT;
        let filtered = policy.filter_for_injection(
            vec![
                mem("profile", "name=Ada"),
                mem("task", "do the thing"),
                mem("knowledge", "prefers rust"),
            ],
            true,
        );
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|m| m.kind != "task"));
    }

    #[test]
    fn char_budget_truncates() {
        let policy = MemoryPolicy {
            per_kind: 10,
            char_budget: 10,
        };
        let filtered = policy.filter_for_injection(
            vec![mem("profile", "abcdefghij"), mem("preference", "overflow")],
            false,
        );
        assert_eq!(filtered.len(), 1);
    }
}
