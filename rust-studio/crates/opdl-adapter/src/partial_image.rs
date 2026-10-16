//! Bounded single axis-aligned DeviceRGB8 JPEG paint over opaque white paper.
//! PDF resource/content validation is the caller's responsibility. This is a
//! proven geometry subset, not a general image interpreter or tiled fallback.
use pic_scale::{ImageSize, ImageStore, ImageStoreMut, ResamplingFunction, Scaler};
use std::error::Error;
use zune_jpeg::{
    zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
    JpegDecoder,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct OpaqueImagePage {
    pub width: u32,
    pub height: u32,
    pub intermediate: [u32; 2],
    pub rgba: Vec<u8>,
}

/// The only accepted geometry is the original zero-origin MediaBox with no
/// rotation or CropBox. The source must be a single, validated JPEG resource.
pub fn render(
    jpeg: &[u8],
    source: [u32; 2],
    media: [f64; 4],
    rect: [f64; 4],
    matrix: [f64; 6],
    scale: f32,
) -> Result<OpaqueImagePage> {
    render_impl(Some(jpeg), None, source, media, rect, matrix, scale)
}

/// Research-only path for measuring decoded-source reuse. Its caller must
/// prove that `pixels` came from the *same* validated JPEG and source-bound
/// PDF; native dispatch does not use this entry point.
pub fn render_predecoded_research(
    pixels: &[u8],
    source: [u32; 2],
    media: [f64; 4],
    rect: [f64; 4],
    matrix: [f64; 6],
    scale: f32,
) -> Result<OpaqueImagePage> {
    render_impl(None, Some(pixels), source, media, rect, matrix, scale)
}

/// Decode a bounded RGB8 source once for research into cross-scale reuse.
/// The result must stay bound to its exact source JPEG before any native use.
pub fn decode_rgb_research(jpeg: &[u8], source: [u32; 2]) -> Result<Vec<u8>> {
    if jpeg.len() > 16_000_000
        || source.contains(&0)
        || source.iter().any(|&n| n > 8192)
        || source[0]
            .checked_mul(source[1])
            .is_none_or(|n| n > 40_000_000)
        || !jpeg.starts_with(&[0xff, 0xd8])
        || !jpeg.ends_with(&[0xff, 0xd9])
    {
        return Err("JPEG source bound or incomplete JPEG".into());
    }
    let options = DecoderOptions::default()
        .set_max_width(8192)
        .set_max_height(8192)
        .jpeg_set_max_scans(16)
        .set_strict_mode(true)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(jpeg), options);
    let phase_start = std::time::Instant::now();
    decoder.decode_headers()?;
    let info = decoder.info().ok_or("missing JPEG info")?;
    if [u32::from(info.width), u32::from(info.height)] != source
        || info.components != 3
        || !matches!(
            decoder.input_colorspace(),
            Some(ColorSpace::RGB | ColorSpace::YCbCr)
        )
    {
        return Err("JPEG dictionary/header disagreement".into());
    }
    let pixels = decoder.decode()?;
    if std::env::var_os("OPDS_TEXT_PHASES").is_some() {
        eprintln!(
            "image phase jpeg_decode: {:.3} ms",
            phase_start.elapsed().as_secs_f64() * 1000.
        );
    }
    if pixels.len() != source[0] as usize * source[1] as usize * 3 {
        return Err("JPEG pixel length".into());
    }
    Ok(pixels)
}

