//! App Server Store adapter for the composition root (`nomifun-app`).
//!
//! The store is the winget-style unified catalog: it aggregates *all* entries
//! of *all* enabled marketplaces into one flat item list (experts / teams /
//! skills / connectors), carrying plugin.json display fidelity plus the local
//! install state, and exposes a single idempotent "install" action that runs
//! import + runtime registration behind one call.
//!
//! The adapter never executes content and never exposes absolute source paths;
//! only public projections cross the protocol seam.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerLocalizedText, AppServerStoreInstallResult, AppServerStoreItem, AppServerStoreList,
};
use nomifun_app_server::{InstallProvider, MarketplaceProvider, StoreProvider};
use nomifun_common::AppError;
use nomifun_db::{IMarketplaceRepository, IPluginSnapshotRepository, PluginMarketplaceRow};
use nomifun_importer::{ImporterService, LocalizedText, PluginManifest};

/// Composition-root Store provider: marketplace catalog + provenance install
/// state + one-click install.
#[derive(Clone)]
pub struct AppServerStoreProvider {
    markets: Arc<dyn MarketplaceProvider>,
    market_rows: Arc<dyn IMarketplaceRepository>,
    snapshots: Arc<dyn IPluginSnapshotRepository>,
    importer: ImporterService,
    installs: Arc<dyn InstallProvider>,
    /// Remote materialization root (`{work_dir}/agent-store-markets`),
    /// shared with the marketplace provider so relative entry sources resolve.
    market_root: PathBuf,
}

impl AppServerStoreProvider {
    pub fn new(
        markets: Arc<dyn MarketplaceProvider>,
        market_rows: Arc<dyn IMarketplaceRepository>,
        snapshots: Arc<dyn IPluginSnapshotRepository>,
        importer: ImporterService,
        installs: Arc<dyn InstallProvider>,
        market_root: PathBuf,
    ) -> Self {
        Self { markets, market_rows, snapshots, importer, installs, market_root }
    }
}

fn to_localized(text: &LocalizedText) -> AppServerLocalizedText {
    AppServerLocalizedText { en: text.en.clone(), zh: text.zh.clone() }
}

fn localized_or_none(text: &LocalizedText) -> Option<AppServerLocalizedText> {
    if text.is_empty() {
        None
    } else {
        Some(to_localized(text))
    }
}

fn desc_from_display(display: &PluginManifest) -> Option<String> {
    display
        .display_description
        .as_ref()
        .and_then(|text| text.primary().map(str::to_owned))
        .or_else(|| display.description.clone())
}

/// Display metadata for one connector-market entry (from the market's
/// `.codebuddy-connector/connectors.json` index). Used when the entry
/// directory ships no plugin.json display block.
#[derive(Debug, Clone, Default)]
struct ConnectorIndexInfo {
    name_zh: Option<String>,
    name_en: Option<String>,
    version: Option<String>,
    description: Option<String>,
}

/// Load the connector-market index (`id` → display info) from the market
/// root's `.codebuddy-connector/connectors.json` (best-effort; a missing or
/// malformed index yields an empty map).
fn read_connector_index(root: &Path) -> HashMap<String, ConnectorIndexInfo> {
    let text = match std::fs::read_to_string(root.join(".codebuddy-connector/connectors.json")) {
        Ok(text) => text,
        Err(_) => return HashMap::new(),
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return HashMap::new(),
    };
    let Some(items) = value.get("connectors").and_then(|value| value.as_array()) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for item in items {
        let Some(id) = item.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        map.insert(
            id.to_owned(),
            ConnectorIndexInfo {
                name_zh: item.get("name_zh").and_then(|v| v.as_str()).map(str::to_owned),
                name_en: item
                    .get("name_en")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("name").and_then(|v| v.as_str()))
                    .map(str::to_owned),
                version: item.get("version").and_then(|v| v.as_str()).map(str::to_owned),
                description: item
                    .get("description_zh")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("description").and_then(|v| v.as_str()))
                    .map(str::to_owned),
            },
        );
    }
    map
}

/// The market root directory for index lookups (best-effort; for remote
/// markets this is the mirrored live root, for directory sources the source).
fn market_root_dir(market: &PluginMarketplaceRow, market_root: &Path) -> Option<PathBuf> {
    if market.source_kind == "directory" {
        Some(PathBuf::from(&market.source_uri))
    } else {
        Some(crate::market_fetch::live_root_for(market_root, &market.marketplace_id))
    }
}

/// Icon extensions the store asset endpoint serves, in probe order.
const ICON_EXTS: [&str; 6] = ["png", "svg", "jpg", "jpeg", "webp", "gif"];

