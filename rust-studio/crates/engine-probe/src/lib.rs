//! Reusable pure-Rust PDF raster entry point for the future native viewer.
#![forbid(unsafe_code)]

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};
use std::time::Instant;

pub const MAX_SIDE: f64 = 4096.0;
pub const MAX_PIXELS: f64 = 16_000_000.0;

#[derive(Debug)]
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

/// Page numbers are one-based at this API boundary. `bytes` is owned because
/// Hayro's parser stores it; the UI can pass a clone of its immutable Arc bytes.
/// The caller runs this off the UI thread and checks its document render ticket.
pub fn render_page(bytes: Vec<u8>, page_number: usize, scale: f32) -> Result<Raster, RenderError> {
    if page_number == 0 {
        return Err(RenderError::InvalidPage);
    }
    if !scale.is_finite() || !(0.1..=8.0).contains(&scale) {
        return Err(RenderError::InvalidScale);
    }
    let started = Instant::now();
    let pdf = Pdf::new(bytes).map_err(|error| RenderError::Parse(format!("{error:?}")))?;
    let pages = pdf.pages();
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
    let parse_ms = started.elapsed().as_millis();
    let started = Instant::now();
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        },
    );
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    let rgba = pixmap.data_as_u8_slice().to_vec();
    if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
        return Err(RenderError::InvalidBitmap);
    }
    Ok(Raster {
        width,
        height,
        rgba,
        page_width_pt,
        page_height_pt,
        total_pages: pages.len(),
        parse_ms,
        render_ms: started.elapsed().as_millis(),
    })
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
    }
}
