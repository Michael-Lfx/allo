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
use nomifun_db::{
    IMarketplaceRepository, IPluginSnapshotRepository, PluginMarketplaceRow,
    PluginSnapshotComponentRow,
};
use nomifun_importer::{ImporterService, LocalizedText, PluginManifest};

/// Composition-root Store provider: marketplace catalog + provenance install
/// state + one-click install.
#[derive(Clone)]
pub struct AppServerStoreProvider {
    markets: Arc<dyn MarketplaceProvider>,
    market_rows: Arc<dyn IMarketplaceRepository>,
    snapshots: Arc<dyn IPluginSnapshotRepository>,
    #[allow(dead_code)]
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

/// Display metadata for one market index row (`.codebuddy-connector/connectors.json`
/// or `.codebuddy-skill/marketplace.json`). Used when the entry directory ships
/// no plugin.json display block — which is the norm for connectors and skills.
#[derive(Debug, Clone, Default)]
pub(crate) struct MarketIndexInfo {
    name_zh: Option<String>,
    name_en: Option<String>,
    /// The version the market advertises for this entry. This is the only
    /// version a connector or skill entry has: neither carries a manifest of its
    /// own, so without it the entry is pinned at the `1.0.0` placeholder and can
    /// never be seen to change (`36` D2 / `18` §11 D9).
    version: Option<String>,
    #[allow(dead_code)]
    description: Option<String>,
}

/// One market's own entry indexes, read once per market.
///
/// Both maps are keyed by the **entry name the prober carries**, which is the id
/// for connectors and the row's `name` for skills — the same string
/// [`entry_facts`] looks up.
#[derive(Debug, Clone, Default)]
pub(crate) struct MarketIndex {
    connectors: HashMap<String, MarketIndexInfo>,
    skills: HashMap<String, MarketIndexInfo>,
}

impl MarketIndex {
    /// Read the indexes a market root may carry (best-effort: a missing or
    /// malformed index yields an empty map rather than failing the catalog).
    pub(crate) fn read(root: &Path) -> Self {
        Self {
            connectors: read_index(root, ".codebuddy-connector/connectors.json", "connectors", "id"),
            skills: read_index(root, ".codebuddy-skill/marketplace.json", "skills", "name"),
        }
    }

