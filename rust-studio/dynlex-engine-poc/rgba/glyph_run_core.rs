//! Experimental ordered OPGS text-run core: Rust validates, DynLex covers.
//! The native backend and standalone probe share this bounded decoder.
use std::error::Error;
#[path = "glyph_multi_validation.rs"]
mod legacy;

#[link(name = "opds_dynlex_glyph_strip", kind = "static")]
unsafe extern "C" {
    fn opds_glyph_strip_raster(
        scene: *const u8,
        scene_len: i32,
        output: *mut u8,
        output_len: i32,
    ) -> i32;
}

fn words_for_opm(bytes: &[u8]) -> Result<Vec<u64>, Box<dyn Error>> {
    let contours = legacy::read_scene(bytes)?;
    let mut quantized = Vec::with_capacity(contours.len());
    for contour in &contours {
        let mut path = Vec::with_capacity(contour.len());
        for &[x, y] in contour {
            if !(0.0..=256.0).contains(&x) || !(0.0..=192.0).contains(&y) {
                return Err("glyph point outside text tile".into());
            }
            path.push([(x as f32) as f64, (y as f32) as f64]);
        }
        quantized.push(path);
    }
    legacy::validate_topology(&quantized)?;
    let mut words = Vec::with_capacity((bytes.len() + 7) / 8);
    words.push(u64::from_le_bytes(*b"OPM2\x01\0\0\0"));
    words.push((192_u64 << 32) | 256);
    words.push(contours.len() as u64);
    for contour in contours {
        words.push(contour.len() as u64);
        for point in contour {
            for value in point {
                words.push(value.to_bits().to_le());
            }
        }
    }
    Ok(words)
}

fn read_scene(bytes: &[u8]) -> Result<Vec<(Vec<u64>, [u8; 3])>, Box<dyn Error>> {
    if bytes.len() < 20 || bytes.len() > 140_000 || &bytes[..4] != b"OPGS" {
        return Err("invalid OPGS size or magic".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = word(16) as usize;
    if word(4) != 1 || word(8) != 256 || word(12) != 192 || !(2..=32).contains(&count) {
        return Err("invalid OPGS version, dimensions or glyph count".into());
    }
    let mut at = 20;
    let mut scenes = Vec::with_capacity(count);
    for _ in 0..count {
        if at + 44 > bytes.len() {
            return Err("truncated OPGS glyph record".into());
        }
        let name = &bytes[at..at + 32];
        let name_len = name.iter().position(|&b| b == 0).unwrap_or(32);
        if name_len == 0
            || name_len > 31
            || !name[..name_len]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
            || name[name_len..].iter().any(|&b| b != 0)
        {
            return Err("invalid OPGS font resource name".into());
        }
        let cid = u16::from_le_bytes(bytes[at + 32..at + 34].try_into()?);
        let gid = u16::from_le_bytes(bytes[at + 34..at + 36].try_into()?);
        if cid != gid {
            return Err("unproven CID-to-GID mapping".into());
        }
        let color = [bytes[at + 36], bytes[at + 37], bytes[at + 38]];
        if bytes[at + 39] != 0 {
            return Err("unsupported OPGS glyph flags".into());
        }
        let length = word(at + 40) as usize;
        at += 44;
        if !(88..=4148).contains(&length) || at + length > bytes.len() {
            return Err("invalid OPGS nested OPM2 length".into());
        }
        scenes.push((words_for_opm(&bytes[at..at + length])?, color));
        at += length;
    }
    if at != bytes.len() {
        return Err("trailing OPGS bytes".into());
    }
    Ok(scenes)
}

fn render(scenes: &[(Vec<u64>, [u8; 3])]) -> Result<Vec<u8>, Box<dyn Error>> {
    // PDF page content is first composited inside an isolated transparency
    // group. The completed group is then composited over the white page.
    // Painting each glyph directly over white differs by one byte at some
    // colored overlapping antialiased edges.
    let mut layer = vec![0_u8; 256 * 192 * 4];
    let mut alpha = vec![0_u8; 256 * 192];
    for (words, color) in scenes {
        // SAFETY: complete OPM2/Rust topology validation happened before ABI,
        // and both aligned input and writable output outlive this call.
        let status = unsafe {
            opds_glyph_strip_raster(
                words.as_ptr().cast(),
                i32::try_from(words.len() * 8)?,
                alpha.as_mut_ptr(),
                i32::try_from(alpha.len())?,
            )
        };
        if status != 0 {
            return Err(format!("DynLex glyph strip status {status}").into());
        }
        for (pixel, coverage) in alpha.iter().enumerate() {
            // Opaque RGB ink through the DynLex glyph-coverage mask, in PDF
            // content order. This composition still belongs to the Rust probe.
            let offset = pixel * 4;
            for (channel, source) in layer[offset..offset + 3].iter_mut().zip(color) {
                let numerator = u16::from(*source) * u16::from(*coverage)
                    + u16::from(*channel) * u16::from(255 - *coverage);
                *channel = ((numerator + 255) >> 8) as u8;
            }
            let background_alpha = layer[offset + 3];
            let numerator = u16::from(*coverage) * 255
                + u16::from(background_alpha) * u16::from(255 - *coverage);
            layer[offset + 3] = ((numerator + 255) >> 8) as u8;
        }
    }
    let mut rgba = vec![255_u8; layer.len()];
    for (target, pixel) in rgba.chunks_exact_mut(4).zip(layer.chunks_exact(4)) {
        for channel in 0..3 {
            target[channel] = pixel[channel].saturating_add(255 - pixel[3]);
        }
    }
    Ok(rgba)
}

pub fn render_opgs(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    render(&read_scene(bytes)?)
}
