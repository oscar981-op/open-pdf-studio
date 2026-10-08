//! Experimental bounded page-to-pixel contract. No PDF interpretation or rasterizer.
//! OPGX v1 stores resolved boxes as f64; unlike legacy OPGM, dimensions need not
//! be integral. The native adapter does not emit this contract yet.

const BYTES: usize = 80;
const COORD_LIMIT: f64 = 16_384.0;

#[derive(Clone, Debug)]
pub struct PageSpace {
    media: [f64; 4],
    requested_crop: [f64; 4],
    crop: [f64; 4],
    rotation: u32,
    scale: f32,
    base: [f32; 2],
    pixels: [u32; 2],
}

fn valid_box(b: [f64; 4]) -> bool {
    b.iter().all(|n| n.is_finite() && n.abs() <= COORD_LIMIT) && b[0] < b[2] && b[1] < b[3]
}

impl PageSpace {
    pub fn new(
        media: [f64; 4],
        crop: [f64; 4],
        rotation: u32,
        scale: f32,
    ) -> Result<Self, &'static str> {
        if !valid_box(media)
            || !valid_box(crop)
            || !matches!(rotation, 0 | 90 | 180 | 270)
            || !scale.is_finite()
            || !(0.01..=8.0).contains(&scale)
        {
            return Err("invalid page boxes, rotation or scale");
        }
        let requested_crop = crop;
        let crop = [
            media[0].max(crop[0]),
            media[1].max(crop[1]),
            media[2].min(crop[2]),
            media[3].min(crop[3]),
        ];
        // Deliberately reject degenerate/sub-point pages rather than adopting
        // an implicit A4/default-size repair policy.
        if crop[2] - crop[0] < 1.0 || crop[3] - crop[1] < 1.0 {
            return Err("empty or sub-point visible page");
        }
        let base = [(crop[2] - crop[0]) as f32, (crop[3] - crop[1]) as f32];
        let size = if matches!(rotation, 90 | 270) {
            [base[1], base[0]]
        } else {
            base
        };
        // Match the established Hayro oracle's f32 dimension*scale followed by
        // floor, while point transforms below retain f64 coordinate precision.
        let scaled = [size[0] * scale, size[1] * scale];
        if scaled
            .iter()
            .any(|n| !n.is_finite() || *n < 1.0 || *n > 4096.0)
            || f64::from(scaled[0]) * f64::from(scaled[1]) > 6_000_000.0
        {
            return Err("page exceeds bounded output extent");
        }
        let pixels = [scaled[0].floor() as u32, scaled[1].floor() as u32];
        Ok(Self {
            media,
            requested_crop,
            crop,
            rotation,
            scale,
            base,
            pixels,
        })
    }

    pub fn pixels(&self) -> [u32; 2] {
        self.pixels
    }

    pub fn screen_point(&self, point: [f64; 2]) -> Result<[f64; 2], &'static str> {
        if point
            .iter()
            .any(|v| !v.is_finite() || v.abs() > COORD_LIMIT)
        {
            return Err("point exceeds coordinate bound");
        }
        let x = point[0] - self.crop[0];
        let y = point[1] - self.crop[1];
        let w = f64::from(self.base[0]);
        let h = f64::from(self.base[1]);
        let result = match self.rotation {
            0 => [x, h - y],
            90 => [y, x],
            180 => [w - x, y],
            270 => [h - y, w - x],
            _ => unreachable!(),
        };
        Ok(result.map(|v| v * f64::from(self.scale)))
    }

    /// Lossless lowering only; fractional ink edges require an AA command.
    /// A rectangle extending outside the raster is rejected, not truncated.
    pub fn aligned_rect(&self, rect: [f64; 4]) -> Option<[u32; 4]> {
        if !valid_box(rect)
            || rect[0] < self.crop[0]
            || rect[1] < self.crop[1]
            || rect[2] > self.crop[2]
            || rect[3] > self.crop[3]
        {
            return None;
        }
        let a = self.screen_point([rect[0], rect[1]]).ok()?;
        let b = self.screen_point([rect[2], rect[3]]).ok()?;
        let r = [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ];
        if r.iter().any(|n| *n < 0.0 || n.fract() != 0.0)
            || r[0] >= r[2]
            || r[1] >= r[3]
            || r[2] > f64::from(self.pixels[0])
            || r[3] > f64::from(self.pixels[1])
        {
            return None;
        }
        Some(r.map(|n| n as u32))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(BYTES);
        out.extend_from_slice(b"OPGX");
        out.extend_from_slice(&1_u32.to_le_bytes());
        for value in self.media.into_iter().chain(self.requested_crop) {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.rotation.to_le_bytes());
        out.extend_from_slice(&self.scale.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != BYTES || &bytes[..4] != b"OPGX" || bytes[4..8] != 1_u32.to_le_bytes() {
            return Err("invalid OPGX header or length");
        }
        let mut values = [0.0; 8];
        for (i, value) in values.iter_mut().enumerate() {
            *value = f64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().unwrap());
        }
        Self::new(
            values[..4].try_into().unwrap(),
            values[4..].try_into().unwrap(),
            u32::from_le_bytes(bytes[72..76].try_into().unwrap()),
            f32::from_le_bytes(bytes[76..80].try_into().unwrap()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_roundtrip_and_malformed_records() {
        let space = PageSpace::new(
            [-3.25, -8.5, 595.32, 841.92],
            [0.25, 2.5, 580.75, 820.25],
            90,
            1.25,
        )
        .unwrap();
        let bytes = space.encode();
        assert_eq!(bytes.len(), 80);
        let decoded = PageSpace::decode(&bytes).unwrap();
        assert_eq!(decoded.encode(), bytes);
        assert_eq!(
            decoded.screen_point([17.125, 22.75]),
            space.screen_point([17.125, 22.75])
        );
        for len in 0..80 {
            assert!(PageSpace::decode(&bytes[..len]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(PageSpace::decode(&extra).is_err());
        for index in 0..80 {
            let mut changed = bytes.clone();
            changed[index] ^= 0xff;
            let _ = PageSpace::decode(&changed);
        }
        let mut nan = bytes.clone();
        nan[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(PageSpace::decode(&nan).is_err());
        for (media, crop, rotation, scale) in [
            ([0., 0., 0., 100.], [0., 0., 100., 100.], 0, 1.),
            ([0., 0., 100., 100.], [200., 200., 300., 300.], 0, 1.),
            ([0., 0., 100., 100.], [0., 0., 100., 100.], 45, 1.),
            ([0., 0., 100., 100.], [0., 0., 100., 100.], 0, f32::NAN),
            ([0., 0., 4000., 4000.], [0., 0., 4000., 4000.], 0, 1.),
        ] {
            assert!(PageSpace::new(media, crop, rotation, scale).is_err());
        }
        assert!(space.screen_point([f64::INFINITY, 0.]).is_err());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn fractional_pdf_boxes_match_hayro_dimensions_and_aligned_dynlex_pixels() {
        use lopdf::{dictionary, Document, Object, Stream};
        use opds_engine_probe::ParsedPdf;
        use std::sync::Arc;
        let layouts = [
            (
                [0., 0., 64.25, 48.5],
                [0., 0., 64.25, 48.5],
                [2.25, 3.5, 10.25, 11.5],
            ),
            (
                [-3.25, -8.5, 100.25, 90.5],
                [-20., -20., 96.25, 88.5],
                [-1.25, 1.5, 6.75, 9.5],
            ),
            (
                [0., 0., 128.5, 96.25],
                [4.5, 8.25, 124.5, 88.25],
                [12.5, 16.25, 28.5, 32.25],
            ),
            (
                [0., 0., 595.32, 841.92],
                [0., 0., 595.32, 841.92],
                [16., 32., 32., 48.],
            ),
        ];
        let mut checked = 0;
        let mut rejected_ink = 0;
        for (media, crop, ink) in layouts {
            for rotation in [0, 90, 180, 270] {
                let mut doc = Document::with_version("1.7");
                let root = doc.new_object_id();
                let stream = doc.add_object(Stream::new(
                    dictionary! {},
                    format!(
                        "1 0 0 rg\n{} {} {} {} re\nf\n",
                        ink[0],
                        ink[1],
                        ink[2] - ink[0],
                        ink[3] - ink[1]
                    )
                    .into_bytes(),
                ));
                let page =
                    doc.add_object(dictionary! {"Type"=>"Page","Parent"=>root,"Contents"=>stream});
                let box_objects = |b: [f64; 4]| b.map(|v| Object::Real(v as f32)).to_vec();
                doc.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>1,
                    "Kids"=>vec![page.into()],"MediaBox"=>box_objects(media),"CropBox"=>box_objects(crop),
                    "Rotate"=>rotation as i64,"Resources"=>dictionary!{}}));
                let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
                doc.trailer.set("Root", catalog);
                let mut bytes = Vec::new();
                doc.save_to(&mut bytes).unwrap();
                let pdf = ParsedPdf::open(Arc::new(bytes)).unwrap();
                for scale in [0.25, 1.0, 2.0, 4.0] {
                    let Ok(space) = PageSpace::new(media, crop, rotation, scale) else {
                        continue;
                    };
                    let reference = pdf.render_page(1, scale).unwrap();
                    assert_eq!(space.pixels(), [reference.width, reference.height]);
                    let Some(r) = space.aligned_rect(ink) else {
                        rejected_ink += 1;
                        continue;
                    };
                    let [w, h] = space.pixels();
                    let mut scene = b"OPDL".to_vec();
                    for n in [3, w, h, 0, 0, w, h, 0, 1, 1, r[0], r[1], r[2], r[3]] {
                        scene.extend_from_slice(&n.to_le_bytes());
                    }
                    scene.extend_from_slice(&[255, 0, 0, 255]);
                    let (scene, w, h) = crate::validated_ordered_scene(scene).unwrap();
                    let actual = crate::render_validated_scene(&scene, w, h).unwrap();
                    assert_eq!(
                        actual.rgba, reference.rgba,
                        "{media:?}/{crop:?}/{rotation}/{scale}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 24);
        assert!(rejected_ink > 0);
        eprintln!("OPGX: {checked} aligned renders byte-exact; {rejected_ink} fractional ink cases rejected");
    }

    #[test]
    fn bounds_intersection_floor_and_unproven_ink_rejection() {
        let space =
            PageSpace::new([0., 0., 595.32, 841.92], [-1., -2., 600., 850.], 0, 1.).unwrap();
        assert_eq!(space.pixels(), [595, 841]);
        assert!(space.aligned_rect([0., 0., 595.32, 841.92]).is_none());
        assert!(space.aligned_rect([-2., 0., 10., 10.]).is_none());
        let space = PageSpace::new([0., 0., 64.25, 48.5], [0., 0., 64.25, 48.5], 90, 4.).unwrap();
        assert_eq!(space.pixels(), [194, 257]);
        assert_eq!(
            space.aligned_rect([2.25, 3.5, 10.25, 11.5]),
            Some([14, 9, 46, 41])
        );
    }
}
