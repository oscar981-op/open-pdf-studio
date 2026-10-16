//! Research-only comparison of Type1 cubic line points with local Vello.
//! This oracle is never called by DynLex or the native application.

#[path = "../../../dynlex-backend/src/bin/type1_research/type1_cubic_scalar.rs"]
mod scalar;

use fearless_simd::Level;
use std::{env, error::Error, fs};
use vello_common::{
    flatten::{self, FlattenCtx},
    geometry::RectU16,
    kurbo::{Affine, BezPath},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or("usage: type1_curve_oracle CURVES.txt")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let source = fs::read_to_string(input)?;
    if source.len() > 1_000_000 || source.lines().count() > 256 {
        return Err("curve input limit".into());
    }
    let mut differing_curves = 0;
    let mut differing_points = 0;
    for (index, line) in source.lines().enumerate() {
        let numbers: Vec<f64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        if numbers.len() != 8
            || numbers
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=256.0).contains(v))
        {
            return Err("invalid curve".into());
        }
        let c = [
            [numbers[0], numbers[1]],
            [numbers[2], numbers[3]],
            [numbers[4], numbers[5]],
            [numbers[6], numbers[7]],
        ];
        let mut candidate = Vec::new();
        scalar::flatten(c, &mut candidate)?;
        let mut path = BezPath::new();
        path.move_to((c[0][0], c[0][1]));
        path.curve_to((c[1][0], c[1][1]), (c[2][0], c[2][1]), (c[3][0], c[3][1]));
        let mut lines = Vec::new();
        flatten::fill(
            Level::new(),
            path.elements().iter().copied(),
            Affine::IDENTITY,
            &mut lines,
            &mut FlattenCtx::default(),
            RectU16::new(0, 0, 256, 192),
        );
        let reference: Vec<[f64; 2]> = lines
            .iter()
            .filter(|line| [f64::from(line.p1.x), f64::from(line.p1.y)] != c[0])
            .map(|line| [f64::from(line.p1.x), f64::from(line.p1.y)])
            .collect();
        let differences = candidate
            .iter()
            .zip(&reference)
            .filter(|(a, b)| *a != *b)
            .count()
            + candidate.len().abs_diff(reference.len());
        if differences > 0 {
            differing_curves += 1;
            differing_points += differences;
            let first = candidate.iter().zip(&reference).position(|(a, b)| a != b);
            println!(
                "curve={} candidate_points={} reference_points={} differences={} first={:?}",
                index,
                candidate.len(),
                reference.len(),
                differences,
                first.map(|i| (candidate[i], reference[i]))
            );
        }
    }
    println!(
        "total_curves={} differing_curves={} differing_points={}",
        source.lines().count(),
        differing_curves,
        differing_points
    );
    Ok(())
}
