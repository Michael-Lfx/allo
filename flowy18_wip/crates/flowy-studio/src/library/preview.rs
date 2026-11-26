//! Background-only document loading and bounded page rasterization.
use gpui::RenderImage;
use hayro::hayro_syntax::Pdf;
use image::{Frame, ImageDecoder as _, ImageError, ImageReader, RgbaImage};
use oxml_opc::{OpcError, OpcPackage, PackageReadLimits};
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_PIXELS: u32 = 16_000_000;
const MAX_SIDE: u32 = 16_000;
pub(super) const CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_INPUT: u64 = 128 * 1024 * 1024;
const MAX_TEXT_PREVIEW_BYTES: u64 = 1024 * 1024;
const MAX_JSON_PRETTY_BYTES: usize = 256 * 1024;
const MAX_CSV_ROWS: usize = 500;
const MAX_CSV_COLS: usize = 64;
const PPTX_READ_LIMITS: PackageReadLimits = PackageReadLimits {
    max_entries: 4096,
    max_part_uncompressed_bytes: 32 * 1024 * 1024,
    max_total_uncompressed_bytes: 128 * 1024 * 1024,
};
// One decoder/rasterizer at a time bounds transient memory during rapid zooms
// or switching documents. All callers already run on background executors.
static WORKER: Mutex<()> = Mutex::new(());

pub(super) struct Document {
    pub dims: Vec<(f32, f32)>,
    pub warning: bool,
    pub file_size: u64,
    content: Content,
}
pub(super) struct TablePreview<'a> {
    pub headers: &'a [String],
    pub rows: &'a [Vec<String>],
    pub truncated_rows: bool,
}
enum Content {
    Image(Arc<RenderImage>),
    Pdf(Pdf),
    Markdown(String),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        truncated_rows: bool,
    },
    CodeText(String),
}

pub(super) fn load(
    bytes: Vec<u8>,
    extension: &str,
) -> Result<Arc<Document>, &'static str> {
    let _worker = WORKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::panic::catch_unwind(|| load_inner(&bytes, extension))
        .unwrap_or(Err("library.error.render"))
}

fn is_text_preview_ext(ext: &str) -> bool {
    matches!(
        ext,
        "md" | "markdown"
            | "txt"
            | "log"
            | "csv"
            | "tsv"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "html"
            | "htm"
            | "css"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "py"
            | "rs"
            | "sql"
            | "sh"
            | "ps1"
    )
}