/// Resolve the market-level icon for an entry (`icons/<base>.<ext>` in the
/// market root, where `<base>` is the entry source basename — e.g.
/// `tencent-docs.svg`, `agent-earth.png`). Skills and connectors declare no
/// plugin.json avatar; the market ships an `icons/` directory instead.
/// Returns the icon file name (relative to the market root) when present.
fn market_icon_for(root: &Path, source_uri: &str) -> Option<String> {
    let base = Path::new(source_uri)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())?;
    for ext in ICON_EXTS {
        let icon = root.join("icons").join(format!("{base}.{ext}"));
        if icon.is_file() {
            return Some(format!("icons/{base}.{ext}"));
        }
    }
    None
}

#[async_trait]
impl StoreProvider for AppServerStoreProvider {
    async fn list(&self) -> Result<AppServerStoreList, AppError> {
        let market_rows = self
            .market_rows
            .list_marketplaces()
            .await
            .map_err(AppError::from)?;
        let mut items = Vec::new();
        for market in market_rows {
            if market.enabled != 1 {
                continue;
            }
            let market_root = market_root_dir(&market, &self.market_root);
            let index = market_root
                .as_ref()
                .map(|root| read_connector_index(root))
                .unwrap_or_default();
            for entry in market.entries() {
                // Resolve the entry payload directory (internal).
                let source = crate::market_fetch::entry_source_path(
                    &market.source_kind,
                    &market.source_uri,
                    &self.market_root,
                    &market.marketplace_id,
                    &entry.source_uri,
                );
                let manifest = nomifun_importer::read_plugin_display(&source);

                // `02` §8: a `strict=true` entry must ship its own
                // `.codebuddy-plugin/plugin.json`. Checked against the live tree
                // with the same predicate the import gate uses, so a listed item
                // and an actual install can never disagree about whether it is
                // installable.
                let blocked_reason = crate::app_server_marketplace::strict_entry_block(
                    entry.strict,
                    source.join(".codebuddy-plugin/plugin.json").is_file(),
                );

                let kind = derive_kind(&source, &entry.source_kind, &manifest);
                // Connector markets declare display metadata in their
                // `connectors.json` index (id → name_zh/name_en/version);
                // fall back to it when no plugin.json display block exists.
                let index_info = if kind == "connector" {
                    index.get(&entry.name)
                } else {
                    None
                };
                let display_name = manifest
                    .as_ref()
                    .and_then(|m| m.display_name.as_ref())
                    .and_then(localized_or_none)
                    .or_else(|| {
                        // Fall back to the entry's own display metadata when
                        // no plugin.json display fields exist.
                        None
                    });
                let name = display_name
                    .as_ref()
                    .and_then(|text| text.zh.as_ref().or(text.en.as_ref()))
                    .cloned()
                    .or_else(|| index_info.and_then(|info| info.name_zh.clone()))
                    .or_else(|| index_info.and_then(|info| info.name_en.clone()))
                    .unwrap_or_else(|| entry.name.clone());
                let version = manifest
                    .as_ref()
                    .map(|m| m.version())
                    .map(str::to_owned)
                    .or_else(|| index_info.and_then(|info| info.version.clone()))
                    .unwrap_or_else(|| entry.version.clone().unwrap_or_else(|| "1.0.0".into()));
                let avatar = manifest
                    .as_ref()
                    .and_then(|m| m.avatar.as_ref())
                    .cloned()
                    .or_else(|| {
                        // Skills / connectors carry no plugin.json avatar;
                        // the market ships `icons/<source-basename>.<ext>`.
                        market_root
                            .as_ref()
                            .and_then(|root| market_icon_for(root, &entry.source_uri))
                    });

                // Installed state via provenance.
                let snapshot = self
                    .snapshots
                    .find_snapshot_by_provenance(&market.marketplace_id, &entry.name)
                    .await
                    .map_err(AppError::from)?;
                let (installed, installed_version, snapshot_id, update_available) =
                    match snapshot {
                        Some(row) => {
                            let components = self
                                .snapshots
                                .get_components(&row.snapshot_id)
                                .await
                                .map_err(AppError::from)?;
                            let installed_any = components
                                .iter()
                                .any(|component| component.installed == 1);
                            (
                                installed_any,
                                Some(row.version.clone()),
                                Some(row.snapshot_id.clone()),
                                row.version != version,
                            )
                        }
                        None => (false, None, None, false),
                    };

                items.push(AppServerStoreItem {
                    id: format!("{}/{}", market.marketplace_id, entry.name),
                    marketplace_id: market.marketplace_id.clone(),
                    marketplace_name: market.name.clone(),
                    entry_name: entry.name.clone(),
                    kind: kind.to_owned(),
                    name,
                    display_name,
                    profession: manifest
                        .as_ref()
                        .and_then(|m| m.profession.as_ref())
                        .and_then(localized_or_none),
                    description: manifest
                        .as_ref()
                        .and_then(desc_from_display)
                        .or_else(|| entry.description.clone()),
                    display_description: manifest
                        .as_ref()
                        .and_then(|m| m.display_description.as_ref())
                        .and_then(localized_or_none),
                    tags: manifest
                        .as_ref()
                        .map(|m| m.tags.iter().filter_map(|t| localized_or_none(t)).collect())
                        .unwrap_or_default(),
                    quick_prompts: manifest
                        .as_ref()
                        .map(|m| {
                            m.quick_prompts.iter().filter_map(|p| localized_or_none(p)).collect()
                        })
                        .unwrap_or_default(),
                    avatar_url: avatar.map(|path| {
                        format!(
                            "/api/app-server/store/{}/entries/{}/assets/{}",
                            market.marketplace_id,
                            entry.name,
                            path.trim_start_matches('/')
                        )
                    }),
                    version,
                    source_kind: entry.source_kind.clone(),
                    installed,
                    update_available,
                    snapshot_id,
                    installed_version,
                    blocked_reason,
                });
            }
        }
        Ok(AppServerStoreList {
            items,
            // D-SDK-1 ①: the builtin default marketplaces register in the
            // background, so a cold `store/list` may be legitimately partial.
            markets_pending: nomifun_app_server::marketplaces_warming(),
        })
    }

