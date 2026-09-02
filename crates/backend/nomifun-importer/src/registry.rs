//! Idempotency and digest-conflict decision
//! (docs/agent-store/02 §9 & §11.1 TC-IMP-006).

use nomifun_db::models::PluginSnapshotRow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyDecision {
    /// No existing snapshot for this identity: persist the new one.
    New,
    /// Exact `(plugin_id, declared_version, content_digest)` match: reuse the
    /// existing immutable snapshot, never insert a duplicate.
    Reuse(PluginSnapshotRow),
    /// Same identity+version but a different digest: blocked, never overwrite.
    Conflict(PluginSnapshotRow),
}

pub fn decide(
    exact_match: Option<PluginSnapshotRow>,
    same_identity: &[PluginSnapshotRow],
) -> IdempotencyDecision {
    if let Some(row) = exact_match {
        return IdempotencyDecision::Reuse(row);
    }
    match same_identity.first() {
        Some(row) => IdempotencyDecision::Conflict(row.clone()),
        None => IdempotencyDecision::New,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(snapshot_id: &str, digest: &str) -> PluginSnapshotRow {
        PluginSnapshotRow {
            id: 1,
            snapshot_id: snapshot_id.into(),
            name: "p".into(),
            version: "1.0.0".into(),
            source_kind: "codebuddy-plugin".into(),
            source_uri: None,
            plugin_id: "p".into(),
            declared_version: "1.0.0".into(),
            resolved_revision: None,
            content_digest: digest.into(),
            status: "completed".into(),
            imported_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn exact_digest_reuses_and_conflict_blocks() {
        assert_eq!(decide(None, &[]), IdempotencyDecision::New);
        let exact = row("snap-1", "abc");
        assert_eq!(decide(Some(exact.clone()), &[]), IdempotencyDecision::Reuse(exact.clone()));
        let conflict = row("snap-1", "different");
        assert_eq!(
            decide(None, std::slice::from_ref(&conflict)),
            IdempotencyDecision::Conflict(conflict)
        );
    }
}