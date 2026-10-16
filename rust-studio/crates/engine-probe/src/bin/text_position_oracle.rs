//! Test-only Hayro interpreter oracle. Never called by a DynLex/native route.
use hayro::hayro_interpret::{self as hi, font::Glyph, util::TransformExt, Device};
use hayro::hayro_syntax::Pdf;
use std::{error::Error, io::Read, sync::Arc};
use vello_cpu::kurbo::{Affine, BezPath, Rect};
#[derive(Default)]
struct Trace {
    lines: Vec<String>,
}
impl<'a> Device<'a> for Trace {
    fn set_soft_mask(&mut self, _: Option<hi::SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: hi::BlendMode) {}
    fn draw_path(&mut self, p: &BezPath, t: Affine, _: &hi::Paint<'a>, _: &hi::PathDrawMode) {
        self.lines
            .push(format!("R\t{:?}\t{:?}", p.elements(), t.as_coeffs()));
    }
    fn push_clip_path(&mut self, _: &hi::ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<hi::SoftMask<'a>>, _: hi::BlendMode) {}
    fn draw_glyph(
        &mut self,
        g: &Glyph<'a>,
        t: Affine,
        gt: Affine,
        _: &hi::Paint<'a>,
        _: &hi::GlyphDrawMode,
    ) {
        match g {
            Glyph::Outline(g) if !g.outline().elements().is_empty() => self.lines.push(format!(
                "G\t{}\t{:?}",
                g.glyph_id().to_u32(),
                (t * gt).as_coeffs()
            )),
            Glyph::Outline(_) => {}
            _ => self.lines.push("UNSUPPORTED TYPE3".into()),
        }
    }
    fn draw_image(&mut self, _: hi::Image<'a, '_>, _: Affine) {
        self.lines.push("IMAGE".into());
    }
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}
fn main() -> Result<(), Box<dyn Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 5 {
        return Err("usage: text_position_oracle INPUT.pdf PAGE SCALE OUTPUT.tsv".into());
    };
    let mut b = Vec::new();
    std::fs::File::open(&a[1])?
        .take(16_000_001)
        .read_to_end(&mut b)?;
    if b.len() > 16_000_000 {
        return Err("source limit".into());
    };
    let pdf = Pdf::new(Arc::new(b)).map_err(|e| format!("PDF parse: {e:?}"))?;
    let page: usize = a[2].parse()?;
    let p = pdf
        .pages()
        .get(page.checked_sub(1).ok_or("page starts at 1")?)
        .ok_or("page missing")?;
    let scale: f32 = a[3].parse()?;
    if !scale.is_finite() || !(0.01..=8.).contains(&scale) {
        return Err("scale bound".into());
    };
    let (w, h) = p.render_dimensions();
    let transform = Affine::scale(f64::from(scale)) * p.initial_transform(true).to_kurbo();
    let cache = hi::InterpreterCache::new();
    let mut ctx = hi::Context::new(
        transform,
        Rect::new(0., 0., f64::from(w * scale), f64::from(h * scale)),
        &cache,
        p.xref(),
        hi::InterpreterSettings::default(),
    );
    let mut trace = Trace::default();
    hi::interpret_page(p, &mut ctx, &mut trace);
    if trace.lines.len() > 16384 {
        return Err("trace limit".into());
    };
    std::fs::write(&a[4], trace.lines.join("\n") + "\n")?;
    Ok(())
}
