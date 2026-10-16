//! Reusable bounded, ordered nonzero-winding page scene. No PDF parsing here.
//! The resource-bounded text worker supplies OPNW tiles; native IPC revalidates them.
use crate::{experimental_winding, glyph_scene, Bitmap};
use std::error::Error;

const MAX_PAINTS: usize = 8192;
const MAX_SCENE_BYTES: usize = 32 * 1024 * 1024;

struct Paint {
    words: Vec<u64>,
    color: [u8; 3],
    origin: [u32; 2],
    clip: [u32; 4],
    fractional_clip: Option<[f64; 4]>,
}

/// Owns validated immutable geometry. The scene is bound to one page and zoom;
/// replay allocates a fresh output and cannot retain pixels from another render.
pub struct PreparedWindingPage {
    width: u32,
    height: u32,
    paints: Vec<Paint>,
    bytes: usize,
}

impl PreparedWindingPage {
    pub fn new(width: u32, height: u32) -> Result<Self, Box<dyn Error>> {
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 6_000_000
        {
            return Err("prepared page exceeds pixel budget".into());
        }
        Ok(Self {
            width,
            height,
            paints: Vec::with_capacity(MAX_PAINTS),
            bytes: MAX_PAINTS * std::mem::size_of::<Paint>(),
        })
    }

    /// A rejected paint leaves the scene unchanged. All fields remain private,
    /// so replay never accepts unvalidated kernel words or arbitrary RGBA data.
    pub fn push(
        &mut self,
        scene: &[u8],
        color: [u8; 3],
        origin: [u32; 2],
    ) -> Result<(), Box<dyn Error>> {
        self.push_clipped(scene, color, origin, [0, 0, self.width, self.height])
    }

    /// Integer clip edges retain the established v2 path. Fractional
    /// rectangles use `push_fractional_clipped` and validated v3 records.
    pub fn push_clipped(
        &mut self,
        scene: &[u8],
        color: [u8; 3],
        origin: [u32; 2],
        clip: [u32; 4],
    ) -> Result<(), Box<dyn Error>> {
        self.push_fractional_clipped(scene, color, origin, clip, None)
    }

    /// A fractional clip is a bounded axis-aligned rectangle in screen
    /// coordinates. V3 IPC and each paint are revalidated before the ABI.
    pub fn push_fractional_clipped(
        &mut self,
        scene: &[u8],
        color: [u8; 3],
        origin: [u32; 2],
        clip: [u32; 4],
        fractional_clip: Option<[f64; 4]>,
    ) -> Result<(), Box<dyn Error>> {
        if self.paints.len() >= MAX_PAINTS
            || origin[0] >= self.width
            || origin[1] >= self.height
            || origin.iter().any(|v| v % 4 != 0)
            || clip[0] >= clip[2]
            || clip[1] >= clip[3]
            || clip[2] > self.width
            || clip[3] > self.height
            || fractional_clip.is_some_and(|c| {
                c.iter().any(|v| !v.is_finite())
                    || c[0] < 0.
                    || c[1] < 0.
                    || c[2] > f64::from(self.width)
                    || c[3] > f64::from(self.height)
                    || c[0] >= c[2]
                    || c[1] >= c[3]
                    || c[0].max(f64::from(clip[0])) >= c[2].min(f64::from(clip[2]))
                    || c[1].max(f64::from(clip[1])) >= c[3].min(f64::from(clip[3]))
            })
        {
            return Err("prepared paint count/origin/strip alignment".into());
        }
        let words = experimental_winding::words(scene)?;
        let bytes = words.capacity() * std::mem::size_of::<u64>();
        if self
            .bytes
            .checked_add(bytes)
            .is_none_or(|n| n > MAX_SCENE_BYTES)
        {
            return Err("prepared geometry exceeds memory budget".into());
        }
        self.paints.push(Paint {
            words,
            color,
            origin,
            clip,
            fractional_clip,
        });
        self.bytes += bytes;
        Ok(())
    }

