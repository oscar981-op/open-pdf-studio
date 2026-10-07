//! Source-bound, lazily prepared first-page display list. PDF interpretation is
//! shared across zooms; unsupported sources are remembered as well.
use super::{Backend, Bitmap, RenderOutcome};
use opds_engine_probe::{ParsedPdf, RenderError};
use std::sync::Arc;
#[cfg(feature = "kernel")]
use std::sync::OnceLock;

pub struct DynlexDocument {
    source: Arc<Vec<u8>>,
    #[cfg(feature = "kernel")]
    prepared: OnceLock<Result<Option<PreparedPage>, String>>,
}

impl DynlexDocument {
    pub fn new(source: Arc<Vec<u8>>) -> Self {
        Self {
            source,
            #[cfg(feature = "kernel")]
            prepared: OnceLock::new(),
        }
    }

    pub fn try_render(&self, page: usize, scale: f32) -> Result<Option<Bitmap>, String> {
        if page != 1 || !scale.is_finite() || !(0.01..=8.0).contains(&scale) {
            return Ok(None);
        }
        #[cfg(feature = "kernel")]
        {
            match self
                .prepared
                .get_or_init(|| PreparedPage::from_pdf(&self.source))
            {
                Ok(Some(prepared)) => prepared.render(scale),
                Ok(None) => Ok(None),
                Err(error) => Err(error.clone()),
            }
        }
        #[cfg(not(feature = "kernel"))]
        {
            Ok(None)
        }
    }

    /// Strict native viewer route: never invokes another rasterizer, including
    /// the mixed Hayro-body/DynLex-annotation appearance route.
    pub fn render_strict(
        &self,
        pdf: &ParsedPdf,
        page: usize,
        scale: f32,
    ) -> Result<RenderOutcome, String> {
        let started = std::time::Instant::now();
        let bitmap = self.try_render(page, scale)?
            .ok_or_else(|| "DynLex does not support this page or zoom yet. No fallback rendered. Select another renderer manually.".to_string())?;
        let (page_width_pt, page_height_pt) =
            pdf.page_dimensions(page).map_err(|e| e.to_string())?;
        Ok(RenderOutcome {
            used: Backend::Dynlex,
            imported_annotations_dynlex: false,
            raster: opds_engine_probe::Raster {
                width: bitmap.width,
                height: bitmap.height,
                rgba: bitmap.rgba,
                page_width_pt,
                page_height_pt,
                total_pages: pdf.page_count(),
                parse_ms: 0,
                render_ms: started.elapsed().as_millis(),
            },
        })
    }

    /// The source belongs to this cache; callers cannot accidentally reuse a
    /// prepared scene for different PDF bytes. The host owns the matching metadata.
    pub fn render_with_report(
        &self,
        pdf: &ParsedPdf,
        page: usize,
        scale: f32,
        backend: Backend,
    ) -> Result<RenderOutcome, RenderError> {
        super::render_with_report_using(pdf, &self.source, page, scale, backend, || {
            self.try_render(page, scale)
        })
    }
}

#[cfg(feature = "kernel")]
mod scene {
    use super::super::{
        prepare_scene, render_validated_scene, validated_ordered_scene, word, Bitmap,
    };
    use crate::lbvh::Lbvh;

    /// A rectangle in output pixels, after page rotation and crop.
    #[derive(Debug, Clone, Copy)]
    pub struct PixelViewport {
        pub x: u32,
        pub y: u32,
        pub width: u32,
        pub height: u32,
    }

    struct Command {
        rect: [u32; 4],
        color: [u8; 4],
    }
    struct Vectors {
        commands: Vec<Command>,
        index: Lbvh,
        edge_gcd: u32,
    }

    pub struct PreparedPage {
        scene: Vec<u8>,
        width: u32,
        height: u32,
        vectors: Option<Vectors>,
    }

    fn gcd(mut a: u32, mut b: u32) -> u32 {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }

    // Exact integer edges only: no tolerance that silently changes AA or image sampling.
    fn scaled(value: u32, scale: f32) -> Option<u32> {
        let n = f64::from(value) * f64::from(scale);
        (n.is_finite() && n >= 0.0 && n <= 4096.0 && n.fract() == 0.0).then_some(n as u32)
    }

    #[test]
    fn gcd_alignment_agrees_with_every_edge_at_binary_scales() {
        for step in 1..=64 {
            let edges = [0, step, step * 3, step * 7, step * 16];
            let g = edges.iter().fold(0, |g, &n| gcd(g, n));
            for numerator in 1..=512 {
                let scale = numerator as f32 / 64.0;
                // Dimensions are checked separately; this tests only alignment.
                let expected = edges
                    .iter()
                    .all(|&n| (f64::from(n) * f64::from(scale)).fract() == 0.0);
                assert_eq!(scaled(g, scale).is_some(), expected);
            }
        }
    }

