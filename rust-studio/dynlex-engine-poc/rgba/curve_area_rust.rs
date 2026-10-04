//! Independent bounded polygon-area raster trial for pre-flattened curve outlines.
//! This accepts only trusted fixture geometry, not a PDF or raw OPDL-v4 scene.

use std::{env, error::Error, fs};

const WIDTH: usize = 256;
const HEIGHT: usize = 192;
type Point = [f64; 2];

fn read_polygon(path: &std::ffi::OsStr) -> Result<Vec<Point>, Box<dyn Error>> {
    let input = fs::read_to_string(path)?;
    let mut points: Vec<Point> = Vec::new();
    for line in input.lines().filter(|line| !line.starts_with('#')) {
        let values: Vec<f64> = line
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<Result<_, _>>()?;
        if values.len() != 4
            || values
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 4096.0)
        {
            return Err("invalid flattened line".into());
        }
        if let Some(previous) = points.last() {
            if (previous[0] - values[0]).abs() > 0.000001
                || (previous[1] - values[1]).abs() > 0.000001
            {
                return Err("disconnected outline".into());
            }
        } else {
            points.push([values[0], values[1]]);
        }
        points.push([values[2], values[3]]);
        if points.len() > 4097 {
            return Err("too many flattened lines".into());
        }
    }
    if points.len() < 4
        || (points[0][0] - points.last().unwrap()[0]).abs() > 0.000001
        || (points[0][1] - points.last().unwrap()[1]).abs() > 0.000001
    {
        return Err("open or empty outline".into());
    }
    points.pop();
    Ok(points)
}

fn clip(poly: &[Point], axis: usize, boundary: f64, greater: bool) -> Vec<Point> {
    if poly.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(poly.len() + 2);
    let mut previous = *poly.last().unwrap();
    let mut previous_inside = if greater {
        previous[axis] >= boundary
    } else {
        previous[axis] <= boundary
    };
    for &point in poly {
        let inside = if greater {
            point[axis] >= boundary
        } else {
            point[axis] <= boundary
        };
        if inside != previous_inside {
            let ratio = (boundary - previous[axis]) / (point[axis] - previous[axis]);
            result.push([
                previous[0] + ratio * (point[0] - previous[0]),
                previous[1] + ratio * (point[1] - previous[1]),
            ]);
        }
        if inside {
            result.push(point);
        }
        previous = point;
        previous_inside = inside;
    }
    result
}

fn pixel_coverage(poly: &[Point], x: usize, y: usize) -> u8 {
    let mut clipped = poly.to_vec();
    for (axis, boundary, greater) in [
        (0, x as f64, true),
        (0, (x + 1) as f64, false),
        (1, y as f64, true),
        (1, (y + 1) as f64, false),
    ] {
        clipped = clip(&clipped, axis, boundary, greater);
        if clipped.is_empty() {
            return 0;
        }
    }
    let mut signed = 0.0;
    for index in 0..clipped.len() {
        let a = clipped[index];
        let b = clipped[(index + 1) % clipped.len()];
        signed += a[0] * b[1] - b[0] * a[1];
    }
    ((signed.abs() / 2.0).min(1.0) * 255.0).round() as u8
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: curve_area_rust LINES.txt COVERAGE.raw")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let polygon = read_polygon(&input)?;
    let min_x = polygon
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as usize;
    let max_x = polygon
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(WIDTH as f64) as usize;
    let min_y = polygon
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as usize;
    let max_y = polygon
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(HEIGHT as f64) as usize;
    let mut pixels = vec![0u8; WIDTH * HEIGHT];
    for y in min_y..max_y {
        for x in min_x..max_x {
            pixels[y * WIDTH + x] = pixel_coverage(&polygon, x, y);
        }
    }
    fs::write(output, pixels)?;
    Ok(())
}
