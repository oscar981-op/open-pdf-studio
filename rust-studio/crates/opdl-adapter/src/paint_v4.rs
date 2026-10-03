//! Experimental OPDL-v4 stroked paths. Strict storage contract, no renderer.

use crate::path_v3::Segment;
use std::error::Error;

const HEADER: usize = 20;
const RECORD: usize = 8;
pub const MAX_RECORDS: usize = 16_384;
pub const MAX_BYTES: usize = HEADER + MAX_RECORDS * 56;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    StrokeStyle { rgb: [u8; 3], width: f64 },
    Path(Segment),
    Stroke,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    pub width: u32,
    pub height: u32,
    pub commands: Vec<Command>,
}

fn invalid(message: &str) -> Box<dyn Error> {
    message.to_owned().into()
}

fn valid_point(point: [f64; 2]) -> bool {
    point
        .iter()
        .all(|value| value.is_finite() && value.abs() <= 1_000_000_000.0)
}

impl DisplayList {
    fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.width == 0
            || self.height == 0
            || self.width > 16_384
            || self.height > 16_384
            || self.commands.is_empty()
            || self.commands.len() > MAX_RECORDS
        {
            return Err(invalid("invalid OPDL-v4 dimensions or record count"));
        }
        let mut style = false;
        let mut subpath = false;
        let mut pending_path = false;
        let mut has_segment = false;
        let mut stroke_count = 0;
        for command in &self.commands {
            match command {
                Command::StrokeStyle { width, .. }
                    if !pending_path
                        && width.is_finite()
                        && (0.0..=1_000.0).contains(width)
                        && *width > 0.0 =>
                {
                    style = true;
                }
                Command::Path(Segment::Move(point)) if style && valid_point(*point) => {
                    subpath = true;
                    pending_path = true;
                }
                Command::Path(Segment::Line(point)) if subpath && valid_point(*point) => {
                    has_segment = true;
                }
                Command::Path(Segment::Cubic(points))
                    if subpath && points.iter().all(|point| valid_point(*point)) =>
                {
                    has_segment = true;
                }
                Command::Path(Segment::Close) if subpath && has_segment => {
                    subpath = false;
                }
                Command::Stroke if pending_path && has_segment => {
                    stroke_count += 1;
                    subpath = false;
                    pending_path = false;
                    has_segment = false;
                }
                _ => return Err(invalid("invalid OPDL-v4 path or stroke state")),
            }
        }
        if pending_path || stroke_count == 0 {
            return Err(invalid("OPDL-v4 path is unpainted"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(HEADER + self.commands.len() * 24);
        bytes.extend_from_slice(b"OPDL");
        for value in [4, self.width, self.height, self.commands.len() as u32] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for command in &self.commands {
            let (tag, points): (u8, &[[f64; 2]]) = match command {
                Command::Path(Segment::Move(point)) => (1, std::slice::from_ref(point)),
                Command::Path(Segment::Line(point)) => (2, std::slice::from_ref(point)),
                Command::Path(Segment::Cubic(points)) => (3, points),
                Command::Path(Segment::Close) => (4, &[]),
                Command::StrokeStyle { .. } => (5, &[]),
                Command::Stroke => (6, &[]),
            };
            let length = if tag == 5 {
                24
            } else {
                RECORD + 16 * points.len()
            };
            bytes.extend_from_slice(&(length as u32).to_le_bytes());
            bytes.extend_from_slice(&[tag, 0, 0, 0]);
            match command {
                Command::StrokeStyle { rgb, width } => {
                    bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                    bytes.extend_from_slice(&[0; 4]);
                    bytes.extend_from_slice(&width.to_le_bytes());
                }
                _ => {
                    for point in points {
                        for coordinate in point {
                            bytes.extend_from_slice(&coordinate.to_le_bytes());
                        }
                    }
                }
            }
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() < HEADER || bytes.len() > MAX_BYTES || &bytes[..4] != b"OPDL" {
            return Err(invalid("invalid OPDL-v4 header or size"));
        }
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if word(4) != 4 {
            return Err(invalid("not an OPDL-v4 display list"));
        }
        let count = word(16) as usize;
        if count == 0 || count > MAX_RECORDS {
            return Err(invalid("invalid OPDL-v4 record count"));
        }
        let mut commands = Vec::with_capacity(count);
        let mut offset = HEADER;
        for _ in 0..count {
            if offset
                .checked_add(RECORD)
                .is_none_or(|end| end > bytes.len())
            {
                return Err(invalid("truncated OPDL-v4 record header"));
            }
            let length = word(offset) as usize;
            let tag = bytes[offset + 4];
            let expected = match tag {
                1 | 2 | 5 => 24,
                3 => 56,
                4 | 6 => 8,
                _ => return Err(invalid("unknown OPDL-v4 record")),
            };
            if length != expected || bytes[offset + 5..offset + 8] != [0; 3] {
                return Err(invalid("invalid OPDL-v4 record length or flags"));
            }
            if offset
                .checked_add(length)
                .is_none_or(|end| end > bytes.len())
            {
                return Err(invalid("truncated OPDL-v4 record"));
            }
            let point = |index: usize| {
                let start = offset + RECORD + index * 16;
                [
                    f64::from_le_bytes(bytes[start..start + 8].try_into().unwrap()),
                    f64::from_le_bytes(bytes[start + 8..start + 16].try_into().unwrap()),
                ]
            };
            commands.push(match tag {
                1 => Command::Path(Segment::Move(point(0))),
                2 => Command::Path(Segment::Line(point(0))),
                3 => Command::Path(Segment::Cubic([point(0), point(1), point(2)])),
                4 => Command::Path(Segment::Close),
                5 => {
                    if bytes[offset + 11] != 255 || bytes[offset + 12..offset + 16] != [0; 4] {
                        return Err(invalid("unsupported OPDL-v4 alpha or style flags"));
                    }
                    Command::StrokeStyle {
                        rgb: [bytes[offset + 8], bytes[offset + 9], bytes[offset + 10]],
                        width: f64::from_le_bytes(
                            bytes[offset + 16..offset + 24].try_into().unwrap(),
                        ),
                    }
                }
                6 => Command::Stroke,
                _ => unreachable!(),
            });
            offset += length;
        }
        if offset != bytes.len() {
            return Err(invalid("trailing OPDL-v4 data"));
        }
        let result = Self {
            width: word(8),
            height: word(12),
            commands,
        };
        result.validate()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> DisplayList {
        DisplayList {
            width: 1684,
            height: 2384,
            commands: vec![
                Command::StrokeStyle {
                    rgb: [255, 0, 0],
                    width: 1.0,
                },
                Command::Path(Segment::Move([20.5, 80.5])),
                Command::Path(Segment::Cubic([[35.5, 130.5], [75.5, 130.5], [90.5, 80.5]])),
                Command::Stroke,
                Command::StrokeStyle {
                    rgb: [0, 0, 255],
                    width: 2.0,
                },
                Command::Path(Segment::Move([40.0, 40.0])),
                Command::Path(Segment::Line([80.0, 40.0])),
                Command::Stroke,
            ],
        }
    }

    #[test]
    fn preserves_two_strokes_and_layer_order() {
        let list = example();
        let encoded = list.encode().unwrap();
        assert_eq!(DisplayList::decode(&encoded).unwrap(), list);
    }

    #[test]
    fn rejects_unpainted_or_styleless_paths() {
        let mut list = example();
        list.commands.pop();
        assert!(list.encode().is_err());
        list.commands = vec![Command::Path(Segment::Move([0.0, 0.0])), Command::Stroke];
        assert!(list.encode().is_err());
        list.commands = vec![Command::StrokeStyle {
            rgb: [0; 3],
            width: f64::NAN,
        }];
        assert!(list.encode().is_err());
    }

    #[test]
    fn rejects_corruption_without_panicking() {
        let encoded = example().encode().unwrap();
        for index in 0..encoded.len() {
            let mut changed = encoded.clone();
            changed[index] ^= 0xff;
            let _ = DisplayList::decode(&changed);
        }
        assert!(DisplayList::decode(&encoded[..encoded.len() - 1]).is_err());
        assert!(DisplayList::decode(&vec![0; MAX_BYTES + 1]).is_err());
    }
}
