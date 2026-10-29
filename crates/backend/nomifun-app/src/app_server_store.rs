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
    AppServerLocalizedText, AppServerStoreCategoriesResponse, AppServerStoreInstallResult,
    AppServerStoreItem, AppServerStoreList,
};
use nomifun_app_server::{InstallProvider, MarketplaceProvider, StoreProvider};
use nomifun_common::AppError;
use nomifun_db::{
    IMarketplaceRepository, IPluginSnapshotRepository, PluginMarketplaceRow,
    PluginSnapshotComponentRow,
};
use nomifun_importer::{DisplayFields, ImporterService, LocalizedText, PluginManifest};

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
    /// Category dictionary reader (`43` M4): one cached read across
    /// `list()` and the `store/categories` face. Shared through an `Arc`
    /// because the provider itself is `Clone` and the cache holds a mutex.
    categories: std::sync::Arc<crate::app_server_categories::CategoryDictionary>,
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
        Self {
            markets,
            market_rows,
            snapshots,
            importer,
            installs,
            market_root,
            categories: std::sync::Arc::new(
                crate::app_server_categories::CategoryDictionary::default(),
            ),
        }
    }

    /// Point the dictionary reader at the currently enabled markets' roots.
    ///
    /// One helper for both store faces on purpose: `list()` used to be the only
    /// caller, which made `store/categories` answer differently depending on
    /// whether a `store/list` had happened first (a client whose first call is
    /// the filter-bar call got the built-in dictionary and no tag vocabulary —
    /// probed 2026-10-14). Cheap: a registry read plus path resolution, no
    /// per-entry work, so calling it from both is free.
    async fn remember_market_roots(&self) -> Result<(), AppError> {
        let markets = self.market_rows.list_marketplaces().await.map_err(AppError::from)?;
        let roots = markets
            .iter()
            .filter(|market| market.enabled == 1)
            .filter_map(|market| {
                market_root_dir(market, &self.market_root)
                    .map(|root| (market.marketplace_id.clone(), root))
            })
            .collect();
        self.categories.set_roots(roots);
        Ok(())
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
    /// The row's description, both languages kept apart (`50` §3.3).
    ///
    /// The row's **top-level** `description` is deliberately not used: its
    /// language is not reliable (210 of 283 connector rows carry English there,
    /// `50` P4), so it must never stand in for a missing locale (`50` INV-3).
    description: LocalizedText,
    /// The index row's localized tags (`tags_zh`/`tags_en`, position-paired,
    /// `legacy_tags_*` merged with zh-value dedup — `43` §3.3). The only tag
    /// source for skill/connector entries, which carry no `plugin.json`. Read
    /// by the marketplace import face to stamp them into the component payload
    /// (`44`).
    pub(crate) tags: Vec<LocalizedText>,
    /// The index row's `examples_zh` / `examples_en`, position-paired like
    /// `tags_*`. These are the **prompt examples** the market declares for a
    /// connector or skill node — the only prompt source those kinds have, since
    /// neither ships a `plugin.json` (doc `49` §3.7). Display data only: never
    /// search input (doc `49` D9).
    pub(crate) examples: Vec<LocalizedText>,
    /// The index row's `default_init_prompt` (`{zh,en}`), the opening prompt an
    /// `index` market declares for the entry. Only index markets carry it.
    pub(crate) default_init_prompt: Option<LocalizedText>,
    /// The index row's `quick_prompts` (`[{zh,en}]`), the card's suggested
    /// prompts. Kept apart from `examples` because both fields exist upstream:
    /// experts use `quick_prompts`, skill/connector rows use `examples_*`.
    pub(crate) quick_prompts: Vec<LocalizedText>,
    /// The absolute icon URL an `index` market declares (`icon_url`). The store
    /// listing prefers it over the host asset route, so a client never needs to
    /// download the market root to render a card.
    icon_url: Option<String>,
    /// Entry archive digest/size from an `index` market; the import path uses
    /// them to verify the downloaded `tar.gz` before extraction.
    sha256: Option<String>,
    size: Option<i64>,
    /// `false` when the market marks the entry unavailable. Always `true` for
    /// the legacy connector/skill indexes, which have no such concept.
    available: bool,
    unavailable_reason: Option<String>,
    /// The index row's own `kind` (`agent` | `team` | `plugin` | `skill` |
    /// `connector`). Only `index` markets declare it; the legacy indexes do not.
    kind: Option<String>,
    /// The index row's `category_id`, used when the entry has no plugin.json.
    category_id: Option<String>,
}