    impl PreparedPage {
        pub fn from_pdf(bytes: &[u8]) -> Result<Option<Self>, String> {
            let Some((scene, width, height)) = prepare_scene(bytes)? else {
                return Ok(None);
            };
            let mut vectors = None;
            if word(&scene, 32)? == 0 {
                let count = word(&scene, 36)? as usize;
                let mut commands = Vec::with_capacity(count);
                for index in 0..count {
                    let at = 40 + index * 24;
                    if !matches!(word(&scene, at)?, 1 | 3) {
                        commands.clear();
                        break;
                    }
                    commands.push(Command {
                        rect: [
                            word(&scene, at + 4)?,
                            word(&scene, at + 8)?,
                            word(&scene, at + 12)?,
                            word(&scene, at + 16)?,
                        ],
                        color: scene[at + 20..at + 24]
                            .try_into()
                            .map_err(|_| "invalid color")?,
                    });
                }
                if commands.len() == count {
                    let bounds: Vec<_> = commands.iter().map(|c| c.rect).collect();
                    let index = Lbvh::build(&bounds, width, height)?;
                    let edge_gcd = bounds
                        .iter()
                        .flatten()
                        .fold(gcd(width, height), |g, &n| gcd(g, n));
                    vectors = Some(Vectors {
                        commands,
                        index,
                        edge_gcd,
                    });
                }
            }
            Ok(Some(Self {
                scene,
                width,
                height,
                vectors,
            }))
        }

        pub fn render(&self, scale: f32) -> Result<Option<Bitmap>, String> {
            if scale == 1.0 {
                return render_validated_scene(&self.scene, self.width, self.height).map(Some);
            }
            let Some((width, height)) = self.scaled_size(scale) else {
                return Ok(None);
            };
            self.render_viewport(
                scale,
                PixelViewport {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
            )
        }

        fn scaled_size(&self, scale: f32) -> Option<(u32, u32)> {
            if !scale.is_finite() || !(0.01..=8.0).contains(&scale) {
                return None;
            }
            let (width, height) = (scaled(self.width, scale)?, scaled(self.height, scale)?);
            (width > 0 && height > 0 && width.checked_mul(height)? <= 6_000_000)
                .then_some((width, height))
        }

        /// Experimental viewport API for opaque axis-aligned vector commands.
        /// Image/mask resampling is deliberately unsupported. All scene edges must
        /// remain pixel-aligned at the requested scale, including off-screen ones.
        pub fn render_viewport(
            &self,
            scale: f32,
            view: PixelViewport,
        ) -> Result<Option<Bitmap>, String> {
            let Some((width, height)) = self.scaled_size(scale) else {
                return Ok(None);
            };
            let Some(vectors) = &self.vectors else {
                return Ok(None);
            };
            let Some(right) = view.x.checked_add(view.width) else {
                return Err("viewport overflow".into());
            };
            let Some(bottom) = view.y.checked_add(view.height) else {
                return Err("viewport overflow".into());
            };
            if view.width == 0 || view.height == 0 || right > width || bottom > height {
                return Err("viewport outside page".into());
            }
            // Every coordinate is a multiple of this GCD. With bounded integer
            // edges and an exact binary f32 scale, this proves alignment for all
            // commands once per query, including commands outside the viewport.
            if scaled(vectors.edge_gcd, scale).is_none() {
                return Ok(None);
            }
            let candidates = vectors.index.query([view.x, view.y, right, bottom], scale);
            let mut scene = Vec::with_capacity(40 + candidates.len() * 24);
            scene.extend_from_slice(b"OPDL");
            for value in [
                3,
                view.width,
                view.height,
                0,
                0,
                view.width,
                view.height,
                0,
                0,
            ] {
                scene.extend_from_slice(&value.to_le_bytes());
            }
            let mut count = 0_u32;
            // Original command order is retained after spatial selection.
            for index in candidates {
                let command = &vectors.commands[index];
                let r = command
                    .rect
                    .map(|n| scaled(n, scale).expect("GCD proved bounded aligned edges"));
                let r = [
                    r[0].max(view.x),
                    r[1].max(view.y),
                    r[2].min(right),
                    r[3].min(bottom),
                ];
                if r[0] >= r[2] || r[1] >= r[3] {
                    continue;
                }
                for value in [
                    1,
                    r[0] - view.x,
                    r[1] - view.y,
                    r[2] - view.x,
                    r[3] - view.y,
                ] {
                    scene.extend_from_slice(&value.to_le_bytes());
                }
                scene.extend_from_slice(&command.color);
                count += 1;
            }
            scene[36..40].copy_from_slice(&count.to_le_bytes());
            let (scene, width, height) = validated_ordered_scene(scene)?;
            render_validated_scene(&scene, width, height).map(Some)
        }
    }
}
#[cfg(feature = "kernel")]
pub use scene::{PixelViewport, PreparedPage};

#[cfg(all(test, feature = "kernel"))]
mod tests {
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};

