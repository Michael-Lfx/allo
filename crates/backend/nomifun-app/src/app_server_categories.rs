//! Category dictionary reader for the store faces (`43` M4, `44` §5).
//!
//! Two sources, in strict precedence:
//!
//! 1. **The market's own index** — an experts market ships a top-level
//!    `manifest.json` next to its entry directories (`$schema` / `version` /
//!    `paths.experts` / `categories[]`), where each category is
//!    `{id, name: {en, zh}, description: {en, zh}}` and a plugin's
//!    `plugin.json` references one by `categoryId`. When a market ships one, it
//!    is authoritative — nobody knows its vocabulary better than the market.
//! 2. **The built-in fallback** ([`BUILTIN_CATEGORIES`]) — and this is not a
//!    convenience: the official archives are built from the WorkBuddy
//!    **marketplace** tree (`plugins/` + `.codebuddy-plugin/marketplace.json`),
//!    while the category vocabulary lives in a *different* WorkBuddy subsystem
//!    (its app-side catalog index). The published `experts.zip` carries no
//!    `manifest.json` at all — probed 2026-10-14: its central directory lists
//!    14,713 entries under two root names, `.codebuddy-plugin/` and `plugins/`
//!    — so without a fallback every real `categoryId` would stay unresolved.
//!    **This constant is the mechanism, not a stopgap**: the project decided
//!    against republishing the market archives to carry a dictionary that only
//!    this host needs (doc `44` §6.3). The vocabulary is a closed set — probed
//!    the same day: all 448 rows of the upstream catalog reference one of its 15
//!    declared ids, and 377 of the archive's 381 plugins appear in that same
//!    catalog. A market that *does* ship a dictionary still wins (source 1).
//!
//! The dictionary is pure display metadata: a missing or malformed file is an
//! *empty* dictionary, never an error — the raw `categoryId` on an entry stays
//! honest whether or not anything resolves it.
//!
//! Caching (`43` §3.3): the parsed result is memoized per **file identity**
//! (path + mtime + length), not merely per marketplace. The market sweep
//! refreshes a live root in place, so keying on the path alone would serve a
//! stale dictionary forever; a changed file yields a changed stamp and is
//! re-parsed. Nothing here is persisted: no table, no cache file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Deserialize;

use nomifun_api_types::{AppServerLocalizedText, AppServerStoreCategoryItem};
use nomifun_common::AppError;

/// The built-in fallback vocabulary, verbatim from the upstream catalog index.
///
/// Kept as data (not hand-written Rust) so the transcription cannot drift, and
/// it carries its own provenance (`_source*` keys) for whoever reads it next.
const BUILTIN_CATEGORIES: &str = include_str!("app_server_categories_builtin.json");

/// One `categories[]` row of an experts manifest, as the source spells it.
#[derive(Debug, Deserialize)]
struct SourceCategory {
    id: String,
    #[serde(default)]
    name: SourceLocalized,
    #[serde(default)]
    description: Option<SourceLocalized>,
}

/// The source's locale map (`{"en": "...", "zh": "..."}`).
#[derive(Debug, Default, Deserialize)]
struct SourceLocalized {
    #[serde(default)]
    en: Option<String>,
    #[serde(default)]
    zh: Option<String>,
}

impl SourceLocalized {
    fn to_public(&self) -> AppServerLocalizedText {
        AppServerLocalizedText { en: self.en.clone(), zh: self.zh.clone() }
    }
}

/// A cheap file identity: what we last parsed, and whether it still matches.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl FileStamp {
    fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self { modified: metadata.modified().ok(), len: metadata.len() })
    }
}

/// One memoized parse: the stamp it was parsed from, plus the payload.
struct Memo<T> {
    stamp: FileStamp,
    value: T,
}