impl MarketIndexInfo {
    /// The `index` row's archive digest, when declared.
    pub(crate) fn entry_sha256(&self) -> Option<String> {
        self.sha256.clone()
    }

    /// The `index` row's archive size, when declared.
    pub(crate) fn entry_size(&self) -> Option<i64> {
        self.size
    }

    pub(crate) fn entry_available(&self) -> bool {
        self.available
    }

    pub(crate) fn entry_unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason.as_deref()
    }

    pub(crate) fn entry_icon_url(&self) -> Option<&str> {
        self.icon_url.as_deref()
    }

    pub(crate) fn entry_version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The display metadata this row declares (`50` §3.3), in the shape the
    /// importer stamps into a payload and the catalogs read back out.
    pub(crate) fn display(&self) -> DisplayFields {
        DisplayFields {
            name: localized_pair(self.name_zh.as_deref(), self.name_en.as_deref()),
            description: (!self.description.is_empty()).then(|| self.description.clone()),
            prompts: self.examples.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// A `{zh, en}` pair with blank sides dropped — a market that writes `""`
/// declared nothing (`50` INV-3).
fn localized_pair(zh: Option<&str>, en: Option<&str>) -> Option<LocalizedText> {
    let text = LocalizedText {
        zh: zh.map(str::to_owned).filter(|text| !text.trim().is_empty()),
        en: en.map(str::to_owned).filter(|text| !text.trim().is_empty()),
    };
    (!text.is_empty()).then_some(text)
}

/// Parse one `{zh,en}` object (`index` rows' `default_init_prompt`).
fn localized_object(value: Option<&serde_json::Value>) -> Option<LocalizedText> {
    let value = value?;
    let text = |key: &str| -> Option<String> {
        value
            .get(key)
            .and_then(|side| side.as_str())
            .map(str::trim)
            .filter(|side| !side.is_empty())
            .map(str::to_owned)
    };
    let text = LocalizedText { zh: text("zh"), en: text("en") };
    (!text.is_empty()).then_some(text)
}

/// Parse an array of `{zh,en}` objects (`index` rows' `quick_prompts`).
fn localized_objects(value: Option<&serde_json::Value>) -> Vec<LocalizedText> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| localized_object(Some(item)))
                .collect()
        })
        .unwrap_or_default()
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
    /// `root/index.json` rows keyed by name — the `index` source kind
    /// (schema_version 2). Kept apart from the two legacy maps because it is a
    /// single mixed-kind index, not a per-market manifest.
    index: HashMap<String, MarketIndexInfo>,
}

impl MarketIndex {
    /// Read the indexes a market root may carry (best-effort: a missing or
    /// malformed index yields an empty map rather than failing the catalog).
    pub(crate) fn read(root: &Path) -> Self {
        Self {
            connectors: read_index(root, ".codebuddy-connector/connectors.json", "connectors", "id"),
            skills: read_index(root, ".codebuddy-skill/marketplace.json", "skills", "name"),
            index: read_store_index(root),
        }
    }

    fn info(&self, kind: &str, entry_name: &str) -> Option<&MarketIndexInfo> {
        match kind {
            "connector" => self.connectors.get(entry_name),
            "skill" => self.skills.get(entry_name),
            _ => None,
        }
        // Doc 30 §6.1: an `index` market declares a single mixed-kind list, so
        // its row is looked up by name after the kind-specific maps miss. The
        // derived kind may differ from the row's own `kind` (e.g. `plugin` →
        // `agent`), which is why this fallback is name-only.
        .or_else(|| self.index.get(entry_name))
    }

    /// The absolute icon URL an `index` market declares for this entry.
    pub(crate) fn index_icon_url(&self, entry_name: &str) -> Option<&str> {
        self.index
            .get(entry_name)
            .and_then(|info| info.icon_url.as_deref())
    }

