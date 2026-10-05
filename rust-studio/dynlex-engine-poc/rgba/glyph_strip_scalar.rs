//! Independent scalar f32 tile-strip experiment for validated OPM2 outlines.
//!
//! The area and winding equations were studied in the Apache-2.0/MIT licensed
//! vello_common 0.0.8 `strip.rs`. This implementation has no Vello dependency
//! or production routing. The separate Rust OPM2 parser checks topology.
use std::{env, error::Error, fs};

#[path = "glyph_multi_probe.rs"]
#[allow(dead_code)]
mod scene;

const WIDTH: usize = 256;
const HEIGHT: usize = 192;
const TILE: usize = 4;

type Point = (f32, f32);

fn max_if_first_nan_take_second(value: f32, bound: f32) -> f32 {
    if value.is_nan() {
        bound
    } else {
        value.max(bound)
    }
}

// A bounding-box overlap alone also visits tiles touched only by a line's
// endpoint or bbox corner. Vello's line binning skips those tiles, which can
// change a half-byte coverage tie after several f32 winding additions.
fn line_crosses_tile(x0: f32, y0: f32, x1: f32, y1: f32, left: f32, top: f32) -> bool {
    let lo = top.max(y0.min(y1));
    let hi = (top + TILE as f32).min(y0.max(y1));
    if lo >= hi {
        return false;
    }
    if x0 == x1 {
        // A vertical line on a tile boundary belongs to the tile whose
        // left edge it occupies.
        return x0 >= left && x0 < left + TILE as f32;
    }
    let slope = (x1 - x0) / (y1 - y0);
    let xa = (lo - y0).mul_add(slope, x0);
    let xb = (hi - y0).mul_add(slope, x0);
    xa.max(xb) > left && xa.min(xb) < left + TILE as f32
}

fn render(contours: &[Vec<[f64; 2]>]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut paths = Vec::with_capacity(contours.len());
    let mut quantized = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut path = Vec::with_capacity(contour.len());
        let mut screen_path = Vec::with_capacity(contour.len());
        for &[x, y] in contour {
            if !(0.0..=WIDTH as f64).contains(&x) || !(0.0..=HEIGHT as f64).contains(&y) {
                return Err("scalar tile proof requires an outline inside the page".into());
            }
            path.push((x as f32, y as f32));
            screen_path.push([(x as f32) as f64, (y as f32) as f64]);
        }
        paths.push(path);
        quantized.push(screen_path);
    }
    scene::validate_topology(&quantized)?;

    let mut output = vec![0_u8; WIDTH * HEIGHT];
    for tile_y in 0..HEIGHT / TILE {
        let mut winding = [0.0_f32; TILE];
        for tile_x in 0..WIDTH / TILE {
            let left = (tile_x * TILE) as f32;
            let top = (tile_y * TILE) as f32;
            let mut location = [winding; TILE];
            for path in &paths {
                for edge in path.windows(2) {
                    let ((x0, y0), (x1, y1)): (Point, Point) = (edge[0], edge[1]);
                    if !line_crosses_tile(x0, y0, x1, y1, left, top) {
                        continue;
                    }
                    let (p0x, p0y, p1x, p1y) = (x0 - left, y0 - top, x1 - left, y1 - top);
                    let sign = (p0y - p1y).signum();
                    let (line_top_y, line_top_x, line_bottom_y, line_bottom_x) = if p0y < p1y {
                        (p0y, p0x, p1y, p1x)
                    } else {
                        (p1y, p1x, p0y, p0x)
                    };
                    let y_slope = (line_bottom_y - line_top_y) / (line_bottom_x - line_top_x);
                    let x_slope = 1.0 / y_slope;
                    let line_px_base_yx = line_top_y.mul_add(-x_slope, line_top_x);
                    let mut acc = [0.0_f32; TILE];
                    for (column, column_winding) in location.iter_mut().enumerate() {
                        for row in 0..TILE {
                            let ymin = line_top_y.max(row as f32);
                            let ymax = line_bottom_y.min((row + 1) as f32);
                            let left_y =
                                ((column as f32) - line_top_x).mul_add(y_slope, line_top_y);
                            let right_y =
                                (((column + 1) as f32) - line_top_x).mul_add(y_slope, line_top_y);
                            let left_y = max_if_first_nan_take_second(left_y, ymin).min(ymax);
                            let right_y = max_if_first_nan_take_second(right_y, ymin).min(ymax);
                            let left_yx = left_y.mul_add(x_slope, line_px_base_yx);
                            let right_yx = right_y.mul_add(x_slope, line_px_base_yx);
                            let h = (right_y - left_y).abs();
                            let area = h * (right_yx + left_yx).mul_add(-0.5, (column + 1) as f32);
                            column_winding[row] += area.mul_add(sign, acc[row]);
                            acc[row] = h.mul_add(sign, acc[row]);
                        }
                    }
                    for row in 0..TILE {
                        winding[row] += acc[row];
                    }
                }
            }
            for (column, column_winding) in location.iter().enumerate() {
                for (row, value) in column_winding.iter().enumerate() {
                    let alpha = value.abs().mul_add(255.0, 0.5).min(255.0);
                    output[(tile_y * TILE + row) * WIDTH + tile_x * TILE + column] = alpha as u8;
                }
            }
        }
    }
    Ok(output)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: glyph_strip_scalar INPUT.opm OUTPUT.raw")?;
    let output = args.next().ok_or("missing output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let contours = scene::read_scene(&fs::read(input)?)?;
    fs::write(output, render(&contours)?)?;
    Ok(())
}
