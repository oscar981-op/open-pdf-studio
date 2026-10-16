//! Export a freshly rendered page from the explicitly selected backend.
//! A strict DynLex rejection remains an error; no other renderer supplies pixels.
use super::{pdf_save, render_page_selected};
use opds_dynlex_backend::Backend;
use opds_engine_probe::ParsedPdf;
use opds_native_core::annotations::Annotation;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const EXPORT_CANCELLED: &str = "Export cancelled";

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err(EXPORT_CANCELLED.into())
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageDpi {
    Dpi72,
    #[default]
    Dpi150,
    Dpi300,
}

impl ImageDpi {
    pub const ALL: [Self; 3] = [Self::Dpi72, Self::Dpi150, Self::Dpi300];

    pub const fn value(self) -> u16 {
        match self {
            Self::Dpi72 => 72,
            Self::Dpi150 => 150,
            Self::Dpi300 => 300,
        }
    }
}

impl std::fmt::Display for ImageDpi {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} DPI", self.value())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
    Tiff,
}

impl ImageFormat {
    pub const ALL: [Self; 3] = [Self::Png, Self::Jpeg, Self::Tiff];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
        }
    }

    fn accepts_extension(self, extension: &str) -> bool {
        match self {
            Self::Png => extension.eq_ignore_ascii_case("png"),
            Self::Jpeg => {
                extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
            }
            Self::Tiff => {
                extension.eq_ignore_ascii_case("tif") || extension.eq_ignore_ascii_case("tiff")
            }
        }
    }
}

impl std::fmt::Display for ImageFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Png => write!(formatter, "PNG"),
            Self::Jpeg => write!(formatter, "JPEG"),
            Self::Tiff => write!(formatter, "TIFF"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegQuality(u8);

impl JpegQuality {
    pub fn new(value: u8) -> Result<Self, String> {
        if (10..=100).contains(&value) {
            Ok(Self(value))
        } else {
            Err("Choose JPEG quality from 10 to 100".into())
        }
    }

    pub const fn value(self) -> u8 {
        self.0
    }
}

impl Default for JpegQuality {
    fn default() -> Self {
        Self(92)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImagePageRange {
    #[default]
    Current,
    All,
    Custom,
}

impl ImagePageRange {
    pub const ALL: [Self; 3] = [Self::Current, Self::All, Self::Custom];
}

impl std::fmt::Display for ImagePageRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Current => write!(formatter, "Current page"),
            Self::All => write!(formatter, "All pages"),
            Self::Custom => write!(formatter, "Custom range"),
        }
    }
}

pub fn parse_page_range(spec: &str, total_pages: usize) -> Result<Vec<usize>, String> {
    fn page_number(raw: &str) -> Result<usize, String> {
        if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Invalid page number".into());
        }
        raw.parse::<usize>()
            .map_err(|_| "Invalid page number".into())
    }

    if total_pages == 0 || spec.trim().is_empty() {
        return Err("Enter a page range such as 1-3,5".into());
    }
    let mut pages = std::collections::BTreeSet::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("Empty item in page range".into());
        }
        let (start, end) = if let Some((start, end)) = part.split_once('-') {
            (page_number(start.trim())?, page_number(end.trim())?)
        } else {
            let page = page_number(part)?;
            (page, page)
        };
        if start == 0 || end == 0 || start > total_pages || end > total_pages {
            return Err(format!("Choose pages from 1 to {total_pages}"));
        }
        for page in start.min(end)..=start.max(end) {
            pages.insert(page - 1);
        }
    }
    Ok(pages.into_iter().collect())
}

#[cfg(test)]
pub fn current_page_png(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    page: usize,
    backend: Backend,
    dpi: ImageDpi,
    path: &Path,
) -> Result<(), String> {
    current_page_image(
        source,
        annotations,
        page,
        backend,
        dpi,
        ImageFormat::Png,
        JpegQuality::default(),
        path,
    )
}

