//! Disk wallpaper library: `{data_dir}/wallpapers/{id}/` plus `index.json`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use image::GenericImageView;
use nomifun_common::{AppError, WallpaperId, now_ms};
use serde::{Deserialize, Serialize};

use crate::analysis::{WallpaperAnalysis, analyze_image};
use crate::media::{content_type_of, is_animated_image, read_sandbox_source, validate_payload};
use crate::transcode::{decode_or_err, encode_display_jpeg, encode_solid_thumb, encode_thumb_jpeg};

pub const WALLPAPERS_REL_DIR: &str = "wallpapers";
const INDEX_FILE: &str = "index.json";
const META_FILE: &str = "meta.json";
const ANALYSIS_FILE: &str = "analysis.json";
const DISPLAY_FILE: &str = "display.jpg";
const THUMB_FILE: &str = "thumb.jpg";
const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperMeta {
    pub wallpaper_id: String,
    pub name: String,
    pub media_kind: String,
    pub original_ext: String,
    pub width: u32,
    pub height: u32,
    pub created_at: i64,
    pub analysis: WallpaperAnalysis,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WallpaperIndex {
    wallpapers: Vec<WallpaperMeta>,
}

pub fn library_root(data_dir: &Path) -> PathBuf {
    data_dir.join(WALLPAPERS_REL_DIR)
}

fn wallpaper_dir(root: &Path, wallpaper_id: &str) -> PathBuf {
    root.join(wallpaper_id)
}

fn original_name(ext: &str) -> String {
    format!("original.{ext}")
}

fn sanitize_name(raw: &str) -> String {
    let trimmed = raw.trim();
    let name: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        "壁纸".to_owned()
    } else {
        name
    }
}

fn is_safe_id(wallpaper_id: &str) -> bool {
    WallpaperId::parse(wallpaper_id).is_ok()
}

fn inventory_error(root: &Path, detail: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!(
        "wallpaper library {} is inconsistent: {detail}",
        root.display()
    ))
}

fn load_index(root: &Path) -> Result<WallpaperIndex, AppError> {
    let path = root.join(INDEX_FILE);
    let index: WallpaperIndex = crate::fsio::load_json_optional(&path)
        .map_err(|error| AppError::Internal(format!("load wallpaper index {}: {error}", path.display())))?
        .unwrap_or_default();
    let mut ids = HashSet::new();
    for wallpaper in &index.wallpapers {
        WallpaperId::parse(&wallpaper.wallpaper_id).map_err(|error| {
            AppError::Internal(format!(
                "wallpaper index {} contains non-canonical id {:?}: {error}",
                path.display(),
                wallpaper.wallpaper_id
            ))
        })?;
        if !ids.insert(wallpaper.wallpaper_id.as_str()) {
            return Err(AppError::Internal(format!(
                "wallpaper index {} contains duplicate id {}",
                path.display(),
                wallpaper.wallpaper_id
            )));
        }
    }
    Ok(index)
}

fn save_index(root: &Path, index: &WallpaperIndex) -> Result<(), AppError> {
    crate::fsio::save_json_atomic(root, INDEX_FILE, index)
        .map_err(|error| AppError::Internal(format!("save wallpaper index: {error}")))
}

fn validate_inventory(root: &Path, index: &WallpaperIndex) -> Result<(), AppError> {
    let indexed: HashSet<&str> = index
        .wallpapers
        .iter()
        .map(|wallpaper| wallpaper.wallpaper_id.as_str())
        .collect();
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if indexed.is_empty() {
                return Ok(());
            }
            return Err(inventory_error(root, "directory is missing but index is non-empty"));
        }
        Err(error) => return Err(inventory_error(root, error)),
    };
    let mut dirs = HashMap::new();
    for entry in entries {
        let entry = entry.map_err(|error| inventory_error(root, error))?;
        let file_name = entry
            .file_name()
            .into_string()
            .map_err(|_| inventory_error(root, "contains a non-UTF8 entry name"))?;
        if file_name == INDEX_FILE || file_name.starts_with('.') {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| inventory_error(root, error))?;
        if !file_type.is_dir() {
            return Err(inventory_error(
                root,
                format!("contains unexpected entry {file_name:?}"),
            ));
        }
        WallpaperId::parse(&file_name).map_err(|error| {
            inventory_error(
                root,
                format!("contains non-canonical wallpaper directory {file_name:?}: {error}"),
            )
        })?;
        dirs.insert(file_name, entry.path());
    }
    for id in &indexed {
        if !dirs.contains_key(*id) {
            return Err(inventory_error(root, format!("indexed wallpaper {id:?} is missing")));
        }
    }
    for id in dirs.keys() {
        if !indexed.contains(id.as_str()) {
            return Err(inventory_error(root, format!("contains orphaned wallpaper {id:?}")));
        }
    }
    Ok(())
}