    fn info(&self, kind: &str, entry_name: &str) -> Option<&MarketIndexInfo> {
        match kind {
            "connector" => self.connectors.get(entry_name),
            "skill" => self.skills.get(entry_name),
            _ => None,
        }
    }
}

/// Load one market index file (`root/<relative>` → rows keyed by `key_field`).
fn read_index(
    root: &Path,
    relative: &str,
    rows_key: &str,
    key_field: &str,
) -> HashMap<String, MarketIndexInfo> {
    let Ok(text) = std::fs::read_to_string(root.join(relative)) else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    let Some(items) = value.get(rows_key).and_then(|value| value.as_array()) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for item in items {
        let Some(key) = item.get(key_field).and_then(|value| value.as_str()) else {
            continue;
        };
        map.insert(
            key.to_owned(),
            MarketIndexInfo {
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

/// The version this entry currently advertises in its marketplace.
///
/// One derivation, shared by `list`, `install_entry` and — through
/// [`entry_facts`] — `market/entry-import`. If they disagreed, the catalogue
/// could advertise an update that installing would never deliver — or hide one
/// it would silently apply. `list`'s `update_available` is
/// `snapshot.version != this`, and `install_entry` re-imports on exactly the
/// same inequality.
fn entry_live_version(
    manifest: Option<&PluginManifest>,
    index_info: Option<&MarketIndexInfo>,
    entry: &nomifun_db::MarketplaceEntry,
) -> String {
    manifest
        .map(|manifest| manifest.version())
        .map(str::to_owned)
        .or_else(|| index_info.and_then(|info| info.version.clone()))
        .unwrap_or_else(|| entry.version.clone().unwrap_or_else(|| "1.0.0".into()))
}

/// Everything the store derives for one entry, in one place.
///
/// `store/list`, `store/install-entry` and `market/entry-import` all need the
/// same answers: where the entry lives, what it declares, what kind it is, its
/// market index row, and **the version it currently advertises**. Sharing the
/// derivation is what stops the catalog from advertising an update the importer
/// would never deliver (`36` D2) — the importer reads [`EntryFacts::version`]
/// out of here and stores the snapshot under it.
pub(crate) struct EntryFacts {
    /// Entry payload directory (internal; never crosses the protocol seam).
    pub source: PathBuf,
    pub manifest: Option<PluginManifest>,
    pub kind: &'static str,
    pub index_info: Option<MarketIndexInfo>,
    pub version: String,
}

/// Derive [`EntryFacts`] for one entry.
///
/// `index` is the market's own index (connectors / skills), read **once per
/// market** by the caller: re-reading it here would add one file parse per
/// catalog row.
pub(crate) fn entry_facts(
    market: &PluginMarketplaceRow,
    entry: &nomifun_db::MarketplaceEntry,
    market_root: &Path,
    index: &MarketIndex,
) -> EntryFacts {
    let source = crate::market_fetch::entry_source_path(
        &market.source_kind,
        &market.source_uri,
        market_root,
        &market.marketplace_id,
        &entry.source_uri,
    );
    let manifest = nomifun_importer::read_plugin_display(&source);
    let kind = derive_kind(&source, &entry.source_kind, &manifest);
    // Connector markets declare display metadata in their `connectors.json`
    // index (id → name_zh/name_en/version); fall back to it when no plugin.json
    // display block exists. Skill markets keep the same facts in their own
    // `marketplace.json` rows, and it is the only place a skill's version exists
    // at all — a bare `SKILL.md` entry parses to the `1.0.0` placeholder.
    let index_info = index.info(kind, &entry.name).cloned();
    let version = entry_live_version(manifest.as_ref(), index_info.as_ref(), entry);
    EntryFacts { source, manifest, kind, index_info, version }
}

/// The market root directory for index lookups (best-effort; for remote
/// markets this is the mirrored live root, for directory sources the source).
pub(crate) fn market_root_dir(market: &PluginMarketplaceRow, market_root: &Path) -> Option<PathBuf> {
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
                .map(|root| MarketIndex::read(root))
                .unwrap_or_default();
            for entry in market.entries() {
                // One derivation for source / manifest / kind / index row /
                // version (`36` D2). Destructured so the rest of the loop keeps
                // reading the same names.
                let EntryFacts { source, manifest, kind, index_info, version } =
                    entry_facts(&market, &entry, &self.market_root, &index);

                // `02` §8: a `strict=true` entry must ship its own
                // `.codebuddy-plugin/plugin.json`. Checked against the live tree
                // with the same predicate the import gate uses, so a listed item
                // and an actual install can never disagree about whether it is
                // installable.
                let blocked_reason = crate::app_server_marketplace::strict_entry_block(
                    entry.strict,
                    source.join(".codebuddy-plugin/plugin.json").is_file(),
                );

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
                    .or_else(|| index_info.as_ref().and_then(|info| info.name_zh.clone()))
                    .or_else(|| index_info.as_ref().and_then(|info| info.name_en.clone()))
                    .unwrap_or_else(|| entry.name.clone());
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
                    // Carried verbatim from the market's own manifest row; the
                    // discovery layer already dropped anything that was not a
                    // calendar date (`18` §3), so this needs no second check.
                    published_at: entry.published_at.clone(),
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
        let entry = row
            .entries()
            .into_iter()
            .find(|entry| entry.name == entry_name)
            .ok_or_else(|| AppError::NotFound(format!("entry {entry_name} not found")))?;

        // What the marketplace advertises right now, derived exactly the way
        // `store/list` derives it.
        let live = {
            let index = market_root_dir(&row, &self.market_root)
                .map(|root| MarketIndex::read(&root))
                .unwrap_or_default();
            entry_facts(&row, &entry, &self.market_root, &index).version
        };

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
                    // An installed entry stays a no-op here even when
                    // `update_available` is set: re-importing it silently would
                    // make "install" a hidden upgrade. Upgrading is
                    // `store/update-entry`, an explicit action (`36` D3).
                    return Ok(AppServerStoreInstallResult {
                        marketplace_id: marketplace_id.to_owned(),
                        entry_name: entry_name.to_owned(),
                        snapshot_id: row.snapshot_id.clone(),
                        version: row.version.clone(),
                        reused: true,
                        installed_count: 0,
                        warnings: vec!["entry already installed".into()],
                        errors: vec![],
                        // Nothing was attempted, so there is no per-component
                        // detail to report.
                        outcomes: vec![],
                        previous_version: None,
                        previous_snapshot_id: None,
                        released_count: 0,
                    });
                }
                if row.version == live {
                    // Same version: the imported snapshot is what the
                    // marketplace still offers, so install it as-is.
                    row.snapshot_id
                } else {
                    // The marketplace moved on (or rolled back) since this entry
                    // was imported. Install the *current* version rather than
                    // the stale snapshot: without this, "uninstall then install"
                    // would faithfully reinstall the old version. The previous
                    // snapshot stays immutable and keeps its history.
                    let result = self.markets.import_entry(marketplace_id, entry_name).await?;
                    if result.status == "blocked" {
                        return Ok(AppServerStoreInstallResult {
                            marketplace_id: marketplace_id.to_owned(),
                            entry_name: entry_name.to_owned(),
                            snapshot_id: result.snapshot_id,
                            version: result.version,
                            reused: false,
                            installed_count: 0,
                            warnings: result.warnings,
                            errors: result.errors,
                            // The import was refused, so nothing was registered.
                            outcomes: vec![],
                            previous_version: None,
                            previous_snapshot_id: None,
                            released_count: 0,
                        });
                    }
                    result.snapshot_id
                }
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
                        // The import was refused, so nothing was registered.
                        outcomes: vec![],
                        previous_version: None,
                        previous_snapshot_id: None,
                        released_count: 0,
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
            outcomes: install_result.outcomes,
            previous_version: None,
            previous_snapshot_id: None,
            released_count: 0,
        })
    }

    /// Upgrade an installed entry to the version its marketplace advertises
    /// (`36` §5.2).
    ///
    /// The order inside is the contract: the new version is imported and
    /// installed **first**, and only a fully successful install releases the old
    /// one — so a failure leaves the previous installation in place instead of
    /// leaving the user with nothing. `released_count: 0` is the machine
    /// readable form of "the previous installation was not touched".
    async fn update_entry(
        &self,
        marketplace_id: &str,
        entry_name: &str,
    ) -> Result<AppServerStoreInstallResult, AppError> {
        let row = self
            .market_rows
            .get_marketplace(marketplace_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::NotFound(format!("marketplace {marketplace_id} not found")))?;
        let entry = row
            .entries()
            .into_iter()
            .find(|entry| entry.name == entry_name)
            .ok_or_else(|| AppError::NotFound(format!("entry {entry_name} not found")))?;

        // What the marketplace advertises right now, derived exactly the way
        // `store/list` derives it.
        let live = {
            let index = market_root_dir(&row, &self.market_root)
                .map(|root| MarketIndex::read(&root))
                .unwrap_or_default();
            entry_facts(&row, &entry, &self.market_root, &index).version
        };

        let existing = self
            .snapshots
            .find_snapshot_by_provenance(marketplace_id, entry_name)
            .await
            .map_err(AppError::from)?
            // Never installed: refuse instead of installing. An "update" must not
            // become a way to install something nobody asked for (`36` §2) — the
            // client refuses the same case with `not_installed` before it calls.
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "entry {entry_name} has no snapshot to update; use store/install-entry"
                ))
            })?;
        let installed = {
            let components = self
                .snapshots
                .get_components(&existing.snapshot_id)
                .await
                .map_err(AppError::from)?;
            components.iter().any(|component| component.installed == 1)
        };
        if !installed {
            // A snapshot exists (someone imported it) but nothing was ever
            // registered: installing is the honest action, and that is exactly
            // what `store/install-entry` does.
            return Err(AppError::BadRequest(format!(
                "entry {entry_name} is imported but not installed; use store/install-entry"
            )));
        }

        if existing.version == live {
            // Already at the advertised version. Idempotent no-op, not an error:
            // two clients racing the same update must not produce a failure.
            return Ok(AppServerStoreInstallResult {
                marketplace_id: marketplace_id.to_owned(),
                entry_name: entry_name.to_owned(),
                snapshot_id: existing.snapshot_id,
                version: existing.version,
                reused: true,
                installed_count: 0,
                warnings: vec!["entry already at the advertised version".into()],
                errors: vec![],
                outcomes: vec![],
                previous_version: None,
                previous_snapshot_id: None,
                released_count: 0,
            });
        }

        // Import the version the market now advertises (`36` D2 is what makes
        // this the *advertised* version for skills and connectors instead of the
        // `1.0.0` placeholder). Content that changed without a version bump is
        // still refused by the importer: the previous installation then stays
        // exactly as it was, and the remedy is on the market side (`36` D6).
        let imported = self.markets.import_entry(marketplace_id, entry_name).await?;
        if imported.status == "blocked" {
            return Ok(AppServerStoreInstallResult {
                marketplace_id: marketplace_id.to_owned(),
                entry_name: entry_name.to_owned(),
                // The installed snapshot: nothing moved, and naming the refused
                // import would point the client at a row that does not exist.
                snapshot_id: existing.snapshot_id,
                version: existing.version,
                reused: true,
                installed_count: 0,
                warnings: imported.warnings,
                errors: imported.errors,
                outcomes: vec![],
                previous_version: None,
                previous_snapshot_id: None,
                released_count: 0,
            });
        }

        let replaced = self
            .installs
            .replace(&existing.snapshot_id, &imported.snapshot_id)
            .await?;
        let mut warnings = replaced.install.warnings;
        // A component of the old version that could not be released leaves an
        // artifact behind. Its install record is kept (the `install/uninstall`
        // contract), so `install/uninstall` on the old snapshot can still retry —
        // which is why this is a warning rather than a failed update.
        warnings.extend(
            replaced
                .release_errors
                .into_iter()
                .map(|failure| format!("replaced snapshot could not be fully released: {failure}")),
        );
        Ok(AppServerStoreInstallResult {
            marketplace_id: marketplace_id.to_owned(),
            entry_name: entry_name.to_owned(),
            snapshot_id: replaced.install.snapshot_id,
            version: replaced.install.version,
            reused: false,
            installed_count: replaced.install.installed_count,
            warnings,
            errors: replaced.install.errors,
            outcomes: replaced.install.outcomes,
            previous_version: Some(existing.version),
            previous_snapshot_id: Some(existing.snapshot_id),
            released_count: replaced.released_count,
        })
    }

    /// The sweep's candidate set (doc `37` §3.3, D4–D6).
    ///
    /// Built on top of [`Self::list`] on purpose: `installed`,
    /// `update_available`, `kind` and `blocked_reason` are derived there from one
    /// set of facts (`entry_facts` + provenance), and a sweep that re-derived
    /// them would eventually disagree with what the user sees in the catalog.
    /// The one fact `list` does not project is the component `disabled` flag, so
    /// it is fetched here per surviving row.
    async fn auto_update_candidates(
        &self,
        marketplace_id: &str,
        kinds: &[String],
    ) -> Result<Vec<String>, AppError> {
        // `[]` means "upgrade nothing" — the pre-`37` sweep behaviour. Bail
        // before probing every market for a catalog nobody asked for.
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let items = self.list().await?.items;
        let mut candidates = Vec::new();
        for item in items {
            if item.marketplace_id != marketplace_id {
                continue;
            }
            // The row-only clauses first: the `disabled` probe below costs a
            // second query, so it only runs for a row that could pass.
            if !auto_update_eligible_from_row(&item, kinds) {
                continue;
            }
            // D6-1: every component the user switched off by hand. An upgrade
            // would install the new snapshot *enabled*, silently undoing a
            // deliberate decision — the sweep skips instead of re-enabling.
            if let Some(snapshot_id) = item.snapshot_id.as_deref() {
                let components = self
                    .snapshots
                    .get_components(snapshot_id)
                    .await
                    .map_err(AppError::from)?;
                if all_components_disabled(&components) {
                    continue;
                }
            }
            candidates.push(item.entry_name);
        }
        Ok(candidates)
    }
}

