//! Ingest-time wallpaper analysis: luminance, spatial variance, and a seed palette.
//!
//! First-principles mapper: sRGB → relative luminance + CIE Lab, then hue-binned
//! accent extraction. This is cached in `analysis.json` so the renderer never
//! runs ColorThief on every paint. It does **not** replace the CSS skin — it
//! only recommends scrim/plate strength and an optional accent scale.

use image::{DynamicImage, GenericImageView, imageops::FilterType};
use serde::{Deserialize, Serialize};

const ANALYSIS_EDGE: u32 = 128;
const BLOCK: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperAnalysis {
    pub version: u32,
    pub seed_hex: String,
    pub mean_lstar: f32,
    pub luminance_variance: f32,
    pub recommended_scheme: String,
    pub recommended_dim: f32,
    pub recommended_blur_px: f32,
    pub recommended_plate_alpha: f32,
    pub primary_rgb: [u8; 3],
    pub primary_scale: [[u8; 3]; 7],
    pub busy: bool,
    pub animated: bool,
}

impl WallpaperAnalysis {
    pub fn video_placeholder(animated: bool) -> Self {
        let primary = [107, 114, 128];
        Self {
            version: 1,
            seed_hex: hex_of(primary),
            mean_lstar: 50.0,
            luminance_variance: 0.62,
            recommended_scheme: "dark".into(),
            recommended_dim: 0.42,
            recommended_blur_px: 8.0,
            recommended_plate_alpha: 0.78,
            primary_rgb: primary,
            primary_scale: primary_scale_from_rgb(primary),
            busy: true,
            animated,
        }
    }
}

pub fn analyze_image(image: &DynamicImage, animated: bool) -> WallpaperAnalysis {
    let small = image.resize_exact(ANALYSIS_EDGE, ANALYSIS_EDGE, FilterType::Lanczos3);
    let mut sum_l = 0.0f64;
    let mut sum_r = 0.0f64;
    let mut sum_g = 0.0f64;
    let mut sum_b = 0.0f64;
    let mut count = 0.0f64;
    let mut hue_weights = [0.0f64; 16];
    let mut hue_rgb = [[0.0f64; 3]; 16];

    for (_, _, pixel) in small.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 16 {
            continue;
        }
        let rf = f64::from(r);
        let gf = f64::from(g);
        let bf = f64::from(b);
        let lab = rgb_to_lab(r, g, b);
        sum_l += f64::from(lab.0);
        sum_r += rf;
        sum_g += gf;
        sum_b += bf;
        count += 1.0;
        let chroma = f64::from(lab.1.hypot(lab.2));
        if chroma > 8.0 {
            let hue = hue_deg(lab.1, lab.2);
            let bin = ((hue / 22.5).floor() as usize).min(15);
            hue_weights[bin] += chroma;
            hue_rgb[bin][0] += rf * chroma;
            hue_rgb[bin][1] += gf * chroma;
            hue_rgb[bin][2] += bf * chroma;
        }
    }

    if count < 1.0 {
        return WallpaperAnalysis::video_placeholder(animated);
    }

    let mean_lstar = (sum_l / count) as f32;
    let seed = [
        (sum_r / count).round().clamp(0.0, 255.0) as u8,
        (sum_g / count).round().clamp(0.0, 255.0) as u8,
        (sum_b / count).round().clamp(0.0, 255.0) as u8,
    ];

    let mut best_bin = 0usize;
    let mut best_weight = 0.0f64;
    for (i, weight) in hue_weights.iter().enumerate() {
        if *weight > best_weight {
            best_weight = *weight;
            best_bin = i;
        }
    }
    let primary = if best_weight > 0.0 {
        [
            (hue_rgb[best_bin][0] / best_weight).round().clamp(0.0, 255.0) as u8,
            (hue_rgb[best_bin][1] / best_weight).round().clamp(0.0, 255.0) as u8,
            (hue_rgb[best_bin][2] / best_weight).round().clamp(0.0, 255.0) as u8,
        ]
    } else {
        seed
    };

    let variance = block_luminance_variance(&small);
    let busy = variance > 0.18;
    let dim = (if mean_lstar > 60.0 { 0.22 } else { 0.32 } + variance * 0.38).clamp(0.12, 0.80);
    let blur = if variance > 0.22 {
        (6.0 + (variance - 0.22) * 40.0).clamp(6.0, 18.0)
    } else {
        0.0
    };
    let plate = (0.55 + variance * 0.40).clamp(0.55, 0.86);
    let scheme = if mean_lstar > 60.0 { "light" } else { "dark" };

    WallpaperAnalysis {
        version: 1,
        seed_hex: hex_of(seed),
        mean_lstar,
        luminance_variance: (variance as f32).clamp(0.0, 1.0),
        recommended_scheme: scheme.into(),
        recommended_dim: dim as f32,
        recommended_blur_px: blur as f32,
        recommended_plate_alpha: plate as f32,
        primary_rgb: primary,
        primary_scale: primary_scale_from_rgb(primary),
        busy,
        animated,
    }
}