pub fn open(root: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(root)
        .map_err(|error| AppError::Internal(format!("create wallpaper library {}: {error}", root.display())))?;
    let index = load_index(root)?;
    validate_inventory(root, &index)?;
    Ok(())
}

pub fn list(root: &Path) -> Result<Vec<WallpaperMeta>, AppError> {
    let index = load_index(root)?;
    validate_inventory(root, &index)?;
    Ok(index.wallpapers)
}

pub fn get(root: &Path, wallpaper_id: &str) -> Result<WallpaperMeta, AppError> {
    if !is_safe_id(wallpaper_id) {
        return Err(AppError::BadRequest("invalid wallpaper_id".into()));
    }
    load_index(root)?
        .wallpapers
        .into_iter()
        .find(|wallpaper| wallpaper.wallpaper_id == wallpaper_id)
        .ok_or_else(|| AppError::NotFound(format!("wallpaper {wallpaper_id}")))
}

pub fn ingest(root: &Path, source_path: &Path, name: &str) -> Result<WallpaperMeta, AppError> {
    let bytes = read_sandbox_source(source_path)?;
    let class = validate_payload(&bytes)?;
    let wallpaper_id = WallpaperId::new().to_string();
    let dir = wallpaper_dir(root, &wallpaper_id);
    let original_ext = class.extension();
    let animated = class.is_image() && is_animated_image(class, &bytes);

    let (width, height, analysis, display, thumb) = if class.is_video() {
        let analysis = WallpaperAnalysis::video_placeholder(true);
        let thumb = encode_solid_thumb(&analysis)?;
        (0, 0, analysis, None, thumb)
    } else {
        let image = decode_or_err(&bytes)?;
        let (width, height) = image.dimensions();
        let analysis = analyze_image(&image, animated);
        let thumb = encode_thumb_jpeg(&image)?;
        let display = if animated {
            None
        } else {
            Some(encode_display_jpeg(&image)?)
        };
        (width, height, analysis, display, thumb)
    };

    crate::fsio::save_bytes_atomic(&dir, &original_name(original_ext), &bytes)
        .map_err(|error| AppError::Internal(format!("save wallpaper original: {error}")))?;
    crate::fsio::save_bytes_atomic(&dir, THUMB_FILE, &thumb)
        .map_err(|error| AppError::Internal(format!("save wallpaper thumb: {error}")))?;
    if let Some(display) = display.as_ref() {
        crate::fsio::save_bytes_atomic(&dir, DISPLAY_FILE, display)
            .map_err(|error| AppError::Internal(format!("save wallpaper display: {error}")))?;
    }
    crate::fsio::save_json_atomic(&dir, ANALYSIS_FILE, &analysis)
        .map_err(|error| AppError::Internal(format!("save wallpaper analysis: {error}")))?;

    let media_kind = if class.is_video() {
        "video"
    } else if animated {
        "animated"
    } else {
        "still"
    };
    let meta = WallpaperMeta {
        wallpaper_id: wallpaper_id.clone(),
        name: sanitize_name(name),
        media_kind: media_kind.into(),
        original_ext: original_ext.into(),
        width,
        height,
        created_at: now_ms(),
        analysis,
    };
    crate::fsio::save_json_atomic(&dir, META_FILE, &meta)
        .map_err(|error| AppError::Internal(format!("save wallpaper meta: {error}")))?;

    let mut index = load_index(root)?;
    index.wallpapers.push(meta.clone());
    save_index(root, &index)?;
    tracing::info!(
        wallpaper_id = %wallpaper_id,
        media_kind,
        width,
        height,
        "wallpaper ingested"
    );
    Ok(meta)
}

pub fn rename(root: &Path, wallpaper_id: &str, name: &str) -> Result<WallpaperMeta, AppError> {
    if !is_safe_id(wallpaper_id) {
        return Err(AppError::BadRequest("invalid wallpaper_id".into()));
    }
    let mut index = load_index(root)?;
    let Some(entry) = index
        .wallpapers
        .iter_mut()
        .find(|wallpaper| wallpaper.wallpaper_id == wallpaper_id)
    else {
        return Err(AppError::NotFound(format!("wallpaper {wallpaper_id}")));
    };
    entry.name = sanitize_name(name);
    let meta = entry.clone();
    let dir = wallpaper_dir(root, wallpaper_id);
    crate::fsio::save_json_atomic(&dir, META_FILE, &meta)
        .map_err(|error| AppError::Internal(format!("save wallpaper meta: {error}")))?;
    save_index(root, &index)?;
    Ok(meta)
}

pub fn delete(root: &Path, wallpaper_id: &str) -> Result<(), AppError> {
    if !is_safe_id(wallpaper_id) {
        return Err(AppError::BadRequest("invalid wallpaper_id".into()));
    }
    let mut index = load_index(root)?;
    let before = index.wallpapers.len();
    index.wallpapers.retain(|wallpaper| wallpaper.wallpaper_id != wallpaper_id);
    if index.wallpapers.len() == before {
        return Err(AppError::NotFound(format!("wallpaper {wallpaper_id}")));
    }
    save_index(root, &index)?;
    crate::fsio::remove_path_entry(&wallpaper_dir(root, wallpaper_id))
        .map_err(|error| AppError::Internal(format!("delete wallpaper files: {error}")))
}

