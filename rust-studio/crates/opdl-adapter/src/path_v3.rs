//! Experimental OPDL-v3 path container. Validation only: no renderer consumes it.

use std::error::Error;

const HEADER_LEN: usize = 20;
const RECORD_HEADER_LEN: usize = 8;
pub const MAX_SEGMENTS: usize = 16_384;
pub const MAX_BYTES: usize = HEADER_LEN + MAX_SEGMENTS * (RECORD_HEADER_LEN + 48);
const MAX_COORDINATE: f64 = 1_000_000_000.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Move([f64; 2]),
    Line([f64; 2]),
    Cubic([[f64; 2]; 3]),
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathList {
    pub width: u32,
    pub height: u32,
    pub segments: Vec<Segment>,
}

fn invalid(message: &str) -> Box<dyn Error> {
    message.to_owned().into()
}

fn valid_point(point: [f64; 2]) -> bool {
    point
        .iter()
        .all(|n| n.is_finite() && n.abs() <= MAX_COORDINATE)
}

impl PathList {
    fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.width == 0 || self.height == 0 || self.width > 16_384 || self.height > 16_384 {
            return Err(invalid("invalid OPDL-v3 page dimensions"));
        }
        if self.segments.is_empty() || self.segments.len() > MAX_SEGMENTS {
            return Err(invalid("invalid OPDL-v3 segment count"));
        }
        let mut current_subpath = false;
        for segment in &self.segments {
            match segment {
                Segment::Move(point) => {
                    if !valid_point(*point) {
                        return Err(invalid("invalid OPDL-v3 coordinate"));
                    }
                    current_subpath = true;
                }
                Segment::Line(point) if current_subpath && valid_point(*point) => {}
                Segment::Cubic(points)
                    if current_subpath && points.iter().all(|point| valid_point(*point)) => {}
                Segment::Close if current_subpath => current_subpath = false,
                _ => return Err(invalid("invalid OPDL-v3 path grammar or coordinate")),
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.segments.len() * 24);
        bytes.extend_from_slice(b"OPDL");
        for value in [3, self.width, self.height, self.segments.len() as u32] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for segment in &self.segments {
            let (tag, points): (u8, &[[f64; 2]]) = match segment {
                Segment::Move(point) => (1, std::slice::from_ref(point)),
                Segment::Line(point) => (2, std::slice::from_ref(point)),
                Segment::Cubic(points) => (3, points),
                Segment::Close => (4, &[]),
            };
            let length = RECORD_HEADER_LEN + points.len() * 16;
            bytes.extend_from_slice(&(length as u32).to_le_bytes());
            bytes.extend_from_slice(&[tag, 0, 0, 0]);
            for point in points {
                for coordinate in point {
                    bytes.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() < HEADER_LEN || bytes.len() > MAX_BYTES || &bytes[..4] != b"OPDL" {
            return Err(invalid("invalid OPDL-v3 header or size"));
        }
        let u32_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if u32_at(4) != 3 {
            return Err(invalid("not an OPDL-v3 path list"));
        }
        let count = u32_at(16) as usize;
        if count == 0 || count > MAX_SEGMENTS {
            return Err(invalid("invalid OPDL-v3 segment count"));
        }
        let mut offset = HEADER_LEN;
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            let end_header = offset
                .checked_add(RECORD_HEADER_LEN)
                .ok_or_else(|| invalid("overflow"))?;
            if end_header > bytes.len() {
                return Err(invalid("truncated OPDL-v3 record header"));
            }
            let length = u32_at(offset) as usize;
            let tag = bytes[offset + 4];
            let expected = match tag {
                1 | 2 => 24,
                3 => 56,
                4 => 8,
                _ => return Err(invalid("unknown OPDL-v3 segment")),
            };
            if length != expected || bytes[offset + 5..offset + 8] != [0; 3] {
                return Err(invalid("invalid OPDL-v3 record length or flags"));
            }
            let end = offset
                .checked_add(length)
                .ok_or_else(|| invalid("overflow"))?;
            if end > bytes.len() {
                return Err(invalid("truncated OPDL-v3 record"));
            }
            let point_at = |index: usize| {
                let start = offset + RECORD_HEADER_LEN + index * 16;
                [
                    f64::from_le_bytes(bytes[start..start + 8].try_into().unwrap()),
                    f64::from_le_bytes(bytes[start + 8..start + 16].try_into().unwrap()),
                ]
            };
            segments.push(match tag {
                1 => Segment::Move(point_at(0)),
                2 => Segment::Line(point_at(0)),
                3 => Segment::Cubic([point_at(0), point_at(1), point_at(2)]),
                4 => Segment::Close,
                _ => unreachable!(),
            });
            offset = end;
        }
        if offset != bytes.len() {
            return Err(invalid("trailing OPDL-v3 data"));
        }
        let result = Self {
            width: u32_at(8),
            height: u32_at(12),
            segments,
        };
        result.validate()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> PathList {
        PathList {
            width: 1684,
            height: 2384,
            segments: vec![
                Segment::Move([20.5, 80.5]),
                Segment::Line([30.5, 80.5]),
                Segment::Cubic([[40.5, 100.5], [70.5, 100.5], [90.5, 80.5]]),
                Segment::Close,
            ],
        }
    }

    #[test]
    fn roundtrip_mixed_path_on_large_page() {
        let path = example();
        let bytes = path.encode().unwrap();
        assert_eq!(bytes.len(), 20 + 24 + 24 + 56 + 8);
        assert_eq!(PathList::decode(&bytes).unwrap(), path);
    }

    #[test]
    fn rejects_invalid_lengths_versions_flags_and_truncation() {
        let bytes = example().encode().unwrap();
        for changed in [
            {
                let mut b = bytes.clone();
                b[4] = 2;
                b
            },
            {
                let mut b = bytes.clone();
                b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                b
            },
            {
                let mut b = bytes.clone();
                b[20] = 0;
                b
            },
            {
                let mut b = bytes.clone();
                b[25] = 1;
                b
            },
            {
                let mut b = bytes.clone();
                b[24] = 99;
                b
            },
            {
                let mut b = bytes.clone();
                b.push(0);
                b
            },
            bytes[..bytes.len() - 1].to_vec(),
        ] {
            assert!(PathList::decode(&changed).is_err());
        }
    }

    #[test]
    fn rejects_nonfinite_and_invalid_path_grammar() {
        let mut path = example();
        path.segments[0] = Segment::Line([0.0, 0.0]);
        assert!(path.encode().is_err());
        path.segments[0] = Segment::Move([f64::NAN, 0.0]);
        assert!(path.encode().is_err());
        let mut bytes = example().encode().unwrap();
        bytes[28..36].copy_from_slice(&f64::INFINITY.to_le_bytes());
        assert!(PathList::decode(&bytes).is_err());
    }

    #[test]
    fn bounded_segment_count_and_input_size() {
        let path = PathList {
            width: 256,
            height: 192,
            segments: std::iter::once(Segment::Move([0.0, 0.0]))
                .chain(std::iter::repeat_n(
                    Segment::Line([1.0, 1.0]),
                    MAX_SEGMENTS - 1,
                ))
                .collect(),
        };
        assert!(path.encode().is_ok());
        let mut excessive = path;
        excessive.segments.push(Segment::Line([1.0, 1.0]));
        assert!(excessive.encode().is_err());
        assert!(PathList::decode(&vec![0; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn single_byte_mutations_never_panic() {
        let bytes = example().encode().unwrap();
        for index in 0..bytes.len() {
            let mut mutated = bytes.clone();
            mutated[index] ^= 0xff;
            let _ = PathList::decode(&mutated);
        }
    }
}
