//! Post-generation quality checks for persisted media.

use std::path::Path;

use nomi_types::ToolError;

const MIN_IMAGE_BYTES: usize = 512;
const MIN_VIDEO_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaReport {
    pub passed: bool,
    pub issues: Vec<String>,
}

impl QaReport {
    pub fn into_result(self, context: &str) -> Result<(), ToolError> {
        if self.passed {
            return Ok(());
        }
        Err(ToolError::ExecutionFailed(format!(
            "{context} QA failed: {}",
            self.issues.join("; ")
        )))
    }
}

/// Validate a persisted image file.
pub fn qa_check_image(path: &Path, bytes: &[u8]) -> QaReport {
    let mut issues = Vec::new();
    if bytes.len() < MIN_IMAGE_BYTES {
        issues.push(format!("file too small ({} bytes)", bytes.len()));
    }
    if !path.exists() {
        issues.push("local file missing".into());
    }
    if let Ok(reader) = image::ImageReader::open(path) {
        if let Ok(img) = reader.decode() {
            let w = img.width();
            let h = img.height();
            if w < 64 || h < 64 {
                issues.push(format!("resolution too low ({w}x{h})"));
            }
            if is_mostly_blank(&img) {
                issues.push("image appears mostly blank/uniform".into());
            }
        } else {
            issues.push("could not decode image".into());
        }
    } else {
        issues.push("could not open image".into());
    }
    QaReport {
        passed: issues.is_empty(),
        issues,
    }
}

/// Validate a persisted video file (size + presence).
pub fn qa_check_video(path: &Path, size_bytes: u64) -> QaReport {
    let mut issues = Vec::new();
    if !path.exists() {
        issues.push("local file missing".into());
    }
    if size_bytes < MIN_VIDEO_BYTES {
        issues.push(format!("file too small ({size_bytes} bytes)"));
    }
    QaReport {
        passed: issues.is_empty(),
        issues,
    }
}

fn is_mostly_blank(img: &image::DynamicImage) -> bool {
    let thumb = img.thumbnail(32, 32);
    let rgba = thumb.to_rgba8();
    let pixels: Vec<_> = rgba.pixels().collect();
    if pixels.is_empty() {
        return true;
    }
    let first = pixels[0].0;
    let uniform = pixels.iter().filter(|p| p.0 == first).count();
    uniform * 100 / pixels.len() > 95
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_tiny_image() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("tiny.png");
        std::fs::write(&path, b"x").expect("write");
        let report = qa_check_image(&path, b"x");
        assert!(!report.passed);
    }

    #[test]
    fn accepts_small_png() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("ok.png");
        let mut img = image::RgbaImage::new(128, 128);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgba([
                ((x * 3) % 256) as u8,
                ((y * 5) % 256) as u8,
                ((x + y) % 256) as u8,
                255,
            ]);
        }
        let mut bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("encode");
        std::fs::write(&path, &bytes).expect("write");
        let report = qa_check_image(&path, &bytes);
        assert!(report.passed, "{:?}", report.issues);
    }
}
