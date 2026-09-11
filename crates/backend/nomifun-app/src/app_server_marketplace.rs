//! App Server Marketplace adapter for the composition root (`nomifun-app`).
//!
//! A marketplace (Phase A) is a local directory catalog discovered on the
//! trusted host:
//! - `add`           probes the directory, derives entries (connector market /
//!                   skill market / plugin root / CLI connector), and registers
//!                   the marketplace + its entries projection;
//! - `import_entry`  runs the existing import pipeline against the entry's
//!                   sub-path and records provenance (marketplace_id + entry);
//! - `remove`        soft-deletes the registry row and, when `cascade`, clears
//!                   the install state of snapshots imported from it (the
//!                   snapshots themselves are kept).
//!
//! The adapter never executes content and never exposes the source URI; it
//! only converts registry rows into public projections.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerImportResult, AppServerMarketplaceAddRequest, AppServerMarketplaceDetail,
    AppServerMarketplaceEntry, AppServerMarketplaceEntrySnapshot,
    AppServerMarketplaceRefreshResult, AppServerMarketplaceRemoveResult,
    AppServerMarketplaceSourceKind, AppServerMarketplaceSummary,
};
use nomifun_app_server::{InstallProvider, MarketplaceProvider};
use nomifun_common::{AppError, LocalizedVariant, collect_localized_variants};
use nomifun_db::{
    IMarketplaceRepository, MarketplaceEntry, NewPluginMarketplace, PluginMarketplaceRow,
};
use nomifun_importer::{ImporterService, SourceKind, sanitize_slug};

/// Directory market kinds detected by the probe (Phase A: directory only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketKind {
    /// `connectors/` + `connectors.json`: each connector is an entry.
    Connectors,
    /// `.codebuddy-skill/marketplace.json`: each skill is an entry.
    Skills,
    /// `.codebuddy-plugin/marketplace.json`: each plugin (often under
    /// `plugins/<id>/`) is an entry. This is the real WorkBuddy expert-market
    /// layout (`marketplaces/experts/.codebuddy-plugin/marketplace.json`).
    PluginMarket,
    /// `.codebuddy-plugin/plugin.json` at the root: the root itself is one
    /// plugin entry (CodeBuddy plugin directory).
    PluginRoot,
    /// `cli.json` at the root: a single CLI connector entry.
    CliConnector,
    /// Plain directory of plugin sub-directories (`plugins/<id>/` with a
    /// manifest inside): each sub-directory is an entry.
    PluginCollection,
}

impl MarketKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connectors => "connector-market",
            Self::Skills => "skill-market",
            Self::PluginMarket => "plugin-market",
            Self::PluginRoot => "plugin-root",
            Self::CliConnector => "cli-connector",
            Self::PluginCollection => "plugin-collection",
        }
    }
}

/// Individual entry candidate before normalization.
#[derive(Debug, Clone)]
pub struct ScannedEntry {
    pub name: String,
    pub relative: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub category: Option<String>,
    /// Localized `<field>_<lang>` variants from the manifest row, carried
    /// through to the registry and the public projection (doc `18` §4 /
    /// D8=A). The reader picks the language, never the server.
    pub localized: BTreeMap<String, LocalizedVariant>,
    /// The entry's declared `strict` (`02` §8); `false` when the row says
    /// nothing (the pre-existing behaviour).
    pub strict: bool,
    /// Set when discovery already knows the entry cannot be imported.
    pub blocked_reason: Option<String>,
}

/// `strict=true` requires the plugin source to carry **its own**
/// `.codebuddy-plugin/plugin.json` (`02` §8 / §11.1; `17` §3 / §7).
///
/// Returns the blocking reason when the rule is violated, `None` otherwise.
///
/// `strict=false` deliberately returns `None` even when `plugin.json` is
/// absent: the spec lets that entry *supplement or replace* the manifest, so
/// absence is not by itself an error — implementing that substitution is the
/// outstanding half of `17` §10 P5, and callers keep their existing behaviour
/// for it (discovery skips such rows).
///
/// The reason never contains a filesystem path: source paths are internal
/// traceability only (`02` §9), so the message names the entry instead. It is
/// also just a sentence for humans — nothing parses it.
pub(crate) fn strict_entry_block(strict: bool, plugin_json_present: bool) -> Option<String> {
    if !strict || plugin_json_present {
        return None;
    }
    Some(
        "市场条目声明 strict=true，要求插件来源自带 .codebuddy-plugin/plugin.json，\
         但该来源缺少此文件（02 §11.1 阻断规则）"
            .to_owned(),
    )
}

/// Probe a local directory and derive its market kind + entries.
///
/// Entry sources are always *relative* to the market root so `entry import`
/// can resolve them without exposing absolute paths.
pub fn probe_directory(root: &Path) -> Result<(MarketKind, Vec<ScannedEntry>), AppError> {
    if !root.is_dir() {
        return Err(AppError::NotFound(format!(
            "marketplace directory not found: {}",
            root.display()
        )));
    }
    // 1. connector market: `.codebuddy-connector/connectors.json`
    if root.join(".codebuddy-connector/connectors.json").is_file() {
        let entries = probe_connector_market(root)?;
        return Ok((MarketKind::Connectors, entries));
    }
    // 2. server-side skill market: `.codebuddy-skill/marketplace.json`
    if root.join(".codebuddy-skill/marketplace.json").is_file() {
        let entries = probe_skill_market(root)?;
        return Ok((MarketKind::Skills, entries));
    }
    // 2b. plugin market: `.codebuddy-plugin/marketplace.json` (WorkBuddy
    // expert markets; `plugins` array, entries under `plugins/<id>/`).
    if root.join(".codebuddy-plugin/marketplace.json").is_file() {
        let entries = probe_plugin_market(root)?;
        return Ok((MarketKind::PluginMarket, entries));
    }
    // 3. plugin root / CLI connector at the root
    if root.join(".codebuddy-plugin/plugin.json").is_file() {
        return Ok((MarketKind::PluginRoot, vec![scan_root_as_entry(root)]));
    }
    if root.join("cli.json").is_file() {
        return Ok((MarketKind::CliConnector, vec![scan_root_as_entry(root)]));
    }
    // 4. plugin collection: sub-directories that look like plugins
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(root)
        .map_err(|error| AppError::Internal(format!("read market dir: {error}")))?
        .filter_map(|entry| entry.ok())
    {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if looks_like_plugin(&entry.path()) {
            entries.push(ScannedEntry {
                name: name.clone(),
                relative: name.clone(),
                description: None,
                keywords: vec![],
                category: None,
                localized: BTreeMap::new(),
                // A plain collection has no manifest row, hence no `strict`
                // declaration to honour.
                strict: false,
                blocked_reason: None,
            });
        }
    }
    if entries.is_empty() {
        return Err(AppError::BadRequest(
            "directory does not look like a marketplace (no manifest or plugin sub-directories)".into(),
        ));
    }
    Ok((MarketKind::PluginCollection, entries))
}

