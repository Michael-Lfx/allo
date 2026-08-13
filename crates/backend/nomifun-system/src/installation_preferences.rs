use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use nomifun_common::{AppError, dir_config, normalize_ui_language};
use nomifun_db::IClientPreferenceRepository;
use serde_json::{Map, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Stable installation-local preference file. It lives below `data_dir`,
/// which remains fixed when the user changes the workspace root.
pub use nomifun_common::INSTALLATION_PREFERENCES_FILE;

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
const INSTALLATION_PREFERENCES_BACKUP_SUFFIX: &str = ".bak";

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

        let key_refs = INSTALLATION_PREFERENCE_KEYS.to_vec();
        let mut values = self.read_map_async().await?.unwrap_or_default();
        let rows = repo.get_by_keys(&key_refs).await.map_err(|error| {
            AppError::Internal(format!("read legacy installation preferences: {error}"))
        })?;
        for row in rows {
            if values.contains_key(&row.key) {
                // The installation file is authoritative once it contains a
                // valid value, even when SQLite still has the old row.
                continue;
            }
            let value = serde_json::from_str::<Value>(&row.value)
                .unwrap_or_else(|_| Value::String(row.value.clone()));
            if validate_preference_value(&row.key, &value).is_ok() {
                values.insert(row.key, value);
            } else {
                tracing::warn!(
                    key = %row.key,
                    "skipping invalid legacy installation preference during migration"
                );
            }
        }

        self.write_map_async(values).await?;
        let verified = self.read_map_async().await?.unwrap_or_default();
        let removable_keys: Vec<&str> = key_refs
            .iter()
            .copied()
            .filter(|key| verified.contains_key(*key))
            .collect();
        if !removable_keys.is_empty() {
            repo.delete_keys(&removable_keys).await.map_err(|error| {
                AppError::Internal(format!("remove legacy installation preferences: {error}"))
            })?;
        }
        let remaining = repo.get_by_keys(&INSTALLATION_PREFERENCE_KEYS.to_vec()).await.map_err(|error| {
            AppError::Internal(format!("verify legacy installation preference cleanup: {error}"))
        })?;
        if remaining.is_empty() {
            self.legacy_cleanup_done.store(true, Ordering::Release);
        }
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

    /// Persist the normalized OS locale when `language` has never been stored.
    /// An existing non-empty value is left untouched.
    pub async fn ensure_language_default(&self, os_locale: Option<&str>) -> Result<String, AppError> {
        let values = self.get(Some(&["language"])).await?;
        if let Some(Value::String(existing)) = values.get("language") {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return Ok(normalize_ui_language(Some(trimmed)));
            }
        }
        let language = normalize_ui_language(os_locale);
        self.update(&[("language".into(), Value::String(language.clone()))])
            .await?;
        Ok(language)
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

fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), INSTALLATION_PREFERENCES_BACKUP_SUFFIX))
}

fn validate_map(raw: Map<String, Value>, path: &Path) -> Result<BTreeMap<String, Value>, AppError> {
    let mut values = BTreeMap::new();
    for (key, value) in raw {
        if !is_installation_preference_key(&key) {
            tracing::warn!(key = %key, path = %path.display(), "preserving unknown installation preference key");
            values.insert(key, value);
            continue;
        }
        validate_preference_value(&key, &value)?;
        values.insert(key, value);
    }
    Ok(values)
}

fn read_valid_map(path: &Path) -> Result<Option<(Vec<u8>, BTreeMap<String, Value>)>, AppError> {
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
    Ok(Some((bytes, validate_map(raw, path)?)))
}

fn quarantine_file(path: &Path) -> Result<Option<PathBuf>, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::Internal(format!("inspect corrupt preference {}: {error}", path.display()))),
    }
    let quarantined = PathBuf::from(format!(
        "{}.corrupt-{}-{}",
        path.display(),
        chrono_like_timestamp(),
        Uuid::now_v7()
    ));
    std::fs::rename(path, &quarantined).map_err(|error| {
        AppError::Internal(format!("quarantine installation preference {}: {error}", path.display()))
    })?;
    Ok(Some(quarantined))
}

fn chrono_like_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn read_map_from_path(path: &Path) -> Result<Option<BTreeMap<String, Value>>, AppError> {
    match read_valid_map(path) {
        Ok(Some((_, values))) => return Ok(Some(values)),
        Ok(None) => {}
        Err(main_error) => {
            tracing::warn!(error = %main_error, "installation preference main file is invalid; trying backup");
        }
    }

    let backup = backup_path(path);
    match read_valid_map(&backup) {
        Ok(Some((bytes, values))) => {
            // Replace the main entry only after the backup has passed all
            // bounded-file and JSON validation. A symlink is renamed as an
            // entry, never followed.
            let _ = quarantine_file(path)?;
            dir_config::write_atomic_replace(path, &bytes).map_err(|error| {
                AppError::Internal(format!("restore installation preferences {}: {error}", path.display()))
            })?;
            tracing::warn!(path = %path.display(), "restored installation preferences from backup");
            Ok(Some(values))
        }
        Ok(None) | Err(_) => {
            // Both copies are unusable. Quarantine them before allowing the
            // caller to rebuild from SQLite/defaults; a failed quarantine is
            // propagated so a suspicious file is never silently overwritten.
            let _ = quarantine_file(path)?;
            let _ = quarantine_file(&backup)?;
            Ok(None)
        }
    }
}

