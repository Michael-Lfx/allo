use crate::{
    CATEGORY_TTL_SECS, Card, Category, InstallRecord, InstallStep,
    SEARCH_TTL_SECS, parse_skill_id,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::fmt;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cache JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cache directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid install step: {0}")]
    InvalidInstallStep(String),
    #[error("invalid installed skill identity: {0}")]
    InvalidSkillId(String),
}

pub struct Store {
    connection: Connection,
}

impl fmt::Debug for Store {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let connection = Connection::open_with_flags(path, flags)?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn replace_shelf(
        &self,
        shelf_key: &str,
        cards: &[Card],
        fetched_at_unix: i64,
    ) -> Result<(), StoreError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM shelf_card WHERE shelf_key = ?1",
            [shelf_key],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO shelf_card \
                 (shelf_key, position, owner_handle, slug, version, summary, \
                  categories_json, fetched_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (position, card) in cards.iter().enumerate() {
                let position = i64::try_from(position).unwrap_or(i64::MAX);
                let categories = serde_json::to_string(&card.categories)?;
                insert.execute(params![
                    shelf_key,
                    position,
                    card.id.owner_handle,
                    card.id.slug,
                    card.id.version,
                    card.summary,
                    categories,
                    fetched_at_unix,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn shelf(&self, shelf_key: &str) -> Result<Vec<Card>, StoreError> {
        let mut query = self.connection.prepare(
            "SELECT owner_handle, slug, version, summary, categories_json \
             FROM shelf_card WHERE shelf_key = ?1 ORDER BY position",
        )?;
        let rows = query.query_map([shelf_key], |row| {
            let categories_json: String = row.get(4)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                categories_json,
            ))
        })?;
        let mut cards = Vec::new();
        for row in rows {
            let (owner_handle, slug, version, summary, categories_json) = row?;
            let categories = serde_json::from_str(&categories_json)?;
            cards.push(Card {
                id: crate::parse_skill_id(&owner_handle, &slug, &version)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                summary,
                categories,
            });
        }
        Ok(cards)
    }

    pub fn search_fresh(
        &self,
        cache_key: &str,
        now_unix: i64,
    ) -> Result<Option<Vec<Card>>, StoreError> {
        let Some((cards, fetched_at)) = self.search_row(cache_key)? else {
            return Ok(None);
        };
        Ok((now_unix.saturating_sub(fetched_at)
            < i64::try_from(SEARCH_TTL_SECS).unwrap_or(i64::MAX))
        .then_some(cards))
    }

    pub fn search_cached(
        &self,
        cache_key: &str,
    ) -> Result<Option<Vec<Card>>, StoreError> {
        Ok(self.search_row(cache_key)?.map(|(cards, _)| cards))
    }

    pub fn save_search(
        &self,
        cache_key: &str,
        cards: &[Card],
        fetched_at_unix: i64,
    ) -> Result<(), StoreError> {
        let body = serde_json::to_string(cards)?;
        self.connection.execute(
            "INSERT INTO search_cache (cache_key, body_json, fetched_at) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(cache_key) DO UPDATE SET \
               body_json = excluded.body_json, fetched_at = excluded.fetched_at",
            params![cache_key, body, fetched_at_unix],
        )?;
        Ok(())
    }

    pub fn categories_fresh(
        &self,
        now_unix: i64,
    ) -> Result<Option<Vec<Category>>, StoreError> {
        let Some((categories, fetched_at)) = self.categories_row()? else {
            return Ok(None);
        };
        let ttl = i64::try_from(CATEGORY_TTL_SECS).unwrap_or(i64::MAX);
        Ok((now_unix.saturating_sub(fetched_at) < ttl).then_some(categories))
    }

    pub fn categories_cached(
        &self,
    ) -> Result<Option<Vec<Category>>, StoreError> {
        Ok(self.categories_row()?.map(|(categories, _)| categories))
    }

    fn categories_row(
        &self,
    ) -> Result<Option<(Vec<Category>, i64)>, StoreError> {
        let mut query = self.connection.prepare(
            "SELECT slug, label, fetched_at FROM category_cache ORDER BY position",
        )?;
        let rows = query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut fetched_at = None;
        let mut categories = Vec::new();
        for row in rows {
            let (slug, label, row_fetched_at) = row?;
            fetched_at.get_or_insert(row_fetched_at);
            categories.push(Category { slug, label });
        }
        Ok(fetched_at.map(|fetched_at| (categories, fetched_at)))
    }

    pub fn save_categories(
        &self,
        categories: &[Category],
        fetched_at_unix: i64,
    ) -> Result<(), StoreError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM category_cache", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO category_cache (slug, label, position, fetched_at) \
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (position, category) in categories.iter().enumerate() {
                let position = i64::try_from(position).unwrap_or(i64::MAX);
                insert.execute(params![
                    category.slug,
                    category.label,
                    position,
                    fetched_at_unix,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_install(
        &self,
        record: &InstallRecord,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO install \
             (owner_handle, slug, version, step, auto_invoke, scripts_enabled, content_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(owner_handle, slug, version) DO UPDATE SET \
               step = excluded.step, \
               auto_invoke = excluded.auto_invoke, \
               scripts_enabled = excluded.scripts_enabled, \
               content_hash = excluded.content_hash",
            params![
                record.id.owner_handle,
                record.id.slug,
                record.id.version,
                record.step.as_str(),
                u8::from(record.auto_invoke),
                u8::from(record.scripts_enabled),
                record.content_hash,
            ],
        )?;
        Ok(())
    }

    pub fn installed(&self) -> Result<Vec<InstallRecord>, StoreError> {
        let mut query = self.connection.prepare(
            "SELECT owner_handle, slug, version, step, auto_invoke, \
             scripts_enabled, content_hash FROM install \
             ORDER BY owner_handle, slug, version",
        )?;
        let rows = query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (
                owner,
                slug,
                version,
                step,
                auto_invoke,
                scripts_enabled,
                hash,
            ) = row?;
            let step = InstallStep::parse(&step)
                .ok_or_else(|| StoreError::InvalidInstallStep(step.clone()))?;
            let id =
                parse_skill_id(&owner, &slug, &version).map_err(|error| {
                    StoreError::InvalidSkillId(error.to_string())
                })?;
            records.push(InstallRecord {
                id,
                step,
                auto_invoke: auto_invoke != 0,
                scripts_enabled: scripts_enabled != 0,
                content_hash: hash,
            });
        }
        Ok(records)
    }
    fn search_row(
        &self,
        cache_key: &str,
    ) -> Result<Option<(Vec<Card>, i64)>, StoreError> {
        let row = self
            .connection
            .query_row(
                "SELECT body_json, fetched_at FROM search_cache WHERE cache_key = ?1",
                [cache_key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((body, fetched_at)) = row else {
            return Ok(None);
        };
        Ok(Some((serde_json::from_str(&body)?, fetched_at)))
    }

    fn migrate(&self) -> Result<(), StoreError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS shelf_card (
               shelf_key TEXT NOT NULL,
               position INTEGER NOT NULL,
               owner_handle TEXT NOT NULL,
               slug TEXT NOT NULL,
               version TEXT NOT NULL,
               summary TEXT NOT NULL,
               categories_json TEXT NOT NULL,
               fetched_at INTEGER NOT NULL,
               PRIMARY KEY (shelf_key, position)
             );
             CREATE TABLE IF NOT EXISTS search_cache (
               cache_key TEXT PRIMARY KEY,
               body_json TEXT NOT NULL,
               fetched_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS category_cache (
               slug TEXT PRIMARY KEY,
               label TEXT NOT NULL,
               position INTEGER NOT NULL,
               fetched_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS install (
               owner_handle TEXT NOT NULL,
               slug TEXT NOT NULL,
               version TEXT NOT NULL,
               step TEXT NOT NULL,
               auto_invoke INTEGER NOT NULL,
               scripts_enabled INTEGER NOT NULL,
               content_hash TEXT NOT NULL,
               PRIMARY KEY (owner_handle, slug, version)
             );",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InstallRecord, InstallStep, parse_skill_id};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    struct TestDb {
        dir: PathBuf,
        path: PathBuf,
    }

    impl TestDb {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DB.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "flowy-skills-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("cache.sqlite3");
            Self { dir, path }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn card(owner: &str, slug: &str) -> Card {
        Card {
            id: parse_skill_id(owner, slug, "1.0.0").unwrap(),
            summary: slug.into(),
            categories: vec!["knowledge".into()],
        }
    }

    #[test]
    fn shelf_round_trips_in_order() {
        let db = TestDb::new("shelf");
        let store = Store::open(&db.path).unwrap();
        let cards = vec![card("a", "one"), card("b", "two")];
        store.replace_shelf("trending", &cards, 1000).unwrap();
        assert_eq!(store.shelf("trending").unwrap(), cards);
    }

    #[test]
    fn search_is_fresh_only_inside_ttl() {
        let db = TestDb::new("search");
        let store = Store::open(&db.path).unwrap();
        let cards = vec![card("a", "one")];
        store.save_search("pdf", &cards, 1000).unwrap();
        assert_eq!(
            store.search_fresh("pdf", 1899).unwrap(),
            Some(cards.clone())
        );
        assert_eq!(store.search_fresh("pdf", 1900).unwrap(), None);
        assert_eq!(store.search_cached("pdf").unwrap(), Some(cards));
    }

    #[test]
    fn install_records_round_trip() {
        let db = TestDb::new("install");
        let store = Store::open(&db.path).unwrap();
        let record = InstallRecord {
            id: parse_skill_id("awspace", "pdf", "1.2.3").unwrap(),
            step: InstallStep::Commit,
            auto_invoke: false,
            scripts_enabled: false,
            content_hash: "abc123".into(),
        };
        store.upsert_install(&record).unwrap();
        assert_eq!(store.installed().unwrap(), vec![record]);
    }
    #[test]
    fn categories_are_fresh_only_inside_ttl_and_keep_order() {
        let db = TestDb::new("categories");
        let store = Store::open(&db.path).unwrap();
        let categories = vec![
            Category {
                slug: "knowledge".into(),
                label: "Knowledge".into(),
            },
            Category {
                slug: "agents".into(),
                label: "Agents".into(),
            },
        ];
        store.save_categories(&categories, 1000).unwrap();
        assert_eq!(store.categories_fresh(87_399).unwrap(), Some(categories));
        assert_eq!(store.categories_fresh(87_400).unwrap(), None);
    }
}
