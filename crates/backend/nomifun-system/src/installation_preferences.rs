use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use nomifun_common::{AppError, dir_config};
use nomifun_db::IClientPreferenceRepository;
use serde_json::{Map, Value};
use tokio::sync::Mutex;

/// Stable installation-local preference file. It lives below `data_dir`,
/// which remains fixed when the user changes the workspace root.
pub const INSTALLATION_PREFERENCES_FILE: &str = "installation-preferences.json";

/// Preferences that describe the current installation's UI rather than the
/// selected workspace. Keep this list deliberately closed: routing an
/// unknown key here would silently change its lifecycle semantics.
pub const INSTALLATION_PREFERENCE_KEYS: &[&str] = &[
    "language",
    "theme",
    "colorScheme",
    "ui.zoomFactor",
    "window.bounds",
    "customCss",
    "css.themes",
    "css.activeThemeId",
];

const MAX_INSTALLATION_PREFERENCES_BYTES: u64 = 4 * 1024 * 1024;

type InstallationFileLock = Mutex<()>;

/// Both the system and shell routers expose the same client-preference API and
/// may therefore construct separate services for one data directory. Share a
/// lock by path so their read-modify-write operations cannot overwrite one
/// another. Weak values avoid retaining temporary test directories forever.
static FILE_LOCKS: OnceLock<StdMutex<HashMap<PathBuf, Weak<InstallationFileLock>>>> = OnceLock::new();

fn shared_file_lock(path: &Path) -> Arc<InstallationFileLock> {
    let registry = FILE_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut registry = registry.lock().expect("installation preference lock registry poisoned");
    if let Some(lock) = registry.get(path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    registry.insert(path.to_path_buf(), Arc::downgrade(&lock));
    lock
}

#[derive(Clone)]
pub struct InstallationPreferenceStore {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
    legacy_cleanup_done: Arc<AtomicBool>,
}

impl InstallationPreferenceStore {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join(INSTALLATION_PREFERENCES_FILE);
        Self {
            lock: shared_file_lock(&path),
            path,
            legacy_cleanup_done: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Move the installation-scoped keys out of the old SQLite location once.
    ///
    /// The file is published before the legacy rows are deleted. If deletion
    /// fails, the next request retries cleanup while reads already prefer the
    /// durable installation copy, so a partial migration cannot lose values.
    pub async fn ensure_migrated(
        &self,
        repo: &dyn IClientPreferenceRepository,
    ) -> Result<(), AppError> {
        if self.legacy_cleanup_done.load(Ordering::Acquire) {
            return Ok(());
        }

        let _guard = self.lock.lock().await;
        if self.legacy_cleanup_done.load(Ordering::Acquire) {
            return Ok(());
        }

        if self.read_map_async().await?.is_none() {
            let key_refs = INSTALLATION_PREFERENCE_KEYS.to_vec();
            let rows = repo.get_by_keys(&key_refs).await.map_err(|error| {
                AppError::Internal(format!("read legacy installation preferences: {error}"))
            })?;
            let mut migrated = BTreeMap::new();
            for row in rows {
                let value = serde_json::from_str::<Value>(&row.value)
                    .unwrap_or_else(|_| Value::String(row.value.clone()));
                if validate_preference_value(&row.key, &value).is_ok() {
                    migrated.insert(row.key, value);
                } else {
                    tracing::warn!(
                        key = %row.key,
                        "skipping invalid legacy installation preference during migration"
                    );
                }
            }
            self.write_map_async(migrated).await?;
        }

        let key_refs = INSTALLATION_PREFERENCE_KEYS.to_vec();
        repo.delete_keys(&key_refs).await.map_err(|error| {
            AppError::Internal(format!("remove legacy installation preferences: {error}"))
        })?;
        self.legacy_cleanup_done.store(true, Ordering::Release);
        Ok(())
    }

    pub async fn get(&self, keys: Option<&[&str]>) -> Result<BTreeMap<String, Value>, AppError> {
        let _guard = self.lock.lock().await;
        let values = self.read_map_async().await?.unwrap_or_default();
        Ok(match keys {
            Some(keys) => values
                .into_iter()
                .filter(|(key, _)| keys.iter().any(|requested| *requested == key))
                .collect(),
            None => values,
        })
    }

    pub async fn update(&self, entries: &[(String, Value)]) -> Result<(), AppError> {
        let _guard = self.lock.lock().await;
        let mut values = self.read_map_async().await?.unwrap_or_default();
        for (key, value) in entries {
            if !is_installation_preference_key(key) {
                return Err(AppError::BadRequest(format!(
                    "preference key '{key}' is not installation-scoped"
                )));
            }
            if value.is_null() {
                values.remove(key);
            } else {
                validate_preference_value(key, value)?;
                values.insert(key.clone(), value.clone());
            }
        }
        self.write_map_async(values).await
    }

    async fn read_map_async(&self) -> Result<Option<BTreeMap<String, Value>>, AppError> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || read_map_from_path(&path))
            .await
            .map_err(|error| {
                AppError::Internal(format!(
                    "join installation preference read task for {}: {error}",
                    self.path.display()
                ))
            })?
    }

    async fn write_map_async(&self, values: BTreeMap<String, Value>) -> Result<(), AppError> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || write_map_to_path(&path, &values))
            .await
            .map_err(|error| {
                AppError::Internal(format!(
                    "join installation preference write task for {}: {error}",
                    self.path.display()
                ))
            })?
    }
}

