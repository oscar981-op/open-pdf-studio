//! OPGM-v1: experimental bounded page geometry for one opaque rectangle.
//! Converts display coordinates to PDF geometry and a vector-only OPDL-v2
//! scene. No PDF parser, app route, or general renderer consumes OPGM.
use std::{env, error::Error, fs};

const BYTES: usize = 56;
const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: u32 = 6_000_000;

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn convert(bytes: &[u8]) -> Result<(Vec<u8>, [u32; 4]), Box<dyn Error>> {
    if bytes.len() != BYTES || &bytes[..4] != b"OPGM" || word(bytes, 4) != 1 {
        return Err("invalid OPGM-v1 header or size".into());
    }
    let media = [word(bytes, 8), word(bytes, 12)];
    let crop = [
        word(bytes, 16),
        word(bytes, 20),
        word(bytes, 24),
        word(bytes, 28),
    ];
    let turn = word(bytes, 32);
    let rect = [
        word(bytes, 36),
        word(bytes, 40),
        word(bytes, 44),
        word(bytes, 48),
    ];
    if media.iter().any(|n| *n == 0 || *n > MAX_SIDE)
        || crop[0] >= crop[2]
        || crop[1] >= crop[3]
        || crop[2] > media[0]
        || crop[3] > media[1]
        || !matches!(turn, 0 | 90 | 180 | 270)
        || bytes[52..56] != [255, 0, 0, 255]
    {
        return Err("invalid OPGM-v1 geometry or ink".into());
    }
    let raw_width = crop[2] - crop[0];
    let raw_height = crop[3] - crop[1];
    let (width, height) = if matches!(turn, 90 | 270) {
        (raw_height, raw_width)
    } else {
        (raw_width, raw_height)
    };
    if width
        .checked_mul(height)
        .is_none_or(|pixels| pixels > MAX_PIXELS)
        || rect[0] >= rect[2]
        || rect[1] >= rect[3]
        || rect[2] > width
        || rect[3] > height
    {
        return Err("OPGM-v1 rectangle or page exceeds bound".into());
    }
    let [left, top, right, bottom] = rect;
    let pdf_rect = match turn {
        0 => [crop[0] + left, crop[3] - bottom, right - left, bottom - top],
        90 => [crop[0] + top, crop[1] + left, bottom - top, right - left],
        180 => [crop[2] - right, crop[1] + top, right - left, bottom - top],
        270 => [
            crop[2] - bottom,
            crop[3] - right,
            bottom - top,
            right - left,
        ],
        _ => unreachable!(),
    };
    let mut scene = Vec::with_capacity(64);
    scene.extend_from_slice(b"OPDL");
    for number in [
        2, width, height, 0, 0, width, height, 0, 1, 1, left, top, right, bottom,
    ] {
        scene.extend_from_slice(&number.to_le_bytes());
    }
    scene.extend_from_slice(&bytes[52..56]);
    Ok((scene, pdf_rect))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: page_geometry_probe INPUT.opgm OUTPUT.opdl")?;
    let output = args.next().ok_or("missing output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let (scene, pdf_rect) = convert(&fs::read(input)?)?;
    fs::write(output, scene)?;
    println!(
        "{} {} {} {}",
        pdf_rect[0], pdf_rect[1], pdf_rect[2], pdf_rect[3]
    );
    Ok(())
}