    /// Versioned, bounded scene IPC. No pointers, PDF objects or font handles.
    pub fn encode(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut out = b"PWPG".to_vec();
        // Preserve v1 on unclipped pages so a running older app can still
        // consume a freshly rebuilt helper. Only clips require v2 records.
        let version = if self.paints.iter().any(|p| p.fractional_clip.is_some()) {
            3
        } else if self
            .paints
            .iter()
            .any(|p| p.clip != [0, 0, self.width, self.height])
        {
            2
        } else {
            1
        };
        for n in [
            version,
            self.width,
            self.height,
            self.paints.len() as u32,
            0,
        ] {
            out.extend(n.to_le_bytes());
        }
        for paint in &self.paints {
            let mut scene = b"OPNW".to_vec();
            let count = paint.words[2] as usize;
            for n in [1, 256, 192, count as u32] {
                scene.extend(n.to_le_bytes());
            }
            let mut at = 3;
            for _ in 0..count {
                let points = paint.words[at] as usize;
                scene.extend((points as u32).to_le_bytes());
                at += 1;
                for word in &paint.words[at..at + points * 2] {
                    scene.extend(word.to_ne_bytes());
                }
                at += points * 2;
            }
            for v in paint.origin {
                out.extend(v.to_le_bytes());
            }
            out.extend(paint.color);
            out.push(0);
            if version >= 2 {
                for v in paint.clip {
                    out.extend(v.to_le_bytes());
                }
            }
            if version == 3 {
                for value in paint.fractional_clip.unwrap_or([0.; 4]) {
                    out.extend(value.to_le_bytes());
                }
            }
            out.extend((scene.len() as u32).to_le_bytes());
            out.extend(scene);
            if out.len() > MAX_SCENE_BYTES {
                return Err("scene IPC byte budget".into());
            }
        }
        Ok(out)
    }