fn read_map_from_path(path: &Path) -> Result<Option<BTreeMap<String, Value>>, AppError> {
    let bytes = match dir_config::read_bounded_regular_file(path, MAX_INSTALLATION_PREFERENCES_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "read installation preferences safely {}: {error}",
                    path.display()
                )));
            }
        };
        let raw: Map<String, Value> = serde_json::from_slice(&bytes).map_err(|error| {
            AppError::Internal(format!(
                "parse installation preferences {}: {error}",
                path.display()
            ))
        })?;
        let mut values = BTreeMap::new();
        for (key, value) in raw {
            if !is_installation_preference_key(&key) {
                return Err(AppError::Internal(format!(
                    "installation preferences contains unknown key '{key}'"
                )));
            }
            validate_preference_value(&key, &value)?;
            values.insert(key, value);
        }
        Ok(Some(values))
}

fn write_map_to_path(path: &Path, values: &BTreeMap<String, Value>) -> Result<(), AppError> {
        let json = serde_json::to_vec_pretty(values).map_err(|error| {
            AppError::Internal(format!("serialize installation preferences: {error}"))
        })?;
        if json.len() as u64 > MAX_INSTALLATION_PREFERENCES_BYTES {
            return Err(AppError::BadRequest(format!(
                "installation preferences exceed the {MAX_INSTALLATION_PREFERENCES_BYTES}-byte limit"
            )));
        }
        dir_config::write_atomic_replace(path, &json).map_err(|error| {
            AppError::Internal(format!(
                "write installation preferences {}: {error}",
                path.display()
            ))
        })
}

pub fn is_installation_preference_key(key: &str) -> bool {
    INSTALLATION_PREFERENCE_KEYS.contains(&key)
}

fn validate_preference_value(key: &str, value: &Value) -> Result<(), AppError> {
    let valid = match key {
        "language" | "theme" | "colorScheme" | "customCss" | "css.activeThemeId" => value.is_string(),
        "ui.zoomFactor" => value.as_f64().is_some_and(f64::is_finite),
        "window.bounds" => value.is_object(),
        "css.themes" => value.is_array(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "invalid value for installation preference '{key}'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_db::{IClientPreferenceRepository, SqliteClientPreferenceRepository, init_database_memory};
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn migrates_ui_preferences_and_removes_legacy_rows() {
        let data = tempdir().unwrap();
        let db = init_database_memory().await.unwrap();
        let repo = SqliteClientPreferenceRepository::new(db.pool().clone());
        repo.upsert_batch(&[("language", "\"zh-CN\""), ("system.closeToTray", "true")])
            .await
            .unwrap();

        let store = InstallationPreferenceStore::new(data.path());
        store.ensure_migrated(&repo).await.unwrap();

        assert_eq!(store.get(None).await.unwrap().get("language"), Some(&json!("zh-CN")));
        assert!(repo.get_by_keys(&["language"]).await.unwrap().is_empty());
        assert_eq!(repo.get_by_keys(&["system.closeToTray"]).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rejects_oversized_files() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        let path = data.path().join(INSTALLATION_PREFERENCES_FILE);
        std::fs::write(&path, vec![b'x'; (MAX_INSTALLATION_PREFERENCES_BYTES + 1) as usize]).unwrap();
        let error = store.get(None).await.unwrap_err();
        assert!(error.to_string().contains("safely"));
    }

    #[tokio::test]
    async fn rejects_malformed_files() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        let path = data.path().join(INSTALLATION_PREFERENCES_FILE);
        std::fs::write(&path, b"{\"language\":").unwrap();

        let error = store.get(None).await.unwrap_err();
        assert!(error.to_string().contains("parse installation preferences"));
    }

    #[test]
    fn validates_the_closed_installation_key_set() {
        assert!(is_installation_preference_key("language"));
        assert!(!is_installation_preference_key("nomi.defaultModel"));
        assert!(validate_preference_value("ui.zoomFactor", &json!(1.1)).is_ok());
        assert!(validate_preference_value("ui.zoomFactor", &json!("1.1")).is_err());
    }
}