fn block_luminance_variance(small: &DynamicImage) -> f64 {
    let (width, height) = small.dimensions();
    let bw = width / BLOCK;
    let bh = height / BLOCK;
    if bw == 0 || bh == 0 {
        return 0.0;
    }
    let mut means = Vec::with_capacity((bw * bh) as usize);
    for by in 0..bh {
        for bx in 0..bw {
            let mut sum = 0.0f64;
            let mut n = 0.0f64;
            for y in by * BLOCK..(by + 1) * BLOCK {
                for x in bx * BLOCK..(bx + 1) * BLOCK {
                    let px = small.get_pixel(x, y);
                    if px.0[3] < 16 {
                        continue;
                    }
                    sum += f64::from(relative_luminance(px.0[0], px.0[1], px.0[2]));
                    n += 1.0;
                }
            }
            if n > 0.0 {
                means.push(sum / n);
            }
        }
    }
    if means.len() < 2 {
        return 0.0;
    }
    let avg = means.iter().sum::<f64>() / means.len() as f64;
    let var = means.iter().map(|m| (m - avg).powi(2)).sum::<f64>() / means.len() as f64;
    // Relative luminance variance of 8×8 means sits roughly in 0..0.08 for photos.
    (var * 18.0).clamp(0.0, 1.0)
}

fn srgb_to_linear(channel: u8) -> f32 {
    let unit = f32::from(channel) / 255.0;
    if unit <= 0.04045 {
        unit / 12.92
    } else {
        ((unit + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

fn rgb_to_lab(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rl = srgb_to_linear(r);
    let gl = srgb_to_linear(g);
    let bl = srgb_to_linear(b);
    let x = 0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl;
    let y = 0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl;
    let z = 0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl;
    const XN: f32 = 0.95047;
    const YN: f32 = 1.0;
    const ZN: f32 = 1.08883;
    let fx = lab_f(x / XN);
    let fy = lab_f(y / YN);
    let fz = lab_f(z / ZN);
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

fn lab_f(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;
    if t > DELTA.powi(3) {
        t.cbrt()
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

fn hue_deg(a: f32, b: f32) -> f64 {
    let mut deg = f64::from(b.atan2(a)).to_degrees();
    if deg < 0.0 {
        deg += 360.0;
    }
    deg
}

pub fn hex_of(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn primary_scale_from_rgb(rgb: [u8; 3]) -> [[u8; 3]; 7] {
    let (h, s, _) = rgb_to_hsl(rgb);
    let sat = s.clamp(0.35, 0.72);
    let lights = [0.94, 0.86, 0.74, 0.62, 0.50, 0.40, 0.30];
    let mut scale = [[0u8; 3]; 7];
    for (i, light) in lights.iter().enumerate() {
        scale[i] = hsl_to_rgb(h, sat, *light);
    }
    scale
}

fn rgb_to_hsl(rgb: [u8; 3]) -> (f32, f32, f32) {
    let r = f32::from(rgb[0]) / 255.0;
    let g = f32::from(rgb[1]) / 255.0;
    let b = f32::from(rgb[2]) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let hue_to_rgb = |p: f32, q: f32, mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s.abs() < f32::EPSILON {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (
            hue_to_rgb(p, q, h + 1.0 / 3.0),
            hue_to_rgb(p, q, h),
            hue_to_rgb(p, q, h - 1.0 / 3.0),
        )
    };
    [
        (r * 255.0).round().clamp(0.0, 255.0) as u8,
        (g * 255.0).round().clamp(0.0, 255.0) as u8,
        (b * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|error| format!("decode wallpaper image: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn white_field_recommends_light_and_low_variance() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([250, 250, 250])));
        let analysis = analyze_image(&img, false);
        assert_eq!(analysis.recommended_scheme, "light");
        assert!(analysis.luminance_variance < 0.05, "{}", analysis.luminance_variance);
        assert!(!analysis.busy);
        assert!(analysis.recommended_dim < 0.40);
        assert_eq!(analysis.seed_hex, "#FAFAFA");
    }

    #[test]
    fn checkerboard_is_busy() {
        let mut img = RgbImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                let on = ((x / 4) + (y / 4)) % 2 == 0;
                img.put_pixel(x, y, Rgb(if on { [20, 20, 20] } else { [230, 230, 230] }));
            }
        }
        let analysis = analyze_image(&DynamicImage::ImageRgb8(img), false);
        assert!(analysis.busy, "variance {}", analysis.luminance_variance);
        assert!(analysis.recommended_plate_alpha >= 0.70);
        assert!(analysis.recommended_blur_px >= 6.0);
    }

    #[test]
    fn primary_scale_has_seven_rgb_triplets() {
        let scale = primary_scale_from_rgb([30, 90, 200]);
        assert_eq!(scale.len(), 7);
        assert!(scale[0][0] >= scale[6][0] || scale[0][1] >= scale[6][1] || scale[0][2] >= scale[6][2]);
    }
}