pub struct WallpaperBytes {
    pub bytes: Vec<u8>,
    pub mtime: u64,
    pub content_type: &'static str,
}

fn read_file_bytes(path: &Path) -> Result<Option<(Vec<u8>, u64)>, AppError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "inspect wallpaper file {}: {error}",
                path.display()
            )));
        }
    };
    if !metadata.file_type().is_file() {
        return Err(AppError::Internal(format!(
            "wallpaper path is not a regular file: {}",
            path.display()
        )));
    }
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let bytes = std::fs::read(path)
        .map_err(|error| AppError::Internal(format!("read wallpaper file {}: {error}", path.display())))?;
    Ok(Some((bytes, mtime)))
}

fn original_path(root: &Path, meta: &WallpaperMeta) -> PathBuf {
    wallpaper_dir(root, &meta.wallpaper_id).join(original_name(&meta.original_ext))
}

pub fn read_display(root: &Path, wallpaper_id: &str) -> Result<WallpaperBytes, AppError> {
    let meta = get(root, wallpaper_id)?;
    let dir = wallpaper_dir(root, wallpaper_id);
    let preferred = if meta.media_kind == "still" {
        dir.join(DISPLAY_FILE)
    } else {
        original_path(root, &meta)
    };
    if let Some((bytes, mtime)) = read_file_bytes(&preferred)? {
        return Ok(WallpaperBytes {
            content_type: content_type_of(&bytes),
            bytes,
            mtime,
        });
    }
    let (bytes, mtime) = read_file_bytes(&original_path(root, &meta))?
        .ok_or_else(|| AppError::NotFound(format!("wallpaper display {wallpaper_id}")))?;
    Ok(WallpaperBytes {
        content_type: content_type_of(&bytes),
        bytes,
        mtime,
    })
}

pub fn read_thumb(root: &Path, wallpaper_id: &str) -> Result<WallpaperBytes, AppError> {
    let _ = get(root, wallpaper_id)?;
    let (bytes, mtime) = read_file_bytes(&wallpaper_dir(root, wallpaper_id).join(THUMB_FILE))?
        .ok_or_else(|| AppError::NotFound(format!("wallpaper thumb {wallpaper_id}")))?;
    Ok(WallpaperBytes {
        content_type: content_type_of(&bytes),
        bytes,
        mtime,
    })
}

pub fn read_original(root: &Path, wallpaper_id: &str) -> Result<WallpaperBytes, AppError> {
    let meta = get(root, wallpaper_id)?;
    let (bytes, mtime) = read_file_bytes(&original_path(root, &meta))?
        .ok_or_else(|| AppError::NotFound(format!("wallpaper original {wallpaper_id}")))?;
    Ok(WallpaperBytes {
        content_type: content_type_of(&bytes),
        bytes,
        mtime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};
    use std::io::Cursor;

    fn png_bytes(color: [u8; 3]) -> Vec<u8> {
        let img = RgbImage::from_pixel(24, 16, Rgb(color));
        let mut buf = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn stage(bytes: &[u8]) -> PathBuf {
        let dir = crate::media::upload_root();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.png", WallpaperId::new()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn ingest_list_rename_delete_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = library_root(tmp.path());
        open(&root).unwrap();
        let source = stage(&png_bytes([12, 80, 180]));
        let created = ingest(&root, &source, "  Ocean  ").unwrap();
        assert_eq!(created.name, "Ocean");
        assert_eq!(created.media_kind, "still");
        assert_eq!(created.width, 24);
        assert_eq!(created.height, 16);
        assert_eq!(created.original_ext, "png");
        assert!(created.analysis.seed_hex.starts_with('#'));

        let listed = list(&root).unwrap();
        assert_eq!(listed.len(), 1);
        let display = read_display(&root, &created.wallpaper_id).unwrap();
        assert_eq!(display.content_type, "image/jpeg");
        let thumb = read_thumb(&root, &created.wallpaper_id).unwrap();
        assert_eq!(thumb.content_type, "image/jpeg");

        let renamed = rename(&root, &created.wallpaper_id, "Harbor").unwrap();
        assert_eq!(renamed.name, "Harbor");
        delete(&root, &created.wallpaper_id).unwrap();
        assert!(list(&root).unwrap().is_empty());
    }

    #[test]
    fn rejects_path_escape() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = library_root(tmp.path());
        open(&root).unwrap();
        let outside = tmp.path().join("outside.png");
        std::fs::write(&outside, png_bytes([1, 2, 3])).unwrap();
        let err = ingest(&root, &outside, "no").unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_) | AppError::BadRequest(_)));
    }
}