fn load_inner(
    source: &[u8],
    extension: &str,
) -> Result<Arc<Document>, &'static str> {
    let ext = extension.trim_start_matches('.').to_ascii_lowercase();
    let is_image = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp");
    let is_pdf_or_deck = matches!(ext.as_str(), "pdf" | "pptx");
    let is_text = is_text_preview_ext(&ext);
    if !is_image && !is_pdf_or_deck && !is_text {
        return Err("library.error.unsupported");
    }
    if source.is_empty() {
        return Err("library.error.missing");
    }
    let file_size = source.len() as u64;
    if is_text {
        if file_size > MAX_TEXT_PREVIEW_BYTES {
            return Err("library.error.large");
        }
        if source.contains(&0) {
            return Err("library.error.unsupported");
        }
        let mut text = String::from_utf8_lossy(source).into_owned();
        let content = match ext.as_str() {
            "md" | "markdown" => Content::Markdown(text),
            "csv" | "tsv" => {
                let delimiter = if ext == "tsv" { '\t' } else { ',' };
                let (headers, rows, truncated_rows) =
                    parse_delimited_table(&text, delimiter);
                Content::Table {
                    headers,
                    rows,
                    truncated_rows,
                }
            }
            "json" => {
                if text.len() <= MAX_JSON_PRETTY_BYTES {
                    let trimmed =
                        text.strip_prefix('\u{feff}').unwrap_or(&text);
                    if let Ok(value) =
                        serde_json::from_str::<serde_json::Value>(trimmed)
                        && let Ok(pretty) = serde_json::to_string_pretty(&value)
                    {
                        text = pretty;
                    }
                }
                Content::CodeText(text)
            }
            _ => Content::CodeText(text),
        };
        return Ok(Arc::new(Document {
            dims: Vec::new(),
            warning: false,
            file_size,
            content,
        }));
    }
    if file_size > MAX_INPUT {
        return Err("library.error.large");
    }
    if is_image {
        let rgba = decode_image(source, MAX_PIXELS)?;
        let (w, h) = (rgba.width() as f32, rgba.height() as f32);
        return Ok(Arc::new(Document {
            dims: vec![(w, h)],
            warning: false,
            file_size,
            content: Content::Image(bgra_image(rgba)),
        }));
    }
    let (pdf_bytes, warning) = if ext == "pptx" {
        let deck = read_presentation(source, PPTX_READ_LIMITS)?;
        let fonts: Vec<_> = fonts()
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        let (pdf, diagnostics) = deck.to_pdf_with_diagnostics(&fonts).map_err(|error| { tracing::warn!(%error, "presentation preview conversion failed"); "library.error.render" })?;
        (pdf, !diagnostics.is_empty())
    } else {
        (source.to_vec(), false)
    };
    let pdf = Pdf::new(pdf_bytes).map_err(|_| "library.error.render")?;
    let dims: Vec<_> = pdf
        .pages()
        .iter()
        .map(hayro::hayro_syntax::page::Page::render_dimensions)
        .collect();
    if dims.is_empty()
        || dims.iter().any(|(w, h)| {
            !w.is_finite() || !h.is_finite() || *w <= 0. || *h <= 0.
        })
    {
        return Err("library.error.render");
    }
    Ok(Arc::new(Document {
        dims,
        warning,
        file_size,
        content: Content::Pdf(pdf),
    }))
}

fn read_presentation(
    bytes: &[u8],
    limits: PackageReadLimits,
) -> Result<rpptx::Presentation, &'static str> {
    // rpptx 0.15 does not expose its constructor for a verified OpcPackage.
    // Check actual expansion with the same reader before handing over the
    // immutable bytes, and release the preflight package before the second read.
    drop(
        OpcPackage::from_reader_with_limits(Cursor::new(bytes), limits)
            .map_err(|error| match error {
                OpcError::PackageLimitExceeded { .. } => "library.error.large",
                _ => "library.error.render",
            })?,
    );
    rpptx::Presentation::from_bytes(bytes).map_err(|_| "library.error.render")
}

fn decode_image(
    bytes: &[u8],
    max_pixels: u32,
) -> Result<RgbaImage, &'static str> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "library.error.render")?;
    // Only use decoders whose header read precedes pixel allocation.
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Jpeg
                | image::ImageFormat::Png
                | image::ImageFormat::WebP
        )
    ) {
        return Err("library.error.unsupported");
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_INPUT);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err("library.error.render");
    }
    if u64::from(w) * u64::from(h) > u64::from(max_pixels)
        || decoder.total_bytes() > MAX_INPUT
    {
        return Err("library.error.large");
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut decoded =
        image::DynamicImage::from_decoder(decoder).map_err(image_error)?;
    decoded.apply_orientation(orientation);
    Ok(decoded.into_rgba8())
}

fn image_error(error: ImageError) -> &'static str {
    if matches!(error, ImageError::Limits(_)) {
        "library.error.large"
    } else {
        "library.error.render"
    }
}

fn bounded_scale((width, height): (f32, f32), requested: f32) -> f32 {
    requested
        .clamp(0.01, 16.)
        .min(
            ((f64::from(MAX_PIXELS) / f64::from(width) / f64::from(height))
                .sqrt()) as f32,
        )
        .min(MAX_SIDE as f32 / width.max(height))
}

