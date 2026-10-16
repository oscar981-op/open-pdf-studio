//! Bounded OPNW-v1 nonzero-winding geometry. Native text accepts it only through
//! the revalidated PWPG scene; standalone coverage/paint functions remain probes.
//! Unlike OPM2, this explicitly permits closed self-crossing/overlapping paths.
//! Geometry is structurally bounded before calling the existing DynLex strip
//! kernel; correct PDF antialiasing remains subject to independent pixel tests.
use std::error::Error;
const PIXELS: usize = 256 * 192;

#[link(name = "opds_dynlex_glyph_strip", kind = "static")]
unsafe extern "C" {
    fn opds_glyph_strip_raster(
        scene: *const u8,
        length: i32,
        output: *mut u8,
        output_length: i32,
    ) -> i32;
}

pub(crate) fn words(bytes: &[u8]) -> Result<Vec<u64>, Box<dyn Error>> {
    if bytes.len() < 20 || bytes.len() > 4148 || &bytes[..4] != b"OPNW" {
        return Err("OPNW size/magic".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = word(16) as usize;
    if word(4) != 1 || word(8) != 256 || word(12) != 192 || !(1..=8).contains(&count) {
        return Err("OPNW version/dimensions/contours".into());
    }
    // The private aligned kernel ABI keeps its historical magic. OPNW is
    // accepted only by this research boundary, never by the OPM2 validator.
    let mut output = vec![
        u64::from_le_bytes(*b"OPM2\x01\0\0\0"),
        (192_u64 << 32) | 256,
        count as u64,
    ];
    let mut at = 20;
    let mut total = 0;
    for _ in 0..count {
        if at + 4 > bytes.len() {
            return Err("truncated OPNW count".into());
        }
        let points = word(at) as usize;
        at += 4;
        total += points;
        if !(4..=256).contains(&points) || total > 256 || at + points * 16 > bytes.len() {
            return Err("OPNW point limit/length".into());
        }
        output.push(points as u64);
        let mut first = [0.; 2];
        let mut last = [0.; 2];
        for i in 0..points {
            for axis in 0..2 {
                let offset = at + i * 16 + axis * 8;
                let value = f64::from_le_bytes(bytes[offset..offset + 8].try_into()?);
                if !value.is_finite() || !(0.0..=[256., 192.][axis]).contains(&value) {
                    return Err("OPNW nonfinite/outside tile".into());
                }
                if i == 0 {
                    first[axis] = value;
                }
                last[axis] = value;
                output.push(value.to_bits().to_le());
            }
        }
        if first != last {
            return Err("open OPNW contour".into());
        }
        at += points * 16;
    }
    if at != bytes.len() {
        return Err("trailing OPNW bytes".into());
    }
    Ok(output)
}

/// A fresh 256×192 8-bit nonzero coverage buffer, without paint composition.
pub fn coverage(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let words = words(bytes)?;
    let mut output = vec![0; PIXELS];
    // SAFETY: lengths, all contour boundaries, point counts and finite tile
    // bounds have been validated; aligned input and full output outlive FFI.
    // Topology affects winding semantics, not indexing/allocation in this ABI.
    let code = unsafe {
        opds_glyph_strip_raster(
            words.as_ptr().cast(),
            (words.len() * 8) as i32,
            output.as_mut_ptr(),
            output.len() as i32,
        )
    };
    if code != 0 {
        return Err(format!("DynLex winding status {code}").into());
    }
    Ok(output)
}

/// Whole-page research composition; never used by the native dispatcher.
pub fn paint(
    bytes: &[u8],
    color: [u8; 3],
    output: Vec<u8>,
    width: u32,
    height: u32,
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn Error>> {
    let validated = words(bytes)?;
    crate::glyph_scene::paint_experimental_winding(validated, color, output, width, height, origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> Vec<u8> {
        let mut b = b"OPNW".to_vec();
        for n in [1_u32, 256, 192, 1, 5] {
            b.extend(n.to_le_bytes());
        }
        for p in [
            [10_f64, 10.],
            [20., 20.],
            [10., 20.],
            [20., 10.],
            [10., 10.],
        ] {
            for v in p {
                b.extend(v.to_le_bytes());
            }
        }
        b
    }
    #[test]
    fn closed_crossing_has_bounded_winding_output() {
        let bytes = scene();
        let output = coverage(&bytes).unwrap();
        assert_eq!(output.len(), PIXELS);
        assert_eq!(output[11 * 256 + 14], 255);
        assert_eq!(output[15 * 256 + 5], 0);
        assert!(
            crate::render_validated_polygon_over_premultiplied_at_origin_owned(
                &[b"OPM2".as_slice(), &bytes[4..]].concat(),
                [0; 3],
                vec![0; PIXELS * 4],
                256,
                192,
                [0, 0, 256, 192],
                [0, 0]
            )
            .is_err()
        );
    }
    #[test]
    fn malformed_buffers_never_reach_abi() {
        let b = scene();
        for end in 0..b.len() {
            assert!(words(&b[..end]).is_err());
        }
        for (at, value) in [(4, 2_u32), (8, 257), (12, 193), (16, 9), (20, 257)] {
            let mut v = b.clone();
            v[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(words(&v).is_err());
        }
        for value in [f64::NAN, f64::INFINITY, -1., 257.] {
            let mut v = b.clone();
            v[24..32].copy_from_slice(&value.to_le_bytes());
            assert!(words(&v).is_err());
        }
        let mut v = b.clone();
        v.push(0);
        assert!(words(&v).is_err());
        let mut v = b.clone();
        v[88..96].copy_from_slice(&11_f64.to_le_bytes());
        assert!(words(&v).is_err());
        for i in 0..b.len() {
            let mut v = b.clone();
            v[i] ^= 255;
            assert!(std::panic::catch_unwind(|| words(&v)).is_ok());
        }
    }
}