/// In-process cache of parsed dictionaries and tag vocabularies.
///
/// Keyed by the file actually read, so two markets sharing a mirror path do not
/// parse twice, and a refresh is picked up by its new stamp. Lives for the
/// process lifetime beside the store provider.
#[derive(Default)]
pub struct CategoryDictionary {
    /// Root per market, learned while the store walks the enabled markets.
    roots: Mutex<HashMap<String, PathBuf>>,
    categories: Mutex<HashMap<PathBuf, Memo<Vec<AppServerStoreCategoryItem>>>>,
    tags: Mutex<HashMap<PathBuf, Memo<Vec<AppServerLocalizedText>>>>,
}

impl CategoryDictionary {
    /// Point the reader at the **currently enabled** markets' source roots.
    ///
    /// Replaces the previous set instead of accumulating: the caller walks the
    /// enabled markets, so one that was disabled or removed since must stop
    /// contributing its dictionary. Cheap (a registry read, no per-entry work) —
    /// which is exactly why `store/categories` can call it itself rather than
    /// depending on a prior `store/list`. That dependency was a real defect: a
    /// client whose first call is the filter-bar call got the built-in dictionary
    /// and an empty tag vocabulary (probed 2026-10-14: `marketplace_tags` was `0`
    /// before a `store/list` and `11` after it).
    pub fn set_roots(&self, roots: Vec<(String, PathBuf)>) {
        let mut map = self.roots.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        map.clear();
        for (marketplace_id, root) in roots {
            map.insert(marketplace_id, root);
        }
    }

    fn roots(&self) -> Vec<PathBuf> {
        let map = self.roots.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        map.values().cloned().collect()
    }

    /// The category rows to serve: the declared markets' dictionaries when any
    /// market ships one, otherwise [`BUILTIN_CATEGORIES`].
    ///
    /// Deliberately **all-or-nothing** rather than merged: a market that ships
    /// its own vocabulary is authoritative, and mixing two vocabularies would
    /// invite duplicate ids with different names.
    pub async fn load(&self) -> Result<Vec<AppServerStoreCategoryItem>, AppError> {
        let mut declared = Vec::new();
        for root in self.roots() {
            // The manifest sits at the market root (the real archive layout),
            // or one level above the entry directories in some local trees —
            // both spellings are tried, source order first.
            for candidate in [
                Some(root.join("manifest.json")),
                root.parent().map(|p| p.join("manifest.json")),
            ]
            .into_iter()
            .flatten()
            {
                if !candidate.is_file() {
                    continue;
                }
                declared.extend(self.categories_at(&candidate));
                break;
            }
        }
        if declared.is_empty() {
            return Ok(Self::builtin());
        }
        Ok(declared)
    }

    /// The market-level tag vocabulary: a `.codebuddy-skill/marketplace.json`
    /// top-level `tags_zh`/`tags_en` pair, position-paired (`43` §3.1). Only
    /// fully paired rows travel — an unmatched locale would render a half label
    /// in a filter list. No fallback: this vocabulary is genuinely the market's
    /// own, and the official skills archive does ship it.
    pub async fn load_marketplace_tags(&self) -> Vec<AppServerLocalizedText> {
        let mut out = Vec::new();
        for root in self.roots() {
            let index = root.join(".codebuddy-skill").join("marketplace.json");
            if !index.is_file() {
                continue;
            }
            out.extend(self.tags_at(&index));
        }
        out
    }

    /// One manifest's categories, memoized by file stamp.
    fn categories_at(&self, path: &Path) -> Vec<AppServerStoreCategoryItem> {
        let Some(stamp) = FileStamp::read(path) else {
            return Vec::new();
        };
        if let Some(memo) = self
            .categories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
        {
            if memo.stamp == stamp {
                return memo.value.clone();
            }
        }
        let parsed = read_categories(path);
        self.categories
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.to_path_buf(), Memo { stamp, value: parsed.clone() });
        parsed
    }

    /// One skills index's tag vocabulary, memoized by file stamp.
    fn tags_at(&self, path: &Path) -> Vec<AppServerLocalizedText> {
        let Some(stamp) = FileStamp::read(path) else {
            return Vec::new();
        };
        if let Some(memo) = self.tags.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(path) {
            if memo.stamp == stamp {
                return memo.value.clone();
            }
        }
        let parsed = read_marketplace_tags(path);
        self.tags
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.to_path_buf(), Memo { stamp, value: parsed.clone() });
        parsed
    }

    /// Parse [`BUILTIN_CATEGORIES`]; a malformed constant yields an empty
    /// dictionary rather than a panic (the same rule every other read follows).
    fn builtin() -> Vec<AppServerStoreCategoryItem> {
        parse_categories(BUILTIN_CATEGORIES)
    }
}