fn parse_delimited_table(
    raw: &str,
    delimiter: char,
) -> (Vec<String>, Vec<Vec<String>>, bool) {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut all_rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_started = false;
    let mut truncated_rows = false;
    let mut chars = input.chars().peekable();

    let push_field = |row: &mut Vec<String>, field: &mut String| {
        let value = std::mem::take(field);
        if row.len() < MAX_CSV_COLS {
            row.push(value);
        }
    };

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && !field_started {
            in_quotes = true;
            field_started = true;
        } else if ch == delimiter {
            push_field(&mut row, &mut field);
            field_started = false;
        } else if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            push_field(&mut row, &mut field);
            field_started = false;
            if all_rows.len() <= MAX_CSV_ROWS {
                all_rows.push(std::mem::take(&mut row));
            } else {
                let trailing_blank = row.len() == 1
                    && row[0].is_empty()
                    && chars.peek().is_none();
                row.clear();
                if !trailing_blank {
                    truncated_rows = true;
                    break;
                }
            }
        } else {
            field_started = true;
            field.push(ch);
        }
    }

    if !truncated_rows
        && (field_started || !field.is_empty() || !row.is_empty())
    {
        push_field(&mut row, &mut field);
        if all_rows.len() <= MAX_CSV_ROWS {
            all_rows.push(row);
        } else {
            truncated_rows = true;
        }
    }

    if all_rows.is_empty() {
        (Vec::new(), Vec::new(), false)
    } else {
        let headers = all_rows.remove(0);
        (headers, all_rows, truncated_rows)
    }
}

impl Document {
    pub(super) const fn is_image(&self) -> bool {
        matches!(self.content, Content::Image(_))
    }

    pub(super) const fn is_paginated(&self) -> bool {
        matches!(self.content, Content::Image(_) | Content::Pdf(_))
    }

    pub(super) const fn markdown_source(&self) -> Option<&str> {
        match &self.content {
            Content::Markdown(text) => Some(text.as_str()),
            _ => None,
        }
    }

    pub(super) const fn table_data(&self) -> Option<TablePreview<'_>> {
        match &self.content {
            Content::Table {
                headers,
                rows,
                truncated_rows,
            } => Some(TablePreview {
                headers: headers.as_slice(),
                rows: rows.as_slice(),
                truncated_rows: *truncated_rows,
            }),
            _ => None,
        }
    }

    pub(super) const fn code_text(&self) -> Option<&str> {
        match &self.content {
            Content::CodeText(text) => Some(text.as_str()),
            _ => None,
        }
    }

    pub(super) fn raster_bytes(
        &self,
        page: usize,
        scale: f32,
    ) -> Result<usize, &'static str> {
        let dims = *self.dims.get(page).ok_or("library.error.render")?;
        match &self.content {
            Content::Image(image) => image
                .as_bytes(0)
                .map(<[u8]>::len)
                .ok_or("library.error.render"),
            Content::Pdf(_) => raster_size(dims, scale).map(|(_, bytes)| bytes),
            Content::Markdown(_)
            | Content::Table { .. }
            | Content::CodeText(_) => Err("library.error.unsupported"),
        }
    }
}

fn raster_size(
    dims: (f32, f32),
    requested: f32,
) -> Result<(f32, usize), &'static str> {
    let mut scale = bounded_scale(dims, requested);
    // Hayro truncates each scaled dimension when allocating the output pixmap.
    loop {
        let width = (dims.0 * scale) as u32;
        let height = (dims.1 * scale) as u32;
        if width == 0 || height == 0 {
            return Err("library.error.render");
        }
        let pixels = u64::from(width) * u64::from(height);
        if width <= MAX_SIDE
            && height <= MAX_SIDE
            && pixels <= u64::from(MAX_PIXELS)
        {
            return Ok((scale, pixels as usize * 4));
        }
        // Rounding can put the actual raster just above either limit.
        scale = scale.next_down();
    }
}

