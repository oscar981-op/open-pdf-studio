//! First bounded DynLex paint path for native annotations over an opaque page.
//! Editing gestures and unsupported annotation kinds remain with the host;
//! selected pixel-aligned rectangles can also paint their controls here.
use crate::Bitmap;
use opds_engine_probe::Raster;
use opds_native_core::annotations::Annotation;
#[cfg(feature = "kernel")]
use opds_native_core::annotations::{Kind, Rect};

#[cfg(feature = "kernel")]
fn whole(value: f64, limit: u32) -> Option<u32> {
    (value.is_finite() && value >= 0.0 && value <= f64::from(limit) && value.fract() == 0.0)
        .then(|| value as u32)
}

#[cfg(feature = "kernel")]
fn pixel_rect(bounds: Rect, width: u32, height: u32) -> Option<[u32; 4]> {
    let x0 = whole(bounds.x, width)?;
    let y0 = whole(bounds.y, height)?;
    let x1 = whole(bounds.x + bounds.width, width)?;
    let y1 = whole(bounds.y + bounds.height, height)?;
    (x1 >= x0 + 4 && y1 >= y0 + 4).then_some([x0, y0, x1, y1])
}

#[cfg(feature = "kernel")]
fn command(scene: &mut Vec<u8>, kind: u32, rect: [u32; 4], tail: [u8; 4]) {
    for value in [kind, rect[0], rect[1], rect[2], rect[3]] {
        scene.extend_from_slice(&value.to_le_bytes());
    }
    scene.extend_from_slice(&tail);
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
pub(crate) struct ImportedCommand {
    pub kind: u32,
    pub rect: [u32; 4],
    pub tail: [u8; 4],
}

#[cfg(feature = "kernel")]
pub(crate) fn native_imported_commands(
    mark: &Annotation,
    width: u32,
    height: u32,
) -> Option<Vec<ImportedCommand>> {
    let Kind::Rectangle { color_rgb } = mark.kind else {
        return None;
    };
    let bounds = pixel_rect(mark.bounds, width, height)?;
    Some(native_commands_for_rect(bounds, color_rgb))
}

#[cfg(feature = "kernel")]
fn native_commands_for_rect(
    [x0, y0, x1, y1]: [u32; 4],
    color_rgb: [u8; 3],
) -> Vec<ImportedCommand> {
    let mut result = Vec::with_capacity(9);
    if color_rgb == [255, 226, 67] {
        // The native Highlight's partially covered Vello start tile follows
        // a different rounded blend from its fully covered interior.
        let left_full = (x0 / 4 + 1) * 4;
        let right_full = (x1 / 4) * 4;
        let top_full = y0.div_ceil(4) * 4;
        let bottom_full = (y1 / 4) * 4;
        let mut xs = [x0, left_full.min(x1), right_full.max(x0), x1];
        let mut ys = [y0, top_full.min(y1), bottom_full.max(y0), y1];
        xs.sort_unstable();
        ys.sort_unstable();
        for [y_start, y_end] in ys.windows(2).map(|window| [window[0], window[1]]) {
            for [x_start, x_end] in xs.windows(2).map(|window| [window[0], window[1]]) {
                if x_start == x_end || y_start == y_end {
                    continue;
                }
                let masked = x_start < left_full
                    || x_start >= right_full
                    || y_start < top_full
                    || y_start >= bottom_full;
                result.push(ImportedCommand {
                    kind: if masked { 3 } else { 4 },
                    rect: [x_start, y_start, x_end, y_end],
                    tail: [89, 79, 23, 89],
                });
            }
        }
    } else {
        result.push(ImportedCommand {
            kind: 2,
            rect: [x0, y0, x1, y1],
            tail: [color_rgb[0], color_rgb[1], color_rgb[2], 255],
        });
    }
    result
}

/// Paint a fully staged OPAN list in one DynLex call. Both imported `/AP`
/// forms and preview-only marks use this validated boundary.
#[cfg(feature = "kernel")]
pub(crate) fn paint_imported_commands(
    layer_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    commands: &[ImportedCommand],
) -> Result<Option<Bitmap>, String> {
    if layer_rgba
        .chunks_exact(4)
        .any(|pixel| pixel[..3].iter().any(|channel| *channel > pixel[3]))
    {
        return Ok(None);
    }
    paint_imported_commands_owned(layer_rgba.to_vec(), width, height, page_points, commands)
}

/// Reuse the caller's validated premultiplied page allocation across ordered
/// shape/text batches. The imported-Form caller starts with Hayro's page and
/// every subsequent DynLex operation preserves premultiplication.
#[cfg(feature = "kernel")]
pub(crate) fn paint_imported_commands_owned(
    mut output: Vec<u8>,
    width: u32,
    height: u32,
    page_points: (f32, f32),
    commands: &[ImportedCommand],
) -> Result<Option<Bitmap>, String> {
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 6_000_000
        || page_points != (width as f32, height as f32)
        || output.len() != width as usize * height as usize * 4
        || commands.is_empty()
    {
        return Ok(None);
    }
    let mut union = [u32::MAX, u32::MAX, 0, 0];
    for entry in commands {
        union[0] = union[0].min(entry.rect[0]);
        union[1] = union[1].min(entry.rect[1]);
        union[2] = union[2].max(entry.rect[2]);
        union[3] = union[3].max(entry.rect[3]);
    }
    if union[0] >= union[2] || union[1] >= union[3] {
        return Ok(None);
    }
    // Keep OPAN's 4x4 coverage grid at the same global origin. The narrow
    // tile avoids repeatedly scanning and painting an entire A1 page for a
    // small annotation between other ordered appearance layers.
    let bounds = [
        union[0] / 4 * 4,
        union[1] / 4 * 4,
        union[2].div_ceil(4).saturating_mul(4).min(width),
        union[3].div_ceil(4).saturating_mul(4).min(height),
    ];
    let tile_pixels = u64::from(bounds[2].saturating_sub(bounds[0]))
        * u64::from(bounds[3].saturating_sub(bounds[1]));
    if tile_pixels == 0 || tile_pixels > 700_000 {
        return paint_commands_owned(output, width, height, page_points, commands, PaintMode::Pdf);
    }
    let tile_width = (bounds[2] - bounds[0]) as usize;
    let tile_height = (bounds[3] - bounds[1]) as usize;
    let mut tile = Vec::with_capacity(tile_width * tile_height * 4);
    for y in bounds[1]..bounds[3] {
        let start = (y as usize * width as usize + bounds[0] as usize) * 4;
        tile.extend_from_slice(&output[start..start + tile_width * 4]);
    }
    let translated: Vec<_> = commands
        .iter()
        .map(|entry| ImportedCommand {
            kind: entry.kind,
            rect: [
                entry.rect[0].saturating_sub(bounds[0]),
                entry.rect[1].saturating_sub(bounds[1]),
                entry.rect[2].saturating_sub(bounds[0]),
                entry.rect[3].saturating_sub(bounds[1]),
            ],
            tail: entry.tail,
        })
        .collect();
    let Some(painted) = paint_commands_owned(
        tile,
        tile_width as u32,
        tile_height as u32,
        (tile_width as f32, tile_height as f32),
        &translated,
        PaintMode::Pdf,
    )?
    else {
        return Ok(None);
    };
    for (tile_y, y) in (bounds[1]..bounds[3]).enumerate() {
        let destination = (y as usize * width as usize + bounds[0] as usize) * 4;
        let source = tile_y * tile_width * 4;
        output[destination..destination + tile_width * 4]
            .copy_from_slice(&painted.rgba[source..source + tile_width * 4]);
    }
    Ok(Some(Bitmap {
        width,
        height,
        rgba: output,
    }))
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
enum PaintMode {
    Pdf,
    Selection(&'static [u8; 768]),
    UiPreview(&'static [u8; 768]),
}

#[cfg(feature = "kernel")]
fn paint_commands_owned(
    mut output: Vec<u8>,
    width: u32,
    height: u32,
    page_points: (f32, f32),
    commands: &[ImportedCommand],
    mode: PaintMode,
) -> Result<Option<Bitmap>, String> {
    let opaque_preview = !matches!(mode, PaintMode::Pdf);
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 6_000_000
        || page_points != (width as f32, height as f32)
        || output.len() != width as usize * height as usize * 4
        || output
            .chunks_exact(4)
            .any(|pixel| pixel[..3].iter().any(|channel| *channel > pixel[3]))
        || (opaque_preview && output.chunks_exact(4).any(|pixel| pixel[3] != 255))
        || commands.is_empty()
        || commands.len() > 576
        || (matches!(mode, PaintMode::Selection(_))
            && (commands[0].kind != 5 || commands.iter().skip(1).any(|entry| entry.kind != 1)))
        || commands.iter().any(|entry| {
            let [x0, y0, x1, y1] = entry.rect;
            !(match mode {
                PaintMode::Pdf => matches!(entry.kind, 1..=4),
                PaintMode::Selection(_) => matches!(entry.kind, 1 | 5),
                PaintMode::UiPreview(_) => matches!(entry.kind, 2 | 6),
            }) || x0 >= x1
                || y0 >= y1
                || x1 > width
                || y1 > height
                || (matches!(entry.kind, 1 | 2) && entry.tail[3] != 255)
                || (matches!(entry.kind, 3 | 4) && entry.tail != [89, 79, 23, 89])
                || (matches!(mode, PaintMode::Selection(_))
                    && if entry.kind == 5 {
                        entry.tail != [31, 136, 255, 191]
                    } else {
                        entry.tail != [31, 136, 255, 255]
                    })
                || (entry.kind == 6 && entry.tail != [255, 226, 67, 89])
        })
    {
        return Ok(None);
    }
    let mut scene =
        Vec::with_capacity(20 + commands.len() * 24 + if opaque_preview { 768 } else { 0 });
    scene.extend_from_slice(b"OPAN");
    let version = match mode {
        PaintMode::Pdf => 2,
        PaintMode::Selection(_) => 3,
        PaintMode::UiPreview(_) => 4,
    };
    for value in [version, width, height, commands.len() as u32] {
        scene.extend_from_slice(&value.to_le_bytes());
    }
    for entry in commands {
        command(&mut scene, entry.kind, entry.rect, entry.tail);
    }
    if let PaintMode::Selection(lut) | PaintMode::UiPreview(lut) = mode {
        scene.extend_from_slice(lut);
    }
    // SAFETY: Rust has validated the complete scene and RGBA buffer before
    // the narrow ABI. The wrapper also checks lengths and page dimensions.
    let status = unsafe {
        super::opds_paint_annotations(
            scene.as_ptr(),
            i32::try_from(scene.len()).map_err(|_| "annotation scene too large")?,
            output.as_mut_ptr(),
            i32::try_from(output.len()).map_err(|_| "annotation bitmap too large")?,
        )
    };
    if status != 0 {
        return Err(format!("DynLex annotation kernel status {status}"));
    }
    if output
        .chunks_exact(4)
        .any(|pixel| pixel[..3].iter().any(|channel| *channel > pixel[3]))
    {
        return Err("DynLex imported annotation output is not premultiplied RGBA".into());
    }
    Ok(Some(Bitmap {
        width,
        height,
        rgba: output,
    }))
}

/// Opaque pixel rectangles for imported annotations without `/AP`. The
/// caller stages these in `/Annots` order from a single validated page.
/// This reuses OPAN's fill command rather than emulating annotation pixels
/// in Rust. Unknown geometry must be rejected before calling this function.
#[cfg(feature = "kernel")]
pub(crate) fn paint_opaque_imported_rectangles(
    layer_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    rectangles: &[([u32; 4], [u8; 3])],
) -> Result<Option<Bitmap>, String> {
    if rectangles.is_empty() || rectangles.len() > 256 {
        return Ok(None);
    }
    let commands: Vec<_> = rectangles
        .iter()
        .map(|&(rect, color)| ImportedCommand {
            kind: 1,
            rect,
            tail: [color[0], color[1], color[2], 255],
        })
        .collect();
    paint_imported_commands(layer_rgba, width, height, page_points, &commands)
}

#[cfg(feature = "kernel")]
fn linear_blend_lut(ink: [u8; 3], coverage: f64) -> [u8; 768] {
    let to_linear = |byte: u8| {
        let value = f64::from(byte) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let to_srgb = |linear: f64| {
        let value = if linear <= 0.0031308 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut lut = [0_u8; 768];
    for (channel, ink) in ink.into_iter().enumerate() {
        for source in 0_u16..=255 {
            let mixed = to_linear(source as u8) * (1.0 - coverage) + to_linear(ink) * coverage;
            lut[channel * 256 + source as usize] = to_srgb(mixed);
        }
    }
    lut
}

#[cfg(feature = "kernel")]
fn selection_linear_lut() -> &'static [u8; 768] {
    static LUT: std::sync::OnceLock<[u8; 768]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| linear_blend_lut([31, 136, 255], 191.0 / 255.0))
}

#[cfg(feature = "kernel")]
fn highlight_linear_lut() -> &'static [u8; 768] {
    static LUT: std::sync::OnceLock<[u8; 768]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| linear_blend_lut([255, 226, 67], 0.35))
}

/// UI-only Rectangle/Highlight paint route. Its centered red stroke and
/// linear-light yellow fill match Iced; PDF `/AP` staging remains separate.
pub fn render_native_ui_preview_pixels(
    base_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    #[cfg(not(feature = "kernel"))]
    {
        let _ = (base_rgba, width, height, page_points, annotations, page);
        Ok(None)
    }
    #[cfg(feature = "kernel")]
    {
        let marks: Vec<_> = annotations
            .iter()
            .filter(|mark| mark.page == page)
            .collect();
        if marks.is_empty() || marks.len() > 64 {
            return Ok(None);
        }
        let mut commands = Vec::with_capacity(marks.len());
        for mark in marks {
            let Kind::Rectangle { color_rgb } = &mark.kind else {
                return Ok(None);
            };
            let Some([x0, y0, x1, y1]) = pixel_rect(mark.bounds, width, height) else {
                return Ok(None);
            };
            if *color_rgb == [255, 226, 67] {
                commands.push(ImportedCommand {
                    kind: 6,
                    rect: [x0, y0, x1, y1],
                    tail: [255, 226, 67, 89],
                });
            } else {
                if x0 == 0 || y0 == 0 || x1 >= width || y1 >= height {
                    return Ok(None);
                }
                commands.push(ImportedCommand {
                    kind: 2,
                    rect: [x0 - 1, y0 - 1, x1 + 1, y1 + 1],
                    tail: [color_rgb[0], color_rgb[1], color_rgb[2], 255],
                });
            }
        }
        paint_commands_owned(
            base_rgba.to_vec(),
            width,
            height,
            page_points,
            &commands,
            PaintMode::UiPreview(highlight_linear_lut()),
        )
    }
}

/// Draw the selected native annotation's frame and eight resize grips in the
/// DynLex layer. Coordinates are deliberately restricted to whole pixels;
/// the Iced overlay remains the fallback for zoomed or fractional geometry.
pub fn render_native_selection_overlay_pixels(
    base_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    bounds: opds_native_core::annotations::Rect,
) -> Result<Option<Bitmap>, String> {
    #[cfg(not(feature = "kernel"))]
    {
        let _ = (base_rgba, width, height, page_points, bounds);
        Ok(None)
    }
    #[cfg(feature = "kernel")]
    {
        let Some([x0, y0, x1, y1]) = pixel_rect(bounds, width, height) else {
            return Ok(None);
        };
        // The handles are six pixels wide around every corner and midpoint.
        // Require an even span so every grip has an integral six-pixel box.
        if (x1 - x0) % 2 != 0
            || (y1 - y0) % 2 != 0
            || x0 == 0
            || y0 == 0
            || x1 >= width
            || y1 >= height
        {
            return Ok(None);
        }
        let blue = [31, 136, 255, 255];
        let mut commands = Vec::with_capacity(9);
        commands.push(ImportedCommand {
            // Iced centers its 1.5 px stroke on the annotation boundary:
            // one 75%-covered pixel on either side of each integer edge.
            kind: 5,
            rect: [x0 - 1, y0 - 1, x1 + 1, y1 + 1],
            tail: [31, 136, 255, 191],
        });
        for (cx, cy) in [
            (x0, y0),
            ((x0 + x1) / 2, y0),
            (x1, y0),
            (x1, (y0 + y1) / 2),
            (x1, y1),
            ((x0 + x1) / 2, y1),
            (x0, y1),
            (x0, (y0 + y1) / 2),
        ] {
            let left = cx.saturating_sub(3);
            let top = cy.saturating_sub(3);
            let right = cx.saturating_add(3).min(width);
            let bottom = cy.saturating_add(3).min(height);
            if left < right && top < bottom {
                commands.push(ImportedCommand {
                    kind: 1,
                    rect: [left, top, right, bottom],
                    tail: blue,
                });
            }
        }
        paint_commands_owned(
            base_rgba.to_vec(),
            width,
            height,
            page_points,
            &commands,
            PaintMode::Selection(selection_linear_lut()),
        )
    }
}

/// Return `None` rather than partially painting a document when any native
/// annotation is outside the verified 1px/page-point Square subset.
pub fn render_native_annotation_overlay(
    base: &Raster,
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels(
        &base.rgba,
        base.width,
        base.height,
        (base.page_width_pt, base.page_height_pt),
        annotations,
        page,
    )
}

/// The optional transparent PDF-content layer proves that a colored source
/// under a Highlight is fully opaque before the annotation is composited.
/// The caller must bind this layer to the same rendered page and scale.
pub fn render_native_annotation_overlay_pixels_with_content(
    base_rgba: &[u8],
    content_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels_inner(
        base_rgba,
        base_rgba,
        Some(content_rgba),
        false,
        false,
        width,
        height,
        page_points,
        annotations,
        page,
    )
}

/// Paint later annotations on a page that already contains validated native
/// marks. `source_rgba` and `content_rgba` describe the original PDF page;
/// every Highlight pixel must be fully opaque there. The caller must derive
/// `composited_rgba` only by ordered DynLex annotation operations from that
/// same source page and bind all three buffers to one render ticket.
pub fn render_native_annotation_overlay_pixels_on_opaque_source(
    composited_rgba: &[u8],
    source_rgba: &[u8],
    content_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels_inner(
        composited_rgba,
        source_rgba,
        Some(content_rgba),
        true,
        false,
        width,
        height,
        page_points,
        annotations,
        page,
    )
}

/// Borrow the page pixels. DynLex paints the bounded marks on one cloned RGBA
/// output, without materializing a second full-page OPDL RGB payload.
pub fn render_native_annotation_overlay_pixels(
    base_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels_inner(
        base_rgba,
        base_rgba,
        None,
        false,
        false,
        width,
        height,
        page_points,
        annotations,
        page,
    )
}

/// Paint validated native shapes into an existing premultiplied PDF page
/// content layer. The caller must prove that this layer belongs to the same
/// page and add the viewer's white paper only after all marks are composited.
pub fn render_native_annotation_overlay_pixels_over_premultiplied(
    layer_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels_inner(
        layer_rgba,
        layer_rgba,
        None,
        false,
        true,
        width,
        height,
        page_points,
        annotations,
        page,
    )
}

fn render_native_annotation_overlay_pixels_inner(
    base_rgba: &[u8],
    source_rgba: &[u8],
    content_rgba: Option<&[u8]>,
    require_opaque_source: bool,
    premultiplied: bool,
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    #[cfg(not(feature = "kernel"))]
    {
        let _ = (
            base_rgba,
            source_rgba,
            content_rgba,
            require_opaque_source,
            premultiplied,
            width,
            height,
            page_points,
            annotations,
            page,
        );
        return Ok(None);
    }
    #[cfg(feature = "kernel")]
    {
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 6_000_000
            || page_points.0 != width as f32
            || page_points.1 != height as f32
            || base_rgba.len() != (width as usize) * (height as usize) * 4
            || source_rgba.len() != base_rgba.len()
            || base_rgba.chunks_exact(4).any(|pixel| {
                if premultiplied {
                    pixel[..3].iter().any(|channel| *channel > pixel[3])
                } else {
                    pixel[3] != 255
                }
            })
            || (!premultiplied && source_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255))
        {
            return Ok(None);
        }
        let marks: Vec<_> = annotations
            .iter()
            .filter(|item| item.page == page)
            .collect();
        if marks.is_empty() || marks.len() > 64 {
            return Ok(None);
        }
        let mut rectangles = Vec::with_capacity(marks.len());
        for mark in marks {
            let Kind::Rectangle { color_rgb } = &mark.kind else {
                return Ok(None);
            };
            let Some(bounds) = pixel_rect(mark.bounds, width, height) else {
                return Ok(None);
            };
            if *color_rgb == [255, 226, 67] && !premultiplied {
                let mut colored_underlay = require_opaque_source;
                for y in bounds[1]..bounds[3] {
                    for x in bounds[0]..bounds[2] {
                        let at = ((y * width + x) as usize) * 4;
                        if source_rgba[at..at + 3] != [255, 255, 255] {
                            colored_underlay = true;
                            break;
                        }
                    }
                    if colored_underlay {
                        break;
                    }
                }
                if colored_underlay {
                    let Some(content) = content_rgba.filter(|layer| layer.len() == base_rgba.len())
                    else {
                        return Ok(None);
                    };
                    for y in bounds[1]..bounds[3] {
                        for x in bounds[0]..bounds[2] {
                            let at = ((y * width + x) as usize) * 4;
                            if content[at + 3] != 255
                                || content[at..at + 3] != source_rgba[at..at + 3]
                            {
                                // Compositing over a translucent PDF source
                                // group can differ by thousands of 8-bit
                                // channels after the viewer adds white paper.
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            rectangles.push((bounds, *color_rgb));
        }
        let mut commands = Vec::with_capacity(rectangles.len() * 9);
        for (bounds, color) in rectangles {
            commands.extend(native_commands_for_rect(bounds, color));
        }
        let command_count = commands.len() as u32;
        let mut scene = Vec::with_capacity(20 + commands.len() * 24);
        scene.extend_from_slice(b"OPAN");
        for value in [
            if premultiplied { 2 } else { 1 },
            width,
            height,
            command_count,
        ] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        for entry in commands {
            command(&mut scene, entry.kind, entry.rect, entry.tail);
        }
        debug_assert!(command_count <= 576 && scene.len() <= 13_844);
        let mut output = base_rgba.to_vec();
        // SAFETY: Rust checked the full mark list, sizes and pixel buffer.
        // The ABI wrapper rechecks lengths before invoking the DynLex painter.
        let status = unsafe {
            super::opds_paint_annotations(
                scene.as_ptr(),
                i32::try_from(scene.len()).map_err(|_| "annotation scene too large")?,
                output.as_mut_ptr(),
                i32::try_from(output.len()).map_err(|_| "annotation bitmap too large")?,
            )
        };
        if status != 0 {
            return Err(format!("DynLex annotation kernel status {status}"));
        }
        if premultiplied
            && output
                .chunks_exact(4)
                .any(|pixel| pixel[..3].iter().any(|channel| *channel > pixel[3]))
        {
            return Err("DynLex annotation kernel broke premultiplied RGBA".into());
        }
        Ok(Some(Bitmap {
            width,
            height,
            rgba: output,
        }))
    }
}

#[cfg(all(test, feature = "kernel"))]
mod tests {
    use super::*;

    #[test]
    fn ui_preview_centers_rectangle_and_blends_highlight_without_touching_pdf_path() {
        let white = vec![255_u8; 64 * 48 * 4];
        let marks = [
            Annotation {
                id: 1,
                page: 0,
                bounds: Rect {
                    x: 8.0,
                    y: 8.0,
                    width: 20.0,
                    height: 16.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [231, 76, 60],
                },
            },
            Annotation {
                id: 2,
                page: 0,
                bounds: Rect {
                    x: 32.0,
                    y: 8.0,
                    width: 20.0,
                    height: 16.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            },
        ];
        let preview = render_native_ui_preview_pixels(&white, 64, 48, (64.0, 48.0), &marks, 0)
            .unwrap()
            .unwrap();
        let pixel = |x: usize, y: usize| &preview.rgba[(y * 64 + x) * 4..][..4];
        assert_eq!(pixel(7, 12), [231, 76, 60, 255]);
        assert_eq!(pixel(8, 12), [231, 76, 60, 255]);
        assert_eq!(pixel(9, 12), [255, 255, 255, 255]);
        assert_eq!(pixel(40, 12), [255, 245, 214, 255]);

        let saved_appearance =
            render_native_annotation_overlay_pixels(&white, 64, 48, (64.0, 48.0), &marks, 0)
                .unwrap()
                .unwrap();
        assert_ne!(saved_appearance.rgba, preview.rgba);
        assert_eq!(&white[(12 * 64 + 40) * 4..][..4], &[255; 4]);
    }

    #[test]
    fn selected_frame_and_eight_grips_are_painted_by_dynlex() {
        let width = 40;
        let height = 32;
        let mut page = vec![255_u8; width * height * 4];
        // Preserve an existing colored mark beneath the selection overlay.
        page[(12 * width + 14) * 4..(12 * width + 14) * 4 + 4].copy_from_slice(&[220, 45, 30, 255]);
        page[(7 * width + 14) * 4..(7 * width + 14) * 4 + 4].copy_from_slice(&[231, 76, 60, 255]);
        let rect = Rect {
            x: 8.0,
            y: 8.0,
            width: 20.0,
            height: 16.0,
        };
        let result = render_native_selection_overlay_pixels(
            &page,
            width as u32,
            height as u32,
            (width as f32, height as f32),
            rect,
        )
        .unwrap()
        .unwrap();
        let pixel = |x: usize, y: usize| &result.rgba[(y * width + x) * 4..][..4];
        for (x, y) in [
            (8, 8),
            (18, 8),
            (28, 8),
            (28, 16),
            (28, 24),
            (18, 24),
            (8, 24),
            (8, 16),
        ] {
            assert_eq!(pixel(x, y), [31, 136, 255, 255]);
        }
        assert_eq!(pixel(14, 12), [220, 45, 30, 255]);
        assert_eq!(pixel(14, 7), [127, 124, 226, 255]);
        assert_eq!(pixel(4, 4), [255, 255, 255, 255]);
        assert!(render_native_selection_overlay_pixels(
            &page,
            width as u32,
            height as u32,
            (width as f32, height as f32),
            Rect {
                width: 21.0,
                ..rect
            },
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn sixty_four_disjoint_highlights_fit_expanded_command_bound() {
        let marks: Vec<_> = (0..64)
            .map(|index| Annotation {
                id: index + 1,
                page: 0,
                bounds: Rect {
                    x: (5 + (index % 8) * 16) as f64,
                    y: (7 + (index / 8) * 16) as f64,
                    width: 9.0,
                    height: 9.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            })
            .collect();
        let white = vec![255_u8; 256 * 256 * 4];
        let output =
            render_native_annotation_overlay_pixels(&white, 256, 256, (256.0, 256.0), &marks, 0)
                .unwrap()
                .expect("64 disjoint Highlights must fit the bounded OPAN scene");
        assert_ne!(output.rgba, white);
    }

    #[test]
    fn ordered_highlight_rejects_stale_or_translucent_source_proof() {
        let mark = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 4.0,
                y: 4.0,
                width: 8.0,
                height: 8.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let source = [30_u8, 70, 140, 255].repeat(16 * 16);
        let changed = [231_u8, 76, 60, 255].repeat(16 * 16);
        assert!(render_native_annotation_overlay_pixels_on_opaque_source(
            &changed,
            &source,
            &source,
            16,
            16,
            (16.0, 16.0),
            &[mark.clone()],
            0,
        )
        .unwrap()
        .is_some());
        let mut stale = source.clone();
        stale[((4 * 16 + 4) * 4) as usize] ^= 1;
        assert!(render_native_annotation_overlay_pixels_on_opaque_source(
            &changed,
            &source,
            &stale,
            16,
            16,
            (16.0, 16.0),
            &[mark.clone()],
            0,
        )
        .unwrap()
        .is_none());
        stale = source.clone();
        stale[((4 * 16 + 4) * 4 + 3) as usize] = 128;
        assert!(render_native_annotation_overlay_pixels_on_opaque_source(
            &changed,
            &source,
            &stale,
            16,
            16,
            (16.0, 16.0),
            &[mark],
            0,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn premultiplied_shape_input_rejects_unassociated_color() {
        let mark = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 4.0,
                y: 4.0,
                width: 8.0,
                height: 8.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let mut invalid = vec![0_u8; 16 * 16 * 4];
        invalid[0] = 1;
        assert!(render_native_annotation_overlay_pixels_over_premultiplied(
            &invalid,
            16,
            16,
            (16.0, 16.0),
            &[mark.clone()],
            0,
        )
        .unwrap()
        .is_none());
        let blank = vec![0_u8; invalid.len()];
        let output = render_native_annotation_overlay_pixels_over_premultiplied(
            &blank,
            16,
            16,
            (16.0, 16.0),
            &[mark],
            0,
        )
        .unwrap()
        .expect("valid transparent PDF content should reach DynLex");
        assert_eq!(&output.rgba[(4 * 16 + 4) * 4..][..4], &[89, 79, 23, 89]);
    }

    #[test]
    fn annotation_abi_rejects_null_short_and_mismatched_buffers() {
        let mut scene = Vec::from(&b"OPAN"[..]);
        for value in [1_u32, 4, 4, 0] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        let mut pixels = [255_u8; 64];
        // SAFETY: Only the ABI's own rejection paths are exercised; it must
        // reject before dereferencing a null or undersized allocation.
        unsafe {
            assert_eq!(
                super::super::opds_paint_annotations(
                    std::ptr::null(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    63,
                ),
                2
            );
            scene[16..20].copy_from_slice(&1_u32.to_le_bytes());
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
            scene.resize(20 + 577 * 24, 0);
            scene[16..20].copy_from_slice(&577_u32.to_le_bytes());
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
            // V3 requires exactly one 768-byte selection LUT after its
            // records. A truncated table must not reach DynLex indexing.
            scene.truncate(20);
            scene[4..8].copy_from_slice(&3_u32.to_le_bytes());
            scene[16..20].copy_from_slice(&0_u32.to_le_bytes());
            scene.resize(20 + 767, 0);
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
            scene[4..8].copy_from_slice(&4_u32.to_le_bytes());
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
        }
    }
}