/// Parse one manifest file's `categories`; any failure is an empty vector.
fn read_categories(manifest: &Path) -> Vec<AppServerStoreCategoryItem> {
    match std::fs::read_to_string(manifest) {
        Ok(text) => parse_categories(&text),
        Err(_) => Vec::new(),
    }
}

/// Parse the `categories` array out of a manifest's text.
fn parse_categories(text: &str) -> Vec<AppServerStoreCategoryItem> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Ok(categories) = serde_json::from_value::<Vec<SourceCategory>>(
        value.get("categories").cloned().unwrap_or_default(),
    ) else {
        return Vec::new();
    };
    categories
        .into_iter()
        .map(|category| AppServerStoreCategoryItem {
            id: category.id,
            name: category.name.to_public(),
            description: category.description.map(|text| text.to_public()),
        })
        .collect()
}

/// Read one `.codebuddy-skill/marketplace.json`'s market-level tag vocabulary.
fn read_marketplace_tags(index: &Path) -> Vec<AppServerLocalizedText> {
    let Ok(text) = std::fs::read_to_string(index) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let strings = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(|v| v.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let (zh, en) = (strings("tags_zh"), strings("tags_en"));
    zh.into_iter()
        .zip(en)
        .map(|(zh, en)| AppServerLocalizedText { en: Some(en), zh: Some(zh) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_vocabulary_is_the_verbatim_upstream_set() {
        let items = CategoryDictionary::builtin();
        assert_eq!(items.len(), 15, "the upstream catalog declares 15 categories");
        // The id a real plugin.json uses (`plugins/stock-partner-team` declares
        // `categoryId: "08-FinanceInvestment"`), and its localized names.
        let finance = items.iter().find(|c| c.id == "08-FinanceInvestment").expect("id present");
        assert_eq!(finance.name.zh.as_deref(), Some("金融投资"));
        assert_eq!(finance.name.en.as_deref(), Some("Finance Investment"));
        assert!(items.iter().all(|c| c.name.zh.is_some() && c.name.en.is_some()));
        // Ids are the upstream `NN-Name` spelling, never normalized by us.
        assert!(items.iter().all(|c| c.id.chars().next().is_some_and(|c| c.is_ascii_digit())));
    }

    #[test]
    fn a_market_dictionary_parses_and_an_unreadable_one_is_empty() {
        let parsed = parse_categories(
            r#"{"categories":[{"id":"04-DataAI","name":{"en":"Data & AI","zh":"数据智能"}}]}"#,
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name.zh.as_deref(), Some("数据智能"));
        assert!(parsed[0].description.is_none(), "an absent description stays absent");

        assert!(parse_categories("not json").is_empty());
        assert!(parse_categories(r#"{"categories":"nope"}"#).is_empty());
        assert!(parse_categories(r#"{"other":1}"#).is_empty());
    }

    #[test]
    fn the_market_tag_vocabulary_pairs_by_position_and_drops_half_rows() {
        let dir = std::env::temp_dir().join(format!("tags-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("marketplace.json");
        std::fs::write(
            &index,
            r#"{"tags_zh":["办公协同","开发工具","落单"],"tags_en":["Collaboration","Development Tools"]}"#,
        )
        .unwrap();
        let tags = read_marketplace_tags(&index);
        assert_eq!(tags.len(), 2, "the unpaired zh row is dropped, not half-rendered");
        assert_eq!(tags[0].zh.as_deref(), Some("办公协同"));
        assert_eq!(tags[0].en.as_deref(), Some("Collaboration"));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("categories-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The precedence rule: a market that ships a dictionary is authoritative
    /// wholesale; the built-in vocabulary answers only when none does. This is
    /// what makes a real `categoryId` resolve on the published archives (which
    /// ship no dictionary) without ever mixing two vocabularies.
    #[tokio::test]
    async fn a_declared_market_wins_and_the_builtin_covers_its_absence() {
        let dict = CategoryDictionary::default();

        // No market remembers a root: the built-in set answers.
        let items = dict.load().await.unwrap();
        assert_eq!(items.len(), 15);
        // The id a real plugin.json declares (`plugins/stock-partner-team`).
        let finance = items.iter().find(|c| c.id == "08-FinanceInvestment").expect("resolves");
        assert_eq!(finance.name.zh.as_deref(), Some("金融投资"));

        // A market that declares its own dictionary replaces it entirely — not
        // merged, so an id can never carry two different names.
        let dir = scratch("market");
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"categories":[{"id":"99-Custom","name":{"zh":"自定义","en":"Custom"}}]}"#,
        )
        .unwrap();
        dict.set_roots(vec![("demo-market".to_owned(), dir.clone())]);
        let items = dict.load().await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "99-Custom");
        assert!(
            !items.iter().any(|c| c.id == "08-FinanceInvestment"),
            "the builtin set must not leak into a market that declares its own"
        );

        // Disabling or removing that market drops its dictionary again: the root
        // set is replaced wholesale, never accumulated.
        dict.set_roots(Vec::new());
        assert_eq!(dict.load().await.unwrap().len(), 15, "back to the builtin set");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One known market contributes its tags; a later `set_roots` that no longer
    /// lists it must stop contributing them. This is the shape of the call-order
    /// defect the live probe found: `store/categories` was blind until a
    /// `store/list` taught it the roots, so a filter-bar-first client got
    /// `marketplace_tags: []`.
    #[tokio::test]
    async fn the_market_tag_vocabulary_follows_the_root_set() {
        let dict = CategoryDictionary::default();
        let dir = scratch("roots");
        std::fs::create_dir_all(dir.join(".codebuddy-skill")).unwrap();
        std::fs::write(
            dir.join(".codebuddy-skill").join("marketplace.json"),
            r#"{"tags_zh":["办公协同"],"tags_en":["Collaboration"]}"#,
        )
        .unwrap();

        assert!(dict.load_marketplace_tags().await.is_empty(), "no roots known yet");
        dict.set_roots(vec![("skills".to_owned(), dir.clone())]);
        let tags = dict.load_marketplace_tags().await;
        assert_eq!(tags.len(), 1, "a known root is read without any prior store/list");
        assert_eq!(tags[0].zh.as_deref(), Some("办公协同"));

        dict.set_roots(Vec::new());
        assert!(dict.load_marketplace_tags().await.is_empty(), "a removed market stops contributing");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The reason the cache is keyed by **file stamp** rather than by path: the
    /// market sweep replaces a live root's files in place, so a path-keyed cache
    /// would serve the previous sweep's dictionary forever.
    #[tokio::test]
    async fn a_changed_dictionary_file_is_re_read() {
        let dict = CategoryDictionary::default();
        let dir = scratch("refresh");
        let manifest = dir.join("manifest.json");
        std::fs::write(
            &manifest,
            r#"{"categories":[{"id":"01-One","name":{"zh":"一"}}]}"#,
        )
        .unwrap();
        dict.set_roots(vec![("demo-market".to_owned(), dir.clone())]);
        assert_eq!(dict.load().await.unwrap().len(), 1);

        // A refresh rewrites the file (different length ⇒ a different stamp even
        // on a coarse-grained clock).
        std::fs::write(
            &manifest,
            r#"{"categories":[{"id":"01-One","name":{"zh":"一"}},{"id":"02-Two","name":{"zh":"二"}}]}"#,
        )
        .unwrap();
        let items = dict.load().await.unwrap();
        assert_eq!(items.len(), 2, "the refreshed dictionary must be visible");
        assert_eq!(items[1].id, "02-Two");
        std::fs::remove_dir_all(&dir).ok();
    }
}
