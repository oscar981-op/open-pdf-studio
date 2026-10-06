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

#[link(name = "opds_dynlex_glyph_compositor", kind = "static")]
unsafe extern "C" {
    fn opds_glyph_direct_compose(
        scene: *const u8,
        scene_len: i32,
        output: *mut u8,
        output_len: i32,
    ) -> i32;
}

const GLYPH_PIXELS: usize = 256 * 192;

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

pub fn validate_opm_outline(bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    words_for_opm(bytes).map(|_| ())
}

fn read_scene(bytes: &[u8]) -> Result<Vec<(Vec<u64>, [u8; 3])>, Box<dyn Error>> {
    if bytes.len() < 20 || bytes.len() > 140_000 || &bytes[..4] != b"OPGS" {
        return Err("invalid OPGS size or magic".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = word(16) as usize;
    if word(4) != 1 || word(8) != 256 || word(12) != 192 || !(1..=32).contains(&count) {
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

fn render_layer(scenes: &[(Vec<u64>, [u8; 3])]) -> Result<Vec<u8>, Box<dyn Error>> {
    // PDF page content is first composited inside an isolated transparency
    // group. The completed group is then composited over the white page.
    // Painting each glyph directly over white differs by one byte at some
    // colored overlapping antialiased edges.
    let mut layer = vec![0_u8; 256 * 192 * 4];
    let mut alpha = vec![0_u8; GLYPH_PIXELS];
    for (words, color) in scenes {
        raster_glyph(words, &mut alpha)?;
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
    Ok(layer)
}

fn raster_glyph(words: &[u64], alpha: &mut [u8]) -> Result<(), Box<dyn Error>> {
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
    Ok(())
}

/// Directly paint each validated glyph over an existing opaque page. This
/// follows the non-isolated FreeText appearance order: an isolated glyph
/// group produces different rounding on colored page content.
pub fn render_opgs_over_opaque(
    scene: &[u8],
    base: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
) -> Result<Vec<u8>, Box<dyn Error>> {
    render_opgs_over_page(scene, base, width, height, clip, [0, 0], false, false)
}

pub fn render_opgs_over_opaque_at_origin(
    scene: &[u8],
    base: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn Error>> {
    render_opgs_over_page(scene, base, width, height, clip, origin, false, true)
}

pub fn render_opgs_over_opaque_at_origin_owned(
    scene: &[u8],
    base: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn Error>> {
    render_opgs_over_page_owned(scene, base, width, height, clip, origin, false, true)
}

/// Experimental direct drawing into a premultiplied page-content layer.
/// The caller must compose the resulting layer over the viewer's paper.
pub fn render_opgs_over_premultiplied(
    scene: &[u8],
    base: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
) -> Result<Vec<u8>, Box<dyn Error>> {
    render_opgs_over_page(scene, base, width, height, clip, [0, 0], true, false)
}

pub fn render_opgs_over_premultiplied_at_origin(
    scene: &[u8],
    base: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn Error>> {
    render_opgs_over_page(scene, base, width, height, clip, origin, true, true)
}

pub fn render_opgs_over_premultiplied_at_origin_owned(
    scene: &[u8],
    base: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn Error>> {
    render_opgs_over_page_owned(scene, base, width, height, clip, origin, true, true)
}

fn render_opgs_over_page(
    scene: &[u8],
    base: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
    premultiplied: bool,
    tiled: bool,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let pixel_count = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > if tiled { 4096 } else { 256 }
        || height > 4096
        || pixel_count > 6_000_000
        || base.len() as u64 != pixel_count * 4
    {
        return Err("glyph page is outside bounded compositor route".into());
    }
    render_opgs_over_page_owned(
        scene,
        base.to_vec(),
        width,
        height,
        clip,
        origin,
        premultiplied,
        tiled,
    )
}

fn render_opgs_over_page_owned(
    scene: &[u8],
    output: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
    premultiplied: bool,
    tiled: bool,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let scenes = read_scene(scene)?;
    render_outlines_over_page_owned(
        scenes,
        output,
        width,
        height,
        clip,
        origin,
        premultiplied,
        tiled,
    )
}

/// Paint one validated geometric OPM2 outline without requiring a fake font
/// resource or CID. PDF path staging and paint order stay with the caller.
pub fn render_opm_over_premultiplied_at_origin_owned(
    outline: &[u8],
    color: [u8; 3],
    output: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn Error>> {
    let words = words_for_opm(outline)?;
    render_outlines_over_page_owned(
        vec![(words, color)],
        output,
        width,
        height,
        clip,
        origin,
        true,
        true,
    )
}

fn render_outlines_over_page_owned(
    scenes: Vec<(Vec<u64>, [u8; 3])>,
    mut output: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
    premultiplied: bool,
    tiled: bool,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let pixels = usize::try_from(width)?
        .checked_mul(usize::try_from(height)?)
        .ok_or("glyph page dimensions overflow")?;
    if width == 0
        || width > if tiled { 4096 } else { 256 }
        || height == 0
        || height > 4096
        || (width as u64) * (height as u64) > 6_000_000
        || output.len() != pixels.checked_mul(4).ok_or("glyph RGBA length overflow")?
        || output.chunks_exact(4).any(|pixel| {
            if premultiplied {
                pixel[..3].iter().any(|channel| *channel > pixel[3])
            } else {
                pixel[3] != 255
            }
        })
        || clip[0] >= clip[2]
        || clip[1] >= clip[3]
        || clip[2] > width
        || clip[3] > height
        || (tiled
            && (origin[0] >= width
                || origin[1] >= height
                || origin[0] >= clip[2]
                || origin[1] >= clip[3]
                || origin[0] + 256 <= clip[0]
                || origin[1] + 192 <= clip[1]))
        || (!tiled && (origin != [0, 0] || clip[2] > 256 || clip[3] > 192))
    {
        return Err("glyph page is outside bounded compositor route".into());
    }
    let mut alpha = vec![0_u8; GLYPH_PIXELS];
    let header_len = if tiled { 44 } else { 36 };
    let mut command = vec![0_u8; header_len + GLYPH_PIXELS];
    command[0..4].copy_from_slice(b"OPGC");
    command[4..8].copy_from_slice(&(if tiled { 2_u32 } else { 1_u32 }).to_le_bytes());
    command[8..12].copy_from_slice(&width.to_le_bytes());
    command[12..16].copy_from_slice(&height.to_le_bytes());
    command[35] = u8::from(premultiplied);
    if tiled {
        command[36..40].copy_from_slice(&origin[0].to_le_bytes());
        command[40..44].copy_from_slice(&origin[1].to_le_bytes());
    }
    for (words, color) in scenes {
        raster_glyph(&words, &mut alpha)?;
        let mut bounds = [u32::MAX, u32::MAX, 0, 0];
        for (index, &coverage) in alpha.iter().enumerate() {
            if coverage == 0 {
                continue;
            }
            let x = origin[0] + (index % 256) as u32;
            let y = origin[1] + (index / 256) as u32;
            if x < clip[0] || x >= clip[2] || y < clip[1] || y >= clip[3] {
                return Err("glyph antialias coverage extends beyond appearance clip".into());
            }
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x + 1);
            bounds[3] = bounds[3].max(y + 1);
        }
        if bounds[2] == 0 {
            continue;
        }
        for (index, value) in bounds.into_iter().enumerate() {
            command[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        command[32..35].copy_from_slice(&color);
        command[header_len..].copy_from_slice(&alpha);
        // SAFETY: Rust validated the full OPGS/OPM2 input, constructed the
        // fixed OPGC command, checked bounds and allocated the output. The
        // LLVM ABI wrapper rechecks lengths and header before DynLex enters.
        let status = unsafe {
            opds_glyph_direct_compose(
                command.as_ptr(),
                i32::try_from(command.len())?,
                output.as_mut_ptr(),
                i32::try_from(output.len())?,
            )
        };
        if status != 0 {
            return Err(format!("DynLex glyph compositor status {status}").into());
        }
    }
    Ok(output)
}

fn over_white(layer: &[u8]) -> Vec<u8> {
    let mut rgba = vec![255_u8; layer.len()];
    for (target, pixel) in rgba.chunks_exact_mut(4).zip(layer.chunks_exact(4)) {
        for channel in 0..3 {
            target[channel] = pixel[channel].saturating_add(255 - pixel[3]);
        }
    }
    rgba
}

pub fn render_opgs(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(over_white(&render_layer(&read_scene(bytes)?)?))
}

/// Premultiplied RGBA glyph group. A page compositor may place this group
/// over a validated opaque page after the PDF blending rule is proved.
pub fn render_opgs_layer(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    render_layer(&read_scene(bytes)?)
}