    fn fills_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let content = doc.add_object(Stream::new(dictionary! {}, b"1 0 0 rg\n16 16 160 120 re\nf\n0 0 1 rg\n64 48 144 96 re\nf\n0 1 0 rg\n240 176 16 16 re\nf\n".to_vec()));
        let page = doc.add_object(dictionary! { "Type"=>"Page", "Parent"=>pages,
        "MediaBox"=>vec![0.into(),0.into(),256.into(),192.into()],
        "Resources"=>dictionary!{}, "Contents"=>content });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}),
        );
        let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn crop(rgba: &[u8], width: u32, v: PixelViewport) -> Vec<u8> {
        (v.y..v.y + v.height)
            .flat_map(|y| {
                let start = ((y * width + v.x) * 4) as usize;
                rgba[start..start + v.width as usize * 4].iter().copied()
            })
            .collect()
    }

    #[test]
    fn aligned_zoom_and_viewport_tiles_match_complete_hayro_pages() {
        let bytes = Arc::new(fills_pdf());
        let pdf = ParsedPdf::open(bytes.clone()).unwrap();
        let prepared = PreparedPage::from_pdf(&bytes).unwrap().unwrap();
        for scale in [0.25, 0.5, 1.0, 1.25, 2.0, 4.0] {
            let reference = pdf.render_page(1, scale).unwrap();
            let actual = prepared.render(scale).unwrap().unwrap();
            assert_eq!(
                (actual.width, actual.height),
                (reference.width, reference.height)
            );
            assert_eq!(actual.rgba, reference.rgba, "scale {scale}");
            // Odd tile sizes exercise partial rectangles, empty tiles, grid
            // boundaries and overlapping paint order across adjacent tiles.
            for y in (0..actual.height).step_by(37) {
                for x in (0..actual.width).step_by(53) {
                    let view = PixelViewport {
                        x,
                        y,
                        width: 53.min(actual.width - x),
                        height: 37.min(actual.height - y),
                    };
                    let tile = prepared.render_viewport(scale, view).unwrap().unwrap();
                    assert_eq!(
                        tile.rgba,
                        crop(&reference.rgba, reference.width, view),
                        "scale {scale}, tile {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn scaled_axis_strokes_match_and_uncertain_edges_fall_back() {
        let bytes = Arc::new(include_bytes!("../tests/fixtures/simple-fill-stroke.pdf").to_vec());
        let pdf = ParsedPdf::open(bytes.clone()).unwrap();
        let document = DynlexDocument::new(bytes);
        for scale in [1.0, 2.0, 3.0, 4.0] {
            let actual = document.try_render(1, scale).unwrap().unwrap();
            assert_eq!(
                actual.rgba,
                pdf.render_page(1, scale).unwrap().rgba,
                "scale {scale}"
            );
        }
        for scale in [0.25, 1.25, f32::NAN, 0.0, f32::INFINITY, 8.1] {
            assert!(document.try_render(1, scale).unwrap().is_none());
        }
        assert!(document.try_render(2, 1.0).unwrap().is_none());
    }

    #[test]
    fn cached_scene_keeps_raster_and_hybrid_baselines_and_negative_results() {
        for bytes in [
            include_bytes!("../tests/fixtures/full-page-raster.pdf").as_slice(),
            include_bytes!("../tests/fixtures/two-images-hybrid.pdf").as_slice(),
        ] {
            let source = Arc::new(bytes.to_vec());
            let pdf = ParsedPdf::open(source.clone()).unwrap();
            let doc = DynlexDocument::new(source);
            let first = doc.try_render(1, 1.0).unwrap().unwrap();
            let scene = doc.prepared.get().unwrap() as *const _;
            assert_eq!(first.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
            assert!(doc.try_render(1, 2.0).unwrap().is_none());
            assert_eq!(doc.try_render(1, 1.0).unwrap().unwrap().rgba, first.rgba);
            assert_eq!(doc.prepared.get().unwrap() as *const _, scene);
        }
        let doc = DynlexDocument::new(Arc::new(b"invalid".to_vec()));
        assert!(doc.try_render(1, 1.0).unwrap().is_none());
        assert!(matches!(doc.prepared.get(), Some(Ok(None))));
        assert!(doc.try_render(1, 2.0).unwrap().is_none());
    }

    #[test]
    fn viewport_and_pixel_limits_are_checked_before_rendering() {
        let page = PreparedPage::from_pdf(&fills_pdf()).unwrap().unwrap();
        for view in [
            PixelViewport {
                x: 256,
                y: 0,
                width: 1,
                height: 1,
            },
            PixelViewport {
                x: u32::MAX,
                y: 0,
                width: 2,
                height: 1,
            },
            PixelViewport {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
        ] {
            assert!(page.render_viewport(1.0, view).is_err());
        }
        assert!(page.render(8.01).unwrap().is_none());
        assert!(page.render(f32::NAN).unwrap().is_none());
    }
}