fn render_impl(
    jpeg: Option<&[u8]>,
    cached_pixels: Option<&[u8]>,
    source: [u32; 2],
    media: [f64; 4],
    rect: [f64; 4],
    matrix: [f64; 6],
    scale: f32,
) -> Result<OpaqueImagePage> {
    // Local phase diagnosis only; the native worker runs with an empty env.
    let phase_profile = std::env::var_os("OPDS_TEXT_PHASES").is_some();
    if source.contains(&0)
        || source.iter().any(|&n| n > 8192)
        || source[0]
            .checked_mul(source[1])
            .is_none_or(|n| n > 40_000_000)
        || !scale.is_finite()
        || !(0.01..=4.).contains(&scale)
        || media
            .iter()
            .chain(rect.iter())
            .chain(matrix.iter())
            .any(|v| !v.is_finite() || v.abs() > 16_384.)
        || media[0] != 0.
        || media[1] != 0.
        || media[2] <= 0.
        || media[3] <= 0.
        || rect[2] <= 0.
        || rect[3] <= 0.
        || matrix[0] <= 0.
        || matrix[3] <= 0.
        || matrix[1] != 0.
        || matrix[2] != 0.
    {
        return Err("unsupported partial image geometry or source".into());
    }
    let width = ((media[2] as f32) * scale).floor() as u32;
    let height = ((media[3] as f32) * scale).floor() as u32;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || width.checked_mul(height).is_none_or(|n| n > 6_000_000)
    {
        return Err("partial image output pixel bound".into());
    }
    let mut intermediate = [0; 2];
    for axis in 0..2 {
        let advance = ((matrix[axis * 3] / f64::from(source[axis])) * f64::from(scale)) as f32;
        if !(0. ..1.).contains(&advance) {
            return Err("partial image requires downsampling on both axes".into());
        }
        intermediate[axis] = (source[axis] as f32 * advance).ceil() as u32;
    }
    if intermediate.contains(&0)
        || intermediate.iter().any(|&n| n > 4096)
        || intermediate[0]
            .checked_mul(intermediate[1])
            .is_none_or(|n| n > 6_000_000)
    {
        return Err("partial image intermediate pixel bound".into());
    }
    let decoded = jpeg
        .map(|bytes| decode_rgb_research(bytes, source))
        .transpose()?;
    let pixels = cached_pixels
        .or(decoded.as_deref())
        .ok_or("missing RGB source")?;
    if pixels.len() != source[0] as usize * source[1] as usize * 3 {
        return Err("JPEG pixel length".into());
    }
    let mut sampled = vec![0; intermediate[0] as usize * intermediate[1] as usize * 3];
    let from = ImageStore::<u8, 3>::from_slice(&pixels, source[0] as usize, source[1] as usize)?;
    let mut to = ImageStoreMut::<u8, 3>::from_slice(
        &mut sampled,
        intermediate[0] as usize,
        intermediate[1] as usize,
    )?;
    let plan = Scaler::new(ResamplingFunction::CatmullRom).plan_rgb_resampling(
        ImageSize::new(source[0] as usize, source[1] as usize),
        ImageSize::new(intermediate[0] as usize, intermediate[1] as usize),
    )?;
    let resample_start = std::time::Instant::now();
    plan.resample(&from, &mut to)?;
    if phase_profile {
        eprintln!(
            "image phase rgb_resample: {:.3} ms",
            resample_start.elapsed().as_secs_f64() * 1000.
        );
    }
    let compose_start = std::time::Instant::now();
    let mut rgba = vec![255; width as usize * height as usize * 4];
    let clip = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]]
        .map(|edge| f64::from((edge * f64::from(scale)) as f32) / f64::from(scale));
    let image_pixels = [
        (matrix[4] * f64::from(scale)).floor(),
        ((media[3] - matrix[5] - matrix[3]) * f64::from(scale)).floor(),
        ((matrix[4] + matrix[0]) * f64::from(scale)).ceil(),
        ((media[3] - matrix[5]) * f64::from(scale)).ceil(),
    ];
    let half = 0.5 / f64::from(scale);
    let ax = f64::from(intermediate[0]) / (matrix[0] * f64::from(scale));
    let ay = f64::from(intermediate[1]) / (matrix[3] * f64::from(scale));
    let cover =
        |p: f64, a: f64, b: f64| ((p + half).min(b) - (p - half).max(a)).max(0.) * f64::from(scale);
    // Horizontal sampling and clip coverage do not depend on the row. Retain
    // the same f32 accumulation/reset at every 256-pixel strip boundary.
    let mut columns = Vec::with_capacity(width as usize);
    let mut strip_x = 0_f32;
    for x in 0..width {
        let px = (f64::from(x) + 0.5) / f64::from(scale);
        let xcover = cover(px, clip[0], clip[2]);
        let ix = (px - matrix[4]) * f64::from(intermediate[0]) / matrix[0];
        if x % 256 == 0 {
            strip_x = ix as f32;
        }
        let sample_x = strip_x;
        strip_x += ax as f32;
        let sx = (sample_x.floor().max(0.) as u32).min(intermediate[0] - 1) as usize;
        let inside = f64::from(x) >= image_pixels[0] && f64::from(x) < image_pixels[2];
        columns.push((xcover, sx, inside));
    }
    for y in 0..height {
        let py = media[3] - (f64::from(y) + 0.5) / f64::from(scale);
        let ycover = cover(py, clip[1], clip[3]);
        if ycover == 0. || f64::from(y) < image_pixels[1] || f64::from(y) >= image_pixels[3] {
            continue;
        }
        let iy = (matrix[5] + matrix[3] - py) * f64::from(intermediate[1]) / matrix[3];
        let lane = (y % 4) as f64;
        let base_y = (iy - lane * ay) as f32;
        let sample_y = (lane as f32).mul_add(ay as f32, base_y);
        let sy = (sample_y.floor() as u32).min(intermediate[1] - 1) as usize;
        for (x, &(xcover, sx, inside)) in columns.iter().enumerate() {
            if !inside {
                continue;
            }
            let alpha = (xcover * ycover).clamp(0., 1.);
            if alpha == 0. {
                continue;
            }
            let from = (sy * intermediate[0] as usize + sx) * 3;
            let to = (y as usize * width as usize + x) * 4;
            let mask = (alpha as f32).mul_add(255.0, 0.5) as u16;
            let div255 = |value: u16| (value + 255) >> 8;
            let background = div255(255 * (255 - mask));
            for c in 0..3 {
                rgba[to + c] =
                    (div255(u16::from(sampled[from + c]) * mask) + background).min(255) as u8;
            }
        }
    }
    if phase_profile {
        eprintln!(
            "image phase rgba_compose: {:.3} ms",
            compose_start.elapsed().as_secs_f64() * 1000.
        );
    }
    Ok(OpaqueImagePage {
        width,
        height,
        intermediate,
        rgba,
    })
}
