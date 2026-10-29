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
    AppServerMarketplaceSummary,
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
    /// The market's own `publishedAt`, normalized to `YYYY-MM-DD` (doc `18`
    /// §3). Dropped unless it is exactly a calendar date: this is third-party
    /// data, and a bad date must not cost the entry its listing — an entry that
    /// cannot be listed cannot be installed either. So the stored value always
    /// means "the market declared this date", never "the market declared
    /// something".
    pub published_at: Option<String>,
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

/// The manifest row's own `publishedAt`, normalized to `YYYY-MM-DD`.
///
/// Read once, where the other per-entry manifest fields are read, so every
/// probe agrees on what counts: a calendar date, or nothing. See
/// [`ScannedEntry::published_at`] for why a bad value is dropped instead of
/// failing the row.
pub(crate) fn published_at_of(item: &serde_json::Value) -> Option<String> {
    let raw = item.get("publishedAt").and_then(|value| value.as_str())?.trim();
    is_calendar_date(raw).then(|| raw.to_owned())
}

/// `YYYY-MM-DD` with a real month/day pair, checked without a date crate: the
/// manifest value is a *calendar date*, not an instant, and `18` §3 defines the
/// field that way (an RFC 3339 timestamp is a different value and is dropped).
fn is_calendar_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        value[0..4].parse::<u32>(),
        value[5..7].parse::<u32>(),
        value[8..10].parse::<u32>(),
    ) else {
        return false;
    };
    if year == 0 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max_day).contains(&day)
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
                // A plain collection has no manifest row, hence no
                // `publishedAt` either.
                published_at: None,
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
        published_at: None,
        localized: BTreeMap::new(),
        strict: false,
        blocked_reason: None,
    }
}

/// Download, verify and extract one `index` entry archive under `cache`.
///
/// Split from the provider so the integrity/extraction sequence is testable
/// without a database or importer: the cached archive name is content-addressed
/// by the index sha, and a corrupt/mismatched archive is deleted rather than
/// reused on the next attempt.
async fn materialize_remote_archive(
    cache: &Path,
    entry_name: &str,
    download: &str,
    sha256: &str,
    size: i64,
) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(cache)
        .map_err(|error| AppError::Internal(format!("create entry cache: {error}")))?;
    let stem = sanitize_slug(entry_name);
    let archive = cache.join(format!("{stem}-{sha256}.tar.gz"));
    if !archive.is_file() {
        let (downloaded, server_digest) = crate::market_source::download_archive(download, &archive)
            .await
            .map_err(AppError::Internal)?;
        if let Some(server) = server_digest {
            if server != sha256 {
                let _ = std::fs::remove_file(&archive);
                return Err(AppError::Internal(format!(
                    "entry {entry_name} digest mismatch: index declares {sha256}, host reports {server}"
                )));
            }
        }
        if downloaded != sha256 {
            let _ = std::fs::remove_file(&archive);
            return Err(AppError::Internal(format!(
                "entry {entry_name} downloaded bytes hash to {downloaded}, index declares {sha256}"
            )));
        }
    }
    let actual = crate::market_source::sha256_file(&archive).map_err(AppError::Internal)?;
    if actual != sha256 {
        let _ = std::fs::remove_file(&archive);
        return Err(AppError::Internal(format!(
            "entry {entry_name} archive hash {actual} does not match index sha256 {sha256}"
        )));
    }
    let length = std::fs::metadata(&archive)
        .map_err(|error| AppError::Internal(format!("stat entry archive: {error}")))?
        .len();
    if length != size as u64 {
        let _ = std::fs::remove_file(&archive);
        return Err(AppError::Internal(format!(
            "entry {entry_name} archive is {length} bytes, index declares {size}"
        )));
    }
    let extracted = cache.join(format!("{stem}-{sha256}"));
    if !extracted.is_dir() {
        crate::market_source::extract_tar_gz_market(&archive, &extracted)
            .map_err(AppError::Internal)?;
    }
    Ok(resolve_extracted_root(extracted))
}

/// The icon extension for a URL path, restricted to the importer's whitelist.
fn icon_extension(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or("");
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "png",
        "svg" => "svg",
        "jpg" => "jpg",
        "jpeg" => "jpeg",
        "webp" => "webp",
        "gif" => "gif",
        _ => "png",
    }
}

