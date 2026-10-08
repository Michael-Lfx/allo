//! Display / thumbnail JPEG transcodes. Photos compress well as JPEG without
//! pulling a native libwebp encoder into the server link.

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, ImageEncoder, imageops::FilterType};
use nomifun_common::AppError;

use crate::analysis::WallpaperAnalysis;

pub const DISPLAY_LONG_EDGE: u32 = 2560;
pub const THUMB_LONG_EDGE: u32 = 320;
const DISPLAY_QUALITY: u8 = 85;
const THUMB_QUALITY: u8 = 78;

pub fn encode_display_jpeg(image: &DynamicImage) -> Result<Vec<u8>, AppError> {
    encode_resized_jpeg(image, DISPLAY_LONG_EDGE, DISPLAY_QUALITY)
}

pub fn encode_thumb_jpeg(image: &DynamicImage) -> Result<Vec<u8>, AppError> {
    encode_resized_jpeg(image, THUMB_LONG_EDGE, THUMB_QUALITY)
}

pub fn encode_solid_thumb(analysis: &WallpaperAnalysis) -> Result<Vec<u8>, AppError> {
    let rgb = analysis.primary_rgb;
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        THUMB_LONG_EDGE,
        THUMB_LONG_EDGE * 9 / 16,
        image::Rgb(rgb),
    ));
    encode_jpeg(&img, THUMB_QUALITY)
}

fn encode_resized_jpeg(image: &DynamicImage, long_edge: u32, quality: u8) -> Result<Vec<u8>, AppError> {
    let (width, height) = image.dimensions();
    let max_side = width.max(height);
    let prepared = if max_side > long_edge {
        image.resize(long_edge, long_edge, FilterType::Lanczos3)
    } else {
        image.clone()
    };
    encode_jpeg(&prepared, quality)
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>, AppError> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder
        .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .map_err(|error| AppError::Internal(format!("encode wallpaper jpeg: {error}")))?;
    Ok(out)
}

pub fn decode_or_err(bytes: &[u8]) -> Result<DynamicImage, AppError> {
    crate::analysis::decode_image(bytes).map_err(AppError::BadRequest)
}
