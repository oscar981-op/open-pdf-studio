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
    pub fn decoded_bytes(&self) -> usize {
        self.rgb.len()
    }
}
