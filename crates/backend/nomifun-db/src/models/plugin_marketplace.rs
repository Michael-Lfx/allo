//! Marketplace registry rows (`plugin_marketplaces`), roadmap Phase 2.
//!
//! The marketplace registry is intentionally shallow: it records *where* a
//! catalog lives and the entry projection used by the discovery layer. The
//! actual plugin payloads still flow through `plugin_snapshots` /
//! `plugin_snapshot_components` via the import pipeline; the marketplace only
//! adds provenance (`marketplace_id` / `entry_name` on snapshot rows) and the
//! cascade-uninstall linkage.

use nomifun_common::{LocalizedVariant, TimestampMs};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One discovered entry (discovery-layer projection, stored as JSON on the
/// marketplace row so the entry catalog does not need its own table yet).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceEntry {
    /// Entry identifier within the marketplace (e.g. plugin name / connector id).
    pub name: String,
    /// `directory` | `github` | `git` | `url` — how this entry's payload is fetched.
    pub source_kind: String,
    /// Entry-specific source (relative path inside the market / repo / HTTP URL).
    pub source_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The market's own `publishedAt` (`YYYY-MM-DD`, doc `18` §3), carried
    /// through verbatim. The **discovery layer** normalizes it before storing:
    /// a row written before this field deserializes to `None`, and a malformed
    /// value never reaches here (see `app_server_marketplace::published_at_of`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Localized `<field>_<lang>` variants carried by the market manifest
    /// (`description_zh` / `description_en`, `tags_zh` / `legacy_tags_en`, …),
    /// passed through verbatim so each client can fall back by its own UI
    /// language (doc `18` §4 / D8=A). Empty when the manifest declared none —
    /// rows written before v1.1 deserialize to empty and fill in on the next
    /// refresh.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub localized: BTreeMap<String, LocalizedVariant>,
    /// `strict` as declared by the **market entry** (`02` §8): `true` requires
    /// the plugin source to carry its own `.codebuddy-plugin/plugin.json`;
    /// `false` (the default, and what every row written before this field
    /// deserializes to) lets the market entry supplement or replace the
    /// manifest — see `17` §10 P5 for the half that is still unimplemented.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict: bool,
    /// Why this entry cannot be imported, when that is knowable at probe time
    /// (`02` §11.1 blocking rules). `None` = importable as far as discovery can
    /// tell. Today only the `strict` rule produces one. It is a *projection*:
    /// the import path re-evaluates the rule against the real source tree
    /// rather than trusting this stored value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// Row mapping for `plugin_marketplaces`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
pub struct PluginMarketplaceRow {
    pub id: i64,
    /// Public opaque marketplace id (kebab-case stable name).
    pub marketplace_id: String,
    pub name: String,
    pub description: Option<String>,
    /// `directory` | `github` | `git` | `url`.
    pub source_kind: String,
    /// Internal source location (never exposed publicly).
    pub source_uri: String,
    pub owner_json: Option<String>,
    pub version: Option<String>,
    pub content_digest: Option<String>,
    /// JSON array of [`MarketplaceEntry`].
    pub entries_json: String,
    pub auto_update: i64,
    pub enabled: i64,
    pub last_checked_at: Option<i64>,
    /// Resolved source revision (git commit hash / HTTP freshness marker);
    /// internal traceability only.
    pub resolved_revision: Option<String>,
    /// The HTTP source's raw `ETag`, kept so `market/refresh` can send a real
    /// `If-None-Match` (doc 18 D4 ①). Internal traceability only, `None` for
    /// git / directory sources.
    pub source_etag: Option<String>,
    /// The HTTP source's raw `Last-Modified`, kept for `If-Modified-Since`.
    /// Internal traceability only.
    pub source_last_modified: Option<String>,
    /// Local materialization root for remote sources (staging + live);
    /// internal only; `None` for directory-source marketplaces.
    pub staging_root: Option<String>,
    pub added_at: TimestampMs,
    pub updated_at: TimestampMs,
    /// Soft-delete marker; `None` = active.
    pub removed_at: Option<i64>,
}

impl PluginMarketplaceRow {
    /// Decode the entries projection.
    pub fn entries(&self) -> Vec<MarketplaceEntry> {
        serde_json::from_str(&self.entries_json).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry projection is a JSON blob, so this is the only place its shape
    /// is pinned: rows written before `published_at` existed must keep
    /// deserializing (absence means "this market declared no date").
    #[test]
    fn entries_written_before_published_at_still_deserialize() {
        let legacy = r#"[{"name":"pdf","source_kind":"directory","source_uri":"skills/pdf"}]"#;
        let entries: Vec<MarketplaceEntry> = serde_json::from_str(legacy).expect("legacy row");
        assert_eq!(entries[0].published_at, None);
    }

    /// A declared date survives the round-trip, and an absent one stays
    /// **omitted** rather than serializing as `null` — the wire distinguishes
    /// the two (`AppServerStoreItem::published_at` is `skip_serializing_if`).
    #[test]
    fn published_at_round_trips_and_is_omitted_when_absent() {
        let mut entry: MarketplaceEntry =
            serde_json::from_str(r#"{"name":"pdf","source_kind":"directory","source_uri":"skills/pdf"}"#)
                .expect("row");
        assert!(!serde_json::to_string(&entry).expect("serialize").contains("published_at"));

        entry.published_at = Some("2026-07-30".into());
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains(r#""published_at":"2026-07-30""#));

        let back: MarketplaceEntry = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(back.published_at.as_deref(), Some("2026-07-30"));
    }
}
