//! Bounded OPLN-v1 flattened-outline contract for experimental kernels only.

use std::{env, error::Error, fs};

pub type Point = [f64; 2];
const MAGIC: &[u8; 4] = b"OPLN";
const HEADER: usize = 20;
const MAX_POINTS: usize = 4097;

fn validate(points: &[Point]) -> Result<(), Box<dyn Error>> {
    if !(4..=MAX_POINTS).contains(&points.len())
        || points
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || value.abs() > 4096.0)
        || (points[0][0] - points.last().unwrap()[0]).abs() > 0.000001
        || (points[0][1] - points.last().unwrap()[1]).abs() > 0.000001
    {
        return Err("invalid bounded closed outline".into());
    }
    Ok(())
}

pub fn parse_lines(input: &str) -> Result<Vec<Point>, Box<dyn Error>> {
    let mut points: Vec<Point> = Vec::new();
    for line in input.lines().filter(|line| !line.starts_with('#')) {
        let values: Vec<f64> = line
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<Result<_, _>>()?;
        if values.len() != 4 {
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
        if points.len() > MAX_POINTS {
            return Err("too many flattened lines".into());
        }
    }
    validate(&points)?;
    Ok(points)
}

pub fn encode(points: &[Point]) -> Result<Vec<u8>, Box<dyn Error>> {
    validate(points)?;
    let mut bytes = Vec::with_capacity(HEADER + points.len() * 16);
    bytes.extend_from_slice(MAGIC);
    for value in [1_u32, 256, 192, u32::try_from(points.len())?] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for point in points {
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Point>, Box<dyn Error>> {
    if bytes.len() < HEADER || &bytes[..4] != MAGIC {
        return Err("invalid OPLN header".into());
    }
    let word = |start: usize| u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap());
    let count = usize::try_from(word(16))?;
    if word(4) != 1
        || word(8) != 256
        || word(12) != 192
        || !(4..=MAX_POINTS).contains(&count)
        || bytes.len() != HEADER + count * 16
    {
        return Err("invalid OPLN version, dimensions, count or byte length".into());
    }
    let mut points = Vec::with_capacity(count);
    for pair in bytes[HEADER..].chunks_exact(16) {
        points.push([
            f64::from_le_bytes(pair[..8].try_into().unwrap()),
            f64::from_le_bytes(pair[8..].try_into().unwrap()),
        ]);
    }
    validate(&points)?;
    Ok(points)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let command = args
        .next()
        .ok_or("usage: curve_scene encode INPUT.txt OUTPUT.opln | validate INPUT.opln")?;
    let input = args.next().ok_or("missing input")?;
    match command.to_str() {
        Some("encode") => {
            let output = args.next().ok_or("missing output")?;
            if args.next().is_some() {
                return Err("extra argument".into());
            }
            fs::write(output, encode(&parse_lines(&fs::read_to_string(input)?)?)?)?;
        }
        Some("validate") => {
            if args.next().is_some() {
                return Err("extra argument".into());
            }
            decode(&fs::read(input)?)?;
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_outline_roundtrips_and_corruption_rejects() {
        let points = vec![[1.0, 1.0], [5.0, 1.0], [5.0, 3.0], [1.0, 1.0]];
        let bytes = encode(&points).unwrap();
        assert_eq!(decode(&bytes).unwrap(), points);
        for length in 0..bytes.len() {
            assert!(decode(&bytes[..length]).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[20..28].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&invalid).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        let mut bad_count = bytes.clone();
        bad_count[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bad_count).is_err());
        let mut bad_version = bytes.clone();
        bad_version[4..8].copy_from_slice(&2_u32.to_le_bytes());
        assert!(decode(&bad_version).is_err());
        let mut bad_dimensions = bytes.clone();
        bad_dimensions[8..12].copy_from_slice(&0_u32.to_le_bytes());
        assert!(decode(&bad_dimensions).is_err());
        for index in 0..bytes.len() {
            let mut mutated = bytes.clone();
            mutated[index] ^= 0xff;
            let _ = decode(&mutated); // No byte mutation may panic.
        }
    }
}