/// The sweep's eligibility clauses that need nothing but the catalog row
/// (doc `37` §3.3, D4/D5/D6-2/D6-3).
///
/// A free function because these are the clauses a user can observe as a
/// *refusal* ("why did my connector not upgrade?"), and every one of them is
/// decided from the row `store/list` already shows.
fn auto_update_eligible_from_row(item: &AppServerStoreItem, kinds: &[String]) -> bool {
    // D4: only an installed entry has an installation to replace.
    if !item.installed {
        return false;
    }
    // D6-3: nothing to do while the advertised version *is* the installed one.
    // (`update_entry` would answer a no-op, and filing that as progress would
    // make `upgraded` a lie.)
    if !item.update_available {
        return false;
    }
    // D5: the host's kind whitelist, compared case-insensitively because the
    // whitelist is operator-written while `list` derives the kind.
    if !kinds.iter().any(|kind| kind.eq_ignore_ascii_case(&item.kind)) {
        return false;
    }
    // D6-2: the live source already says this entry cannot be installed. An
    // unattended upgrade must not step over that verdict.
    if item.blocked_reason.is_some() {
        return false;
    }
    true
}

/// D6-1: every component of the installed snapshot is switched off by hand.
///
/// An empty component list is **not** "all disabled" — a snapshot with no
/// components has nothing to switch off, and reading it as disabled would make
/// the sweep skip an entry for a reason the user never expressed.
fn all_components_disabled(components: &[PluginSnapshotComponentRow]) -> bool {
    !components.is_empty() && components.iter().all(|row| row.disabled == 1)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalog row that passes every row-only clause; each test narrows one.
    fn item() -> AppServerStoreItem {
        AppServerStoreItem {
            id: "experts/expert-demo".into(),
            marketplace_id: "experts".into(),
            marketplace_name: "experts".into(),
            entry_name: "expert-demo".into(),
            kind: "agent".into(),
            name: "expert-demo".into(),
            display_name: None,
            profession: None,
            description: None,
            display_description: None,
            tags: vec![],
            quick_prompts: vec![],
            published_at: None,
            avatar_url: None,
            version: "2.0.0".into(),
            source_kind: "directory".into(),
            installed: true,
            update_available: true,
            snapshot_id: Some("snap-1".into()),
            installed_version: Some("1.0.0".into()),
            blocked_reason: None,
        }
    }

    fn component(disabled: i64) -> PluginSnapshotComponentRow {
        PluginSnapshotComponentRow {
            id: 1,
            snapshot_id: "snap-1".into(),
            component_id: "wb-demo-expert".into(),
            kind: "agent".into(),
            name: "expert-demo".into(),
            relative_path: None,
            compatibility_json: "{}".into(),
            payload_json: "{}".into(),
            installed: 1,
            disabled,
            installed_at: None,
            preset_id: None,
            runtime_ref: None,
        }
    }

    /// Doc `37` §3.3 (D4/D5/D6): the four row-only refusals, one assertion each,
    /// so a future edit cannot quietly drop one of them.
    #[test]
    fn auto_update_row_eligibility_is_the_documented_refusals() {
        let kinds = vec!["agent".to_owned(), "team".to_owned(), "skill".to_owned()];
        assert!(auto_update_eligible_from_row(&item(), &kinds), "the happy path");

        // D4: not installed → there is no installation to replace.
        let mut not_installed = item();
        not_installed.installed = false;
        assert!(!auto_update_eligible_from_row(&not_installed, &kinds));

        // D6-3: already at the advertised version → nothing to do.
        let mut current = item();
        current.update_available = false;
        assert!(!auto_update_eligible_from_row(&current, &kinds));

        // D5: connectors are never implicit, and opting in is what enables them.
        // The comparison is case-insensitive because the whitelist is written by
        // an operator while the kind is derived.
        let mut connector = item();
        connector.kind = "connector".into();
        assert!(!auto_update_eligible_from_row(&connector, &kinds));
        assert!(auto_update_eligible_from_row(&connector, &["CONNECTOR".to_owned()]));

        // D6-2: the live source already refused this entry.
        let mut blocked = item();
        blocked.blocked_reason = Some("strict entry ships no plugin.json".into());
        assert!(!auto_update_eligible_from_row(&blocked, &kinds));

        // `[]` refuses everything, which is what makes it mean "refresh the index
        // only" rather than "upgrade everything".
        assert!(!auto_update_eligible_from_row(&item(), &[]));
    }

    /// Doc `37` §3.3 (D6-1): a snapshot whose components are all switched off by
    /// hand is skipped — and "no components" is not that case.
    #[test]
    fn every_component_switched_off_by_hand_skips_the_entry() {
        assert!(!all_components_disabled(&[]), "no components is not 'all disabled'");
        assert!(!all_components_disabled(&[component(0)]));
        assert!(!all_components_disabled(&[component(1), component(0)]));
        assert!(all_components_disabled(&[component(1)]));
        assert!(all_components_disabled(&[component(1), component(1)]));
    }
}
