//! Magic-number classification for wallpaper ingest. No filename trust.

use nomifun_common::AppError;

pub const WALLPAPER_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const WALLPAPER_MAX_DIM: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaClass {
    Jpeg,
    Png,
    Webp,
    Gif,
    Mp4,
    Webm,
}

impl MediaClass {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Gif => "gif",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Mp4 => "video/mp4",
            Self::Webm => "video/webm",
        }
    }

    pub fn is_video(self) -> bool {
        matches!(self, Self::Mp4 | Self::Webm)
    }

    pub fn is_image(self) -> bool {
        !self.is_video()
    }
}

pub fn classify_magic(bytes: &[u8]) -> Option<MediaClass> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some(MediaClass::Jpeg);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(MediaClass::Png);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(MediaClass::Webp);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(MediaClass::Gif);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(MediaClass::Mp4);
    }
    if bytes.len() >= 4 && bytes[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return Some(MediaClass::Webm);
    }
    None
}

pub fn content_type_of(bytes: &[u8]) -> &'static str {
    classify_magic(bytes).map(MediaClass::content_type).unwrap_or("application/octet-stream")
}

pub fn is_animated_image(class: MediaClass, bytes: &[u8]) -> bool {
    match class {
        MediaClass::Gif => gif_frame_count(bytes) > 1,
        MediaClass::Webp => webp_is_animated(bytes),
        _ => false,
    }
}

fn gif_frame_count(bytes: &[u8]) -> usize {
    // Image separators (0x2C) are a stable lower bound on frame count.
    bytes.iter().filter(|byte| **byte == 0x2C).count()
}

fn webp_is_animated(bytes: &[u8]) -> bool {
    // VP8X: flags at payload byte 0 (file offset 20). Animation flag is bit 1.
    bytes.len() >= 21 && &bytes[12..16] == b"VP8X" && (bytes[20] & 0x02) != 0
}

pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return None;
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some((width, height));
    }
    if bytes.len() >= 16 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return match &bytes[12..16] {
            b"VP8X" if bytes.len() >= 30 => {
                let le24 = |b: &[u8]| u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16;
                Some((le24(&bytes[24..27]) + 1, le24(&bytes[27..30]) + 1))
            }
            b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2F => {
                let packed = u32::from_le_bytes(bytes[21..25].try_into().ok()?);
                Some(((packed & 0x3FFF) + 1, ((packed >> 14) & 0x3FFF) + 1))
            }
            b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9D, 0x01, 0x2A] => {
                let width = u16::from_le_bytes(bytes[26..28].try_into().ok()?) & 0x3FFF;
                let height = u16::from_le_bytes(bytes[28..30].try_into().ok()?) & 0x3FFF;
                Some((u32::from(width), u32::from(height)))
            }
            _ => None,
        };
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        if bytes.len() < 10 {
            return None;
        }
        let width = u16::from_le_bytes(bytes[6..8].try_into().ok()?);
        let height = u16::from_le_bytes(bytes[8..10].try_into().ok()?);
        return Some((u32::from(width), u32::from(height)));
    }
    jpeg_dimensions(bytes)
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 8 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        i += 2;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 2 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes(bytes[i..i + 2].try_into().ok()?) as usize;
        if length < 2 || i + length > bytes.len() {
            return None;
        }
        // SOF0 / SOF1 / SOF2 carry height then width as BE u16.
        if matches!(marker, 0xC0 | 0xC1 | 0xC2) && length >= 7 {
            let height = u16::from_be_bytes(bytes[i + 3..i + 5].try_into().ok()?);
            let width = u16::from_be_bytes(bytes[i + 5..i + 7].try_into().ok()?);
            return Some((u32::from(width), u32::from(height)));
        }
        i += length;
    }
    None
}

pub fn upload_root() -> std::path::PathBuf {
    std::env::temp_dir().join(nomifun_common::storage_paths::TEMP_UPLOAD_SANDBOX)
}

pub fn read_sandbox_source(source_path: &std::path::Path) -> Result<Vec<u8>, AppError> {
    let canonical = std::fs::canonicalize(source_path).map_err(|error| {
        AppError::BadRequest(format!(
            "cannot resolve wallpaper source '{}': {error}",
            source_path.display()
        ))
    })?;
    let inside_root = std::fs::canonicalize(upload_root()).is_ok_and(|root| canonical.starts_with(&root));
    if !inside_root {
        return Err(AppError::Forbidden(format!(
            "wallpaper source '{}' is outside the allowed sandbox",
            source_path.display()
        )));
    }
    let size = std::fs::metadata(&canonical)
        .map_err(|error| AppError::Internal(format!("stat wallpaper source: {error}")))?
        .len();
    if size > WALLPAPER_MAX_BYTES {
        return Err(AppError::BadRequest(format!(
            "wallpaper file is too large: {size} bytes (max {WALLPAPER_MAX_BYTES})"
        )));
    }
    std::fs::read(&canonical).map_err(|error| AppError::Internal(format!("read wallpaper source: {error}")))
}

pub fn validate_payload(bytes: &[u8]) -> Result<MediaClass, AppError> {
    if bytes.len() as u64 > WALLPAPER_MAX_BYTES {
        return Err(AppError::BadRequest(format!(
            "wallpaper file is too large: {} bytes (max {WALLPAPER_MAX_BYTES})",
            bytes.len()
        )));
    }
    let class = classify_magic(bytes).ok_or_else(|| {
        AppError::BadRequest("wallpaper file is not a JPEG, PNG, WebP, GIF, MP4, or WebM".into())
    })?;
    if class.is_image() {
        let (width, height) = image_dimensions(bytes)
            .ok_or_else(|| AppError::BadRequest("无法解析图像尺寸".into()))?;
        if width == 0 || height == 0 {
            return Err(AppError::BadRequest("图像尺寸无效".into()));
        }
        if width > WALLPAPER_MAX_DIM || height > WALLPAPER_MAX_DIM {
            return Err(AppError::BadRequest(format!(
                "图像尺寸 {width}x{height} 超出上限 {WALLPAPER_MAX_DIM}x{WALLPAPER_MAX_DIM}"
            )));
        }
    }
    Ok(class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_png_and_gif_magic() {
        assert_eq!(classify_magic(b"\x89PNG\r\n\x1a\nrest"), Some(MediaClass::Png));
        assert_eq!(classify_magic(b"GIF89a...."), Some(MediaClass::Gif));
        assert_eq!(classify_magic(b"not-an-image"), None);
    }

    #[test]
    fn png_header_dimensions() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&32u32.to_be_bytes());
        bytes.extend_from_slice(&16u32.to_be_bytes());
        assert_eq!(image_dimensions(&bytes), Some((32, 16)));
    }
}
