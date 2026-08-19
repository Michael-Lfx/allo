//! Session-scoped action-imitation inputs — one character still + one motion video.
//!
//! Layout (under `{artifact_root}/` = `action2video/`):
//! ```text
//! action2video/
//!   character.{png|jpg|webp}
//!   reference.{mp4|webm|avi}
//!   prompt.txt
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{
    image_magic_kind, is_usable_image_file, is_usable_video_file, video_magic_kind,
    write_image_bytes_atomic, write_video_bytes_atomic,
};

pub const CHARACTER_STEM: &str = "character";
pub const REFERENCE_STEM: &str = "reference";
pub const PROMPT_FILENAME: &str = "prompt.txt";
pub const FINAL_VIDEO_FILENAME: &str = "final_video.mp4";

pub const CHARACTER_MAX_BYTES: usize = 10 * 1024 * 1024;
pub const REFERENCE_MAX_BYTES: usize = 80 * 1024 * 1024;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const VIDEO_EXTS: &[&str] = &["mp4", "webm", "avi", "mov", "m4v"];

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ActionAssetsInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_video: Option<String>,
}

impl ActionAssetsInfo {
    pub fn is_complete(&self) -> bool {
        self.character.is_some() && self.reference_video.is_some()
    }
}

pub fn builtin_prompt() -> &'static str {
    include_str!("../../prompts/action2video__builtin_prompt.txt").trim()
}

pub fn find_assets(artifact_dir: &Path) -> ActionAssetsInfo {
    ActionAssetsInfo {
        character: find_stem(artifact_dir, CHARACTER_STEM, IMAGE_EXTS)
            .filter(|p| is_usable_image_file(p))
            .map(|p| p.to_string_lossy().replace('\\', "/")),
        reference_video: find_stem(artifact_dir, REFERENCE_STEM, VIDEO_EXTS)
            .filter(|p| is_usable_video_file(p))
            .map(|p| p.to_string_lossy().replace('\\', "/")),
    }
}

pub fn character_abs(artifact_dir: &Path) -> Option<PathBuf> {
    find_stem(artifact_dir, CHARACTER_STEM, IMAGE_EXTS).filter(|p| is_usable_image_file(p))
}

pub fn reference_abs(artifact_dir: &Path) -> Option<PathBuf> {
    find_stem(artifact_dir, REFERENCE_STEM, VIDEO_EXTS).filter(|p| is_usable_video_file(p))
}

pub fn require_assets(artifact_dir: &Path) -> VimaxResult<(PathBuf, PathBuf)> {
    let character = character_abs(artifact_dir).ok_or_else(|| {
        VimaxError::InvalidParams("action imitation requires a character image".into())
    })?;
    let video = reference_abs(artifact_dir).ok_or_else(|| {
        VimaxError::InvalidParams("action imitation requires a reference video".into())
    })?;
    Ok((character, video))
}

/// Persist a character still. Replaces any previous `character.*`.
pub async fn save_character(artifact_dir: &Path, bytes: &[u8]) -> VimaxResult<PathBuf> {
    if bytes.len() > CHARACTER_MAX_BYTES {
        return Err(VimaxError::InvalidParams(format!(
            "character image too large ({} bytes); max is {CHARACTER_MAX_BYTES}",
            bytes.len()
        )));
    }
    let kind = image_magic_kind(bytes).ok_or_else(|| {
        VimaxError::InvalidParams("character image must be PNG, JPEG, or WEBP".into())
    })?;
    let ext = match kind {
        "jpeg" => "jpg",
        other => other,
    };
    tokio::fs::create_dir_all(artifact_dir).await?;
    clear_stem(artifact_dir, CHARACTER_STEM, IMAGE_EXTS).await?;
    let dest = artifact_dir.join(format!("{CHARACTER_STEM}.{ext}"));
    // Keep original JPEG/WEBP bytes (identity fidelity). PNG goes through the atomic
    // raster writer so a crash cannot leave a truncated file.
    if kind == "png" {
        write_image_bytes_atomic(bytes, &dest)?;
    } else {
        atomic_write_bytes(&dest, bytes).await?;
    }
    Ok(dest)
}

/// Persist a motion-reference video. Replaces any previous `reference.*`.
pub async fn save_reference_video(artifact_dir: &Path, bytes: &[u8]) -> VimaxResult<PathBuf> {
    if bytes.len() > REFERENCE_MAX_BYTES {
        return Err(VimaxError::InvalidParams(format!(
            "reference video too large ({} bytes); max is {REFERENCE_MAX_BYTES}",
            bytes.len()
        )));
    }
    let kind = video_magic_kind(bytes).ok_or_else(|| {
        VimaxError::InvalidParams("reference video must be MP4, MOV, WebM, or AVI".into())
    })?;
    let ext = match kind {
        "webm" => "webm",
        "avi" => "avi",
        _ => "mp4",
    };
    tokio::fs::create_dir_all(artifact_dir).await?;
    clear_stem(artifact_dir, REFERENCE_STEM, VIDEO_EXTS).await?;
    let dest = artifact_dir.join(format!("{REFERENCE_STEM}.{ext}"));
    write_video_bytes_atomic(&dest, bytes).await?;
    Ok(dest)
}

fn find_stem(dir: &Path, stem: &str, exts: &[&str]) -> Option<PathBuf> {
    for ext in exts {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

async fn clear_stem(dir: &Path, stem: &str, exts: &[&str]) -> VimaxResult<()> {
    for ext in exts {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.exists() {
            tokio::fs::remove_file(&p).await?;
        }
    }
    Ok(())
}

async fn atomic_write_bytes(dest: &Path, bytes: &[u8]) -> VimaxResult<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|s| s.to_str()).unwrap_or("bin")
    ));
    tokio::fs::write(&tmp, bytes).await?;
    if dest.exists() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    tokio::fs::rename(&tmp, dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        // 1×1 transparent PNG.
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    fn fake_mp4(len: usize) -> Vec<u8> {
        let mut b = vec![0u8; len.max(32)];
        b[4..8].copy_from_slice(b"ftyp");
        b
    }

    #[tokio::test]
    async fn save_and_find_both_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("action2video");
        save_character(&root, &png_bytes()).await.unwrap();
        save_reference_video(&root, &fake_mp4(8192)).await.unwrap();
        let info = find_assets(&root);
        assert!(info.is_complete());
        assert!(info.character.as_deref().unwrap().ends_with("character.png"));
        assert!(
            info.reference_video
                .as_deref()
                .unwrap()
                .ends_with("reference.mp4")
        );
        require_assets(&root).unwrap();
    }

    #[tokio::test]
    async fn replacing_character_drops_previous_ext() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        save_character(root, &png_bytes()).await.unwrap();
        assert!(root.join("character.png").is_file());
        // JPEG SOI + filler so size is non-trivial; magic still jpeg.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.resize(64, 0x00);
        save_character(root, &jpeg).await.unwrap();
        assert!(!root.join("character.png").exists());
        assert!(root.join("character.jpg").is_file());
    }

    #[test]
    fn builtin_prompt_is_nonempty() {
        assert!(builtin_prompt().contains("reference video"));
        assert!(builtin_prompt().contains("参考视频"));
    }

    #[test]
    fn rejects_missing_assets() {
        let dir = tempfile::tempdir().unwrap();
        let err = require_assets(dir.path()).unwrap_err();
        assert!(err.to_string().contains("character image"));
    }
}