pub(super) fn render(
    doc: &Document,
    page: usize,
    scale: f32,
) -> Result<Arc<RenderImage>, &'static str> {
    let _worker = WORKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match &doc.content {
            Content::Image(image) => Ok(image.clone()),
            Content::Pdf(pdf) => {
                let dims = *doc.dims.get(page).ok_or("library.error.render")?;
                let (scale, _) = raster_size(dims, scale)?;
                let pixmap = hayro::render_pdf(
                    pdf,
                    scale,
                    hayro::hayro_interpret::InterpreterSettings::default(),
                    &hayro::RenderSettings::default(),
                    Some(page..=page),
                )
                .and_then(|pages| pages.into_iter().next())
                .ok_or("library.error.render")?;
                let (w, h) =
                    (u32::from(pixmap.width()), u32::from(pixmap.height()));
                let mut rgba = pixmap.take_rgba8(
                    hayro::vello_cpu::peniko::ImageAlphaType::AlphaPremultiplied,
                );
                for p in rgba.as_chunks_mut::<4>().0 {
                    let white = 255 - p[3];
                    for channel in &mut p[..3] {
                        *channel = channel.saturating_add(white);
                    }
                    p[3] = 255;
                }
                let rgba = RgbaImage::from_raw(w, h, rgba)
                    .ok_or("library.error.render")?;
                Ok(bgra_image(rgba))
            }
            Content::Markdown(_)
            | Content::Table { .. }
            | Content::CodeText(_) => Err("library.error.unsupported"),
        }
    }))
    .unwrap_or(Err("library.error.render"))
}

fn bgra_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(rgba)]))
}

