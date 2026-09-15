//! Store entry asset resolution shared by the catalog providers.
//!
//! The read-side catalogs (`skill/list`, `connector/list`) expose installed
//! products whose display icons live in the *marketplace* entry, not in the
//! runtime artifact (a materialized `SKILL.md` folder or an MCP server row).
//! This module maps a runtime product back to its marketplace provenance and
//! resolves the same public asset URL the store uses, so the installed panel
//! can render real icons instead of initial badges.
//!
//! Resolution chain:
//!   runtime product → snapshot_id → (marketplace_id, entry_name)
//!     → store entry avatar URL (`/api/app-server/store/{mkt}/entries/{e}/assets/…`)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

use nomifun_db::IPluginSnapshotRepository;

/// Icon extensions the store asset endpoint serves, in probe order.
const ICON_EXTS: [&str; 6] = ["png", "svg", "jpg", "jpeg", "webp", "gif"];

/// Resolves marketplace display assets for installed runtime products.
#[derive(Clone)]
pub struct AppServerEntryAssets {
    snapshots: Arc<dyn IPluginSnapshotRepository>,
    /// Remote materialization root (`{work_dir}/agent-store-markets`), same
    /// value the marketplace/store providers use for live-tree resolution.
    market_root: PathBuf,
    /// `snapshot_id → public avatar URL` memo, so a list call does not probe
    /// the filesystem once per row on every refresh.
    cache: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl AppServerEntryAssets {
    pub fn new(
        snapshots: Arc<dyn IPluginSnapshotRepository>,
        market_root: PathBuf,
    ) -> Self {
        Self { snapshots, market_root, cache: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Public avatar URL for one installed product, keyed by its snapshot.
    ///
    /// Returns `None` when the snapshot carries no marketplace provenance
    /// (builtin / locally imported / user skill) or the market ships no icon.
    pub async fn avatar_for_snapshot(&self, snapshot_id: &str) -> Option<String> {
        self.avatar_for_snapshot_slug(snapshot_id, None).await
    }

    /// Same as [`Self::avatar_for_snapshot`], but with an optional runtime
    /// slug (e.g. the materialized skill directory name) used as an extra icon
    /// candidate. Marketplace entries are addressed by display text
    /// (`腾讯文档`) while `icons/` is keyed by the technical slug
    /// (`tencent-docs`), which only the runtime artifact knows.
    pub async fn avatar_for_snapshot_slug(
        &self,
        snapshot_id: &str,
        slug: Option<&str>,
    ) -> Option<String> {
        let key = match slug {
            Some(slug) => format!("{snapshot_id}\u{0}{slug}"),
            None => snapshot_id.to_owned(),
        };
        if let Some(hit) = self.cache.lock().await.get(&key) {
            return hit.clone();
        }
        let resolved = self.resolve(snapshot_id, slug).await;
        self.cache.lock().await.insert(key, resolved.clone());
        resolved
    }

    /// Public avatar URL for a connector registered as an MCP server row.
    pub async fn avatar_for_mcp_server(&self, mcp_server_id: &str) -> Option<String> {
        let row = self
            .snapshots
            .find_snapshot_by_mcp_server_id(mcp_server_id)
            .await
            .ok()??;
        self.avatar_for_snapshot(&row.snapshot_id).await
    }

    /// Drop the memo (market refresh / re-install may change an entry icon).
    pub async fn invalidate(&self) {
        self.cache.lock().await.clear();
    }

    async fn resolve(&self, snapshot_id: &str, slug: Option<&str>) -> Option<String> {
        let row = self.snapshots.get_by_snapshot_id(snapshot_id).await.ok()??;
        let marketplace_id = row.marketplace_id?;
        let entry_name = row.entry_name?;
        let root = crate::market_fetch::live_root_for(&self.market_root, &marketplace_id);
        let icon = market_icon_for(&root, &entry_name, slug)?;
        Some(format!(
            "/api/app-server/store/{marketplace_id}/entries/{entry_name}/assets/{icon}"
        ))
    }
}

/// The market-level icon for an entry (`icons/<base>.<ext>`). Candidates in
/// order: the runtime slug (technical name, e.g. `tencent-docs`), the store
/// entry name (`tmeet`), then the slug recovered from the entry payload folder.
fn market_icon_for(root: &Path, entry_name: &str, slug: Option<&str>) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(slug) = slug {
        candidates.push(slug.to_owned());
    }
    candidates.push(entry_name.to_owned());
    if let Some(derived) = entry_slug(root, entry_name) {
        candidates.push(derived);
    }
    for candidate in candidates {
        for ext in ICON_EXTS {
            let icon = root.join("icons").join(format!("{candidate}.{ext}"));
            if icon.is_file() {
                return Some(format!("icons/{candidate}.{ext}"));
            }
        }
    }
    None
}

/// Technical slug for a marketplace entry named by display text: the single
/// child directory name under any `<container>/<entry_name>/` payload folder.
fn entry_slug(root: &Path, entry_name: &str) -> Option<String> {
    for container in ["skills", "connectors", "plugins"] {
        let dir = root.join(container).join(entry_name);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                return entry.file_name().to_str().map(str::to_owned);
            }
        }
    }
    None
}
