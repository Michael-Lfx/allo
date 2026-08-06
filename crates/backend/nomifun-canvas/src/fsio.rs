//! Atomic file helpers for canvas docs / media.

use std::path::Path;

use nomifun_common::AppError;

pub async fn ensure_dir(path: &Path) -> Result<(), AppError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| AppError::Internal(format!("mkdir {}: {e}", path.display())))
}

pub async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Internal(format!("path has no parent: {}", path.display()))
    })?;
    ensure_dir(parent).await?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file"),
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(|e| AppError::Internal(format!("write temp {}: {e}", tmp.display())))?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Internal(format!(
            "rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        ))
    })?;
    Ok(())
}

pub async fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| AppError::NotFound(format!("{}: {e}", path.display())))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Internal(format!("parse {}: {e}", path.display())))
}

pub async fn write_json_file<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| AppError::Internal(format!("serialize {}: {e}", path.display())))?;
    write_atomic(path, &bytes).await
}