/// Load installed faces once, without redistributing fonts or scanning on the UI thread.
fn fonts() -> &'static Vec<(String, Vec<u8>)> {
    static FONTS: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut bytes_used = 0;
        db.faces()
            .filter_map(|face| {
                let name = face.families.first()?.0.clone();
                let lower = name.to_lowercase();
                if ![
                    "arial",
                    "aptos",
                    "calibri",
                    "times new roman",
                    "yahei",
                    "simsun",
                    "noto sans cjk",
                    "pingfang",
                    "source han",
                    "wenquanyi",
                ]
                .iter()
                .any(|part| lower.contains(part))
                {
                    return None;
                }
                db.with_face_data(face.id, |data, _| {
                    if bytes_used + data.len() > 64 * 1024 * 1024 {
                        return None;
                    }
                    bytes_used += data.len();
                    Some((name, data.to_vec()))
                })
                .flatten()
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn raster_budget_and_channel_order() {
        let s = bounded_scale((20_000., 30_000.), 8.);
        assert!(20_000. * 30_000. * s * s <= MAX_PIXELS as f32 + 2.);
        let img = bgra_image(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([255, 32, 4, 255]),
        ));
        assert_eq!(img.as_bytes(0).unwrap(), &[4, 32, 255, 255]);
    }
    #[test]
    fn empty_payload_has_a_recoverable_error() {
        assert!(matches!(
            load(Vec::new(), "pdf"),
            Err("library.error.missing")
        ));
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/fixtures/preview")
            .join(name)
    }

    #[test]
    fn images_decode_and_preserve_geometry() {
        for name in ["sample.jpg", "sample.png", "sample.webp"] {
            let path = fixture(name);
            let bytes = std::fs::read(&path).unwrap();
            let extension = path.extension().unwrap().to_str().unwrap();
            let doc = load(bytes, extension).unwrap();
            assert_eq!(doc.dims, [(1280., 720.)]);
            let image = render(&doc, 0, 1.).unwrap();
            assert_eq!(image.as_bytes(0).unwrap().len(), 1280 * 720 * 4);
            // Header is dark blue; JPEG may introduce small channel differences.
            let pixel = &image.as_bytes(0).unwrap()[..4];
            assert!(pixel[0] > pixel[2]);
        }
    }

    #[test]
    fn image_pixel_budget_is_checked_before_decoding_pixels() {
        for name in ["sample.jpg", "sample.png", "sample.webp"] {
            let bytes = std::fs::read(fixture(name)).unwrap();
            let pixels = 1280 * 720;
            assert!(matches!(
                decode_image(&bytes, pixels - 1),
                Err("library.error.large")
            ));
            assert!(decode_image(&bytes, pixels).is_ok());
        }
    }

    #[test]
    fn pdf_has_real_pages_and_scaled_nonempty_pixels() {
        let doc =
            load(std::fs::read(fixture("sample.pdf")).unwrap(), "pdf").unwrap();
        assert_eq!(doc.dims, [(612., 792.); 3]);
        for page in 0..3 {
            let image = render(&doc, page, 0.5).unwrap();
            let bytes = image.as_bytes(0).unwrap().len();
            assert_eq!(bytes, 306 * 396 * 4);
            assert_eq!(doc.raster_bytes(page, 0.5), Ok(bytes));
            assert!(
                image
                    .as_bytes(0)
                    .unwrap()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[0] != 255)
            );
        }
        assert!(render(&doc, 3, 1.).is_err());
    }

    #[test]
    fn pptx_converts_once_and_pages_are_renderable() {
        let doc = load(std::fs::read(fixture("sample.pptx")).unwrap(), "pptx")
            .unwrap();
        assert_eq!(doc.dims.len(), 3);
        assert!((doc.dims[0].0 / doc.dims[0].1 - 16. / 9.).abs() < 0.01);
        assert!(
            render(&doc, 0, 0.5).unwrap().as_bytes(0).unwrap().len() > 1000
        );
    }

    #[test]
    fn pptx_limits_count_parts_and_expanded_bytes() {
        let bytes = std::fs::read(fixture("sample.pptx")).unwrap();
        let total_limit = 64 * 1024;
        assert!((bytes.len() as u64) < total_limit);
        for limits in [
            PackageReadLimits {
                max_entries: 1,
                ..PPTX_READ_LIMITS
            },
            PackageReadLimits {
                max_part_uncompressed_bytes: 1024,
                ..PPTX_READ_LIMITS
            },
            PackageReadLimits {
                max_total_uncompressed_bytes: total_limit,
                ..PPTX_READ_LIMITS
            },
        ] {
            assert!(matches!(
                read_presentation(&bytes, limits),
                Err("library.error.large")
            ));
        }
        assert!(matches!(
            read_presentation(b"not a pptx", PPTX_READ_LIMITS),
            Err("library.error.render")
        ));
    }

    #[test]
    fn pptx_budget_uses_expanded_size_for_compressible_xml() {
        let original = std::fs::read(fixture("sample.pptx")).unwrap();
        let mut package = OpcPackage::from_reader_with_limits(
            Cursor::new(&original),
            PPTX_READ_LIMITS,
        )
        .unwrap();
        let path = "/ppt/slides/slide1.xml";
        let mut slide = package.get_part(path).unwrap().to_vec();
        // Valid trailing XML whitespace. A tiny fixture exercises a reduced limit.
        slide.extend(std::iter::repeat_n(b' ', 24 * 1024));
        package.set_part(path, slide);
        let mut archive = Cursor::new(Vec::new());
        package.write_to(&mut archive).unwrap();
        let archive = archive.into_inner();
        let small_limit = PackageReadLimits {
            max_part_uncompressed_bytes: 20 * 1024,
            ..PPTX_READ_LIMITS
        };
        assert!(read_presentation(&original, small_limit).is_ok());
        assert!(read_presentation(&archive, PPTX_READ_LIMITS).is_ok());
        assert!(archive.len() < original.len() + 1024);
        assert!(matches!(
            read_presentation(&archive, small_limit),
            Err("library.error.large")
        ));
    }

    #[test]
    fn malformed_and_unsupported_files_do_not_panic() {
        let root = std::env::temp_dir()
            .join(format!("flowy-preview-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("bad.pdf"), b"not a pdf").unwrap();
        std::fs::write(root.join("old.ppt"), b"not a pptx").unwrap();
        assert!(matches!(
            load(std::fs::read(root.join("bad.pdf")).unwrap(), "pdf"),
            Err("library.error.render")
        ));
        assert!(matches!(
            load(std::fs::read(root.join("old.ppt")).unwrap(), "ppt"),
            Err("library.error.unsupported")
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn markdown_and_code_preview_enforce_size_and_binary_guards() {
        let md = load(b"# Heading\n\n- [x] Ready\n".to_vec(), "md").unwrap();
        assert!(!md.is_paginated());
        assert_eq!(md.dims.len(), 0);
        assert_eq!(md.markdown_source(), Some("# Heading\n\n- [x] Ready\n"));
        assert!(render(&md, 0, 1.).is_err());

        let json_doc =
            load(b"\xef\xbb\xbf{\"b\":2,\"a\":1}".to_vec(), "json").unwrap();
        let pretty = json_doc.code_text().unwrap();
        assert!(
            pretty.contains('\n'),
            "valid small JSON should pretty-print"
        );
        assert!(pretty.contains("\"a\": 1"));

        let bad_json =
            load(b"{ // comment\n \"a\": 1".to_vec(), "json").unwrap();
        assert_eq!(bad_json.code_text(), Some("{ // comment\n \"a\": 1"));

        let big_json =
            format!("{{\"k\":\"{}\"}}", "x".repeat(MAX_JSON_PRETTY_BYTES + 16));
        let big_json_doc = load(big_json.clone().into_bytes(), "json").unwrap();
        assert_eq!(big_json_doc.code_text(), Some(big_json.as_str()));

        let non_utf8 = load(vec![0x68, 0x69, 0xff, 0x21], "log").unwrap();
        assert_eq!(non_utf8.code_text(), Some("hi\u{fffd}!"));

        assert!(matches!(
            load(b"abc\0def".to_vec(), "txt"),
            Err("library.error.unsupported")
        ));
        assert!(matches!(
            load(vec![b'a'; (MAX_TEXT_PREVIEW_BYTES as usize) + 1], "md"),
            Err("library.error.large")
        ));
    }

    #[test]
    fn csv_and_tsv_parse_rfc4180_and_clamp_bounds() {
        use std::fmt::Write as _;

        let csv = "\u{feff}name,note,amount\r\n\"Alice, B.\",\"line1\n\"\"quoted\"\"\",12\r\n中文名,plain,34\r\n";
        let doc = load(csv.as_bytes().to_vec(), "csv").unwrap();
        let table = doc.table_data().unwrap();
        assert_eq!(table.headers, ["name", "note", "amount"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], ["Alice, B.", "line1\n\"quoted\"", "12"]);
        assert_eq!(table.rows[1], ["中文名", "plain", "34"]);
        assert!(!table.truncated_rows);

        let tsv = "col1\tcol2\nval1\tval2\n";
        let tsv_doc = load(tsv.as_bytes().to_vec(), "tsv").unwrap();
        let tsv_table = tsv_doc.table_data().unwrap();
        assert_eq!(tsv_table.headers, ["col1", "col2"]);
        assert_eq!(tsv_table.rows, vec![vec!["val1", "val2"]]);
        assert!(!tsv_table.truncated_rows);

        let wide_row = (0..80)
            .map(|i| format!("c{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let wide_csv = format!("{wide_row}\n{wide_row}\n");
        let wide_doc = load(wide_csv.into_bytes(), "csv").unwrap();
        let wide_table = wide_doc.table_data().unwrap();
        assert_eq!(wide_table.headers.len(), MAX_CSV_COLS);
        assert_eq!(wide_table.rows[0].len(), MAX_CSV_COLS);

        let mut exact_rows = String::from("h1,h2\n");
        for i in 0..MAX_CSV_ROWS {
            let _ = writeln!(exact_rows, "{i},v");
        }
        let exact_doc = load(exact_rows.clone().into_bytes(), "csv").unwrap();
        let exact_table = exact_doc.table_data().unwrap();
        assert_eq!(exact_table.rows.len(), MAX_CSV_ROWS);
        assert!(!exact_table.truncated_rows);

        exact_rows.push_str("extra,row\n");
        let over_doc = load(exact_rows.into_bytes(), "csv").unwrap();
        let over_table = over_doc.table_data().unwrap();
        assert_eq!(over_table.rows.len(), MAX_CSV_ROWS);
        assert!(over_table.truncated_rows);
    }
}
