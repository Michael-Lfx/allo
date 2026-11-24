//! One-page-at-a-time `ClawHub` shelf refresh with local caching.
//!
//! All methods are synchronous and may perform blocking HTTP or SQLite work.
//! Callers must run them on a blocking worker, never on the UI thread.

use crate::{
    Card, CatalogError, CatalogTransport, Category, Fetch, OfficialError,
    OfficialSkill, PlanError, Store, StoreError, TransportError, VerdictError,
    apply_verdicts, bundled_official, category_url, parse_categories,
    parse_search_page, parse_skill_page, retry_delay, search_url, trending_url,
    verdict_body,
};
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
    ) -> Result<Vec<Card>, MarketError> {
        self.refresh_page(Fetch::Trending, now_unix)
    }

    pub fn refresh_category(
        &mut self,
        slug: &str,
        now_unix: i64,
    ) -> Result<Vec<Card>, MarketError> {
        let _ = category_url(slug)?;
        self.refresh_page(
            Fetch::Category {
                slug: slug.to_owned(),
            },
            now_unix,
        )
    }

    pub fn search(
        &mut self,
        query: &str,
        category: Option<&str>,
        now_unix: i64,
    ) -> Result<Vec<Card>, MarketError> {
        let _ = search_url(query, category)?;
        self.refresh_page(
            Fetch::Search {
                query: query.trim().to_owned(),
                category: category.map(str::to_owned),
            },
            now_unix,
        )
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
    ) -> Result<Vec<Card>, MarketError> {
        let shelf_key = match &fetch {
            Fetch::Trending => Some("trending".to_owned()),
            Fetch::Category { slug } => Some(format!("category:{slug}")),
            Fetch::Search { .. } | Fetch::Categories | Fetch::Verdicts => None,
        };
        let search_key = match &fetch {
            Fetch::Search { query, category } => {
                Some(search_cache_key(query, category.as_deref()))
            }
            Fetch::Trending
            | Fetch::Category { .. }
            | Fetch::Categories
            | Fetch::Verdicts => None,
        };
        if let Some(key) = search_key.as_deref()
            && let Some(cards) = self.store.search_fresh(key, now_unix)?
        {
            return Ok(cards);
        }
        match self.fetch_verified_page(&fetch) {
            Ok(cards) => {
                if let Some(key) = search_key.as_deref() {
                    self.store.save_search(key, &cards, now_unix)?;
                }
                if let Some(key) = shelf_key.as_deref() {
                    self.store.replace_shelf(key, &cards, now_unix)?;
                }
                Ok(cards)
            }
            Err(error) => {
                let cached = match &fetch {
                    Fetch::Search { query, category } => {
                        self.store.search_cached(&search_cache_key(
                            query,
                            category.as_deref(),
                        ))?
                    }
                    Fetch::Trending => {
                        let cards = self.store.shelf("trending")?;
                        (!cards.is_empty()).then_some(cards)
                    }
                    Fetch::Category { slug } => {
                        let cards =
                            self.store.shelf(&format!("category:{slug}"))?;
                        (!cards.is_empty()).then_some(cards)
                    }
                    Fetch::Categories | Fetch::Verdicts => None,
                };
                cached.ok_or(error)
            }
        }
    }

    fn fetch_verified_page(
        &self,
        fetch: &Fetch,
    ) -> Result<Vec<Card>, MarketError> {
        let body = match fetch {
            Fetch::Trending => self.get_text_with_retry(trending_url())?,
            Fetch::Category { slug } => {
                self.get_text_with_retry(&category_url(slug)?)?
            }
            Fetch::Search { query, category } => self.get_text_with_retry(
                &search_url(query, category.as_deref())?,
            )?,
            Fetch::Categories | Fetch::Verdicts => {
                return Err(MarketError::Unavailable);
            }
        };
        let cards = match fetch {
            Fetch::Trending | Fetch::Category { .. } => {
                parse_skill_page(&body)?
            }
            Fetch::Search { .. } => parse_search_page(&body)?,
            Fetch::Categories | Fetch::Verdicts => Vec::new(),
        };
        let verdicts =
            self.post_json_with_retry(VERDICTS_URL, &verdict_body(&cards)?)?;
        Ok(apply_verdicts(&cards, &verdicts)?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CatalogTransport, HttpText, Store, TransportError};
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
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].id.slug, "one");
        assert_eq!(cards[1].id.slug, "two");
        let hits = transport.hits.lock().unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0], search_url("pdf", Some("knowledge")).unwrap());
        assert_eq!(hits[1], VERDICTS_URL);
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
        assert_eq!(cards.len(), 1);
        let cached = market
            .store
            .search_cached(&search_cache_key("pdf", None))
            .unwrap()
            .unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id.slug, "one");
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
        assert_eq!(cached.len(), 2);
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
        let cached = market.refresh_trending(2000).unwrap();
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].id.slug, "one");
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