#[cfg(test)]
pub fn current_page_image(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    page: usize,
    backend: Backend,
    dpi: ImageDpi,
    format: ImageFormat,
    quality: JpegQuality,
    path: &Path,
) -> Result<(), String> {
    current_page_image_cancellable(
        source,
        annotations,
        page,
        backend,
        dpi,
        format,
        quality,
        path,
        &AtomicBool::new(false),
    )
}

pub fn current_page_image_cancellable(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    page: usize,
    backend: Backend,
    dpi: ImageDpi,
    format: ImageFormat,
    quality: JpegQuality,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    check_cancel(cancel)?;
    if path.as_os_str().len() > 4096 {
        return Err("Destination path is too long".into());
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_none_or(|ext| !format.accepts_extension(ext))
    {
        return Err(format!("Choose a .{} destination", format.extension()));
    }
    let rendered_source = if annotations.is_empty() {
        source
    } else {
        Arc::new(pdf_save::export_annotations(&source, annotations)?)
    };
    let pdf = ParsedPdf::open(Arc::clone(&rendered_source)).map_err(|error| error.to_string())?;
    let bytes = page_image_bytes(&pdf, &rendered_source, page, backend, dpi, format, quality)?;
    check_cancel(cancel)?;
    pdf_save::save_new(path, &bytes)
}

fn page_image_bytes(
    pdf: &ParsedPdf,
    rendered_source: &[u8],
    page: usize,
    backend: Backend,
    dpi: ImageDpi,
    format: ImageFormat,
    quality: JpegQuality,
) -> Result<Vec<u8>, String> {
    let page_number = page.checked_add(1).ok_or("Invalid page number")?;
    let raster = render_page_selected(
        &pdf,
        &rendered_source,
        page_number,
        f32::from(dpi.value()) / 72.0,
        backend,
    )?
    .raster;
    let expected_len = usize::try_from(raster.width)
        .ok()
        .and_then(|width| {
            usize::try_from(raster.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("Export image dimensions overflow")?;
    if raster.width == 0 || raster.height == 0 || raster.rgba.len() != expected_len {
        return Err("Renderer returned an invalid RGBA image".into());
    }
    match format {
        ImageFormat::Png => {
            let mut bytes = Vec::new();
            {
                let mut encoder =
                    png::Encoder::new(Cursor::new(&mut bytes), raster.width, raster.height);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                // Match Tauri's pHYs chunk: pixels per metre rounded from the chosen DPI.
                let pixels_per_meter = (u32::from(dpi.value()) * 10_000 + 127) / 254;
                encoder.set_pixel_dims(Some(png::PixelDimensions {
                    xppu: pixels_per_meter,
                    yppu: pixels_per_meter,
                    unit: png::Unit::Meter,
                }));
                let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
                writer
                    .write_image_data(&raster.rgba)
                    .map_err(|error| error.to_string())?;
            }
            Ok(bytes)
        }
        ImageFormat::Jpeg => {
            let mut rgb = Vec::with_capacity(expected_len / 4 * 3);
            for rgba in raster.rgba.chunks_exact(4) {
                let alpha = u16::from(rgba[3]);
                for &channel in &rgba[..3] {
                    // A PDF page has a white canvas; composite any transparent
                    // renderer pixels before JPEG discards the alpha channel.
                    rgb.push(
                        ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                    );
                }
            }
            let mut bytes = Vec::new();
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.value());
            encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(dpi.value()));
            encoder
                .encode(
                    &rgb,
                    raster.width,
                    raster.height,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|error| error.to_string())?;
            Ok(bytes)
        }
        ImageFormat::Tiff => {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut encoder = tiff::encoder::TiffEncoder::new(&mut bytes)
                    .map_err(|error| error.to_string())?;
                let mut image = encoder
                    .new_image::<tiff::encoder::colortype::RGBA8>(raster.width, raster.height)
                    .map_err(|error| error.to_string())?;
                image
                    .encoder()
                    .write_tag(tiff::tags::Tag::ExtraSamples, &[2_u16][..])
                    .map_err(|error| error.to_string())?;
                image.resolution(
                    tiff::tags::ResolutionUnit::Inch,
                    tiff::encoder::Rational {
                        n: u32::from(dpi.value()),
                        d: 1,
                    },
                );
                image
                    .write_data(&raster.rgba)
                    .map_err(|error| error.to_string())?;
            }
            Ok(bytes.into_inner())
        }
    }
}

/// Export a page selection into a newly created directory. The PDF and its
/// native annotations are prepared once; a later render failure removes only
/// files created by this operation, never a pre-existing destination.
#[cfg(test)]
pub fn selected_pages_png(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    pages: &[usize],
    backend: Backend,
    dpi: ImageDpi,
    folder: &Path,
    source_name: &str,
) -> Result<usize, String> {
    selected_pages_image(
        source,
        annotations,
        pages,
        backend,
        dpi,
        ImageFormat::Png,
        JpegQuality::default(),
        folder,
        source_name,
    )
}

#[cfg(test)]
pub fn selected_pages_image(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    pages: &[usize],
    backend: Backend,
    dpi: ImageDpi,
    format: ImageFormat,
    quality: JpegQuality,
    folder: &Path,
    source_name: &str,
) -> Result<usize, String> {
    selected_pages_image_cancellable(
        source,
        annotations,
        pages,
        backend,
        dpi,
        format,
        quality,
        folder,
        source_name,
        &AtomicBool::new(false),
    )
}

#[cfg(test)]
pub fn selected_pages_image_cancellable(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    pages: &[usize],
    backend: Backend,
    dpi: ImageDpi,
    format: ImageFormat,
    quality: JpegQuality,
    folder: &Path,
    source_name: &str,
    cancel: &AtomicBool,
) -> Result<usize, String> {
    selected_pages_image_with_progress(
        source,
        annotations,
        pages,
        backend,
        dpi,
        format,
        quality,
        folder,
        source_name,
        cancel,
        |_| {},
    )
}

pub fn selected_pages_image_with_progress(
    source: Arc<Vec<u8>>,
    annotations: &[Annotation],
    pages: &[usize],
    backend: Backend,
    dpi: ImageDpi,
    format: ImageFormat,
    quality: JpegQuality,
    folder: &Path,
    source_name: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize),
) -> Result<usize, String> {
    check_cancel(cancel)?;
    if folder.as_os_str().len() > 4096 {
        return Err("Destination path is too long".into());
    }
    if pages.is_empty() {
        return Err("Choose at least one page".into());
    }
    let name = Path::new(source_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or("Invalid PDF filename")?;
    let rendered_source = if annotations.is_empty() {
        source
    } else {
        Arc::new(pdf_save::export_annotations(&source, annotations)?)
    };
    let pdf = ParsedPdf::open(Arc::clone(&rendered_source)).map_err(|error| error.to_string())?;
    let mut unique = std::collections::HashSet::with_capacity(pages.len());
    if pages
        .iter()
        .any(|page| *page >= pdf.page_count() || !unique.insert(*page))
    {
        return Err("Page selection is invalid or contains duplicates".into());
    }
    check_cancel(cancel)?;
    std::fs::create_dir(folder).map_err(|error| error.to_string())?;
    let mut written = Vec::new();
    for &page in pages {
        let output = folder.join(format!("{name}_page{:04}.{}", page + 1, format.extension()));
        let result = check_cancel(cancel)
            .and_then(|()| {
                page_image_bytes(&pdf, &rendered_source, page, backend, dpi, format, quality)
            })
            .and_then(|bytes| {
                check_cancel(cancel)?;
                pdf_save::save_new(&output, &bytes)
            });
        if let Err(error) = result {
            for path in written {
                let _ = std::fs::remove_file(path);
            }
            let _ = std::fs::remove_dir(folder);
            return Err(error);
        }
        written.push(output);
        on_progress(written.len());
    }
    if let Err(error) = check_cancel(cancel) {
        for path in written {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_dir(folder);
        return Err(error);
    }
    Ok(written.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use opds_native_core::annotations::{Kind, Rect};

    #[test]
    fn cancellation_before_export_creates_no_file_or_directory() {
        let source = Arc::new(super::super::blank_pdf::create(100.0, 100.0, 2).unwrap());
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "opds-cancel-image-export-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let cancel = AtomicBool::new(true);
        let file = dir.join("page.png");
        assert_eq!(
            current_page_image_cancellable(
                Arc::clone(&source),
                &[],
                0,
                Backend::Hayro,
                ImageDpi::Dpi72,
                ImageFormat::Png,
                JpegQuality::default(),
                &file,
                &cancel,
            ),
            Err(EXPORT_CANCELLED.into())
        );
        assert!(!file.exists());
        let folder = dir.join("pages");
        assert_eq!(
            selected_pages_image_cancellable(
                source,
                &[],
                &[0, 1],
                Backend::Hayro,
                ImageDpi::Dpi72,
                ImageFormat::Png,
                JpegQuality::default(),
                &folder,
                "source.pdf",
                &cancel,
            ),
            Err(EXPORT_CANCELLED.into())
        );
        assert!(!folder.exists());
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn tiff_has_exact_render_pixels_and_selected_dpi_without_overwriting() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("opds-tiff-export-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        for dpi in ImageDpi::ALL {
            let path = dir.join(format!("page-{}.tiff", dpi.value()));
            current_page_image(
                Arc::clone(&source),
                &[],
                0,
                Backend::Hayro,
                dpi,
                ImageFormat::Tiff,
                JpegQuality::default(),
                &path,
            )
            .unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let mut decoder = tiff::decoder::Decoder::new(Cursor::new(&bytes)).unwrap();
            let expected = render_page_selected(
                &pdf,
                &source,
                1,
                f32::from(dpi.value()) / 72.0,
                Backend::Hayro,
            )
            .unwrap()
            .raster;
            assert_eq!(
                decoder.dimensions().unwrap(),
                (expected.width, expected.height)
            );
            assert_eq!(
                decoder
                    .get_tag_unsigned::<u16>(tiff::tags::Tag::ResolutionUnit)
                    .unwrap(),
                tiff::tags::ResolutionUnit::Inch.to_u16()
            );
            assert_eq!(
                decoder.get_tag(tiff::tags::Tag::XResolution).unwrap(),
                tiff::decoder::ifd::Value::Rational(u32::from(dpi.value()), 1)
            );
            assert_eq!(
                decoder.get_tag(tiff::tags::Tag::YResolution).unwrap(),
                tiff::decoder::ifd::Value::Rational(u32::from(dpi.value()), 1)
            );
            assert_eq!(
                decoder
                    .get_tag_u16_vec(tiff::tags::Tag::ExtraSamples)
                    .unwrap(),
                vec![2]
            );
            match decoder.read_image().unwrap() {
                tiff::decoder::DecodingResult::U8(actual) => assert_eq!(actual, expected.rgba),
                other => panic!("unexpected TIFF sample type: {other:?}"),
            }
            assert!(current_page_image(
                Arc::clone(&source),
                &[],
                0,
                Backend::Hayro,
                dpi,
                ImageFormat::Tiff,
                JpegQuality::default(),
                &path,
            )
            .is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let unsupported = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let rejected = dir.join("strict-rejected.tiff");
        assert!(current_page_image(
            unsupported,
            &[],
            0,
            Backend::Dynlex,
            ImageDpi::Dpi72,
            ImageFormat::Tiff,
            JpegQuality::default(),
            &rejected,
        )
        .is_err());
        assert!(!rejected.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn current_page_png_contains_fresh_selected_backend_pixels_and_never_overwrites() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let expected = render_page_selected(&pdf, &source, 1, 150.0 / 72.0, Backend::Hayro)
            .unwrap()
            .raster;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("opds-png-export-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("page.png");
        current_page_png(
            Arc::clone(&source),
            &[],
            0,
            Backend::Hayro,
            ImageDpi::Dpi150,
            &path,
        )
        .unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut decoder = png::Decoder::new(Cursor::new(&original));
        decoder.set_transformations(png::Transformations::IDENTITY);
        let mut reader = decoder.read_info().unwrap();
        let mut actual = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut actual).unwrap();
        assert_eq!((info.width, info.height), (expected.width, expected.height));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&actual[..info.buffer_size()], expected.rgba);
        assert!(current_page_png(source, &[], 0, Backend::Hayro, ImageDpi::Dpi150, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let annotated_path = dir.join("annotated.png");
        let annotation = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 80.0,
                height: 40.0,
            },
            kind: Kind::Ellipse {
                color_rgb: [231, 76, 60],
            },
        };
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        current_page_png(
            Arc::clone(&source),
            &[annotation.clone()],
            0,
            Backend::Hayro,
            ImageDpi::Dpi150,
            &annotated_path,
        )
        .unwrap();
        let bytes = pdf_save::export_annotations(&source, &[annotation]).unwrap();
        let annotated_pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let expected_annotated =
            render_page_selected(&annotated_pdf, &bytes, 1, 150.0 / 72.0, Backend::Hayro)
                .unwrap()
                .raster;
        let file = std::fs::read(&annotated_path).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(file)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(&pixels[..info.buffer_size()], expected_annotated.rgba);
        assert_ne!(expected_annotated.rgba, expected.rgba);
        let unsupported = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let unsupported_path = dir.join("unsupported.png");
        assert!(current_page_png(
            unsupported,
            &[],
            0,
            Backend::Dynlex,
            ImageDpi::Dpi150,
            &unsupported_path
        )
        .is_err());
        assert!(!unsupported_path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn supported_resolution_choices_match_fresh_rgba_buffers() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("opds-png-dpi-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for dpi in ImageDpi::ALL {
            let path = dir.join(format!("{}.png", dpi.value()));
            current_page_png(Arc::clone(&source), &[], 0, Backend::Hayro, dpi, &path).unwrap();
            let expected = render_page_selected(
                &pdf,
                &source,
                1,
                f32::from(dpi.value()) / 72.0,
                Backend::Hayro,
            )
            .unwrap()
            .raster;
            let file = std::fs::read(path).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(file)).read_info().unwrap();
            let pixel_dims = reader.info().pixel_dims.unwrap();
            let expected_ppm = (u32::from(dpi.value()) * 10_000 + 127) / 254;
            assert_eq!(
                (pixel_dims.xppu, pixel_dims.yppu),
                (expected_ppm, expected_ppm)
            );
            assert_eq!(pixel_dims.unit, png::Unit::Meter);
            let mut actual = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut actual).unwrap();
            assert_eq!((info.width, info.height), (expected.width, expected.height));
            assert_eq!(&actual[..info.buffer_size()], expected.rgba);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jpeg_export_has_selected_page_pixels_and_density_without_overwrite() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let expected = render_page_selected(&pdf, &source, 1, 1.0, Backend::Hayro)
            .unwrap()
            .raster;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opds-jpeg-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("page.jpg");
        current_page_image(
            Arc::clone(&source),
            &[],
            0,
            Backend::Hayro,
            ImageDpi::Dpi72,
            ImageFormat::Jpeg,
            JpegQuality::default(),
            &path,
        )
        .unwrap();
        let file = std::fs::read(&path).unwrap();
        let jfif = file
            .windows(5)
            .position(|bytes| bytes == b"JFIF\0")
            .unwrap();
        assert_eq!(file[jfif + 7], 1); // DPI units
        assert_eq!(&file[jfif + 8..jfif + 12], &[0, 72, 0, 72]);
        let actual = image::load_from_memory_with_format(&file, image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8();
        assert_eq!(actual.dimensions(), (expected.width, expected.height));
        let total_difference: u64 = actual
            .as_raw()
            .chunks_exact(3)
            .zip(expected.rgba.chunks_exact(4))
            .map(|(rgb, rgba)| {
                rgb.iter()
                    .zip(&rgba[..3])
                    .map(|(actual, expected)| u64::from(actual.abs_diff(*expected)))
                    .sum::<u64>()
            })
            .sum();
        assert!(total_difference < u64::from(expected.width) * u64::from(expected.height) * 30);
        assert!(current_page_image(
            source,
            &[],
            0,
            Backend::Hayro,
            ImageDpi::Dpi72,
            ImageFormat::Jpeg,
            JpegQuality::default(),
            &path
        )
        .is_err());
        assert_eq!(std::fs::read(path).unwrap(), file);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jpeg_quality_is_bounded_and_changes_encoded_bytes() {
        assert!(JpegQuality::new(9).is_err());
        assert!(JpegQuality::new(101).is_err());
        assert_eq!(JpegQuality::default().value(), 92);
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let low = page_image_bytes(
            &pdf,
            &source,
            0,
            Backend::Hayro,
            ImageDpi::Dpi72,
            ImageFormat::Jpeg,
            JpegQuality::new(10).unwrap(),
        )
        .unwrap();
        let high = page_image_bytes(
            &pdf,
            &source,
            0,
            Backend::Hayro,
            ImageDpi::Dpi72,
            ImageFormat::Jpeg,
            JpegQuality::new(100).unwrap(),
        )
        .unwrap();
        assert_ne!(low, high);
        for file in [&low, &high] {
            let image = image::load_from_memory_with_format(file, image::ImageFormat::Jpeg)
                .unwrap()
                .to_rgb8();
            assert_eq!(image.dimensions(), (256, 192));
        }
    }

    #[test]
    fn jpeg_batch_keeps_page_selection_and_strict_rejection_leaves_no_file() {
        let source = Arc::new(super::super::blank_pdf::create(100.0, 100.0, 2).unwrap());
        let base_pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let base_jpeg = page_image_bytes(
            &base_pdf,
            &source,
            1,
            Backend::Hayro,
            ImageDpi::Dpi72,
            ImageFormat::Jpeg,
            JpegQuality::default(),
        )
        .unwrap();
        let annotation = Annotation {
            id: 1,
            page: 1,
            bounds: Rect {
                x: 10.0,
                y: 10.0,
                width: 35.0,
                height: 25.0,
            },
            kind: Kind::Ellipse {
                color_rgb: [231, 76, 60],
            },
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("opds-jpeg-batch-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let folder = dir.join("pages");
        assert_eq!(
            selected_pages_image(
                source,
                &[annotation],
                &[1],
                Backend::Hayro,
                ImageDpi::Dpi72,
                ImageFormat::Jpeg,
                JpegQuality::default(),
                &folder,
                "two.pdf",
            )
            .unwrap(),
            1
        );
        assert!(!folder.join("two_page0001.jpg").exists());
        let file = std::fs::read(folder.join("two_page0002.jpg")).unwrap();
        let image = image::load_from_memory_with_format(&file, image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8();
        let base = image::load_from_memory_with_format(&base_jpeg, image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8();
        assert_eq!(image.dimensions(), (100, 100));
        assert_ne!(image, base);
        assert!(image
            .pixels()
            .any(|pixel| i16::from(pixel.0[0]) - i16::from(pixel.0[1]) > 30));
        let unsupported = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let rejected = dir.join("unsupported.jpg");
        assert!(current_page_image(
            unsupported,
            &[],
            0,
            Backend::Dynlex,
            ImageDpi::Dpi72,
            ImageFormat::Jpeg,
            JpegQuality::default(),
            &rejected,
        )
        .is_err());
        assert!(!rejected.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selected_second_page_exports_its_own_annotation_and_rejects_out_of_range() {
        let source = Arc::new(super::super::blank_pdf::create(100.0, 100.0, 2).unwrap());
        let annotation = Annotation {
            id: 1,
            page: 1,
            bounds: Rect {
                x: 10.0,
                y: 10.0,
                width: 35.0,
                height: 25.0,
            },
            kind: Kind::Ellipse {
                color_rgb: [231, 76, 60],
            },
        };
        let rendered_bytes = pdf_save::export_annotations(&source, &[annotation.clone()]).unwrap();
        let rendered_pdf = ParsedPdf::open(Arc::new(rendered_bytes.clone())).unwrap();
        let expected = render_page_selected(&rendered_pdf, &rendered_bytes, 2, 1.0, Backend::Hayro)
            .unwrap()
            .raster;
        let first = render_page_selected(&rendered_pdf, &rendered_bytes, 1, 1.0, Backend::Hayro)
            .unwrap()
            .raster;
        assert_ne!(expected.rgba, first.rgba);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "opds-selected-page-export-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("second.png");
        current_page_png(
            Arc::clone(&source),
            &[annotation.clone()],
            1,
            Backend::Hayro,
            ImageDpi::Dpi72,
            &path,
        )
        .unwrap();
        let file = std::fs::read(path).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(file)).read_info().unwrap();
        let mut actual = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut actual).unwrap();
        assert_eq!(&actual[..info.buffer_size()], expected.rgba);
        let invalid = dir.join("invalid.png");
        assert!(current_page_png(
            source,
            &[annotation],
            2,
            Backend::Hayro,
            ImageDpi::Dpi72,
            &invalid
        )
        .is_err());
        assert!(!invalid.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn page_range_is_bounded_sorted_and_deduplicated() {
        assert_eq!(parse_page_range("3-1, 2,5", 5).unwrap(), vec![0, 1, 2, 4]);
        for invalid in ["", "0", "6", "1-6", "1,,2", "1-a", "1-2-3", "+1"] {
            assert!(parse_page_range(invalid, 5).is_err(), "{invalid}");
        }
    }

    #[test]
    fn selected_pages_write_exact_page_pixels_and_leave_existing_targets_untouched() {
        let source = Arc::new(super::super::blank_pdf::create(100.0, 100.0, 2).unwrap());
        let annotation = Annotation {
            id: 1,
            page: 1,
            bounds: Rect {
                x: 10.0,
                y: 10.0,
                width: 35.0,
                height: 25.0,
            },
            kind: Kind::Ellipse {
                color_rgb: [231, 76, 60],
            },
        };
        let rendered_bytes = pdf_save::export_annotations(&source, &[annotation.clone()]).unwrap();
        let pdf = ParsedPdf::open(Arc::new(rendered_bytes.clone())).unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "opds-many-page-export-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let folder = dir.join("images");
        let mut progress = Vec::new();
        assert_eq!(
            selected_pages_image_with_progress(
                Arc::clone(&source),
                &[annotation.clone()],
                &[0, 1],
                Backend::Hayro,
                ImageDpi::Dpi72,
                ImageFormat::Png,
                JpegQuality::default(),
                &folder,
                "source.pdf",
                &AtomicBool::new(false),
                |completed| progress.push(completed),
            )
            .unwrap(),
            2
        );
        assert_eq!(progress, [1, 2]);
        for page in 0..2 {
            let file =
                std::fs::read(folder.join(format!("source_page{:04}.png", page + 1))).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(file)).read_info().unwrap();
            let mut actual = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut actual).unwrap();
            let expected =
                render_page_selected(&pdf, &rendered_bytes, page + 1, 1.0, Backend::Hayro)
                    .unwrap()
                    .raster;
            assert_eq!(&actual[..info.buffer_size()], expected.rgba);
        }
        let first = std::fs::read(folder.join("source_page0001.png")).unwrap();
        assert!(selected_pages_png(
            source,
            &[annotation],
            &[0, 1],
            Backend::Hayro,
            ImageDpi::Dpi72,
            &folder,
            "source.pdf"
        )
        .is_err());
        assert_eq!(
            std::fs::read(folder.join("source_page0001.png")).unwrap(),
            first
        );
        let unsupported = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let rejected_folder = dir.join("rejected");
        assert!(selected_pages_png(
            unsupported,
            &[],
            &[0],
            Backend::Dynlex,
            ImageDpi::Dpi72,
            &rejected_folder,
            "unsupported.pdf"
        )
        .is_err());
        assert!(!rejected_folder.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
