//! Reusable pure-Rust PDF raster entry point for the future native viewer.
#![forbid(unsafe_code)]

mod imported_annotations;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::{TRANSPARENT, WHITE};
use hayro::{RenderCache, RenderSettings};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

pub const MAX_SIDE: f64 = 4096.0;
pub const MAX_PIXELS: f64 = 16_000_000.0;

#[derive(Debug, Clone)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub page_width_pt: f32,
    pub page_height_pt: f32,
    pub total_pages: usize,
    pub parse_ms: u128,
    pub render_ms: u128,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenderError {
    InvalidPage,
    InvalidScale,
    Parse(String),
    TooLarge,
    InvalidBitmap,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RenderError {}

/// Parsed PDF backed by shared original bytes. Reuse it across page and zoom renders.
pub struct ParsedPdf {
    pdf: Pdf,
    parse_ms: u128,
    imported_annotations: Vec<OnceLock<Vec<imported_annotations::ImportedAnnotation>>>,
}

impl std::fmt::Debug for ParsedPdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedPdf")
            .field("pages", &self.page_count())
            .finish()
    }
}

impl ParsedPdf {
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Self, RenderError> {
        let started = Instant::now();
        let pdf = Pdf::new(bytes).map_err(|error| RenderError::Parse(format!("{error:?}")))?;
        let imported_annotations = (0..pdf.pages().len()).map(|_| OnceLock::new()).collect();
        Ok(Self {
            pdf,
            parse_ms: started.elapsed().as_millis(),
            imported_annotations,
        })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    pub fn page_dimensions(&self, page_number: usize) -> Result<(f32, f32), RenderError> {
        let page = self
            .pdf
            .pages()
            .get(page_number.checked_sub(1).ok_or(RenderError::InvalidPage)?)
            .ok_or(RenderError::InvalidPage)?;
        Ok(page.render_dimensions())
    }

    pub fn parse_ms(&self) -> u128 {
        self.parse_ms
    }

    pub fn render_page(&self, page_number: usize, scale: f32) -> Result<Raster, RenderError> {
        self.render_page_with_background(page_number, scale, false, true)
    }

    /// Diagnostic page-content layer before the viewer's white paper is
    /// applied. Imported annotation fallback is deliberately excluded.
    pub fn render_page_content_layer(
        &self,
        page_number: usize,
        scale: f32,
    ) -> Result<Raster, RenderError> {
        self.render_page_with_background(page_number, scale, true, true)
    }

    /// Render the original PDF page without imported annotation appearances
    /// or the preview fallback for annotations lacking /AP. This preserves
    /// the already parsed document and lets a separate renderer paint /Annots.
    pub fn render_page_content_without_annotations(
        &self,
        page_number: usize,
        scale: f32,
    ) -> Result<Raster, RenderError> {
        self.render_page_with_background(page_number, scale, true, false)
    }

    fn render_page_with_background(
        &self,
        page_number: usize,
        scale: f32,
        transparent: bool,
        render_annotations: bool,
    ) -> Result<Raster, RenderError> {
        validate_request(page_number, scale)?;
        let pages = self.pdf.pages();
        let page = pages.get(page_number - 1).ok_or(RenderError::InvalidPage)?;
        let (page_width_pt, page_height_pt) = page.render_dimensions();
        let width_px = page_width_pt as f64 * scale as f64;
        let height_px = page_height_pt as f64 * scale as f64;
        if !width_px.is_finite()
            || !height_px.is_finite()
            || width_px <= 0.0
            || height_px <= 0.0
            || width_px > MAX_SIDE
            || height_px > MAX_SIDE
            || width_px * height_px > MAX_PIXELS
        {
            return Err(RenderError::TooLarge);
        }
        let started = Instant::now();
        let mut interpreter = InterpreterSettings::default();
        interpreter.render_annotations = render_annotations;
        let pixmap = hayro::render(
            page,
            &RenderCache::new(),
            &interpreter,
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: if transparent { TRANSPARENT } else { WHITE },
                ..Default::default()
            },
        );
        let width = u32::from(pixmap.width());
        let height = u32::from(pixmap.height());
        let mut rgba = pixmap.data_as_u8_slice().to_vec();
        if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
            return Err(RenderError::InvalidBitmap);
        }
        if !transparent && render_annotations {
            let annotations = self.imported_annotations[page_number - 1]
                .get_or_init(|| imported_annotations::read(page));
            if !annotations.is_empty() {
                imported_annotations::paint(
                    annotations,
                    &mut rgba,
                    width,
                    height,
                    page.initial_transform(true).as_coeffs(),
                    f64::from(scale),
                );
            }
        }
        Ok(Raster {
            width,
            height,
            rgba,
            page_width_pt,
            page_height_pt,
            total_pages: pages.len(),
            parse_ms: 0,
            render_ms: started.elapsed().as_millis(),
        })
    }
}

fn validate_request(page_number: usize, scale: f32) -> Result<(), RenderError> {
    if page_number == 0 {
        return Err(RenderError::InvalidPage);
    }
    if !scale.is_finite() || !(0.01..=8.0).contains(&scale) {
        return Err(RenderError::InvalidScale);
    }
    Ok(())
}

/// Page numbers are one-based at this API boundary. `bytes` is owned because
/// the CLI harness takes ownership. The GUI uses `ParsedPdf` to avoid reparsing.
pub fn render_page(bytes: Vec<u8>, page_number: usize, scale: f32) -> Result<Raster, RenderError> {
    validate_request(page_number, scale)?;
    let parsed = ParsedPdf::open(Arc::new(bytes))?;
    let mut raster = parsed.render_page(page_number, scale)?;
    raster.parse_ms = parsed.parse_ms();
    Ok(raster)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_requests_without_parsing() {
        assert_eq!(
            render_page(vec![], 0, 1.0).unwrap_err(),
            RenderError::InvalidPage
        );
        assert_eq!(
            render_page(vec![], 1, f32::NAN).unwrap_err(),
            RenderError::InvalidScale
        );
        assert_eq!(
            render_page(vec![], 1, 9.0).unwrap_err(),
            RenderError::InvalidScale
        );
        assert_eq!(
            render_page(vec![], 1, 0.005).unwrap_err(),
            RenderError::InvalidScale
        );
    }

    #[test]
    fn parsed_document_reuses_shared_bytes_for_multiple_renders() {
        let bytes = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&bytes)).unwrap();
        assert!(Arc::strong_count(&bytes) >= 2);
        assert!(pdf.page_count() >= 1);
        let first = pdf.render_page(1, 0.5).unwrap();
        let thumbnail = pdf.render_page(1, 0.05).unwrap();
        let second = pdf.render_page(1, 1.0).unwrap();
        assert!(thumbnail.width < first.width);
        assert!(thumbnail.height < first.height);
        assert!(second.width >= first.width);
        assert!(second.height >= first.height);
        assert_eq!(first.parse_ms, 0);
        assert_eq!(second.parse_ms, 0);
    }
}