    /// The `index` row for an entry, when the market is index-based. Used by
    /// the import path for the archive `sha256`/`size` it must verify.
    pub(crate) fn entry_meta(&self, entry_name: &str) -> Option<&MarketIndexInfo> {
        self.index.get(entry_name)
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
                // The row's `name` is deliberately **not** a fallback here
                // (`50` D6): for a connector it is the entry's display text and
                // for a skill it is the technical slug, so copying it in would
                // put either Chinese into the `en` slot or a slug into
                // `display_name`. The store item's own `name` chain already ends
                // with the entry name, so dropping it changes nothing there.
                name_en: item.get("name_en").and_then(|v| v.as_str()).map(str::to_owned),
                version: item.get("version").and_then(|v| v.as_str()).map(str::to_owned),
                description: LocalizedText {
                    zh: item
                        .get("description_zh")
                        .and_then(|v| v.as_str())
                        .filter(|text| !text.trim().is_empty())
                        .map(str::to_owned),
                    en: item
                        .get("description_en")
                        .and_then(|v| v.as_str())
                        .filter(|text| !text.trim().is_empty())
                        .map(str::to_owned),
                },
                tags: localized_tags_from_index(item),
                examples: localized_examples_from_index(item),
                default_init_prompt: None,
                quick_prompts: Vec::new(),
                icon_url: None,
                sha256: None,
                size: None,
                available: true,
                unavailable_reason: None,
                kind: None,
                category_id: item
                    .get("category_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            },
        );
    }
    map
}

/// Read an `index` market's `root/index.json` (schema_version 2) into the same
/// normalized shape as the legacy indexes.
///
/// Best-effort like [`read_index`]: a missing/malformed/older file yields an
/// empty map, so a market source that briefly has not materialized yet does not
/// fail the whole catalog.
fn read_store_index(root: &Path) -> HashMap<String, MarketIndexInfo> {
    let Ok(text) = std::fs::read_to_string(root.join("index.json")) else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    let Some(items) = value.get("entries").and_then(|value| value.as_array()) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for item in items {
        let Some(name) = item.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let display = item.get("display");
        let description = item.get("description");
        map.insert(
            name.to_owned(),
            MarketIndexInfo {
                name_zh: display.and_then(|display| display.get("zh")).and_then(|v| v.as_str()).map(str::to_owned),
                name_en: display.and_then(|display| display.get("en")).and_then(|v| v.as_str()).map(str::to_owned),
                version: item.get("version").and_then(|value| value.as_str()).map(str::to_owned),
                description: LocalizedText {
                    zh: description.and_then(|d| d.get("zh")).and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()).map(str::to_owned),
                    en: description.and_then(|d| d.get("en")).and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()).map(str::to_owned),
                },
                tags: localized_tags_from_index(item),
                examples: localized_examples_from_index(item),
                default_init_prompt: localized_object(item.get("default_init_prompt")),
                quick_prompts: localized_objects(item.get("quick_prompts")),
                icon_url: item.get("icon_url").and_then(|value| value.as_str()).map(str::to_owned),
                sha256: item.get("sha256").and_then(|value| value.as_str()).map(str::to_owned),
                size: item.get("size").and_then(|value| value.as_i64()),
                available: item.get("available").and_then(|value| value.as_bool()).unwrap_or(true),
                unavailable_reason: item.get("unavailable_reason").and_then(|value| value.as_str()).map(str::to_owned),
                kind: item.get("kind").and_then(|value| value.as_str()).map(str::to_owned),
                category_id: item.get("category_id").and_then(|value| value.as_str()).map(str::to_owned),
            },
        );
    }
    map
}

