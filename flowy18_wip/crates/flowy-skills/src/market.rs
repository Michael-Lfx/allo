//! Cached `ClawHub` shelves. The first page is served from cache when it is
//! fresh. Another page is fetched only when the caller passes a cursor.
//!
//! All methods are synchronous and may perform blocking HTTP or SQLite work.
//! Callers must run them on a blocking worker, never on the UI thread.

use crate::{
    CATEGORY_CARD_CAP, CATEGORY_TTL_SECS, Card, CatalogError, CatalogPage,
    CatalogTransport, Category, Fetch, OfficialError, OfficialSkill, PlanError,
    SEARCH_CARD_CAP, SEARCH_TTL_SECS, Store, StoreError, TRENDING_CARD_CAP,
    TransportError, VerdictError, bundled_official, category_url,
    parse_categories, parse_search_page, parse_skill_page, passing_ids,
    retry_delay, search_url, trending_url, verdict_body,
};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const VERDICTS_URL: &str =
    "https://clawhub.ai/api/v1/skills/-/security-verdicts";
const CATEGORIES_URL: &str = "https://clawhub.ai/api/v1/skills/-/categories";

#[derive(Debug, Error)]
pub enum MarketError {
    #[error("skill market request plan: {0}")]
    Plan(#[from] PlanError),
    #[error("skill market cache: {0}")]
    Store(#[from] StoreError),
    #[error("skill market HTTP: {0}")]
    Transport(#[from] TransportError),
    #[error("skill market catalog: {0}")]
    Catalog(#[from] CatalogError),
    #[error("skill market verdicts: {0}")]
    Verdict(#[from] VerdictError),
    #[error("official skill catalog: {0}")]
    Official(#[from] OfficialError),
    #[error("ClawHub is temporarily unavailable and no cached shelf exists")]
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfWindow {
    pub cards: Vec<Card>,
    pub next_cursor: Option<String>,
    /// No additional request can extend this window within the shelf cap.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedShelf {
    pub window: ShelfWindow,
    pub fresh: bool,
}

#[must_use]
pub const fn trending_cache_reusable(cache: &CachedShelf) -> bool {
    cache_window_is_current(cache)
}

#[must_use]
pub const fn category_cache_reusable(cache: &CachedShelf) -> bool {
    cache_window_is_current(cache)
}

pub struct SkillMarket<T: CatalogTransport> {
    transport: T,
    store: Store,
    sleeper: Arc<dyn Fn(Duration) + Send + Sync>,
    fixed_jitter_ms: Option<u64>,
}

impl<T: CatalogTransport> fmt::Debug for SkillMarket<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SkillMarket")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

impl<T: CatalogTransport> SkillMarket<T> {
    #[must_use]
    pub fn new(transport: T, store: Store) -> Self {
        Self {
            transport,
            store,
            sleeper: Arc::new(thread::sleep),
            fixed_jitter_ms: None,
        }
    }

    #[cfg(test)]
    fn with_controls<S>(
        transport: T,
        store: Store,
        sleeper: S,
        jitter_ms: u64,
    ) -> Self
    where
        S: Fn(Duration) + Send + Sync + 'static,
    {
        Self {
            transport,
            store,
            sleeper: Arc::new(sleeper),
            fixed_jitter_ms: Some(jitter_ms),
        }
    }

    pub fn refresh_trending(
        &mut self,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        self.refresh_page(Fetch::Trending { cursor: None }, now_unix)
    }

    pub fn continue_trending(
        &mut self,
        cursor: &str,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        let cursor = require_cursor(cursor)?;
        self.refresh_page(
            Fetch::Trending {
                cursor: Some(cursor),
            },
            now_unix,
        )
    }

    pub fn refresh_category(
        &mut self,
        slug: &str,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        let _ = category_url(slug, None)?;
        self.refresh_page(
            Fetch::Category {
                slug: slug.to_owned(),
                cursor: None,
            },
            now_unix,
        )
    }

    pub fn continue_category(
        &mut self,
        slug: &str,
        cursor: &str,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        let cursor = require_cursor(cursor)?;
        let _ = category_url(slug, Some(&cursor))?;
        self.refresh_page(
            Fetch::Category {
                slug: slug.to_owned(),
                cursor: Some(cursor),
            },
            now_unix,
        )
    }

    pub fn search(
        &mut self,
        query: &str,
        category: Option<&str>,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        let _ = search_url(query, category, None)?;
        self.refresh_page(
            Fetch::Search {
                query: query.trim().to_owned(),
                category: category.map(str::to_owned),
                cursor: None,
            },
            now_unix,
        )
    }

    pub fn continue_search(
        &mut self,
        query: &str,
        category: Option<&str>,
        cursor: &str,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        let cursor = require_cursor(cursor)?;
        let _ = search_url(query, category, Some(&cursor))?;
        self.refresh_page(
            Fetch::Search {
                query: query.trim().to_owned(),
                category: category.map(str::to_owned),
                cursor: Some(cursor),
            },
            now_unix,
        )
    }

    pub fn cached_trending(
        &self,
        now_unix: i64,
    ) -> Result<Option<CachedShelf>, MarketError> {
        self.cached_named("trending", now_unix, CATEGORY_TTL_SECS)
    }

    pub fn cached_category(
        &self,
        slug: &str,
        now_unix: i64,
    ) -> Result<Option<CachedShelf>, MarketError> {
        let _ = category_url(slug, None)?;
        self.cached_named(
            &format!("category:{slug}"),
            now_unix,
            CATEGORY_TTL_SECS,
        )
    }

    pub fn cached_search(
        &self,
        query: &str,
        category: Option<&str>,
        now_unix: i64,
    ) -> Result<Option<CachedShelf>, MarketError> {
        let _ = search_url(query, category, None)?;
        let Some(cache) = self
            .store
            .search_cached(&search_cache_key(query.trim(), category))?
        else {
            return Ok(None);
        };
        Ok(Some(CachedShelf {
            fresh: within_ttl(cache.fetched_at, now_unix, SEARCH_TTL_SECS),
            window: ShelfWindow {
                cards: cache.cards,
                next_cursor: cache.next_cursor,
                complete: cache.complete,
            },
        }))
    }

    pub fn categories(
        &mut self,
        now_unix: i64,
    ) -> Result<Vec<Category>, MarketError> {
        if let Some(categories) = self.store.categories_fresh(now_unix)? {
            return Ok(categories);
        }
        let result = self
            .get_text_with_retry(CATEGORIES_URL)
            .and_then(|body| Ok(parse_categories(&body)?));
        match result {
            Ok(categories) => {
                self.store.save_categories(&categories, now_unix)?;
                Ok(categories)
            }
            Err(error) => self.store.categories_cached()?.ok_or(error),
        }
    }

    pub fn official(&self) -> Result<Vec<OfficialSkill>, OfficialError> {
        bundled_official()
    }

    fn refresh_page(
        &self,
        fetch: Fetch,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        if fetch.cursor().is_none()
            && let Some(hit) = self.cached_fetch(&fetch, now_unix)?
            && cache_is_current(&fetch, &hit)
        {
            return Ok(hit.window);
        }
        match self.fetch_verified_page(&fetch) {
            Ok(page) => self.store_page(&fetch, page, now_unix),
            Err(error) if fetch.cursor().is_some() => Err(error),
            Err(error) => self
                .cached_fetch(&fetch, now_unix)?
                .map(|hit| hit.window)
                .ok_or(error),
        }
    }

    fn cached_fetch(
        &self,
        fetch: &Fetch,
        now_unix: i64,
    ) -> Result<Option<CachedShelf>, MarketError> {
        match fetch {
            Fetch::Trending { cursor: None } => self.cached_trending(now_unix),
            Fetch::Category { slug, cursor: None } => {
                self.cached_category(slug, now_unix)
            }
            Fetch::Search {
                query,
                category,
                cursor: None,
            } => self.cached_search(query, category.as_deref(), now_unix),
            Fetch::Trending { cursor: Some(_) }
            | Fetch::Category {
                cursor: Some(_), ..
            }
            | Fetch::Search {
                cursor: Some(_), ..
            }
            | Fetch::Categories
            | Fetch::Verdicts => Ok(None),
        }
    }

    fn cached_named(
        &self,
        key: &str,
        now_unix: i64,
        ttl_secs: u64,
    ) -> Result<Option<CachedShelf>, MarketError> {
        let Some(cache) = self.store.shelf_cache(key)? else {
            return Ok(None);
        };
        Ok(Some(CachedShelf {
            fresh: within_ttl(cache.fetched_at, now_unix, ttl_secs),
            window: ShelfWindow {
                cards: cache.cards,
                next_cursor: cache.next_cursor,
                complete: cache.complete,
            },
        }))
    }

    fn store_page(
        &self,
        fetch: &Fetch,
        page: CatalogPage,
        now_unix: i64,
    ) -> Result<ShelfWindow, MarketError> {
        let CatalogPage {
            mut cards,
            next_cursor,
        } = page;
        match fetch {
            Fetch::Trending { cursor } => {
                if cursor.is_some()
                    && let Some(existing) =
                        self.store.shelf_cache("trending")?
                {
                    cards = merge_cards(existing.cards, cards);
                }
                let window = cap_window(cards, next_cursor, TRENDING_CARD_CAP);
                self.store.replace_shelf(
                    "trending",
                    &window.cards,
                    window.next_cursor.as_deref(),
                    window.complete,
                    now_unix,
                )?;
                Ok(window)
            }
            Fetch::Category { slug, cursor } => {
                let key = format!("category:{slug}");
                if cursor.is_some()
                    && let Some(existing) = self.store.shelf_cache(&key)?
                {
                    cards = merge_cards(existing.cards, cards);
                }
                let window = cap_window(cards, next_cursor, CATEGORY_CARD_CAP);
                self.store.replace_shelf(
                    &key,
                    &window.cards,
                    window.next_cursor.as_deref(),
                    window.complete,
                    now_unix,
                )?;
                Ok(window)
            }
            Fetch::Search {
                query,
                category,
                cursor,
            } => {
                let key = search_cache_key(query, category.as_deref());
                if cursor.is_some()
                    && let Some(existing) = self.store.search_cached(&key)?
                {
                    cards = merge_cards(existing.cards, cards);
                }
                let window = cap_window(cards, next_cursor, SEARCH_CARD_CAP);
                self.store.save_search(
                    &key,
                    &window.cards,
                    window.next_cursor.as_deref(),
                    window.complete,
                    now_unix,
                )?;
                Ok(window)
            }
            Fetch::Categories | Fetch::Verdicts => {
                Err(MarketError::Unavailable)
            }
        }
    }

    fn fetch_verified_page(
        &self,
        fetch: &Fetch,
    ) -> Result<CatalogPage, MarketError> {
        let body = match fetch {
            Fetch::Trending { cursor } => {
                self.get_text_with_retry(&trending_url(cursor.as_deref()))?
            }
            Fetch::Category { slug, cursor } => self
                .get_text_with_retry(&category_url(slug, cursor.as_deref())?)?,
            Fetch::Search {
                query,
                category,
                cursor,
            } => self.get_text_with_retry(&search_url(
                query,
                category.as_deref(),
                cursor.as_deref(),
            )?)?,
            Fetch::Categories | Fetch::Verdicts => {
                return Err(MarketError::Unavailable);
            }
        };
        let mut page = match fetch {
            Fetch::Trending { .. } | Fetch::Category { .. } => {
                parse_skill_page(&body)?
            }
            Fetch::Search { .. } => parse_search_page(&body)?,
            Fetch::Categories | Fetch::Verdicts => {
                return Err(MarketError::Unavailable);
            }
        };
        let search_unversioned = matches!(fetch, Fetch::Search { .. });
        let pending: Vec<_> = page
            .cards
            .iter()
            .filter(|card| {
                !(card.preverified
                    || search_unversioned
                        && card.id.version == crate::UNVERSIONED)
            })
            .cloned()
            .collect();
        let passing = if pending.is_empty() {
            HashSet::new()
        } else {
            let verdicts = self
                .post_json_with_retry(VERDICTS_URL, &verdict_body(&pending)?)?;
            passing_ids(&verdicts)?
        };
        page.cards.retain(|card| {
            card.preverified
                || (search_unversioned && card.id.version == crate::UNVERSIONED)
                || passing.contains(&card.id)
        });
        Ok(page)
    }

    fn get_text_with_retry(&self, url: &str) -> Result<String, MarketError> {
        self.request_text_with_retry(|| self.transport.get_text(url))
    }

    fn post_json_with_retry(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<String, MarketError> {
        self.request_text_with_retry(|| self.transport.post_json(url, body))
    }

    fn request_text_with_retry<F>(
        &self,
        request: F,
    ) -> Result<String, MarketError>
    where
        F: Fn() -> Result<crate::HttpText, TransportError>,
    {
        let first = request()?;
        let response = match retry_delay(
            first.status,
            first.retry_after_secs,
            self.jitter_ms(),
        ) {
            Some(delay) => {
                (self.sleeper)(Duration::from_millis(delay));
                request()?
            }
            None => first,
        };
        if (200..=299).contains(&response.status) {
            Ok(response.body)
        } else {
            Err(MarketError::Unavailable)
        }
    }

    fn jitter_ms(&self) -> u64 {
        self.fixed_jitter_ms.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| u64::from(elapsed.subsec_nanos()) % 251)
        })
    }
}

fn search_cache_key(query: &str, category: Option<&str>) -> String {
    format!("search:{query:?}:{category:?}")
}

fn require_cursor(cursor: &str) -> Result<String, MarketError> {
    let cursor = cursor.trim();
    if cursor.is_empty() {
        Err(MarketError::Unavailable)
    } else {
        Ok(cursor.to_owned())
    }
}

impl Fetch {
    fn cursor(&self) -> Option<&str> {
        match self {
            Self::Trending { cursor }
            | Self::Category { cursor, .. }
            | Self::Search { cursor, .. } => cursor.as_deref(),
            Self::Categories | Self::Verdicts => None,
        }
    }
}

const fn cache_window_is_current(cache: &CachedShelf) -> bool {
    cache.fresh && (cache.window.complete || cache.window.next_cursor.is_some())
}

const fn cache_is_current(fetch: &Fetch, hit: &CachedShelf) -> bool {
    match fetch {
        Fetch::Search { .. }
        | Fetch::Trending { .. }
        | Fetch::Category { .. } => cache_window_is_current(hit),
        Fetch::Categories | Fetch::Verdicts => false,
    }
}

fn within_ttl(fetched_at: i64, now_unix: i64, ttl_secs: u64) -> bool {
    now_unix.saturating_sub(fetched_at)
        < i64::try_from(ttl_secs).unwrap_or(i64::MAX)
}

fn merge_cards(existing: Vec<Card>, extra: Vec<Card>) -> Vec<Card> {
    let mut seen = HashSet::new();
    let mut cards = Vec::new();
    for card in existing.into_iter().chain(extra) {
        if seen.insert(format!("{}\u{0}{}", card.id.owner_handle, card.id.slug))
        {
            cards.push(card);
        }
    }
    cards
}

fn cap_window(
    mut cards: Vec<Card>,
    next_cursor: Option<String>,
    cap: usize,
) -> ShelfWindow {
    let complete = next_cursor.is_none() || cards.len() >= cap;
    if cards.len() >= cap {
        cards.truncate(cap);
        ShelfWindow {
            cards,
            next_cursor: None,
            complete: true,
        }
    } else {
        ShelfWindow {
            cards,
            next_cursor,
            complete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CATEGORY_TTL_SECS, CatalogTransport, HttpText, PAGE_LIMIT_TRENDING,
        Store, TransportError,
    };
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    struct TestDb {
        dir: PathBuf,
        path: PathBuf,
    }

    impl TestDb {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DB.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "flowy-skills-market-{name}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("cache.sqlite3");
            Self { dir, path }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    struct ScriptedTransport {
        next: Mutex<VecDeque<(u16, String)>>,
        hits: Mutex<Vec<String>>,
    }

    impl ScriptedTransport {
        fn new(responses: Vec<(u16, String)>) -> Self {
            Self {
                next: Mutex::new(responses.into()),
                hits: Mutex::new(Vec::new()),
            }
        }

        fn next_response(&self, url: &str) -> HttpText {
            self.hits.lock().unwrap().push(url.to_owned());
            let (status, body) = self.next.lock().unwrap().pop_front().unwrap();
            HttpText {
                status,
                body,
                retry_after_secs: (status == 429).then_some(0),
            }
        }
    }

    impl CatalogTransport for Arc<ScriptedTransport> {
        fn get_text(&self, url: &str) -> Result<HttpText, TransportError> {
            Ok(self.next_response(url))
        }

        fn post_json(
            &self,
            url: &str,
            _body: &serde_json::Value,
        ) -> Result<HttpText, TransportError> {
            Ok(self.next_response(url))
        }
    }

    fn market_with(
        transport: Arc<ScriptedTransport>,
        store: Store,
    ) -> (
        SkillMarket<Arc<ScriptedTransport>>,
        Arc<Mutex<Vec<Duration>>>,
    ) {
        let sleeps = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&sleeps);
        let market = SkillMarket::with_controls(
            transport,
            store,
            move |duration| recorded.lock().unwrap().push(duration),
            0,
        );
        (market, sleeps)
    }

    fn search_body() -> String {
        r#"
        {
          "results": [
            {"ownerHandle":"a","slug":"one","summary":"first","version":"1.0.0"},
            {"ownerHandle":"b","slug":"two","summary":"second","version":"1.0.0"}
          ]
        }
        "#
        .to_owned()
    }

    fn clean_page(count: usize, cursor: Option<&str>) -> String {
        let items = (0..count)
            .map(|index| {
                format!(
                    r#"{{"slug":"s{index}","displayName":"S{index}","summary":"s","publisher":{{"handle":"owner{index}"}},"trust":{{"clawHubVerdict":"clean"}}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        match cursor {
            Some(cursor) => {
                format!(r#"{{"items":[{items}],"nextCursor":"{cursor}"}}"#)
            }
            None => format!(r#"{{"items":[{items}]}}"#),
        }
    }

    fn page_body() -> String {
        r#"
        {
          "items": [
            {"ownerHandle":"a","slug":"one","summary":"first","version":"1.0.0"},
            {"ownerHandle":"b","slug":"two","summary":"second","version":"1.0.0"}
          ]
        }
        "#
        .to_owned()
    }
    fn verdict_body(second_decision: &str) -> String {
        format!(
            r#"{{
              "items": [
                {{"decision":"pass","security":{{"status":"clean"}},"requestedOwnerHandle":"a","requestedSlug":"one","requestedVersion":"1.0.0"}},
                {{"decision":"{second_decision}","security":{{"status":"clean"}},"requestedOwnerHandle":"b","requestedSlug":"two","requestedVersion":"1.0.0"}}
              ]
            }}"#
        )
    }

    #[test]
    fn search_refreshes_one_page_and_filters_in_order() {
        let db = TestDb::new("search");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, search_body()),
            (200, verdict_body("pass")),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let cards = market.search("pdf", Some("knowledge"), 1000).unwrap();
        assert_eq!(cards.cards.len(), 2);
        assert_eq!(cards.cards[0].id.slug, "one");
        assert_eq!(cards.cards[1].id.slug, "two");
        let hits = transport.hits.lock().unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0],
            search_url("pdf", Some("knowledge"), None).unwrap()
        );
        assert_eq!(hits[1], VERDICTS_URL);
    }

    #[test]
    fn embedded_clean_trending_skips_the_verdict_request() {
        let db = TestDb::new("embedded");
        let store = Store::open(&db.path).unwrap();
        let body = r#"
        {
          "items": [
            {
              "slug": "clawcall-dev",
              "displayName": "ClawCall",
              "summary": "Place a call",
              "publisher": { "handle": "clawcall-dev" },
              "trust": { "clawHubVerdict": "clean" }
            }
          ]
        }
        "#
        .to_owned();
        let transport = Arc::new(ScriptedTransport::new(vec![(200, body)]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let cards = market.refresh_trending(1000).unwrap();
        assert_eq!(cards.cards.len(), 1);
        assert_eq!(cards.cards[0].title, "ClawCall");
        assert_eq!(transport.hits.lock().unwrap().len(), 1);
    }

    #[test]
    fn search_cache_avoids_network_inside_fifteen_minutes() {
        let db = TestDb::new("fresh");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, search_body()),
            (200, verdict_body("pass")),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        market.search("pdf", None, 1000).unwrap();
        market.search("pdf", None, 1899).unwrap();
        assert_eq!(transport.hits.lock().unwrap().len(), 2);
    }

    #[test]
    fn failed_verdicts_are_not_written_to_search_cache() {
        let db = TestDb::new("verdict");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, search_body()),
            (200, verdict_body("fail")),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let cards = market.search("pdf", None, 1000).unwrap();
        assert_eq!(cards.cards.len(), 1);
        let cached = market
            .store
            .search_cached(&search_cache_key("pdf", None))
            .unwrap()
            .unwrap();
        assert_eq!(cached.cards.len(), 1);
        assert_eq!(cached.cards[0].id.slug, "one");
    }

    #[test]
    fn rate_limit_retries_once_after_recorded_delay() {
        let db = TestDb::new("retry");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (429, "slow down".into()),
            (200, search_body()),
            (200, verdict_body("pass")),
        ]));
        let (mut market, sleeps) = market_with(Arc::clone(&transport), store);
        market.search("pdf", None, 1000).unwrap();
        assert_eq!(transport.hits.lock().unwrap().len(), 3);
        assert_eq!(*sleeps.lock().unwrap(), [Duration::ZERO]);
    }

    #[test]
    fn failed_retry_keeps_previous_search_cache() {
        let db = TestDb::new("stale-search");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, search_body()),
            (200, verdict_body("pass")),
            (429, "retry".into()),
            (503, "still unavailable".into()),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        market.search("pdf", None, 1000).unwrap();
        let cached = market.search("pdf", None, 1900).unwrap();
        assert_eq!(cached.cards.len(), 2);
        assert_eq!(transport.hits.lock().unwrap().len(), 4);
    }

    #[test]
    fn failed_verdicts_keep_previous_shelf() {
        let db = TestDb::new("stale-shelf");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, page_body()),
            (200, verdict_body("pass")),
            (200, page_body()),
            (500, "verdict unavailable".into()),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        market.refresh_trending(1000).unwrap();
        let cached = market
            .refresh_trending(1000 + i64::try_from(CATEGORY_TTL_SECS).unwrap())
            .unwrap();
        assert_eq!(cached.cards.len(), 2);
        assert_eq!(cached.cards[0].id.slug, "one");
    }
    #[test]
    fn category_follow_appends_the_next_page_and_keeps_the_cursor() {
        let db = TestDb::new("follow");
        let store = Store::open(&db.path).unwrap();
        let first = r#"{"items":[{"slug":"one","displayName":"One","summary":"s","publisher":{"handle":"a"},"trust":{"clawHubVerdict":"clean"}}],"nextCursor":"page-2"}"#;
        let second = r#"{"items":[{"slug":"two","displayName":"Two","summary":"s","publisher":{"handle":"b"},"trust":{"clawHubVerdict":"clean"}}],"nextCursor":"page-3"}"#;
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, first.to_owned()),
            (200, second.to_owned()),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let first_page = market.refresh_category("knowledge", 1000).unwrap();
        assert_eq!(first_page.cards.len(), 1);
        assert_eq!(first_page.next_cursor.as_deref(), Some("page-2"));
        let second_page = market
            .continue_category("knowledge", "page-2", 1001)
            .unwrap();
        assert_eq!(second_page.cards.len(), 2);
        assert_eq!(second_page.cards[1].id.slug, "two");
        assert_eq!(second_page.next_cursor.as_deref(), Some("page-3"));
        let (hit_count, second_has_cursor, first_has_cursor) = {
            let hits = transport.hits.lock().unwrap();
            (
                hits.len(),
                hits[1].contains("cursor=page-2"),
                hits[0].contains("cursor"),
            )
        };
        assert_eq!(hit_count, 2);
        assert!(second_has_cursor);
        assert!(!first_has_cursor);
    }

    #[test]
    fn old_trending_cache_is_refetched_and_keeps_the_cursor() {
        let db = TestDb::new("trending-old");
        let store = Store::open(&db.path).unwrap();
        let card = crate::Card {
            id: crate::parse_skill_id("a", "one", "1.0.0").unwrap(),
            summary: "s".into(),
            categories: Vec::new(),
            title: "One".into(),
            preverified: true,
            downloads: None,
            installs: None,
            updated_at_ms: None,
        };
        store
            .replace_shelf("trending", &[card], None, false, 1000)
            .unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![(
            200,
            clean_page(1, Some("more")),
        )]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let window = market.refresh_trending(1001).unwrap();
        assert_eq!(window.next_cursor.as_deref(), Some("more"));
        assert_ne!(window.cards.len(), 0);
        assert_eq!(transport.hits.lock().unwrap().len(), 1);
    }

    #[test]
    fn full_trending_page_without_cursor_is_reused() {
        let db = TestDb::new("trending-full");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![(
            200,
            clean_page(PAGE_LIMIT_TRENDING as usize, None),
        )]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let first = market.refresh_trending(1000).unwrap();
        assert_eq!(first.cards.len(), PAGE_LIMIT_TRENDING as usize);
        assert_eq!(first.next_cursor, None);
        assert!(first.complete);
        let again = market.refresh_trending(1001).unwrap();
        assert!(again.complete);
        assert_eq!(again.cards.len(), PAGE_LIMIT_TRENDING as usize);
        assert_eq!(transport.hits.lock().unwrap().len(), 1);
    }

    #[test]
    fn fresh_category_skips_the_network() {
        let db = TestDb::new("fresh-category");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, page_body()),
            (200, verdict_body("pass")),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        market.refresh_category("knowledge", 1000).unwrap();
        let again = market.refresh_category("knowledge", 1001).unwrap();
        assert_eq!(again.cards.len(), 2);
        assert_eq!(transport.hits.lock().unwrap().len(), 2);
    }

    #[test]
    fn failed_follow_keeps_the_first_page() {
        let db = TestDb::new("failed-follow");
        let store = Store::open(&db.path).unwrap();
        let body = r#"{"items":[{"slug":"one","displayName":"One","summary":"s","publisher":{"handle":"a"},"trust":{"clawHubVerdict":"clean"}}],"nextCursor":"page-2"}"#;
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, body.to_owned()),
            (503, "down".into()),
            (503, "still".into()),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        market.refresh_category("knowledge", 1000).unwrap();
        let error = market
            .continue_category("knowledge", "page-2", 1001)
            .unwrap_err();
        assert!(matches!(error, MarketError::Unavailable));
        let cached =
            market.cached_category("knowledge", 1001).unwrap().unwrap();
        assert_eq!(cached.window.cards.len(), 1);
        assert_eq!(cached.window.next_cursor.as_deref(), Some("page-2"));
    }

    #[test]
    fn unversioned_search_rows_remain_browsable() {
        let db = TestDb::new("unversioned-search");
        let store = Store::open(&db.path).unwrap();
        let body = r#"{
          "results": [{
            "ownerHandle": "awspace",
            "slug": "pdf",
            "displayName": "Pdf",
            "summary": "PDF tools",
            "version": null,
            "trust": { "clawHubVerdict": null },
            "native": { "skill": { "tags": { "latest": "opaque-version-id" } } }
          }]
        }"#;
        let transport =
            Arc::new(ScriptedTransport::new(vec![(200, body.to_owned())]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        let window = market.search("pdf", None, 1000).unwrap();
        assert_eq!(window.cards.len(), 1);
        assert_eq!(window.cards[0].id.version, crate::UNVERSIONED);
        assert!(window.complete);
        assert_eq!(transport.hits.lock().unwrap().len(), 1);
    }

    #[test]
    fn completed_followed_shelf_is_reused_from_cache() {
        let db = TestDb::new("follow-complete");
        let store = Store::open(&db.path).unwrap();
        let first = r#"{"items":[{"slug":"one","displayName":"One","summary":"s","publisher":{"handle":"a"},"trust":{"clawHubVerdict":"clean"}}],"nextCursor":"page-2"}"#;
        let second = r#"{"items":[{"slug":"two","displayName":"Two","summary":"s","publisher":{"handle":"b"},"trust":{"clawHubVerdict":"clean"}}]}"#;
        let transport = Arc::new(ScriptedTransport::new(vec![
            (200, first.to_owned()),
            (200, second.to_owned()),
        ]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        market.refresh_trending(1000).unwrap();
        let followed = market.continue_trending("page-2", 1001).unwrap();
        assert_eq!(followed.cards.len(), 2);
        assert!(followed.complete);
        let cached = market.refresh_trending(1002).unwrap();
        assert_eq!(cached.cards.len(), 2);
        assert!(cached.complete);
        assert_eq!(transport.hits.lock().unwrap().len(), 2);
    }

    #[test]
    fn categories_are_cached_for_one_day() {
        let db = TestDb::new("categories");
        let store = Store::open(&db.path).unwrap();
        let transport = Arc::new(ScriptedTransport::new(vec![(
            200,
            r#"{"categories":[{"slug":"knowledge","label":"Knowledge"}]}"#
                .to_owned(),
        )]));
        let (mut market, _sleeps) = market_with(Arc::clone(&transport), store);
        assert_eq!(market.categories(1000).unwrap().len(), 1);
        assert_eq!(market.categories(1001).unwrap().len(), 1);
        assert_eq!(transport.hits.lock().unwrap().len(), 1);
    }
}
