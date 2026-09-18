//! Shared temporary storage for market installers.
//!
//! Network payloads and extracted Skills stay below the user's Skill root
//! until the installer takes the shared market commit lock and moves a
//! validated directory into place.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use nomifun_common::AppError;

use crate::skill_service::SkillPaths;

pub(crate) struct MarketStaging {
    pub(crate) root: PathBuf,
}

impl Drop for MarketStaging {
    fn drop(&mut self) {
        // Drop is the final cancellation/unwind guard. The extracted archive
        // is untrusted input and must not survive an interrupted install. Do
        // not recurse synchronously on a Tokio worker: archives are bounded
        // but can still make cancellation block the runtime.
        let root = self.root.clone();
        let cleanup = move || {
            let _ = std::fs::remove_dir_all(root);
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(cleanup);
        } else {
            let _ = std::thread::Builder::new()
                .name("market-cleanup".into())
                .spawn(cleanup);
        }
    }
}

pub(crate) async fn create_market_staging(
    paths: &SkillPaths,
    prefix: &str,
) -> Result<MarketStaging, AppError> {
    let parent = paths.user_skills_dir.join(".market-import");
    tokio::fs::create_dir_all(&parent)
        .await
        .map_err(|error| AppError::Internal(format!("create market staging directory: {error}")))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = parent.join(format!("{prefix}-{}-{nonce}", std::process::id()));
    if let Err(error) = tokio::fs::create_dir(&root).await {
        return Err(AppError::Internal(format!("create market staging root: {error}")));
    }
    Ok(MarketStaging { root })
}