fn resolve_extracted_root(extracted: std::path::PathBuf) -> std::path::PathBuf {
    let is_payload = |path: &Path| {
        path.join(".codebuddy-plugin/plugin.json").is_file()
            || path.join("SKILL.md").is_file()
            || path.join("cli.json").is_file()
    };
    if is_payload(&extracted) {
        return extracted;
    }
    // The published experts archives wrap the plugin in `<plugin-name>/`.
    // Descend only when exactly one child is a payload — two candidates mean
    // the archive's layout is ambiguous and guessing would pick wrong.
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&extracted)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && is_payload(path))
                .collect()
        })
        .unwrap_or_default();
    if candidates.len() == 1 {
        candidates.remove(0)
    } else {
        extracted
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
            published_at: published_at_of(item),
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
            published_at: published_at_of(item),
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
            published_at: published_at_of(item),
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

    /// Materialize one `remote` (index-market) entry: download its `tar.gz`,
    /// verify the index-declared sha256/size, and extract it under
    /// `{market_root}/{marketplace_id}/cache/`.
    ///
    /// The published `experts` archives wrap the plugin in a top-level
    /// `<plugin-name>/` directory (e.g. `accessibility-auditor/.codebuddy-plugin/plugin.json`),
    /// so the extracted directory is descended one level when the payload root
    /// is not itself a plugin. Returning the wrapper instead would make the
    /// kind derivation and the importer look for manifests in the wrong place.
    async fn materialize_remote_entry(
        &self,
        row: &PluginMarketplaceRow,
        entry: &MarketplaceEntry,
        index: &crate::app_server_store::MarketIndex,
    ) -> Result<std::path::PathBuf, AppError> {
        let meta = index.entry_meta(&entry.name).ok_or_else(|| {
            AppError::BadRequest(format!(
                "index market {} has no row for entry {}",
                row.marketplace_id, entry.name
            ))
        })?;
        let sha256 = meta
            .entry_sha256()
            .ok_or_else(|| AppError::BadRequest(format!("entry {} has no sha256", entry.name)))?;
        let size = meta
            .entry_size()
            .ok_or_else(|| AppError::BadRequest(format!("entry {} has no size", entry.name)))?;
        if !entry.source_uri.starts_with("https://") {
            return Err(AppError::BadRequest(format!(
                "entry {} download must be an absolute https URL",
                entry.name
            )));
        }
        let cache = self.market_root.join(&row.marketplace_id).join("cache");
        materialize_remote_archive(&cache, &entry.name, &entry.source_uri, &sha256, size).await
    }

    /// Download an `index` market's absolute `icon_url` into the entry cache so
    /// the importer can stamp it into the snapshot (`51` §3.2).
    ///
    /// Best-effort: a missing/broken icon must not fail an otherwise valid
    /// install — the card simply falls back to the market icon or initials.
    async fn materialize_remote_icon(
        &self,
        row: &PluginMarketplaceRow,
        entry: &MarketplaceEntry,
        index: &crate::app_server_store::MarketIndex,
    ) -> Option<std::path::PathBuf> {
        let url = index.entry_meta(&entry.name)?.entry_icon_url()?;
        if !url.starts_with("https://") {
            return None;
        }
        let dir = self.market_root.join(&row.marketplace_id).join("cache/icons");
        let dest = dir.join(format!(
            "{}.{}",
            sanitize_slug(&entry.name),
            icon_extension(url)
        ));
        if dest.is_file() {
            return Some(dest);
        }
        match crate::market_source::download_small_file(url, &dest, 64 * 1024).await {
            Ok(()) => Some(dest),
            Err(_) => None,
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

/// [`MarketplaceProvider::register_unfetched`] against the registry seam
/// directly: the rule lives here so it is testable without the importer +
/// installer + market-root graph the provider itself drags in.
///
/// Non-destructive by construction — two pre-checks, in the order the two
/// UNIQUE constraints on the table can fire:
///
/// 1. **Same id.** Returned as-is whether it holds a downloaded catalog or is a
///    soft-removed row: a restart must not clear the first, and it must not
///    resurrect the second behind the user's back (which is what a
///    reactivating `add` does).
/// 2. **Same source under another id** (the host re-pointed its default ids, or
///    the operator added this archive by hand). A second row would just
///    duplicate the catalog under a second name.
async fn register_unfetched_row(
    markets: &Arc<dyn IMarketplaceRepository>,
    marketplace_id: &str,
    name: &str,
    source_kind: &str,
    source: &str,
) -> Result<AppServerMarketplaceSummary, AppError> {
    if let Some(existing) = markets
        .get_marketplace(marketplace_id)
        .await
        .map_err(AppError::from)?
    {
        return Ok(to_summary(&existing));
    }
    if let Some(existing) = markets
        .find_by_source(source_kind, source)
        .await
        .map_err(AppError::from)?
    {
        return Ok(to_summary(&existing));
    }
    markets
        .insert_marketplace(NewPluginMarketplace {
            marketplace_id,
            name,
            description: None,
            source_kind,
            source_uri: source,
            owner_json: None,
            version: None,
            content_digest: None,
            // No entries: this is the whole point — "registered" is a registry
            // fact, not a download.
            entries: Vec::new(),
            // Deliberately *not* `is_official_source`: all three builtin
            // sources are official, so the sweep would fetch their 324 MiB on
            // the first tick of any host that declared `[marketplace]
            // auto_update_interval_hours` — exactly the download the lazy
            // default exists to avoid. "Download this now" is one decision;
            // "keep re-downloading it" is a separate one the user makes on the
            // market's own switch.
            auto_update: false,
        })
        .await
        .map_err(AppError::from)?;
    let row = markets
        .get_marketplace(marketplace_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| {
            AppError::Internal(format!("marketplace {marketplace_id} missing after insert"))
        })?;
    Ok(to_summary(&row))
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
                published_at: entry.published_at,
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
                    is_official_source(source_kind, &request.source),
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

    async fn register_unfetched(
        &self,
        marketplace_id: &str,
        name: &str,
        source_kind: &str,
        source: &str,
    ) -> Result<AppServerMarketplaceSummary, AppError> {
        register_unfetched_row(&self.markets, marketplace_id, name, source_kind, source).await
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
                    Ok(status) => {
                        // Only what actually lost its registration counts. A
                        // release that failed keeps its install record and its
                        // artifact, so listing it here would overstate the
                        // cascade — and `status` reports the whole snapshot, so
                        // the selection has to be intersected back in to avoid
                        // counting components that were never installed.
                        let released: Vec<String> = status
                            .components
                            .iter()
                            .filter(|component| {
                                component.state.as_str() == "not-installed"
                                    && installed_components.contains(&component.id)
                            })
                            .map(|component| component.id.clone())
                            .collect();
                        if released.len() != installed_components.len() {
                            result.warnings.push(format!(
                                "cascade uninstall {}: released {} of {} components",
                                snapshot.snapshot_id,
                                released.len(),
                                installed_components.len()
                            ));
                        }
                        result.uninstalled_components.extend(released);
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
                    published_at: entry.published_at,
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
        //
        // The same resolution is what `store/list` shows, so it goes through the
        // one derivation (`36` D2) and the version it yields is the version this
        // import is stored under — a bare `SKILL.md` / `mcp.json` entry parses to
        // `1.0.0`, which would otherwise pin every skill and connector to one
        // identity and turn any content change into a digest conflict.
        let index = crate::app_server_store::market_root_dir(&row, &self.market_root)
            .map(|root| crate::app_server_store::MarketIndex::read(&root))
            .unwrap_or_default();
        // Doc 30 §6.1: an index row can explicitly mark an entry unavailable.
        // The catalog keeps it visible with the reason, but install must not
        // download a payload the market says should not be installed.
        if entry.source_kind == "remote" {
            if let Some(meta) = index.entry_meta(&entry.name) {
                if !meta.entry_available() {
                    let reason = meta
                        .entry_unavailable_reason()
                        .unwrap_or("市场标记该条目不可用")
                        .to_owned();
                    let version = meta
                        .entry_version()
                        .or(entry.version.as_deref())
                        .unwrap_or("0")
                        .to_owned();
                    return Ok(nomifun_importer::blocked_import_result(
                        SourceKind::CodeBuddyPlugin,
                        Some((entry.name.clone(), version)),
                        &[reason],
                    ));
                }
            }
        }
        // Doc 30 §6.1: an `index` market ships only `index.json`; the entry's
        // `tar.gz` is downloaded (and integrity-checked) at install time.
        let source = if entry.source_kind == "remote" {
            self.materialize_remote_entry(&row, &entry, &index).await?
        } else {
            crate::market_fetch::entry_source_path(
                &row.source_kind,
                &row.source_uri,
                &self.market_root,
                marketplace_id,
                &entry.source_uri,
            )
        };
        let facts = crate::app_server_store::entry_facts_at(&entry, source.clone(), &index);

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
            let manifest = &facts.manifest;
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

        // The market's icon for this entry, handed to the importer as a plain
        // absolute path (doc `51` §3.2 / D5). Resolved here because only the
        // marketplace layer knows the market root; resolved through the *same*
        // function `store/list` uses for the card, so the picture on the card and
        // the picture in the snapshot can never disagree.
        let market_icon = if entry.source_kind == "remote" {
            // Index markets keep the icon at an absolute URL; bring it local so
            // the importer can place it in the immutable snapshot.
            self.materialize_remote_icon(&row, &entry, &index).await
        } else {
            crate::app_server_store::market_root_dir(&row, &self.market_root).and_then(|root| {
                crate::app_server_store::market_icon_for(&root, &entry.source_uri)
                    .map(|icon| root.join(icon))
            })
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
        }
        // The version the market advertises (`36` D2). Without it, skills and
        // connectors would be stored at the `1.0.0` placeholder while
        // `store/list` shows the index version — an update that can never be
        // delivered, and a content change that collides with the snapshot
        // already stored under `1.0.0`.
        //
        // The index row's localized tags ride along (`44`): the only tag
        // source for skill/connector entries, stamped into the component
        // payload so the installed-skill catalog shows what the store shows.
        // Plugin imports keep their `plugin.json` tags.
        .with_declared_version(Some(facts.version.clone()))
        .with_index_display(facts.index_info.as_ref().map(|info| info.display()))
        .with_market_icon(market_icon);
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

    /// The lazy default-marketplace registration: a registry row with no
    /// entries, no revision and no auto-update — written without touching the
    /// network — and never allowed to disturb a row that is already there.
    #[tokio::test]
    async fn unfetched_registration_stores_a_source_without_downloading_it() {
        let db = nomifun_db::init_database_memory().await.expect("in-memory db");
        let markets: Arc<dyn IMarketplaceRepository> =
            Arc::new(nomifun_db::SqliteMarketplaceRepository::new(db.pool().clone()));
        let (id, kind, source) =
            nomifun_app_server::AgentStoreConfig::builtin_default_marketplaces()
                .into_iter()
                .next()
                .expect("builtin mirrors are configured");

        // 1. Registered, with nothing behind it.
        let summary = register_unfetched_row(&markets, &id, &id, &kind, &source)
            .await
            .expect("register");
        assert_eq!(summary.marketplace_id, id);
        assert_eq!(summary.source_kind, kind);
        assert_eq!(summary.entry_count, 0);
        assert!(summary.resolved_revision.is_none());
        assert!(
            !summary.auto_update,
            "an unfetched official market must not be swept before it is ever downloaded"
        );
        let row = markets.get_marketplace(&id).await.unwrap().expect("row");
        assert!(
            !is_auto_update_eligible(&row),
            "the sweep would otherwise re-download the archive the lazy default exists to defer"
        );

        // 2. Idempotent: the next boot does not insert a second row.
        register_unfetched_row(&markets, &id, &id, &kind, &source)
            .await
            .expect("re-register");
        assert_eq!(markets.list_marketplaces().await.unwrap().len(), 1);

        // 3. Non-destructive: once the user has downloaded it, a restart keeps
        //    the catalog (a reactivating `add` would clear both fields).
        let entry = MarketplaceEntry {
            name: "formatter".to_owned(),
            source_kind: "directory".to_owned(),
            source_uri: "./plugins/formatter".to_owned(),
            version: None,
            description: None,
            keywords: vec![],
            category: None,
            published_at: None,
            localized: std::collections::BTreeMap::new(),
            strict: false,
            blocked_reason: None,
        };
        markets
            .update_marketplace_entries(&id, &[entry], "digest", None)
            .await
            .unwrap();
        markets
            .record_resolved_revision(&id, "rev-1", "/tmp/live")
            .await
            .unwrap();
        register_unfetched_row(&markets, &id, &id, &kind, &source)
            .await
            .expect("re-register");
        let row = markets.get_marketplace(&id).await.unwrap().expect("row");
        assert_eq!(row.entries().len(), 1);
        assert_eq!(row.resolved_revision.as_deref(), Some("rev-1"));

        // 4. The same source under a different id does not duplicate the
        //    catalog under a second name.
        let alias = register_unfetched_row(&markets, "experts-mirror", "experts-mirror", &kind, &source)
            .await
            .expect("alias");
        assert_eq!(alias.marketplace_id, id);
        assert_eq!(markets.list_marketplaces().await.unwrap().len(), 1);

        // 5. A removed market stays removed. This registration runs on **every**
        //    boot, so resurrecting here would make a removal impossible to keep.
        markets
            .soft_remove_marketplace(&id, nomifun_common::now_ms())
            .await
            .unwrap();
        register_unfetched_row(&markets, &id, &id, &kind, &source)
            .await
            .expect("re-register after removal");
        assert!(markets.list_marketplaces().await.unwrap().is_empty());
        assert!(
            markets
                .get_marketplace(&id)
                .await
                .unwrap()
                .expect("row kept")
                .removed_at
                .is_some()
        );
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

    /// `18` §3: the manifest field is a *calendar date*. A timestamp — what an
    /// RSS-style feed would carry — is a different value and is dropped rather
    /// than truncated to its date part.
    #[test]
    fn published_at_accepts_calendar_dates_only() {
        for good in ["2026-07-30", "2024-02-29", "2000-02-29", "1999-12-31"] {
            assert!(is_calendar_date(good), "{good} must be accepted");
        }
        for bad in [
            "",
            "2026-7-30",
            "2026-07-3",
            "2026-13-01",
            "2026-00-10",
            "2025-02-29",
            "1900-02-29",
            "2026-04-31",
            "0000-01-01",
            "2026-07-30T00:00:00Z",
            "2026/07/30",
            "abcd-ef-gh",
        ] {
            assert!(!is_calendar_date(bad), "{bad} must be refused");
        }
    }

    /// Only a declared `publishedAt` that is a calendar date survives; absent,
    /// mistyped and malformed values all become `None` instead of costing the
    /// row its listing (an entry that cannot be listed cannot be installed).
    #[test]
    fn published_at_of_reads_only_a_declared_calendar_date() {
        let read = |value: serde_json::Value| published_at_of(&value);
        assert_eq!(
            read(serde_json::json!({ "publishedAt": "2026-07-30" })).as_deref(),
            Some("2026-07-30")
        );
        assert_eq!(
            read(serde_json::json!({ "publishedAt": "  2026-07-30  " })).as_deref(),
            Some("2026-07-30"),
            "surrounding whitespace is trimmed; the value is not reinterpreted"
        );
        assert_eq!(read(serde_json::json!({})), None);
        assert_eq!(read(serde_json::json!({ "publishedAt": null })), None);
        assert_eq!(read(serde_json::json!({ "publishedAt": 20260730 })), None);
        assert_eq!(read(serde_json::json!({ "publishedAt": "2026-07-30T00:00:00Z" })), None);
        assert_eq!(
            read(serde_json::json!({ "published_at": "2026-07-30" })),
            None,
            "the manifest field is camelCase, like `strict` and `source`"
        );
    }

    /// The probe reads the field where it reads the rest of the manifest row, so
    /// it reaches the stored projection — and therefore `store/list`.
    #[test]
    fn probe_plugin_market_carries_declared_published_at() {
        let dir = std::env::temp_dir().join(format!("as-mkt-pub-{}", nomifun_common::generate_id()));
        write(
            &dir.join(".codebuddy-plugin/marketplace.json"),
            r#"{
                "name": "experts",
                "plugins": [
                    { "name": "dated", "source": "./plugins/dated", "publishedAt": "2026-07-30" },
                    { "name": "undated", "source": "./plugins/undated" },
                    { "name": "broken", "source": "./plugins/broken", "publishedAt": "30/07/2026" }
                ]
            }"#,
        );
        for name in ["dated", "undated", "broken"] {
            write(
                &dir.join(format!("plugins/{name}/.codebuddy-plugin/plugin.json")),
                r#"{ "name": "demo" }"#,
            );
        }
        let (_, entries) = probe_directory(&dir).unwrap();
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .expect("entry must be listed")
                .clone()
        };
        assert_eq!(by_name("dated").published_at.as_deref(), Some("2026-07-30"));
        assert_eq!(by_name("undated").published_at, None);
        let broken = by_name("broken");
        assert_eq!(broken.published_at, None, "a malformed date is dropped");
        assert!(broken.blocked_reason.is_none(), "and the entry stays installable");
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
            published_at: None,
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
            published_at: None,
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

#[cfg(test)]
mod extracted_root_tests {
    use super::resolve_extracted_root;

    fn write_plugin(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join(".codebuddy-plugin")).unwrap();
        std::fs::write(dir.join(".codebuddy-plugin/plugin.json"), "{}").unwrap();
    }

    #[test]
    fn descends_a_single_top_level_plugin_directory() {
        let temp = tempfile::tempdir().unwrap();
        let wrapper = temp.path().join("payload");
        write_plugin(&wrapper.join("accessibility-auditor"));
        assert_eq!(
            resolve_extracted_root(wrapper.clone()),
            wrapper.join("accessibility-auditor")
        );
    }

    #[test]
    fn keeps_the_root_for_a_direct_plugin_payload() {
        let temp = tempfile::tempdir().unwrap();
        let payload = temp.path().join("payload");
        write_plugin(&payload);
        assert_eq!(resolve_extracted_root(payload.clone()), payload);
    }

    #[test]
    fn ambiguous_wrappers_are_not_guessed() {
        let temp = tempfile::tempdir().unwrap();
        let wrapper = temp.path().join("payload");
        write_plugin(&wrapper.join("one"));
        write_plugin(&wrapper.join("two"));
        assert_eq!(resolve_extracted_root(wrapper.clone()), wrapper);
    }
}

#[cfg(test)]
mod icon_name_tests {
    use super::icon_extension;

    #[test]
    fn icon_extension_uses_the_url_path_and_falls_back_to_png() {
        assert_eq!(icon_extension("https://cdn/x/a.webp"), "webp");
        assert_eq!(icon_extension("https://cdn/x/a.SVG?x=1"), "svg");
        assert_eq!(icon_extension("https://cdn/x/icon"), "png");
        assert_eq!(icon_extension("https://cdn/x/a.exe"), "png");
    }
}

#[cfg(test)]
mod archive_materialization_tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn archive_with_wrapped_plugin() -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let encoder = GzEncoder::new(&mut tar_bytes, Compression::fast());
            let mut builder = tar::Builder::new(encoder);
            let payload = br#"{"name":"demo"}"#;
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(0);
            builder
                .append_data(&mut header, "demo/.codebuddy-plugin/plugin.json", &payload[..])
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        tar_bytes
    }

    #[tokio::test]
    async fn download_verify_extract_and_descend_the_wrapper() {
        let bytes = archive_with_wrapped_plugin();
        let sha = crate::market_source::sha256_hex_bytes(&bytes);
        let size = bytes.len() as i64;
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&mock)
            .await;
        let cache = tempfile::tempdir().unwrap();
        let root = materialize_remote_archive(
            cache.path(),
            "demo",
            &format!("{}/a.tar.gz", mock.uri()),
            &sha,
            size,
        )
        .await
        .expect("materialize");
        assert!(root.join(".codebuddy-plugin/plugin.json").is_file());
        assert_eq!(root.file_name().unwrap(), "demo");
    }

    #[tokio::test]
    async fn a_size_mismatch_deletes_the_archive_and_fails() {
        let bytes = archive_with_wrapped_plugin();
        let sha = crate::market_source::sha256_hex_bytes(&bytes);
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&mock)
            .await;
        let cache = tempfile::tempdir().unwrap();
        let result = materialize_remote_archive(
            cache.path(),
            "demo",
            &format!("{}/a.tar.gz", mock.uri()),
            &sha,
            1,
        )
        .await;
        assert!(result.is_err());
        assert!(!cache.path().join(format!("demo-{sha}.tar.gz")).exists());
    }

    #[tokio::test]
    async fn a_hash_mismatch_never_extracts() {
        let bytes = archive_with_wrapped_plugin();
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&mock)
            .await;
        let cache = tempfile::tempdir().unwrap();
        let result = materialize_remote_archive(
            cache.path(),
            "demo",
            &format!("{}/a.tar.gz", mock.uri()),
            &"0".repeat(64),
            1,
        )
        .await;
        assert!(result.is_err());
        assert!(!cache.path().join(format!("demo-{}.tar.gz", "0".repeat(64))).exists());
    }
}
