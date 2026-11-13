//! Local persistence for cloud JWT + profile snapshot.

use crate::error::AuthError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub access_token: String,
    pub email: String,
    pub display_name: String,
    pub user_id: Option<i64>,
}

#[derive(Debug)]
pub struct SessionStore {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    access_token: String,
    email: String,
    display_name: String,
    #[serde(default)]
    user_id: Option<i64>,
}

impl SessionStore {
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn load(&self) -> Option<Session> {
        let file: SessionFile = read_json(&self.path())?;
        if file.version != FORMAT_VERSION || file.access_token.is_empty() {
            return None;
        }
        Some(Session {
            access_token: file.access_token,
            email: file.email,
            display_name: file.display_name,
            user_id: file.user_id,
        })
    }

    pub fn save(&self, session: &Session) -> Result<(), AuthError> {
        let file = SessionFile {
            version: FORMAT_VERSION,
            access_token: session.access_token.clone(),
            email: session.email.clone(),
            display_name: session.display_name.clone(),
            user_id: session.user_id,
        };
        write_json(&self.path(), &file)
    }

    pub fn clear(&self) {
        let _ = fs::remove_file(self.path());
        let _ = fs::remove_file(self.root.join("session.json"));
        let _ = fs::remove_file(self.root.join("accounts.json"));
    }

    fn path(&self) -> PathBuf {
        self.root.join("auth").join("session.json")
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AuthError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| AuthError::Store)?;
    flowy_core::persistence::atomic_write(path, &bytes)
        .map_err(|_| AuthError::Store)
}

#[cfg(test)]
mod tests {
    use super::{Session, SessionStore};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> SessionStore {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root =
            std::env::temp_dir().join(format!("flowy-auth-session-{nanos}-{id}"));
        let _ = std::fs::remove_dir_all(&root);
        SessionStore::new(root)
    }

    #[test]
    fn save_load_clear_roundtrip() {
        let store = temp_store();
        assert!(store.load().is_none());
        let session = Session {
            access_token: "jwt-abc".into(),
            email: "ada@example.com".into(),
            display_name: "Ada".into(),
            user_id: Some(42),
        };
        store.save(&session).unwrap();
        assert_eq!(store.load().unwrap(), session);
        store.clear();
        assert!(store.load().is_none());
    }
}