/// Pair an index row's `tags_zh`/`tags_en` into `LocalizedText`s, by position
/// (`43` §3.3). One side alone is fine — the missing locale stays `None`.
/// `legacy_tags_*` (a real market shape, e.g. 腾讯会议's `legacy_tags_zh`) are
/// appended after the primary tags; a legacy tag whose zh value duplicates a
/// primary one is dropped, because its meaning *is* "the old name of one of
/// these tags".
fn localized_tags_from_index(item: &serde_json::Value) -> Vec<LocalizedText> {
    let strings = |keys: &[&str]| -> Vec<String> {
        keys.iter()
            .filter_map(|key| item.get(*key))
            .filter_map(|value| value.as_array())
            .flatten()
            .filter_map(|value| value.as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let zh = strings(&["tags_zh"]);
    let en = strings(&["tags_en"]);
    let mut out: Vec<LocalizedText> = zh
        .into_iter()
        .zip(en.into_iter().chain(std::iter::repeat(String::new())))
        .map(|(zh, en)| LocalizedText {
            zh: Some(zh),
            en: (!en.is_empty()).then_some(en),
        })
        .collect();
    // If English was the longer list, the tail entries have no zh.
    let en_len = item.get("tags_en").and_then(|v| v.as_array()).map_or(0, Vec::len);
    if en_len > out.len() {
        let extra: Vec<String> = item
            .get("tags_en")
            .and_then(|v| v.as_array())
            .map(|values| {
                values[out.len()..]
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        out.extend(extra.into_iter().map(|en| LocalizedText { en: Some(en), zh: None }));
    }
    let seen: std::collections::HashSet<String> =
        out.iter().filter_map(|tag| tag.zh.clone()).collect();
    for (legacy_zh, legacy_en) in strings(&["legacy_tags_zh"])
        .into_iter()
        .zip(strings(&["legacy_tags_en"]).into_iter().chain(std::iter::repeat(String::new())))
    {
        if seen.contains(&legacy_zh) {
            continue;
        }
        out.push(LocalizedText {
            zh: Some(legacy_zh.clone()),
            en: (!legacy_en.is_empty()).then_some(legacy_en),
        });
    }
    out
}

/// Pair an index row's `examples_zh` / `examples_en` into `LocalizedText`s, by
/// position — the same rule `tags_*` follows (`43` §3.3). One side alone is
/// fine; the missing locale stays `None`. Empty strings are dropped (an empty
/// hint is not a hint).
///
/// Doc `49` §3.7: these reach the store card as `StoreItem.quick_prompts` for
/// entries that ship no `plugin.json` (connectors and skills).
fn localized_examples_from_index(item: &serde_json::Value) -> Vec<LocalizedText> {
    let strings = |key: &str| -> Vec<String> {
        item.get(key)
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let zh = strings("examples_zh");
    let en = strings("examples_en");
    if zh.is_empty() && en.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<LocalizedText> = zh
        .iter()
        .cloned()
        .zip(en.iter().cloned().chain(std::iter::repeat(String::new())))
        .map(|(zh, en)| LocalizedText {
            zh: Some(zh),
            en: (!en.is_empty()).then_some(en),
        })
        .collect();
    // If the English list is longer, its tail entries have no zh counterpart.
    for extra in en.iter().skip(out.len()) {
        out.push(LocalizedText { zh: None, en: Some(extra.clone()) });
    }
    out
}

/// The prompt hints a store card shows for one entry.
///
/// `plugin.json` wins; the market index row's `examples_*` fills the gap for the
/// kinds that ship no manifest (connectors and skills — doc `49` §3.7). Both
/// empty means "this market declared no hints", which travels as an empty array
/// so the card renders no hint line at all (INV-6).
fn store_quick_prompts(
    manifest: Option<&PluginManifest>,
    index_info: Option<&MarketIndexInfo>,
) -> Vec<AppServerLocalizedText> {
    let from_manifest: Vec<AppServerLocalizedText> = manifest
        .map(|manifest| {
            manifest.quick_prompts.iter().filter_map(|prompt| localized_or_none(prompt)).collect()
        })
        .unwrap_or_default();
    if !from_manifest.is_empty() {
        return from_manifest;
    }
    // `index` rows declare `quick_prompts` (experts) or `examples_*`
    // (skills/connectors); both are card prompts, so either fills the slot.
    let from_index_quick: Vec<AppServerLocalizedText> = index_info
        .map(|info| info.quick_prompts.iter().filter_map(localized_or_none).collect())
        .unwrap_or_default();
    if !from_index_quick.is_empty() {
        return from_index_quick;
    }
    index_info
        .map(|info| info.examples.iter().filter_map(localized_or_none).collect())
        .unwrap_or_default()
}

/// The entry's opening prompt: `plugin.json` first, then the index row's
/// `default_init_prompt` (index markets carry it instead of a manifest).
fn store_default_init_prompt(
    manifest: Option<&PluginManifest>,
    index_info: Option<&MarketIndexInfo>,
) -> Option<AppServerLocalizedText> {
    manifest
        .and_then(|manifest| manifest.default_init_prompt.as_ref())
        .and_then(localized_or_none)
        .or_else(|| {
            index_info
                .and_then(|info| info.default_init_prompt.as_ref())
                .and_then(localized_or_none)
        })
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
    entry_facts_at(entry, source, index)
}

/// [`entry_facts`] against an explicit payload directory.
///
/// `import_entry` uses this after downloading + extracting a `remote` entry:
/// the extracted directory is the real source, and the index row (not the
/// directory shape) owns the kind for an index market.
pub(crate) fn entry_facts_at(
    entry: &nomifun_db::MarketplaceEntry,
    source: PathBuf,
    index: &MarketIndex,
) -> EntryFacts {
    let manifest = nomifun_importer::read_plugin_display(&source);
    let derived = derive_kind(&source, &entry.source_kind, &manifest);
    // Doc 30 §6.1: an index market declares the entry's kind itself (`plugin`
    // included). Trust it over the directory guess — an index entry's payload
    // is not extracted yet when the catalog is built, so the directory shape
    // says nothing. `plugin` has no `StoreItemKind` counterpart; a plugin is an
    // agent as far as the store's install model is concerned.
    let index_info = if entry.source_kind == "remote" {
        index.entry_meta(&entry.name).cloned()
    } else {
        index.info(derived, &entry.name).cloned()
    };
    let kind = if entry.source_kind == "remote" {
        index_info
            .as_ref()
            .and_then(|info| info.kind.as_deref())
            .map(store_kind_from_index)
            .unwrap_or(derived)
    } else {
        derived
    };
    let version = entry_live_version(manifest.as_ref(), index_info.as_ref(), entry);
    EntryFacts { source, manifest, kind, index_info, version }
}

/// Map an `index` row's `kind` to the store's closed item-kind set.
///
/// The store has four kinds (`agent` | `team` | `skill` | `connector`); an
/// index may also declare `plugin`, which is the package format of an agent.
fn store_kind_from_index(kind: &str) -> &'static str {
    match kind {
        "team" => "team",
        "skill" => "skill",
        "connector" => "connector",
        _ => "agent",
    }
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
///
/// Doubles as the resolver the **import** path uses to carry that icon into the
/// snapshot (doc `51` §3.2): the listing face and the import face must agree on
/// which file "this entry's icon" means, so they share this one function.
pub(crate) fn market_icon_for(root: &Path, source_uri: &str) -> Option<String> {
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
    /// The category dictionary + market tag vocabulary (`43` §3.1).
    async fn categories(&self) -> Result<AppServerStoreCategoriesResponse, AppError> {
        // The roots are learned by `list()`, so a cold `categories` call before
        // any `store/list` legitimately answers empty — same partial-catalog
        // honesty as `markets_pending`.
        // Learn the enabled markets' roots first: the dictionary reader is
        // otherwise blind on a cold provider, and this endpoint is exactly the
        // filter-bar call a client may make **before** any `store/list`.
        self.remember_market_roots().await?;
        let items = self.categories.load().await?;
        let marketplace_tags = self.categories.load_marketplace_tags().await;
        Ok(AppServerStoreCategoriesResponse {
            fetched_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_default(),
            marketplace_tags,
            items,
        })
    }

    async fn list(&self) -> Result<AppServerStoreList, AppError> {
        let market_rows = self
            .market_rows
            .list_marketplaces()
            .await
            .map_err(AppError::from)?;
        // The dictionary reader learns each enabled market's root here — shared
        // with `categories()` through one helper so the two faces cannot drift.
        self.remember_market_roots().await?;
        // One dictionary read per catalog build (`43` §3.3): the join needs the
        // enabled markets' category rows, and re-reading the (31k-line) experts
        // manifest per entry would be waste.
        let categories = match self.categories.load().await {
            Ok(items) => Some(items),
            // A missing or malformed dictionary is not an error: the raw
            // `category_id` still travels, only the resolved form is absent.
            Err(_) => None,
        };
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
                // Index row (absolute icon URL / archive digest / availability).
                // Read once, before the card fields that consume it.
                let index_meta = index.entry_meta(&entry.name);

                // `02` §8: a `strict=true` entry must ship its own
                // `.codebuddy-plugin/plugin.json`. Checked against the live tree
                // with the same predicate the import gate uses, so a listed item
                // and an actual install can never disagree about whether it is
                // installable.
                let blocked_reason = crate::app_server_marketplace::strict_entry_block(
                    entry.strict,
                    source.join(".codebuddy-plugin/plugin.json").is_file(),
                )
                .or_else(|| {
                    // An index market can declare an entry unavailable; keep it
                    // listed so the reason is readable, but never offer install.
                    index_meta.and_then(|meta| {
                        (!meta.available).then(|| {
                            meta.unavailable_reason
                                .clone()
                                .unwrap_or_else(|| "市场标记该条目不可用".to_owned())
                        })
                    })
                });

                let display_name = manifest
                    .as_ref()
                    .and_then(|m| m.display_name.as_ref())
                    .and_then(localized_or_none)
                    .or_else(|| {
                        // Index markets declare display names in their own row;
                        // the payload has no plugin.json at list time.
                        let display = index_info.as_ref()?.display();
                        display.name.as_ref().and_then(localized_or_none)
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
                // Doc 30 §6.1: an index market declares an **absolute** icon URL.
                // It wins over the host asset route (no market root download
                // needed); an absent one falls back to the legacy route and then
                // to `null`.
                let index_icon = index.index_icon_url(&entry.name).map(str::to_owned);

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
                        // Index rows carry the description themselves; there
                        // is no plugin.json to read it from at list time.
                        .or_else(|| {
                            index_meta
                                .and_then(|meta| meta.description.primary().map(str::to_owned))
                        })
                        .or_else(|| entry.description.clone()),
                    display_description: manifest
                        .as_ref()
                        .and_then(|m| m.display_description.as_ref())
                        .and_then(localized_or_none)
                        .or_else(|| {
                            index_meta
                                .filter(|meta| !meta.description.is_empty())
                                .map(|meta| to_localized(&meta.description))
                        }),
                    tags: manifest
                        .as_ref()
                        .map(|m| m.tags.iter().filter_map(|t| localized_or_none(t)).collect())
                        .filter(|tags: &Vec<AppServerLocalizedText>| !tags.is_empty())
                        // Skill/connector entries carry no plugin.json: the
                        // market index row is their only tag source (`43` §3.3).
                        .unwrap_or_else(|| {
                            index_info
                                .as_ref()
                                .map(|info| {
                                    info.tags.iter().filter_map(|t| localized_or_none(t)).collect()
                                })
                                .unwrap_or_default()
                        }),
                    category_id: manifest
                        .as_ref()
                        .and_then(|m| m.category_id.clone())
                        // Index rows carry the category id themselves; the
                        // payload has no plugin.json at list time.
                        .or_else(|| index_meta.and_then(|meta| meta.category_id.clone())),
                    category: {
                        // The host joins the dictionary (`43` D1): the raw
                        // `category_id` travels regardless, and the resolved
                        // category only appears on an exact dictionary hit —
                        // never guessed from the id's text.
                        match manifest
                            .as_ref()
                            .and_then(|m| m.category_id.as_deref())
                            .or_else(|| index_meta.and_then(|meta| meta.category_id.as_deref()))
                        {
                            Some(category_id) => categories
                                .as_ref()
                                .and_then(|items| {
                                    items.iter().find(|category| category.id == category_id)
                                })
                                .cloned(),
                            None => None,
                        }
                    },
                    quick_prompts: store_quick_prompts(manifest.as_ref(), index_info.as_ref()),
                    default_init_prompt: store_default_init_prompt(
                        manifest.as_ref(),
                        index_info.as_ref(),
                    ),
                    // Carried verbatim from the market's own manifest row; the
                    // discovery layer already dropped anything that was not a
                    // calendar date (`18` §3), so this needs no second check.
                    published_at: entry.published_at.clone(),
                    avatar_url: index_icon.clone().or_else(|| {
                        avatar.map(|path| {
                            format!(
                                "/api/app-server/store/{}/entries/{}/assets/{}",
                                market.marketplace_id,
                                entry.name,
                                path.trim_start_matches('/')
                            )
                        })
                    }),
                    icon_url: index_icon,
                    download_size: index_meta.and_then(|meta| meta.size),
                    sha256: index_meta.and_then(|meta| meta.sha256.clone()),
                    available: index_meta.map(|meta| meta.available).unwrap_or(true),
                    unavailable_reason: index_meta.and_then(|meta| meta.unavailable_reason.clone()),
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
            default_init_prompt: None,
            published_at: None,
            avatar_url: None,
            icon_url: None,
            download_size: None,
            sha256: None,
            available: true,
            unavailable_reason: None,
            version: "2.0.0".into(),
            source_kind: "directory".into(),
            installed: true,
            update_available: true,
            snapshot_id: Some("snap-1".into()),
            installed_version: Some("1.0.0".into()),
            blocked_reason: None,
            category_id: None,
            category: None,
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

    /// Doc `43` §3.3: `tags_zh`/`tags_en` pair by position, an unmatched
    /// locale travels alone, and `legacy_tags_*` merge in with zh-value
    /// deduplication (the real 腾讯会议 row duplicates its legacy tag).
    #[test]
    fn index_row_tags_pair_and_merge_legacy_without_duplicates() {
        let paired = serde_json::json!({
            "name": "腾讯文档",
            "tags_zh": ["办公协同", "效率工具"],
            "tags_en": ["Collaboration", "Productivity"],
        });
        let tags = localized_tags_from_index(&paired);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].zh.as_deref(), Some("办公协同"));
        assert_eq!(tags[0].en.as_deref(), Some("Collaboration"));
        assert_eq!(tags[1].zh.as_deref(), Some("效率工具"));

        // zh-only and en-only rows both survive; the missing locale is None.
        let zh_only = serde_json::json!({ "name": "x", "tags_zh": ["开发工具"] });
        let tags = localized_tags_from_index(&zh_only);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].zh.as_deref(), Some("开发工具"));
        assert_eq!(tags[0].en, None);

        let en_only = serde_json::json!({ "name": "x", "tags_en": ["Website Deployment"] });
        let tags = localized_tags_from_index(&en_only);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].en.as_deref(), Some("Website Deployment"));
        assert_eq!(tags[0].zh, None);

        // The real legacy shape: a legacy zh tag duplicating a primary one is
        // dropped; a genuinely new legacy name is appended.
        let legacy = serde_json::json!({
            "name": "腾讯会议",
            "tags_zh": ["办公协同"],
            "tags_en": ["Collaboration"],
            "legacy_tags_zh": ["办公效率"],
            "legacy_tags_en": ["Productivity"],
        });
        let tags = localized_tags_from_index(&legacy);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[1].zh.as_deref(), Some("办公效率"));

        let duplicate_legacy = serde_json::json!({
            "name": "x",
            "tags_zh": ["办公协同"],
            "legacy_tags_zh": ["办公协同"],
            "legacy_tags_en": ["Collaboration"],
        });
        assert_eq!(localized_tags_from_index(&duplicate_legacy).len(), 1);

        // A row without any tag declaration contributes nothing.
        assert!(localized_tags_from_index(&serde_json::json!({ "name": "y" })).is_empty());
    }

    /// Doc `49` §3.7: `examples_zh`/`examples_en` pair by position, exactly like
    /// `tags_*`, and an empty hint is not a hint.
    #[test]
    fn index_row_examples_pair_by_position() {
        let paired = serde_json::json!({
            "id": "baidu-netdisk",
            "examples_zh": ["查看我百度网盘根目录下的文件", "搜索与「项目合同」相关的文件"],
            "examples_en": ["List my root folder", "Find files about the contract"],
        });
        let examples = localized_examples_from_index(&paired);
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].zh.as_deref(), Some("查看我百度网盘根目录下的文件"));
        assert_eq!(examples[0].en.as_deref(), Some("List my root folder"));
        assert_eq!(examples[1].en.as_deref(), Some("Find files about the contract"));

        // zh-only survives with `en: None`.
        let zh_only = localized_examples_from_index(&serde_json::json!({
            "examples_zh": ["今天 AI 圈有什么新动态？"],
        }));
        assert_eq!(zh_only.len(), 1);
        assert_eq!(zh_only[0].zh.as_deref(), Some("今天 AI 圈有什么新动态？"));
        assert_eq!(zh_only[0].en, None);

        // An English-only tail keeps its position instead of shifting left.
        let en_longer = localized_examples_from_index(&serde_json::json!({
            "examples_zh": ["第一个"],
            "examples_en": ["first", "second"],
        }));
        assert_eq!(en_longer.len(), 2);
        assert_eq!(en_longer[0].zh.as_deref(), Some("第一个"));
        assert_eq!(en_longer[1].zh, None);
        assert_eq!(en_longer[1].en.as_deref(), Some("second"));

        // Empty strings and a missing declaration both mean "nothing to show".
        assert!(localized_examples_from_index(&serde_json::json!({
            "examples_zh": [""],
            "examples_en": [],
        }))
        .is_empty());
        assert!(localized_examples_from_index(&serde_json::json!({ "id": "x" })).is_empty());
    }

    /// Doc `49` TC-13…TC-16: manifest first, index row as the fallback, empty
    /// when neither declares anything (INV-6).
    #[test]
    fn quick_prompts_prefer_the_manifest_and_fall_back_to_the_index_row() {
        let manifest: PluginManifest = serde_json::from_value(serde_json::json!({
            "name": "aihot",
            "quickPrompts": [{ "zh": "今天 AI 圈有什么新动态？", "en": "What's new in AI today?" }],
        }))
        .expect("manifest");

        let index_info = MarketIndexInfo {
            examples: vec![LocalizedText { zh: Some("索引行示例".into()), en: None }],
            ..MarketIndexInfo::default()
        };

        // Manifest wins even when the index row also declares examples.
        let from_manifest = store_quick_prompts(Some(&manifest), Some(&index_info));
        assert_eq!(from_manifest.len(), 1);
        assert_eq!(from_manifest[0].zh.as_deref(), Some("今天 AI 圈有什么新动态？"));

        // A connector / skill entry has no manifest: the index row fills in.
        let from_index = store_quick_prompts(None, Some(&index_info));
        assert_eq!(from_index.len(), 1);
        assert_eq!(from_index[0].zh.as_deref(), Some("索引行示例"));

        // Neither declares anything → no hint line on the card.
        assert!(store_quick_prompts(None, None).is_empty());
        assert!(store_quick_prompts(None, Some(&MarketIndexInfo::default())).is_empty());
    }
}

