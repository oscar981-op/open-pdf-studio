//! Independent diagnostic, never selected by the native renderer router.
use crate::{page_space::PageSpace, render_validated_scene, validated_ordered_scene, Bitmap};
use opds_opdl_adapter::raster_source::RasterPlan;
use std::sync::Arc;

pub struct FractionalProbe {
    source: Arc<Vec<u8>>,
    page: usize,
    plan: RasterPlan,
    rgb: Vec<u8>,
}
impl FractionalProbe {
    pub fn new(source: Arc<Vec<u8>>, page: usize) -> Result<Self, String> {
        let plan = RasterPlan::from_pdf(&source, page).map_err(|e| e.to_string())?;
        let space = PageSpace::from_pdf(source.clone(), page, 1.)?;
        let (media, crop, turn) = plan.geometry();
        if !space.matches_integer_geometry(media, crop, turn) {
            return Err("precise source geometry mismatch".into());
        }
        let rgb = plan.decode().map_err(|e| e.to_string())?;
        Ok(Self {
            source,
            page,
            plan,
            rgb,
        })
    }
    pub fn render(&self, scale: f32, strip_f32: bool) -> Result<Bitmap, String> {
        let space = PageSpace::from_pdf(self.source.clone(), self.page, scale)?;
        let scene = self
            .plan
            .experimental_fractional_scene(&self.rgb, scale, strip_f32)
            .map_err(|e| e.to_string())?;
        let (scene, w, h) = validated_ordered_scene(scene)?;
        if space.pixels() != [w, h] {
            return Err("precise output geometry mismatch".into());
        }
        render_validated_scene(&scene, w, h)
    }
    /// Diagnostic only: this preserves the general tiled candidate, including
    /// known sampling/AA mismatches. The native strict router never calls it.
    pub fn render_unverified_tiled(&self, scale: f32) -> Result<Bitmap, String> {
        let zoom = self
            .plan
            .zoom(&self.rgb, scale)
            .map_err(|e| e.to_string())?;
        let (width, height) = (zoom.width, zoom.height);
        let size = width as usize * height as usize * 4;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(size)
            .map_err(|_| "Not enough memory for zoom bitmap")?;
        rgba.resize(size, 255);
        for y in (0..height).step_by(1024) {
            for x in (0..width).step_by(1024) {
                let w = (width - x).min(1024);
                let h = (height - y).min(1024);
                let scene = zoom.tile_scene(x, y, w, h).map_err(|e| e.to_string())?;
                let (scene, w, h) = validated_ordered_scene(scene)?;
                let tile = render_validated_scene(&scene, w, h)?;
                for row in 0..h as usize {
                    let from = row * w as usize * 4;
                    let to = ((y as usize + row) * width as usize + x as usize) * 4;
                    rgba[to..to + w as usize * 4]
                        .copy_from_slice(&tile.rgba[from..from + w as usize * 4]);
                }
            }
        }
        Ok(Bitmap {
            width,
            height,
            rgba,
        })
    }
    pub fn decoded_bytes(&self) -> usize {
        self.rgb.len()
    }
}