fn ensure_replaceable_regular_file(path: &Path) -> Result<(), AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(AppError::Internal(format!(
            "installation preference path is not a regular file: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Internal(format!("inspect installation preference path {}: {error}", path.display()))),
    }
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
    ensure_replaceable_regular_file(path)?;
    let backup = backup_path(path);
    ensure_replaceable_regular_file(&backup)?;
    if let Some((old_bytes, _)) = read_valid_map(path)? {
        dir_config::write_atomic_replace(&backup, &old_bytes).map_err(|error| {
            AppError::Internal(format!("backup installation preferences {}: {error}", backup.display()))
        })?;
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
    async fn merges_partial_json_without_deleting_unmigrated_rows() {
        let data = tempdir().unwrap();
        let db = init_database_memory().await.unwrap();
        let repo = SqliteClientPreferenceRepository::new(db.pool().clone());
        repo.upsert_batch(&[
            ("language", "\"en-US\""),
            ("theme", "\"dark\""),
            ("colorScheme", "\"purple\""),
        ])
        .await
        .unwrap();
        std::fs::write(
            data.path().join(INSTALLATION_PREFERENCES_FILE),
            br#"{"language":"zh-CN","opaque.future":true}"#,
        )
        .unwrap();

        let store = InstallationPreferenceStore::new(data.path());
        store.ensure_migrated(&repo).await.unwrap();

        let values = store.get(None).await.unwrap();
        assert_eq!(values.get("language"), Some(&json!("zh-CN")));
        assert_eq!(values.get("theme"), Some(&json!("dark")));
        assert_eq!(values.get("colorScheme"), Some(&json!("purple")));
        assert_eq!(values.get("opaque.future"), Some(&json!(true)));
        assert!(repo.get_by_keys(&["language", "theme", "colorScheme"]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_oversized_files_by_quarantining_them() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        let path = data.path().join(INSTALLATION_PREFERENCES_FILE);
        std::fs::write(&path, vec![b'x'; (MAX_INSTALLATION_PREFERENCES_BYTES + 1) as usize]).unwrap();
        assert!(store.get(None).await.unwrap().is_empty());
        assert!(std::fs::read_dir(data.path()).unwrap().any(|entry| {
            entry.unwrap().file_name().to_string_lossy().contains(".corrupt-")
        }));
    }

    #[tokio::test]
    async fn recovers_a_corrupt_main_file_from_the_previous_backup() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        let path = data.path().join(INSTALLATION_PREFERENCES_FILE);
        store.update(&[("language".into(), json!("zh-CN"))]).await.unwrap();
        store.update(&[("theme".into(), json!("dark"))]).await.unwrap();
        std::fs::write(&path, b"{\"language\":").unwrap();

        let values = store.get(None).await.unwrap();
        assert_eq!(values.get("language"), Some(&json!("zh-CN")));
        assert!(values.get("theme").is_none());
        assert!(serde_json::from_slice::<Map<String, Value>>(&std::fs::read(&path).unwrap()).is_ok());
    }

    #[tokio::test]
    async fn quarantines_both_copies_when_main_and_backup_are_corrupt() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        let path = data.path().join(INSTALLATION_PREFERENCES_FILE);
        std::fs::write(&path, b"{\"language\":").unwrap();
        std::fs::write(backup_path(&path), b"not-json").unwrap();

        assert!(store.get(None).await.unwrap().is_empty());
        let quarantined = std::fs::read_dir(data.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .count();
        assert_eq!(quarantined, 2);
    }

    #[test]
    fn validates_the_closed_installation_key_set() {
        assert!(is_installation_preference_key("language"));
        assert!(!is_installation_preference_key("nomi.defaultModel"));
        assert!(validate_preference_value("ui.zoomFactor", &json!(1.1)).is_ok());
        assert!(validate_preference_value("ui.zoomFactor", &json!("1.1")).is_err());
    }

    #[tokio::test]
    async fn ensure_language_default_writes_os_locale_when_missing() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        assert_eq!(
            store.ensure_language_default(Some("zh_CN")).await.unwrap(),
            "zh-CN"
        );
        assert_eq!(store.get(None).await.unwrap().get("language"), Some(&json!("zh-CN")));
    }

    #[tokio::test]
    async fn ensure_language_default_does_not_overwrite_existing() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        store.update(&[("language".into(), json!("en-US"))]).await.unwrap();
        assert_eq!(
            store.ensure_language_default(Some("zh-CN")).await.unwrap(),
            "en-US"
        );
        assert_eq!(store.get(None).await.unwrap().get("language"), Some(&json!("en-US")));
    }

    #[tokio::test]
    async fn ensure_language_default_maps_unsupported_os_locale_to_english() {
        let data = tempdir().unwrap();
        let store = InstallationPreferenceStore::new(data.path());
        assert_eq!(
            store.ensure_language_default(Some("ja-JP")).await.unwrap(),
            "en-US"
        );
    }
}