#[cfg(test)]
mod index_market_tests {
    use super::*;

    fn remote_entry(name: &str) -> nomifun_db::MarketplaceEntry {
        nomifun_db::MarketplaceEntry {
            name: name.into(),
            source_kind: "remote".into(),
            source_uri: format!("https://cdn.example/{name}.tar.gz"),
            version: None,
            description: None,
            keywords: vec![],
            category: None,
            published_at: None,
            localized: std::collections::BTreeMap::new(),
            strict: false,
            blocked_reason: None,
        }
    }

    #[test]
    fn index_json_feeds_icon_digest_and_store_kind() {
        let dir = tempfile::tempdir().unwrap();
        let index = serde_json::json!({
            "schema_version": 2,
            "market": "experts",
            "market_kind": "plugin",
            "entries": [{
                "name": "a-share-analysis",
                "kind": "team",
                "display": {"zh": "A股研究团队", "en": "A-Share Team"},
                "description": {"zh": "研究", "en": "research"},
                "version": "1.2.3",
                "download": "https://cdn.example/a.tar.gz",
                "sha256": "a".repeat(64),
                "size": 42,
                "files": 3,
                "icon_url": "https://cdn.example/icons/a.webp",
                "category_id": "08-FinanceInvestment",
                "default_init_prompt": {"zh": "帮我看看A股", "en": "Review A-shares"},
                "quick_prompts": [{"zh": "今天行情如何", "en": "How is the market today"}],
                "available": true
            }]
        });
        std::fs::write(dir.path().join("index.json"), index.to_string()).unwrap();
        let parsed = MarketIndex::read(dir.path());
        assert_eq!(parsed.index_icon_url("a-share-analysis"), Some("https://cdn.example/icons/a.webp"));
        let meta = parsed.entry_meta("a-share-analysis").expect("row");
        assert_eq!(meta.entry_sha256(), Some("a".repeat(64)));
        assert_eq!(meta.entry_size(), Some(42));
        assert!(meta.available);
        assert_eq!(
            meta.default_init_prompt.as_ref().and_then(|p| p.zh.as_deref()),
            Some("帮我看看A股")
        );
        let prompts = store_quick_prompts(None, Some(meta));
        assert_eq!(prompts[0].zh.as_deref(), Some("今天行情如何"));
        let default_prompt = store_default_init_prompt(None, Some(meta));
        assert_eq!(
            default_prompt.as_ref().and_then(|prompt| prompt.zh.as_deref()),
            Some("帮我看看A股")
        );

        let entry = remote_entry("a-share-analysis");
        let facts = entry_facts_at(&entry, dir.path().join("missing"), &parsed);
        assert_eq!(facts.kind, "team", "index row kind overrides the directory guess");
        assert_eq!(facts.version, "1.2.3");
        assert_eq!(facts.index_info.as_ref().unwrap().name_zh.as_deref(), Some("A股研究团队"));
    }

    #[test]
    fn unavailable_index_row_is_kept_with_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        let index = serde_json::json!({
            "schema_version": 2,
            "market": "experts",
            "entries": [{
                "name": "blocked",
                "kind": "plugin",
                "download": "https://cdn.example/b.tar.gz",
                "sha256": "b".repeat(64),
                "size": 1,
                "available": false,
                "unavailable_reason": "上游已下架"
            }]
        });
        std::fs::write(dir.path().join("index.json"), index.to_string()).unwrap();
        let meta = MarketIndex::read(dir.path()).entry_meta("blocked").cloned().expect("row");
        assert!(!meta.available);
        assert_eq!(meta.unavailable_reason.as_deref(), Some("上游已下架"));
        let facts = entry_facts_at(&remote_entry("blocked"), dir.path().join("missing"), &MarketIndex::read(dir.path()));
        assert_eq!(facts.kind, "agent", "`plugin` maps onto the store's agent kind");
    }
}