    /// Revalidate every record from the worker, even after successful exit.
    /// The worker never supplies native ABI words or an already rendered bitmap.
    pub fn decode(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() < 24 || bytes.len() > MAX_SCENE_BYTES || &bytes[..4] != b"PWPG" {
            return Err("scene IPC length/magic".into());
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if !matches!(word(4), 1 | 2 | 3) || word(20) != 0 || word(16) as usize > MAX_PAINTS {
            return Err("scene IPC version/count/reserved".into());
        }
        let mut page = Self::new(word(8), word(12))?;
        let version = word(4);
        let mut at = 24;
        for _ in 0..word(16) {
            let header_len = match version {
                1 => 16,
                2 => 32,
                3 => 64,
                _ => unreachable!(),
            };
            if bytes.len() - at < header_len {
                return Err("scene IPC record truncated".into());
            }
            let origin = [word(at), word(at + 4)];
            let color = bytes[at + 8..at + 11].try_into()?;
            let clip = if version >= 2 {
                [word(at + 12), word(at + 16), word(at + 20), word(at + 24)]
            } else {
                [0, 0, page.width, page.height]
            };
            let fractional_clip = if version == 3 {
                let mut values = [0.; 4];
                for (axis, value) in values.iter_mut().enumerate() {
                    let start = at + 28 + axis * 8;
                    *value = f64::from_le_bytes(bytes[start..start + 8].try_into()?);
                }
                if values == [0.; 4] {
                    None
                } else {
                    Some(values)
                }
            } else {
                None
            };
            let len = word(at + header_len - 4) as usize;
            if bytes[at + 11] != 0 || !(20..=4148).contains(&len) {
                return Err("scene IPC record size/reserved".into());
            }
            at += header_len;
            let end = at
                .checked_add(len)
                .filter(|n| *n <= bytes.len())
                .ok_or("scene IPC truncated geometry")?;
            page.push_fractional_clipped(&bytes[at..end], color, origin, clip, fractional_clip)?;
            // Bounds are checked before replay, not discovered halfway through FFI.
            let paint = page.paints.last().unwrap();
            let mut p = 3;
            for _ in 0..paint.words[2] {
                let count = paint.words[p] as usize;
                p += 1;
                for point in paint.words[p..p + count * 2].chunks_exact(2) {
                    for axis in 0..2 {
                        if f64::from_bits(u64::from_le(point[axis])) + f64::from(origin[axis])
                            > f64::from([page.width, page.height][axis])
                        {
                            return Err("scene IPC geometry outside page".into());
                        }
                    }
                }
                p += count * 2;
            }
            at = end;
        }
        if at != bytes.len() {
            return Err("scene IPC trailing bytes".into());
        }
        Ok(page)
    }

    pub fn paint_count(&self) -> usize {
        self.paints.len()
    }

    pub fn dimensions(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// Retained geometry including reserved paint slots, excluding the output
    /// and the fixed coverage/command scratch buffers used during replay.
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    pub fn render(&self) -> Result<Bitmap, Box<dyn Error>> {
        let paints: Vec<_> = self
            .paints
            .iter()
            .map(|p| (p.words.as_slice(), p.color, p.origin, p.clip))
            .collect();
        let rgba = if self.paints.iter().any(|p| p.fractional_clip.is_some()) {
            let clips: Vec<_> = self.paints.iter().map(|p| p.fractional_clip).collect();
            glyph_scene::render_prepared_winding_page_fractional(
                self.width,
                self.height,
                &paints,
                &clips,
            )?
        } else {
            glyph_scene::render_prepared_winding_page(self.width, self.height, &paints)?
        };
        Ok(Bitmap {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    /// Replay text/vector paints over a separately validated opaque image
    /// layer. This preserves paint order when an image precedes the glyphs;
    /// source PDF parsing and image validation remain outside this API.
    pub fn render_over_opaque(&self, background: Bitmap) -> Result<Bitmap, Box<dyn Error>> {
        if [background.width, background.height] != self.dimensions() {
            return Err("prepared background dimensions differ from scene".into());
        }
        let paints: Vec<_> = self
            .paints
            .iter()
            .map(|p| (p.words.as_slice(), p.color, p.origin, p.clip))
            .collect();
        let clips: Vec<_> = self.paints.iter().map(|p| p.fractional_clip).collect();
        let rgba = glyph_scene::render_prepared_winding_page_fractional_over_opaque(
            self.width,
            self.height,
            &paints,
            &clips,
            background.rgba,
        )?;
        Ok(Bitmap {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    /// Direct research replay of supplied fractional rectangles. Native
    /// dispatch uses the same bounded compositor through validated v3 IPC.
    pub fn render_fractional_research(
        &self,
        fractional_clips: &[Option<[f64; 4]>],
    ) -> Result<Bitmap, Box<dyn Error>> {
        let paints: Vec<_> = self
            .paints
            .iter()
            .map(|p| (p.words.as_slice(), p.color, p.origin, p.clip))
            .collect();
        let rgba = glyph_scene::render_prepared_winding_page_fractional(
            self.width,
            self.height,
            &paints,
            fractional_clips,
        )?;
        Ok(Bitmap {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    /// Diagnostic timings: raster, coverage bounds/command, compositor, white paper.
    /// Excludes parser/worker and output allocation. Never used by app rendering.
    pub fn profile_replay(&self) -> Result<(Bitmap, [f64; 4]), Box<dyn Error>> {
        let paints: Vec<_> = self
            .paints
            .iter()
            .map(|p| (p.words.as_slice(), p.color, p.origin, p.clip))
            .collect();
        let mut timings = [std::time::Duration::ZERO; 4];
        let rgba = glyph_scene::profile_prepared_winding_page(
            self.width,
            self.height,
            &paints,
            &mut timings,
        )?;
        Ok((
            Bitmap {
                width: self.width,
                height: self.height,
                rgba,
            },
            timings.map(|t| t.as_secs_f64() * 1000.),
        ))
    }

    /// Diagnostic baseline: replay the same prepared geometry through the old
    /// per-paint compositor. It is not the fast page route or a PDF oracle.
    #[doc(hidden)]
    pub fn render_sequential_reference(&self) -> Result<Bitmap, Box<dyn Error>> {
        let mut rgba = vec![0; self.width as usize * self.height as usize * 4];
        for paint in &self.paints {
            if paint.clip != [0, 0, self.width, self.height] {
                return Err("sequential diagnostic does not support clips".into());
            }
            rgba = glyph_scene::paint_experimental_winding(
                paint.words.clone(),
                paint.color,
                rgba,
                self.width,
                self.height,
                paint.origin,
            )?;
        }
        for pixel in rgba.chunks_exact_mut(4) {
            let paper = 255 - pixel[3];
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(paper);
            }
            pixel[3] = 255;
        }
        Ok(Bitmap {
            width: self.width,
            height: self.height,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(left: f64, top: f64, right: f64, bottom: f64) -> Vec<u8> {
        let mut scene = b"OPNW".to_vec();
        for n in [1_u32, 256, 192, 1, 5] {
            scene.extend(n.to_le_bytes());
        }
        for p in [
            [left, top],
            [right, top],
            [right, bottom],
            [left, bottom],
            [left, top],
        ] {
            for v in p {
                scene.extend(v.to_le_bytes());
            }
        }
        scene
    }

    #[test]
    fn ordered_replay_preserves_opaque_image_background_and_rejects_bad_input() {
        let mut page = PreparedWindingPage::new(256, 192).unwrap();
        page.push(&rectangle(12., 16., 38., 44.), [200, 0, 0], [0, 0])
            .unwrap();
        let white = Bitmap {
            width: 256,
            height: 192,
            rgba: vec![255; 256 * 192 * 4],
        };
        assert_eq!(
            page.render_over_opaque(white).unwrap().rgba,
            page.render().unwrap().rgba
        );
        let mut image = Bitmap {
            width: 256,
            height: 192,
            rgba: vec![0; 256 * 192 * 4],
        };
        for pixel in image.rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[35, 90, 140, 255]);
        }
        let composited = page.render_over_opaque(image.clone()).unwrap();
        assert_eq!(&composited.rgba[..4], &[35, 90, 140, 255]);
        assert_ne!(composited.rgba, image.rgba);
        image.rgba[3] = 254;
        assert!(page.render_over_opaque(image.clone()).is_err());
        image.rgba[3] = 255;
        image.width = 255;
        assert!(page.render_over_opaque(image).is_err());
    }

    #[test]
    fn replay_preserves_order_and_matches_sequential_composition() {
        let mut page = PreparedWindingPage::new(300, 220).unwrap();
        let a = rectangle(0.25, 0.5, 30.75, 20.25);
        let b = rectangle(2.5, 1.25, 23.5, 30.75);
        let mut old = vec![0; 300 * 220 * 4];
        for (scene, color, origin) in [(&a, [255, 0, 0], [248, 180]), (&b, [0, 0, 255], [248, 180])]
        {
            page.push(scene, color, origin).unwrap();
            old = experimental_winding::paint(scene, color, old, 300, 220, origin).unwrap();
        }
        for pixel in old.chunks_exact_mut(4) {
            let paper = 255 - pixel[3];
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(paper);
            }
            pixel[3] = 255;
        }
        assert_eq!(page.render().unwrap().rgba, old);
        assert_eq!(page.render().unwrap().rgba, old);
        std::thread::scope(|s| {
            let first = s.spawn(|| page.render().unwrap().rgba);
            let second = s.spawn(|| page.render().unwrap().rgba);
            assert_eq!(first.join().unwrap(), second.join().unwrap());
        });
    }

    #[test]
    fn invalid_inputs_do_not_change_prepared_scene() {
        for size in [(0, 1), (1, 0), (4097, 1), (4096, 4096)] {
            assert!(PreparedWindingPage::new(size.0, size.1).is_err());
        }
        let mut page = PreparedWindingPage::new(256, 192).unwrap();
        let scene = rectangle(1., 1., 4., 4.);
        for end in 0..scene.len() {
            assert!(page.push(&scene[..end], [0; 3], [0, 0]).is_err());
        }
        for origin in [[256, 0], [0, 192], [1, 0], [u32::MAX, 0]] {
            assert!(page.push(&scene, [0; 3], origin).is_err());
        }
        assert_eq!(page.paint_count(), 0);
        assert!(page.render().unwrap().rgba.iter().all(|&v| v == 255));
        for _ in 0..MAX_PAINTS {
            page.push(&scene, [0; 3], [0, 0]).unwrap();
        }
        let before = page.retained_bytes();
        assert!(page.push(&scene, [0; 3], [0, 0]).is_err());
        assert_eq!(page.retained_bytes(), before);
        // A geometrically valid contour with ink beyond the page must never
        // expose a partially rendered bitmap to the caller.
        let mut clipped = PreparedWindingPage::new(16, 16).unwrap();
        clipped
            .push(&rectangle(0., 0., 32., 32.), [0; 3], [0, 0])
            .unwrap();
        assert!(clipped.render().is_err());
    }

    #[test]
    fn bounded_coverage_scan_matches_full_scan_at_strip_and_tile_edges() {
        // Values on both sides of f32 and 4px strip boundaries, plus the
        // largest tile. The reference scans all coverage bytes independently.
        let mut page = PreparedWindingPage::new(512, 384).unwrap();
        for (i, x) in [
            0., 0.00001, 3.9999999, 4., 4.0000001, 127.999999, 128., 248., 251.999999, 252.,
        ]
        .into_iter()
        .enumerate()
        {
            for y in [0., 3.9999999, 4., 183.999999, 188.] {
                let r = rectangle(x, y, (x + 3.75).min(256.), (y + 3.5).min(192.));
                page.push(&r, [(i * 23) as u8, 91, 203], [252, 192])
                    .unwrap();
            }
        }
        // A large paint followed by a tiny one exercises reused coverage and
        // command storage: no stale ink may leak from the previous paint.
        page.push(&rectangle(0., 0., 256., 192.), [80, 40, 0], [0, 0])
            .unwrap();
        page.push(&rectangle(1.25, 2.5, 2.75, 3.25), [0, 255, 0], [252, 192])
            .unwrap();
        assert_eq!(
            page.render().unwrap().rgba,
            page.render_sequential_reference().unwrap().rgba
        );
        let (profiled, _) = page.profile_replay().unwrap();
        assert_eq!(profiled.rgba, page.render().unwrap().rgba);
    }

    #[test]
    fn scene_ipc_revalidates_all_geometry_and_framing() {
        let mut page = PreparedWindingPage::new(256, 192).unwrap();
        page.push(&rectangle(2., 3., 22., 18.), [17, 61, 229], [4, 4])
            .unwrap();
        let wire = page.encode().unwrap();
        let decoded = PreparedWindingPage::decode(&wire).unwrap();
        assert_eq!(decoded.encode().unwrap(), wire);
        assert_eq!(decoded.render().unwrap().rgba, page.render().unwrap().rgba);
        assert_eq!(u32::from_le_bytes(wire[4..8].try_into().unwrap()), 1);
        for end in 0..wire.len() {
            assert!(PreparedWindingPage::decode(&wire[..end]).is_err());
        }
        for (at, value) in [
            (4, 3_u32),
            (8, 4097),
            (16, 8193),
            (20, 1),
            (24, 1),
            (36, 4149),
        ] {
            let mut bad = wire.clone();
            bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(PreparedWindingPage::decode(&bad).is_err());
        }
        let mut bad = wire.clone();
        bad.push(0);
        assert!(PreparedWindingPage::decode(&bad).is_err());
        let mut bad = wire.clone();
        bad[64..72].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(PreparedWindingPage::decode(&bad).is_err());
        let mut bad = wire.clone();
        bad[8..12].copy_from_slice(&8_u32.to_le_bytes());
        assert!(PreparedWindingPage::decode(&bad).is_err());
        for i in 0..wire.len() {
            let mut bad = wire.clone();
            bad[i] ^= 255;
            assert!(std::panic::catch_unwind(|| PreparedWindingPage::decode(&bad)).is_ok());
        }
    }

    #[test]
    fn clipped_scene_roundtrip_and_rejection_preserve_paint_order() {
        let red = rectangle(8., 8., 48., 48.);
        let blue = rectangle(20., 20., 56., 56.);
        let mut page = PreparedWindingPage::new(64, 64).unwrap();
        page.push_clipped(&red, [255, 0, 0], [0, 0], [0, 0, 36, 64])
            .unwrap();
        page.push_clipped(&blue, [0, 0, 255], [0, 0], [24, 0, 64, 64])
            .unwrap();
        let rgba = page.render().unwrap().rgba;
        assert_eq!(
            &rgba[(28 * 64 + 28) * 4..(28 * 64 + 28) * 4 + 4],
            &[0, 0, 255, 255]
        );
        assert_eq!(
            &rgba[(28 * 64 + 16) * 4..(28 * 64 + 16) * 4 + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &rgba[(28 * 64 + 52) * 4..(28 * 64 + 52) * 4 + 4],
            &[0, 0, 255, 255]
        );
        let wire = page.encode().unwrap();
        assert_eq!(u32::from_le_bytes(wire[4..8].try_into().unwrap()), 2);
        assert_eq!(
            PreparedWindingPage::decode(&wire)
                .unwrap()
                .render()
                .unwrap()
                .rgba,
            rgba
        );
        for (offset, value) in [(36, 37_u32), (40, 65), (44, 0), (48, 65)] {
            let mut bad = wire.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                PreparedWindingPage::decode(&bad).is_err(),
                "offset {offset}"
            );
        }
        assert!(page
            .push_clipped(&red, [0; 3], [0, 0], [0, 0, 65, 64])
            .is_err());
        assert!(page.render_sequential_reference().is_err());
    }

    #[test]
    fn separate_fractional_research_mask_is_bounded_and_ordered() {
        let mut page = PreparedWindingPage::new(64, 64).unwrap();
        page.push(&rectangle(0., 0., 64., 64.), [0, 0, 0], [0, 0])
            .unwrap();
        assert_eq!(
            page.render_fractional_research(&[None]).unwrap().rgba,
            page.render().unwrap().rgba
        );
        let clip = Some([10.25, 10.5, 50.75, 50.25]);
        let rgba = page.render_fractional_research(&[clip]).unwrap().rgba;
        assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
        assert_eq!(
            &rgba[(20 * 64 + 20) * 4..(20 * 64 + 20) * 4 + 4],
            &[0, 0, 0, 255]
        );
        assert!((0..255).contains(&rgba[(20 * 64 + 10) * 4]));
        assert!(page.render_fractional_research(&[]).is_err());
        for invalid in [
            [f64::NAN, 1., 3., 4.],
            [-1., 1., 3., 4.],
            [1., 1., 65., 4.],
            [3., 1., 1., 4.],
        ] {
            assert!(page.render_fractional_research(&[Some(invalid)]).is_err());
        }
    }

    #[test]
    fn fractional_v3_ipc_roundtrip_and_corruption_reject() {
        let mut page = PreparedWindingPage::new(64, 64).unwrap();
        let shape = rectangle(0., 0., 64., 64.);
        page.push_fractional_clipped(
            &shape,
            [10, 20, 30],
            [0, 0],
            [0, 0, 64, 64],
            Some([10.25, 10.5, 50.75, 50.25]),
        )
        .unwrap();
        page.push(&rectangle(20., 20., 24., 24.), [255, 0, 0], [0, 0])
            .unwrap();
        let wire = page.encode().unwrap();
        assert_eq!(u32::from_le_bytes(wire[4..8].try_into().unwrap()), 3);
        assert_eq!(
            PreparedWindingPage::decode(&wire)
                .unwrap()
                .render()
                .unwrap()
                .rgba,
            page.render().unwrap().rgba
        );
        for value in [f64::NAN, f64::INFINITY, -1., 65., 50.75] {
            let mut invalid = wire.clone();
            invalid[52..60].copy_from_slice(&value.to_le_bytes());
            assert!(PreparedWindingPage::decode(&invalid).is_err());
        }
        for length in [0, 1, 23, 24, 63, wire.len() - 1] {
            assert!(PreparedWindingPage::decode(&wire[..length]).is_err());
        }
        let mut invalid = wire.clone();
        invalid[4..8].copy_from_slice(&4_u32.to_le_bytes());
        assert!(PreparedWindingPage::decode(&invalid).is_err());
    }

    #[test]
    fn geometry_budget_is_enforced_before_count_limit() {
        let mut scene = b"OPNW".to_vec();
        for n in [1_u32, 256, 192, 1, 256] {
            scene.extend(n.to_le_bytes());
        }
        for i in 0..256 {
            let point = if i == 255 {
                [0., 0.]
            } else {
                [(i % 200) as f64, (i % 150) as f64]
            };
            for v in point {
                scene.extend(v.to_le_bytes());
            }
        }
        let mut page = PreparedWindingPage::new(256, 192).unwrap();
        while page.push(&scene, [0; 3], [0, 0]).is_ok() {}
        assert!(page.paint_count() > 0 && page.paint_count() < MAX_PAINTS);
        assert!(page.retained_bytes() <= MAX_SCENE_BYTES);
        let before = (page.paint_count(), page.retained_bytes());
        assert!(page.push(&scene, [0; 3], [0, 0]).is_err());
        assert_eq!((page.paint_count(), page.retained_bytes()), before);
    }
}
