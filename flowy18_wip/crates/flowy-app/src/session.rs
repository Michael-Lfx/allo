//! App data root. Cloud session persistence lives in `flowy_auth`.

use std::path::PathBuf;

pub use flowy_auth::{AuthError, Session, SessionStore, valid_email};

#[must_use]
pub fn default_root() -> PathBuf {
    directories::ProjectDirs::from(
        crate::brand::qualifier_tld(),
        crate::brand::qualifier_org(),
        crate::brand::qualifier_app(),
    )
    .map(|dirs| dirs.data_dir().to_path_buf())
    .or_else(|| std::env::current_dir().ok())
    .unwrap_or_else(|| PathBuf::from("."))
}