fn looks_like_plugin(path: &Path) -> bool {
    path.join(".codebuddy-plugin/plugin.json").is_file()
        || path.join("cli.json").is_file()
        || path.join("SKILL.md").is_file()
}

fn scan_root_as_entry(root: &Path) -> ScannedEntry {
    let name = root
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "market".into());
    ScannedEntry {
        name,
        relative: ".".into(),
        description: None,
        keywords: vec![],
        category: None,
        localized: BTreeMap::new(),
        strict: false,
        blocked_reason: None,
    }
}

fn probe_connector_market(root: &Path) -> Result<Vec<ScannedEntry>, AppError> {
    let text = std::fs::read_to_string(root.join(".codebuddy-connector/connectors.json"))
        .map_err(|error| AppError::Internal(format!("read connectors.json: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| AppError::Internal(format!("parse connectors.json: {error}")))?;
    let list = value
        .get("connectors")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AppError::BadRequest("connectors.json has no connectors array".into()))?;
    let mut entries = Vec::new();
    for item in list {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        entries.push(ScannedEntry {
            name: id.to_owned(),
            relative: format!("connectors/{id}"),
            description: item.get("description").and_then(|v| v.as_str()).map(str::to_owned),
            keywords: vec![],
            category: None,
            localized: collect_localized_variants(item),
            // `strict` is a plugin-market field (`02` §8); connector rows do
            // not declare it and are never gated by it.
            strict: false,
            blocked_reason: None,
        });
    }
    Ok(entries)
}

fn probe_skill_market(root: &Path) -> Result<Vec<ScannedEntry>, AppError> {    let text = std::fs::read_to_string(root.join(".codebuddy-skill/marketplace.json"))
        .map_err(|error| AppError::Internal(format!("read marketplace.json: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| AppError::Internal(format!("parse marketplace.json: {error}")))?;
    let list = value
        .get("skills")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AppError::BadRequest("marketplace.json has no skills array".into()))?;
    let mut entries = Vec::new();
    for item in list {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        // `source` may be a directory or a direct SKILL.md path. Real
        // workbuddy skill markets keep entries under `skills/<source>/`
        // (or `skills/<source>/SKILL.md`), so probe both layouts.
        let source = item
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .trim_start_matches("./")
            .to_owned();
        let relative = if source.ends_with(".md") {
            source.clone()
        } else {
            format!("{source}/SKILL.md")
        };
        let entry_dir = if root.join("skills").join(&source).is_dir() {
            format!("skills/{source}")
        } else if root.join("skills").join(&relative).is_file() {
            format!("skills/{source}")
        } else if root.join(&relative).is_file() || root.join(&source).is_dir() {
            source.clone()
        } else {
            continue;
        };
        entries.push(ScannedEntry {
            name: name.to_owned(),
            relative: entry_dir,
            description: item.get("description").and_then(|v| v.as_str()).map(str::to_owned),
            keywords: item
                .get("keywords")
                .and_then(|v| v.as_array())
                .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            category: item.get("category").and_then(|v| v.as_str()).map(str::to_owned),
            localized: collect_localized_variants(item),
            // Skill rows declare no `strict` (`02` §8): the rule is about a
            // plugin source carrying its own plugin.json.
            strict: false,
            blocked_reason: None,
        });
    }
    Ok(entries)
}

/// Probe a `.codebuddy-plugin/marketplace.json` market: each `plugins[]`
/// row is an entry (real WorkBuddy expert markets, e.g.
/// `marketplaces/experts/.codebuddy-plugin/marketplace.json` with
/// `plugins: [{ name, source: ./plugins/<id>, description }]`).
///
/// Each row's `strict` (`02` §8) is read here. A row that declares
/// `strict=true` while its source carries no `plugin.json` is **kept and
/// marked** (`blocked_reason`) instead of silently disappearing: the spec
/// says that combination blocks the install (`02` §11.1), and an invisible
/// entry cannot explain that to anyone. Rows without `strict` keep the
/// historical skip — see `strict_entry_block`.
fn probe_plugin_market(root: &Path) -> Result<Vec<ScannedEntry>, AppError> {
    let text = std::fs::read_to_string(root.join(".codebuddy-plugin/marketplace.json"))
        .map_err(|error| AppError::Internal(format!("read plugin marketplace.json: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| AppError::Internal(format!("parse plugin marketplace.json: {error}")))?;
    let list = value
        .get("plugins")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AppError::BadRequest("plugin marketplace.json has no plugins array".into()))?;
    let mut entries = Vec::new();
    for item in list {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        // `source` is relative to the market root (`./plugins/<id>`); fall
        // back to `plugins/<name>` when absent.
        let source = item
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_owned();
        let source = if source.contains('/') || source.ends_with(".json") {
            source
        } else {
            format!("plugins/{source}")
        };
        let strict = item.get("strict").and_then(|v| v.as_bool()).unwrap_or(false);
        let resolved = root.join(&source);
        let plugin_json_present = resolved.join(".codebuddy-plugin/plugin.json").is_file();
        let blocked_reason = strict_entry_block(strict, plugin_json_present);
        if blocked_reason.is_none() && !plugin_json_present {
            // `strict=false` row whose source has no plugin.json: the spec lets
            // the entry supplement/replace the manifest, which is the
            // unimplemented half of `17` §10 P5 — keep the historical skip
            // instead of inventing a manifest from the row's fields.
            continue;
        }
        entries.push(ScannedEntry {
            name: name.to_owned(),
            relative: source,
            description: item.get("description").and_then(|v| v.as_str()).map(str::to_owned),
            keywords: item
                .get("keywords")
                .and_then(|v| v.as_array())
                .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            category: item.get("category").and_then(|v| v.as_str()).map(str::to_owned),
            localized: collect_localized_variants(item),
            strict,
            blocked_reason,
        });
    }
    Ok(entries)
}

/// Read the market manifest `name` (marketplace.json / connectors.json /
/// plugin.json) for display; falls back to `None` so the caller keeps the
/// derived id.
fn read_manifest_name(root: &Path) -> Option<String> {
    for candidate in [
        ".codebuddy-plugin/marketplace.json",
        ".codebuddy-skill/marketplace.json",
        ".codebuddy-connector/connectors.json",
        ".codebuddy-plugin/plugin.json",
    ] {
        let path = root.join(candidate);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(name) = value.get("name").and_then(|value| value.as_str()) {
            if !name.is_empty() {
                return Some(name.to_owned());
            }
        }
    }
    None
}

/// Derive a stable kebab-case marketplace id from a name or source.
fn derive_marketplace_id(name: Option<&str>, source: &str) -> String {
    let hint = name
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            Path::new(source)
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "market".into())
        });
    let slug = sanitize_slug(&hint);
    if slug.is_empty() {
        "market".to_owned()
    } else {
        slug
    }
}

/// Q7 ① / `02` §8: official markets default to auto-update, third-party ones do
/// not (and V1 never auto-updates a third-party source).
///
/// "Official" is the product's own public-mirror set
/// (`AgentStoreConfig::builtin_default_marketplaces`). A source an operator
/// declares in `config.toml [default_marketplaces]` is deliberately treated as
/// third-party: the product does not own it, so it never auto-updates by
/// default.
pub(crate) fn is_official_source(source_kind: &str, source: &str) -> bool {
    nomifun_app_server::AgentStoreConfig::builtin_default_marketplaces()
        .iter()
        .any(|(_, kind, uri)| kind == source_kind && uri == source)
}

/// Composition-root Marketplace provider.
#[derive(Clone)]
pub struct AppServerMarketplaceProvider {
    importer: ImporterService,
    markets: Arc<dyn IMarketplaceRepository>,
    /// Cascade uninstall seam: clears install state + runtime artifacts.
    installs: Arc<dyn InstallProvider>,
    /// Root for remote-source materialization
    /// (`{work_dir}/agent-store-markets/<marketplace_id>/live`).
    pub market_root: std::path::PathBuf,
}

impl AppServerMarketplaceProvider {
    pub fn new(
        importer: ImporterService,
        markets: Arc<dyn IMarketplaceRepository>,
        installs: Arc<dyn InstallProvider>,
        market_root: std::path::PathBuf,
    ) -> Self {
        Self {
            importer,
            markets,
            installs,
            market_root,
        }
    }

    async fn verify_market_row(&self, marketplace_id: &str) -> Result<PluginMarketplaceRow, AppError> {
        let row = self
            .markets
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        if row.removed_at.is_some() {
            return Err(AppError::NotFound(format!(
                "marketplace {marketplace_id} was removed"
            )));
        }
        Ok(row)
    }
}

fn to_summary(row: &PluginMarketplaceRow) -> AppServerMarketplaceSummary {
    AppServerMarketplaceSummary {
        marketplace_id: row.marketplace_id.clone(),
        name: row.name.clone(),
        description: row.description.clone(),
        source_kind: row.source_kind.clone(),
        version: row.version.clone(),
        auto_update: row.auto_update == 1,
        enabled: row.enabled == 1,
        entry_count: row.entries().len(),
        added_at: row.added_at,
        resolved_revision: row.resolved_revision.clone(),
        last_checked_at: row.last_checked_at,
    }
}

fn to_entry(
    entry: &MarketplaceEntry,
    snapshot: Option<&AppServerMarketplaceEntrySnapshot>,
) -> AppServerMarketplaceEntry {
    AppServerMarketplaceEntry {
        name: entry.name.clone(),
        source_kind: entry.source_kind.clone(),
        source: entry.source_uri.clone(),
        version: entry.version.clone(),
        description: entry.description.clone(),
        keywords: entry.keywords.clone(),
        category: entry.category.clone(),
        localized: entry.localized.clone(),
        strict: entry.strict,
        blocked_reason: entry.blocked_reason.clone(),
        snapshot: snapshot.cloned(),
    }
}

#[async_trait]
impl MarketplaceProvider for AppServerMarketplaceProvider {
    async fn add(
        &self,
        request: AppServerMarketplaceAddRequest,
    ) -> Result<AppServerMarketplaceSummary, AppError> {
        let marketplace_id = derive_marketplace_id(request.name.as_deref(), &request.source);
        let source_kind = request.source_kind.as_str();

        // Duplicate-add probe: same active source must not be registered
        // twice (idempotent return). A previously *removed* row is reactivated
        // below instead of returned as-is, so re-adding a source revives it.
        // A same-id add with a *different* source is a re-source: the row is
        // reactivated (active or removed) with the current source, because the
        // id column is unique and the user explicitly asked for the new source.
        let reactivate = match self
            .markets
            .find_by_source(source_kind, &request.source)
            .await
            .map_err(AppError::from)?
        {
            Some(existing) if existing.removed_at.is_none() => {
                return Ok(to_summary(&existing));
            }
            Some(existing) => Some(existing.marketplace_id),
            None => {
                // Same marketplace_id under a different source (e.g. the
                // default sources were re-pointed in config.toml): reactivate
                // the existing row with the new source rather than inserting a
                // duplicate id.
                self.markets
                    .get_marketplace(&marketplace_id)
                    .await
                    .map_err(AppError::from)?
                    .map(|existing| existing.marketplace_id)
            }
        };
        let reactivating = reactivate.is_some();
        let reactivating_id = reactivate.as_deref();

        // Remote sources: fetch + validate + promote, then register.
        if source_kind != "directory" {
            let mut root = self.market_root.clone();
            root.push(&marketplace_id);
            let outcome = crate::market_fetch::fetch_remote(source_kind, &request.source, root.clone(), None, None)
                .await?;
            let fetched = match outcome {
                crate::market_fetch::RemoteFetchOutcome::Fresh(fetched) => fetched,
                crate::market_fetch::RemoteFetchOutcome::Unchanged { .. } => {
                    // Unreachable on add (no previous revision); treat as a
                    // failure so the registry is never half registered.
                    return Err(AppError::Internal(
                        "remote source answered 304 on first add".into(),
                    ));
                }
            };
            crate::market_fetch::register_remote(
                self.markets.clone(),
                &marketplace_id,
                &marketplace_id,
                source_kind,
                &request.source,
                &fetched,
            )
            .await?;
            let row = self
                .markets
                .get_marketplace(&marketplace_id)
                .await
                .map_err(AppError::from)?
                .expect("remote market registered");
            return Ok(to_summary(&row));
        }

        let root = PathBuf::from(&request.source);
        let (_kind, scanned) = probe_directory(&root)?;

        let entries: Vec<MarketplaceEntry> = scanned
            .into_iter()
            .map(|entry| MarketplaceEntry {
                name: entry.name,
                source_kind: "directory".into(),
                source_uri: entry.relative,
                version: None,
                description: entry.description,
                keywords: entry.keywords,
                category: entry.category,
                localized: entry.localized,
                strict: entry.strict,
                blocked_reason: entry.blocked_reason,
            })
            .collect();
        // Display name: marketplace.json `name` when declared, else directory
        // basename (the fixtures' manifests declare `name`).
        let declared_name = read_manifest_name(&root);
        let row = if reactivating {
            let digest = nomifun_importer::digest::tree_digest_of_dir(&root);
            self.markets
                .reactivate_marketplace(
                    reactivating_id.expect("reactivating id is set"),
                    "directory",
                    &request.source,
                    &entries,
                    &digest,
                    None,
                )
                .await
                .map_err(AppError::from)?;
            self.markets
                .get_marketplace(&marketplace_id)
                .await
                .map_err(AppError::from)?
                .expect("marketplace exists after reactivation")
        } else {
            self.markets
                .insert_marketplace(NewPluginMarketplace {
                    marketplace_id: &marketplace_id,
                    name: declared_name.as_deref().unwrap_or(&marketplace_id),
                    description: None,
                    source_kind: "directory",
                    source_uri: &request.source,
                    owner_json: None,
                    version: None,
                    content_digest: None,
                    entries,
                    auto_update: is_official_source(source_kind, &request.source),
                })
                .await
                .map_err(AppError::from)?
        };
        Ok(to_summary(&row))
    }

    async fn list(&self) -> Result<Vec<AppServerMarketplaceSummary>, AppError> {
        let rows = self.markets.list_marketplaces().await.map_err(AppError::from)?;
        Ok(rows.iter().map(to_summary).collect())
    }

    async fn get(&self, marketplace_id: &str) -> Result<AppServerMarketplaceDetail, AppError> {
        let row = self
            .markets
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        if row.removed_at.is_some() {
            return Err(AppError::NotFound(format!(
                "marketplace {marketplace_id} was removed"
            )));
        }
        // Provenance is projected here, not re-derived by the client from the
        // aggregate store listing (doc 16 D-W13-1 ①): the removal confirmation
        // needs "what would a cascade take out?" *before* it runs. Rows arrive
        // newest-first, so the first snapshot seen per entry is the current one.
        let provenance = self
            .markets
            .list_snapshot_provenance_by_marketplace(&row.marketplace_id)
            .await
            .map_err(AppError::from)?;
        let mut snapshots: HashMap<String, AppServerMarketplaceEntrySnapshot> = HashMap::new();
        for provenance_row in provenance {
            let Some(entry_name) = provenance_row.snapshot.entry_name.clone() else {
                continue;
            };
            snapshots.entry(entry_name).or_insert_with(|| {
                AppServerMarketplaceEntrySnapshot {
                    snapshot_id: provenance_row.snapshot.snapshot_id.clone(),
                    name: provenance_row.snapshot.name.clone(),
                    version: provenance_row.snapshot.version.clone(),
                    status: provenance_row.snapshot.status.clone(),
                    component_count: provenance_row.component_count.max(0) as usize,
                    installed_count: provenance_row.installed_count.max(0) as usize,
                    imported_at: provenance_row.snapshot.imported_at,
                }
            });
        }
        Ok(AppServerMarketplaceDetail {
            summary: to_summary(&row),
            entries: row
                .entries()
                .iter()
                .map(|entry| to_entry(entry, snapshots.get(&entry.name)))
                .collect(),
        })
    }

    async fn remove(
        &self,
        marketplace_id: &str,
        cascade: bool,
    ) -> Result<AppServerMarketplaceRemoveResult, AppError> {
        let row = self
            .markets
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        if row.removed_at.is_some() {
            return Err(AppError::NotFound(format!(
                "marketplace {marketplace_id} was already removed"
            )));
        }

        let mut result = AppServerMarketplaceRemoveResult {
            marketplace_id: marketplace_id.to_owned(),
            snapshots: vec![],
            uninstalled_components: vec![],
            warnings: vec![],
        };

        if cascade {
            let snapshots = self
                .markets
                .list_snapshots_by_marketplace(marketplace_id)
                .await
                .map_err(AppError::from)?;
            for snapshot in &snapshots {
                result.snapshots.push(snapshot.snapshot_id.clone());
                // Uninstall every installed/disabled component of the snapshot
                // (empty `component_ids` means "all" in the installer seam via
                // status projection, but the provider requires explicit ids).
                let installed_components: Vec<String> = match self
                    .installs
                    .status(&snapshot.snapshot_id)
                    .await
                {
                    Ok(status) => status
                        .components
                        .iter()
                        .filter(|component| {
                            component.state.as_str() == "installed"
                                || component.state.as_str() == "disabled"
                        })
                        .map(|component| component.id.clone())
                        .collect(),
                    Err(error) => {
                        result
                            .warnings
                            .push(format!("cascade status {}: {error}", snapshot.snapshot_id));
                        vec![]
                    }
                };
                if installed_components.is_empty() {
                    continue;
                }
                match self
                    .installs
                    .uninstall(&snapshot.snapshot_id, &installed_components)
                    .await
                {
                    Ok(_status) => {
                        // The components we asked to uninstall are the ones
                        // that lost their runtime registration.
                        result.uninstalled_components.extend(installed_components.clone());
                    }
                    Err(error) => result
                        .warnings
                        .push(format!("cascade uninstall {}: {error}", snapshot.snapshot_id)),
                }
            }
            for snapshot in &snapshots {
                self.markets
                    .clear_snapshot_provenance(&snapshot.snapshot_id)
                    .await
                    .map_err(AppError::from)?;
            }
        }

        self.markets
            .soft_remove_marketplace(marketplace_id, nomifun_common::now_ms())
            .await
            .map_err(AppError::from)?;
        result.uninstalled_components.dedup();
        Ok(result)
    }

    async fn set_auto_update(
        &self,
        marketplace_id: &str,
        enabled: bool,
    ) -> Result<AppServerMarketplaceSummary, AppError> {
        let _ = self
            .markets
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        self.markets
            .set_auto_update(marketplace_id, enabled)
            .await
            .map_err(AppError::from)?;
        let row = self
            .markets
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .expect("marketplace exists after toggle");
        Ok(to_summary(&row))
    }

    async fn refresh(
        &self,
        marketplace_id: &str,
    ) -> Result<AppServerMarketplaceRefreshResult, AppError> {
        let row = self.verify_market_row(marketplace_id).await?;
        let source_kind = row.source_kind.clone();
        if source_kind == "directory" {
            // Directory sources are live views; refresh re-probes in place.
            let root = PathBuf::from(&row.source_uri);
            let (_kind, scanned) = probe_directory(&root)?;
            let entries: Vec<MarketplaceEntry> = scanned
                .into_iter()
                .map(|entry| MarketplaceEntry {
                    name: entry.name,
                    source_kind: "directory".into(),
                    source_uri: entry.relative,
                    version: None,
                    description: entry.description,
                    keywords: entry.keywords,
                    category: entry.category,
                    localized: entry.localized,
                    strict: entry.strict,
                    blocked_reason: entry.blocked_reason,
                })
                .collect();
            let content_digest = nomifun_importer::digest::tree_digest_of_dir(&root);
            self.markets
                .update_marketplace_entries(marketplace_id, &entries, &content_digest, None)
                .await
                .map_err(AppError::from)?;
            return Ok(AppServerMarketplaceRefreshResult {
                marketplace_id: marketplace_id.to_owned(),
                changed: false,
                resolved_revision: "directory".into(),
                entry_count: entries.len(),
                warnings: vec![],
            });
        }

        // Remote: fetch into a fresh staging, compare revisions.
        let mut root = self.market_root.clone();
        root.push(marketplace_id);
        // D4 ①: offer the raw validators the source sent last time, so a
        // compliant server can answer 304 instead of shipping the manifest.
        let stored_validators = crate::market_source::HttpValidators::from_stored(
            row.source_etag.as_deref(),
            row.source_last_modified.as_deref(),
        );
        let outcome = crate::market_fetch::fetch_remote(
            &source_kind,
            &row.source_uri,
            root.clone(),
            row.resolved_revision.as_deref(),
            stored_validators.as_ref(),
        )
        .await?;
        let fetched = match outcome {
            crate::market_fetch::RemoteFetchOutcome::Unchanged { revision } => {
                let row = self
                    .markets
                    .get_marketplace(marketplace_id)
                    .await
                    .map_err(AppError::from)?
                    .expect("marketplace exists after unchanged refresh");
                return Ok(AppServerMarketplaceRefreshResult {
                    marketplace_id: marketplace_id.to_owned(),
                    changed: false,
                    resolved_revision: revision,
                    entry_count: row.entries().len(),
                    warnings: vec!["remote revision unchanged (freshness short-circuit)".into()],
                });
            }
            crate::market_fetch::RemoteFetchOutcome::Fresh(fetched) => fetched,
        };
        let entries: Vec<MarketplaceEntry> = fetched.entries.iter().map(|(entry, _)| entry.clone()).collect();
        self.markets
            .update_marketplace_entries(marketplace_id, &entries, &fetched.revision, None)
            .await
            .map_err(AppError::from)?;
        self.markets
            .record_resolved_revision(
                marketplace_id,
                &fetched.revision,
                fetched.live_root.to_str().unwrap_or(""),
            )
            .await
            .map_err(AppError::from)?;
        // D4 ①: remember the validators for the next conditional request. A URL
        // source always yields `Some`, so a server that stopped sending an ETag
        // clears the stale one instead of leaving it to be offered forever.
        if let Some(validators) = fetched.validators.as_ref() {
            self.markets
                .record_source_validators(
                    marketplace_id,
                    validators.etag.as_deref(),
                    validators.last_modified.as_deref(),
                )
                .await
                .map_err(AppError::from)?;
        }
        Ok(AppServerMarketplaceRefreshResult {
            marketplace_id: marketplace_id.to_owned(),
            changed: true,
            resolved_revision: fetched.revision,
            entry_count: entries.len(),
            warnings: vec![],
        })
    }

    async fn import_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerImportResult, AppError> {
        let row = self
            .markets
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        if row.removed_at.is_some() {
            return Err(AppError::NotFound(format!(
                "marketplace {marketplace_id} was removed"
            )));
        }
        let entry = row
            .entries()
            .into_iter()
            .find(|entry| entry.name == entry_name)
            .ok_or_else(|| AppError::NotFound(format!("entry {entry_name} not found")))?;

        if entry.source_kind == "external" {
            return Err(AppError::BadRequest(format!(
                "entry {entry_name} declares an external source ({}); URL marketplaces \
                 mirror only inlined entries — add the source as its own marketplace",
                entry.source_uri
            )));
        }

        // Resolve the entry source: directory sources read the source dir
        // directly; remote sources read inside the promoted live root.
        let source = crate::market_fetch::entry_source_path(
            &row.source_kind,
            &row.source_uri,
            &self.market_root,
            marketplace_id,
            &entry.source_uri,
        );

        // `strict=true` refuses an entry whose source carries no plugin.json of
        // its own (`02` §8 / §11.1). Re-evaluated against the real tree instead
        // of trusting the stored projection: a directory market is a live view,
        // so the file may have appeared since discovery. Blocked *before* the
        // importer runs, so no snapshot row is produced — the same contract as
        // the importer's own blocking path.
        //
        // Deliberately **before** the kind derivation below: the entry declares
        // itself a plugin (`strict` only ever arrives `true` from a
        // `.codebuddy-plugin/marketplace.json` row), so a source without a
        // manifest must be refused rather than quietly reinterpreted as
        // whatever its directory shape suggests (often a skill).
        if let Some(reason) =
            strict_entry_block(entry.strict, source.join(".codebuddy-plugin/plugin.json").is_file())
        {
            return Ok(nomifun_importer::blocked_import_result(
                SourceKind::CodeBuddyPlugin,
                Some((
                    entry.name.clone(),
                    entry.version.clone().unwrap_or_else(|| "0".into()),
                )),
                &[reason],
            ));
        }

        // Entry-level kind derivation: a skill sub-directory without its own
        // marketplace.json parses as a single-skill market; a connector
        // sub-directory parses as a CLI connector; plugin sub-directories are
        // plain plugins.
        let source_kind = if source.join("cli.json").is_file() {
            SourceKind::WorkBuddyCliConnector
        } else if source.join(".codebuddy-skill/marketplace.json").is_file() {
            SourceKind::WorkBuddySkillMarket
        } else if source.join(".codebuddy-plugin/plugin.json").is_file() {
            // Skill plugins carry `SKILL.md` and declare no agents; importing
            // them as CodeBuddy plugins would scan empty `agents`/`skills`
            // roots and yield zero components. Treat those as single-skill
            // directories instead.
            let manifest = nomifun_importer::read_plugin_display(&source);
            let has_agents = manifest
                .as_ref()
                .is_some_and(|m| !m.agents.is_empty() || m.team_info.is_some());
            if !has_agents && source.join("SKILL.md").is_file() {
                SourceKind::WorkBuddySkillMarket
            } else {
                SourceKind::CodeBuddyPlugin
            }
        } else if source.join("SKILL.md").is_file() {
            // A root SKILL.md is the strongest skill signal: skill-market
            // entries may carry an auxiliary mcp.json next to it (e.g. 腾讯云知),
            // so the skill wins over the MCP marker.
            SourceKind::WorkBuddySkillMarket
        } else if source.join("mcp.json").is_file() {
            SourceKind::WorkBuddyMcpConnector
        } else {
            row_source_kind(&row)
        };

        // Remote imports carry the resolved revision for provenance.
        let revision = row.resolved_revision.clone().unwrap_or_else(|| "".into());
        let request = if revision.is_empty() {
            nomifun_importer::ImportRequest::from_marketplace(
                source.clone(),
                source_kind,
                marketplace_id.to_owned(),
                entry_name.to_owned(),
            )
        } else {
            nomifun_importer::ImportRequest::from_marketplace_revision(
                source.clone(),
                source_kind,
                marketplace_id.to_owned(),
                entry_name.to_owned(),
                revision,
            )
        };
        self.importer
            .run_import(&request)
            .await
            .map_err(|error| match error {
                nomifun_importer::ImportError::SourceNotFound => {
                    AppError::NotFound(format!("entry source not found: {}", source.display()))
                }
                nomifun_importer::ImportError::Internal(message) => AppError::Internal(message),
            })
    }

    /// Resolve the on-disk root of one entry (trusted host only; used by the
    /// store asset endpoint for un-imported entries' display assets).
    async fn entry_dir(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<std::path::PathBuf, AppError> {
        let row = self.verify_market_row(marketplace_id).await?;
        let entry = row
            .entries()
            .into_iter()
            .find(|entry| entry.name == entry_name)
            .ok_or_else(|| AppError::NotFound(format!("entry {entry_name} not found")))?;
        Ok(crate::market_fetch::entry_source_path(
            &row.source_kind,
            &row.source_uri,
            &self.market_root,
            marketplace_id,
            &entry.source_uri,
        ))
    }

    /// Resolve the on-disk root of a marketplace (internal path, trusted host
    /// only) — e.g. `icons/` assets that live outside any single entry dir.
    async fn market_dir(
        &self,
        marketplace_id: &str,
    ) -> Result<std::path::PathBuf, AppError> {
        let row = self.verify_market_row(marketplace_id).await?;
        Ok(crate::market_fetch::entry_source_path(
            &row.source_kind,
            &row.source_uri,
            &self.market_root,
            marketplace_id,
            ".",
        ))
    }

    /// Host-only seam for the background auto-update sweep (doc 21 D7 ①):
    /// marketplace ids whose toggle is on **and** whose source is one of the
    /// builtin official mirrors.
    ///
    /// Like `entry_dir` / `market_dir`, this is not a wire method — eligibility
    /// has to look at the source URI, which never crosses the public protocol
    /// (`18` §7). A third-party source keeps its `auto_update` flag for
    /// display, but V1 never polls it.
    async fn auto_update_targets(&self) -> Result<Vec<String>, AppError> {
        let rows = self
            .markets
            .list_marketplaces()
            .await
            .map_err(AppError::from)?;
        Ok(rows
            .into_iter()
            .filter(is_auto_update_eligible)
            .map(|row| row.marketplace_id)
            .collect())
    }
}

/// The auto-update sweep predicate: still registered, toggle on, **and** the
/// source is one of the builtin official mirrors. Split out from the query so
/// the rule itself is testable without a database (doc 21 D7 ①).
fn is_auto_update_eligible(row: &PluginMarketplaceRow) -> bool {
    row.removed_at.is_none()
        && row.auto_update == 1
        && is_official_source(row.source_kind.as_str(), row.source_uri.as_str())
}

fn row_source_kind(row: &PluginMarketplaceRow) -> SourceKind {
    match row.source_kind.as_str() {
        "workbuddy-connector-market" => SourceKind::WorkBuddyConnectorMarket,
        "workbuddy-skill-market" => SourceKind::WorkBuddySkillMarket,
        "workbuddy-cli-connector" => SourceKind::WorkBuddyCliConnector,
        "workbuddy-mcp-connector" => SourceKind::WorkBuddyMcpConnector,
        _ => SourceKind::CodeBuddyPlugin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Q7 ①: only the product's own public-mirror sources count as official;
    /// every other source stays third-party (no auto-update in V1).
    #[test]
    fn official_sources_are_the_builtin_public_mirror() {
        for (_, kind, source) in nomifun_app_server::AgentStoreConfig::builtin_default_marketplaces() {
            assert!(is_official_source(&kind, &source), "{source} must be official");
        }
        assert!(
            !is_official_source("url", "https://example.test/.codebuddy-plugin/marketplace.json"),
            "a third-party URL source must not be official"
        );
        assert!(
            !is_official_source("directory", "/tmp/company-tools"),
            "a local directory source must not be official"
        );
    }

    fn marketplace_row(source_kind: &str, source_uri: &str, auto_update: i64) -> PluginMarketplaceRow {
        PluginMarketplaceRow {
            id: 1,
            marketplace_id: "market".into(),
            name: "market".into(),
            description: None,
            source_kind: source_kind.into(),
            source_uri: source_uri.into(),
            owner_json: None,
            version: None,
            content_digest: None,
            entries_json: "[]".into(),
            auto_update,
            enabled: 1,
            last_checked_at: None,
            resolved_revision: None,
            source_etag: None,
            source_last_modified: None,
            staging_root: None,
            added_at: 0,
            updated_at: 0,
            removed_at: None,
        }
    }

    /// D7 ①: the sweep covers official mirrors only. A third-party source with
    /// the toggle flipped on must still be skipped, and a removed row never
    /// comes back through this path.
    #[test]
    fn auto_update_sweep_covers_official_mirrors_only() {
        let (_, official_kind, official_source) =
            nomifun_app_server::AgentStoreConfig::builtin_default_marketplaces()
                .into_iter()
                .next()
                .expect("builtin mirrors are configured");

        assert!(is_auto_update_eligible(&marketplace_row(
            &official_kind,
            &official_source,
            1
        )));

        // Official but toggled off → not swept.
        assert!(!is_auto_update_eligible(&marketplace_row(
            &official_kind,
            &official_source,
            0
        )));

        // Third-party with the flag on → still not swept.
        assert!(!is_auto_update_eligible(&marketplace_row(
            "url",
            "https://example.test/.codebuddy-plugin/marketplace.json",
            1
        )));

        // A removed marketplace is never swept, even when it looks official.
        let mut removed = marketplace_row(&official_kind, &official_source, 1);
        removed.removed_at = Some(1);
        assert!(!is_auto_update_eligible(&removed));
    }

    #[test]
    fn probe_connector_market_derives_entries_from_index() {
        let dir = std::env::temp_dir().join(format!("as-mkt-conn-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-connector/connectors.json"),
            r#"{
                "name": "company-connectors",
                "connectors": [
                    { "id": "wecom", "name": "WeCom", "description": "WeCom CLI" },
                    { "id": "feishu", "name": "Feishu", "description": "Feishu CLI" }
                ]
            }"#,
        );
        write(&dir.join("connectors/wecom/cli.json"), "{}");
        write(&dir.join("connectors/feishu/cli.json"), "{}");
        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::Connectors);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "wecom");
        assert_eq!(entries[0].relative, "connectors/wecom");
        assert_eq!(entries[1].description.as_deref(), Some("Feishu CLI"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_skill_market_derives_entries() {
        let dir = std::env::temp_dir().join(format!("as-mkt-skill-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-skill/marketplace.json"),
            r#"{
                "name": "company-skills",
                "skills": [
                    { "name": "code-review", "source": "./skills/code-review", "description": "Reviews code" },
                    { "name": "commit-message", "source": "./skills/commit-message" }
                ]
            }"#,
        );
        write(&dir.join("skills/code-review/SKILL.md"), "# code-review\n");
        write(&dir.join("skills/commit-message/SKILL.md"), "# commit-message\n");
        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::Skills);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "code-review");
        assert_eq!(entries[0].relative, "skills/code-review");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_skill_market_handles_source_without_skills_prefix() {
        // Real workbuddy skills-marketplace layout: manifest `source` is the
        // bare directory name and entries live under `skills/<source>/`.
        let dir = std::env::temp_dir().join(format!("as-mkt-skill2-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-skill/marketplace.json"),
            r#"{
                "name": "skills-marketplace",
                "skills": [
                    { "name": "腾讯文档", "source": "tencent-docs", "description": "docs" },
                    { "name": "腾讯会议", "source": "tencent-meeting-skill", "description": "meeting" }
                ]
            }"#,
        );
        write(&dir.join("skills/tencent-docs/SKILL.md"), "# docs\n");
        write(&dir.join("skills/tencent-meeting-skill/SKILL.md"), "# meeting\n");
        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::Skills);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "腾讯文档");
        assert_eq!(entries[0].relative, "skills/tencent-docs");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_plugin_collection_derives_subdirectories() {
        let dir = std::env::temp_dir().join(format!("as-mkt-plugin-{}", nomifun_common::generate_id()));
        write(&dir.join("formatter/.codebuddy-plugin/plugin.json"), r#"{ "name": "formatter" }"#);
        write(&dir.join("deploy/.codebuddy-plugin/plugin.json"), r#"{ "name": "deploy" }"#);
        write(&dir.join("README.md"), "not a plugin");
        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::PluginCollection);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.name == "formatter"));
        assert!(entries.iter().any(|entry| entry.name == "deploy"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_plugin_market_derives_entries_from_plugins_array() {
        // Real WorkBuddy expert-market layout:
        //   market/.codebuddy-plugin/marketplace.json { plugins: [{name, source, description}] }
        //   market/plugins/<id>/.codebuddy-plugin/plugin.json
        let dir = std::env::temp_dir().join(format!("as-mkt-pm-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-plugin/marketplace.json"),
            r#"{
                "name": "experts",
                "plugins": [
                    { "name": "fbsir-super-partner", "source": "./plugins/fbsir-super-partner", "description": "Super partner" },
                    { "name": "software-company", "source": "./plugins/software-company", "description": "Software company" }
                ]
            }"#,
        );
        write(
            &dir.join("plugins/fbsir-super-partner/.codebuddy-plugin/plugin.json"),
            r#"{ "name": "fbsir-super-partner", "displayName": "FBSir", "profession": "Super Partner", "agents": ["./agents"] }"#,
        );
        write(
            &dir.join("plugins/software-company/.codebuddy-plugin/plugin.json"),
            r#"{ "name": "software-company" }"#,
        );
        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::PluginMarket);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "fbsir-super-partner");
        assert_eq!(entries[0].relative, "plugins/fbsir-super-partner");
        assert_eq!(entries[0].description.as_deref(), Some("Super partner"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn derive_marketplace_id_uses_name_then_source() {
        assert_eq!(derive_marketplace_id(Some("Company Tools"), "/tmp/x"), "Company-Tools");
        assert_eq!(derive_marketplace_id(None, "/tmp/my-plugins"), "my-plugins");
        assert_eq!(derive_marketplace_id(None, "C:\\work\\市场 目录"), "market");
    }

    #[test]
    fn non_directory_source_is_rejected() {
        let dir = std::env::temp_dir().join(format!("as-mkt-dir-{}", nomifun_common::generate_id()));
        write(&dir.join(".codebuddy-plugin/plugin.json"), r#"{ "name": "demo" }"#);
        let (kind, _) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::PluginRoot);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R28 / D8=A: the market manifest's `<field>_<lang>` variants are carried
    /// verbatim out of the probe, so the client can fall back by its own UI
    /// language. `description` stays the baseline field.
    #[test]
    fn skill_market_probe_carries_localized_variants() {
        let dir = std::env::temp_dir().join(format!("as-mkt-l10n-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-skill/marketplace.json"),
            r#"{
                "name": "skills",
                "skills": [
                    {
                        "name": "pdf-toolkit",
                        "source": "pdf-toolkit",
                        "description": "PDF helpers",
                        "description_zh": "PDF 工具集",
                        "description_en": "PDF toolkit",
                        "tags_zh": ["文档"],
                        "tags_en": ["documents"],
                        "legacy_tags_zh": ["旧文档"],
                        "examples_zh": ["合并 PDF"],
                        "featured": 3
                    }
                ]
            }"#,
        );
        write(&dir.join("skills/pdf-toolkit/SKILL.md"), "# pdf-toolkit");
        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::Skills);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.description.as_deref(), Some("PDF helpers"));
        assert_eq!(
            entry.localized.get("description_zh"),
            Some(&LocalizedVariant::Text("PDF 工具集".into()))
        );
        assert_eq!(
            entry.localized.get("tags_en"),
            Some(&LocalizedVariant::List(vec!["documents".into()]))
        );
        assert_eq!(
            entry.localized.get("legacy_tags_zh"),
            Some(&LocalizedVariant::List(vec!["旧文档".into()]))
        );
        assert_eq!(
            entry.localized.get("examples_zh"),
            Some(&LocalizedVariant::List(vec!["合并 PDF".into()]))
        );
        // Non-variant fields never leak into the projection (`featured` is a
        // number, and the baseline `description`/`tags` are not `_zh`/`_en`).
        assert_eq!(entry.localized.len(), 6);
        assert!(!entry.localized.contains_key("description"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R28: the entry projection onto the wire keeps the variants (additive —
    /// absent when the manifest declared none).
    #[test]
    fn entry_projection_carries_localized_variants() {
        let entry = MarketplaceEntry {
            name: "pdf-toolkit".into(),
            source_kind: "directory".into(),
            source_uri: "skills/pdf-toolkit".into(),
            version: None,
            description: Some("PDF helpers".into()),
            keywords: vec![],
            category: None,
            localized: BTreeMap::from([(
                "description_zh".to_owned(),
                LocalizedVariant::Text("PDF 工具集".to_owned()),
            )]),
            strict: false,
            blocked_reason: None,
        };
        let projected = to_entry(&entry, None);
        assert_eq!(
            projected.localized.get("description_zh"),
            Some(&LocalizedVariant::Text("PDF 工具集".into()))
        );
        let wire = serde_json::to_value(&projected).expect("serialize");
        assert_eq!(wire["localized"]["description_zh"], serde_json::json!("PDF 工具集"));

        // No variants -> the key is omitted entirely (purely additive).
        let bare = MarketplaceEntry { localized: BTreeMap::new(), ..entry };
        let wire = serde_json::to_value(to_entry(&bare, None)).expect("serialize");
        assert!(wire.get("localized").is_none(), "empty variants must not reach the wire");
    }

    // -- `strict` / `blocked_reason` (R24, `02` §8 / §11.1) -------------------

    /// The rule only ever fires for `strict=true` + a missing `plugin.json`
    /// (`17` §7). Everything else must stay silently importable — in particular
    /// `strict=false` without a manifest is **not** an error: the spec lets the
    /// entry supply the manifest (the unimplemented half, `17` §10 P5).
    #[test]
    fn strict_entry_block_fires_only_for_strict_without_a_manifest() {
        assert!(strict_entry_block(true, false).is_some(), "strict + no plugin.json blocks");
        assert!(strict_entry_block(true, true).is_none(), "strict + plugin.json is fine");
        assert!(strict_entry_block(false, false).is_none(), "non-strict is never blocked");
        assert!(strict_entry_block(false, true).is_none());

        // The reason must not leak a filesystem path (`02` §9).
        let reason = strict_entry_block(true, false).expect("reason");
        assert!(reason.contains("strict=true"), "{reason}");
        assert!(reason.contains("plugin.json"), "{reason}");
    }

    /// A default entry keeps the wire unchanged: both keys stay absent
    /// (purely additive), and a blocked one carries what the UI needs.
    #[test]
    fn entry_projection_omits_strict_fields_unless_declared() {
        let plain = MarketplaceEntry {
            name: "pdf-toolkit".into(),
            source_kind: "directory".into(),
            source_uri: "plugins/pdf-toolkit".into(),
            version: None,
            description: None,
            keywords: vec![],
            category: None,
            localized: BTreeMap::new(),
            strict: false,
            blocked_reason: None,
        };
        let wire = serde_json::to_value(to_entry(&plain, None)).expect("serialize");
        assert!(wire.get("strict").is_none(), "strict=false must not reach the wire");
        assert!(wire.get("blocked_reason").is_none(), "no reason, no key");

        let blocked = MarketplaceEntry {
            strict: true,
            blocked_reason: strict_entry_block(true, false),
            ..plain
        };
        let wire = serde_json::to_value(to_entry(&blocked, None)).expect("serialize");
        assert_eq!(wire["strict"], serde_json::json!(true));
        assert!(
            wire["blocked_reason"].as_str().is_some_and(|text| !text.is_empty()),
            "{wire}"
        );
    }

    /// Discovery keeps a `strict=true` row whose source has no `plugin.json`
    /// instead of dropping it (§11.1 blocking is visible, not a silent skip),
    /// and a non-strict row keeps the historical skip.
    #[test]
    fn probe_plugin_market_keeps_blocked_strict_rows_and_skips_the_rest() {
        let dir = std::env::temp_dir().join(format!("as-mkt-strict-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-plugin/marketplace.json"),
            r#"{
                "name": "expert-market",
                "plugins": [
                    { "name": "fine", "source": "./plugins/fine", "strict": true },
                    { "name": "blocked", "source": "./plugins/blocked", "strict": true },
                    { "name": "skipped", "source": "./plugins/skipped" }
                ]
            }"#,
        );
        // Only `fine` ships a plugin.json of its own; `blocked` declares
        // `strict=true` without one; `skipped` declares nothing.
        write(&dir.join("plugins/fine/.codebuddy-plugin/plugin.json"), r#"{ "name": "fine" }"#);

        let (kind, entries) = probe_directory(&dir).unwrap();
        assert_eq!(kind, MarketKind::PluginMarket);

        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["fine", "blocked"], "the non-strict row stays skipped");

        let fine = entries.iter().find(|entry| entry.name == "fine").unwrap();
        assert!(fine.strict, "the declared flag is carried out of the probe");
        assert!(fine.blocked_reason.is_none());

        let blocked = entries.iter().find(|entry| entry.name == "blocked").unwrap();
        assert!(blocked.strict);
        assert!(
            blocked.blocked_reason.as_deref().is_some_and(|reason| !reason.is_empty()),
            "a kept-but-refused row must say why"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
