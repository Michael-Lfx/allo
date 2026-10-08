//! Wallpaper library service. Index mutations are serialized on one mutex.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomifun_common::AppError;
use tokio::sync::Mutex;

use crate::library::{self, WallpaperBytes, WallpaperMeta};

pub struct AppearanceService {
    root: PathBuf,
    lock: Mutex<()>,
}

impl AppearanceService {
    pub fn open(data_dir: &Path) -> Result<Arc<Self>, AppError> {
        let root = library::library_root(data_dir);
        library::open(&root)?;
        Ok(Arc::new(Self {
            root,
            lock: Mutex::new(()),
        }))
    }

    pub async fn list(&self) -> Result<Vec<WallpaperMeta>, AppError> {
        let _guard = self.lock.lock().await;
        library::list(&self.root)
    }

    pub async fn get(&self, wallpaper_id: &str) -> Result<WallpaperMeta, AppError> {
        let _guard = self.lock.lock().await;
        library::get(&self.root, wallpaper_id)
    }

    pub async fn ingest(&self, source_path: &str, name: &str) -> Result<WallpaperMeta, AppError> {
        let _guard = self.lock.lock().await;
        let root = self.root.clone();
        let source = PathBuf::from(source_path);
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || library::ingest(&root, &source, &name))
            .await
            .map_err(|error| AppError::Internal(format!("wallpaper ingest task: {error}")))?
    }

    pub async fn rename(&self, wallpaper_id: &str, name: &str) -> Result<WallpaperMeta, AppError> {
        let _guard = self.lock.lock().await;
        library::rename(&self.root, wallpaper_id, name)
    }

    pub async fn delete(&self, wallpaper_id: &str) -> Result<(), AppError> {
        let _guard = self.lock.lock().await;
        library::delete(&self.root, wallpaper_id)
    }

    pub async fn read_display(&self, wallpaper_id: &str) -> Result<WallpaperBytes, AppError> {
        let _guard = self.lock.lock().await;
        library::read_display(&self.root, wallpaper_id)
    }

    pub async fn read_thumb(&self, wallpaper_id: &str) -> Result<WallpaperBytes, AppError> {
        let _guard = self.lock.lock().await;
        library::read_thumb(&self.root, wallpaper_id)
    }

    pub async fn read_original(&self, wallpaper_id: &str) -> Result<WallpaperBytes, AppError> {
        let _guard = self.lock.lock().await;
        library::read_original(&self.root, wallpaper_id)
    }
}