    async fn install_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerStoreInstallResult, AppError> {
        // State probe first: an already installed entry is a no-op.
        let row = self
            .market_rows
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        let _entry = row
            .entries()
            .into_iter()
            .find(|entry| entry.name == entry_name)
            .ok_or_else(|| AppError::NotFound(format!("entry {entry_name} not found")))?;

        let existing = self
            .snapshots
            .find_snapshot_by_provenance(marketplace_id, entry_name)
            .await
            .map_err(AppError::from)?;

        let snapshot_id = match existing {
            Some(row) => {
                let installed = {
                    let components = self
                        .snapshots
                        .get_components(&row.snapshot_id)
                        .await
                        .map_err(AppError::from)?;
                    components.iter().any(|component| component.installed == 1)
                };
                if installed {
                    return Ok(AppServerStoreInstallResult {
                        marketplace_id: marketplace_id.to_owned(),
                        entry_name: entry_name.to_owned(),
                        snapshot_id: row.snapshot_id.clone(),
                        version: row.version.clone(),
                        reused: true,
                        installed_count: 0,
                        warnings: vec!["entry already installed".into()],
                        errors: vec![],
                    });
                }
                row.snapshot_id
            }
            None => {
                // Import through the marketplace pipeline (provenance set).
                let result = self
                    .markets
                    .import_entry(marketplace_id, entry_name)
                    .await?;
                if result.status == "blocked" {
                    // `02` §11.1: the snapshot was refused (e.g. a `strict=true`
                    // entry with no plugin.json of its own). Nothing was
                    // persisted, so `snapshot_id` names no row — registering it
                    // would install a phantom. Report the refusal instead.
                    return Ok(AppServerStoreInstallResult {
                        marketplace_id: marketplace_id.to_owned(),
                        entry_name: entry_name.to_owned(),
                        snapshot_id: result.snapshot_id,
                        version: result.version,
                        reused: false,
                        installed_count: 0,
                        warnings: result.warnings,
                        errors: result.errors,
                    });
                }
                result.snapshot_id
            }
        };

        // Register components into the runtime (idempotent; components already
        // installed stay).
        let install_result = self
            .installs
            .install(nomifun_api_types::AppServerInstallRequest {
                snapshot_id: snapshot_id.clone(),
            })
            .await?;
        Ok(AppServerStoreInstallResult {
            marketplace_id: marketplace_id.to_owned(),
            entry_name: entry_name.to_owned(),
            snapshot_id,
            version: install_result.version,
            reused: false,
            installed_count: install_result.installed_count,
            warnings: install_result.warnings,
            errors: install_result.errors,
        })
    }
}

/// Derive the item kind from the entry directory shape (plugin.json agent /
/// teamInfo, SKILL.md, cli.json).
///
/// Priority: `cli.json` → connector; plugin.json with declared agents or a
/// team marker → agent/team; a `SKILL.md` (with or without a plugin.json that
/// declares no agents) → skill; otherwise the market's source kind.
fn derive_kind(source: &std::path::Path, source_kind: &str, manifest: &Option<PluginManifest>) -> &'static str {
    if source.join("cli.json").is_file() {
        return "connector";
    }
    if let Some(manifest) = manifest {
        if manifest.expert_type.as_deref() == Some("team") {
            return "team";
        }
        if manifest.team_info.is_some() {
            return "team";
        }
        if !manifest.agents.is_empty() {
            return "agent";
        }
    }
    // A root `SKILL.md` is the strongest skill signal: skill-market entries
    // frequently ship an auxiliary `mcp.json` next to it (e.g. 腾讯云知), so
    // a skill wins over the MCP marker unless the dir is a CLI connector.
    if source.join("SKILL.md").is_file() {
        return "skill";
    }
    if source.join("mcp.json").is_file() {
        return "connector";
    }
    match source_kind {
        "workbuddy-connector-market" => "connector",
        "workbuddy-skill-market" => "skill",
        _ => "agent",
    }
}
