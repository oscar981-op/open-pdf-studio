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

/// Pixel-verified, bounded stroke primitive shared by experimental renderers.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedStroke {
    pub kind: u32,
    pub rect: [u32; 4],
    pub rgb: [u8; 3],
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
    /// Bridge only verified pixel-aligned 1px axis and 45° strokes to the
    /// existing DynLex OPDL-v2 kernel. No cubic, clip or image path.
    pub fn to_v2_axis_strokes(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let strokes = self.verified_strokes()?;
        let mut bytes = Vec::with_capacity(40 + strokes.len() * 24);
        bytes.extend_from_slice(b"OPDL");
        for value in [
            2,
            self.width,
            self.height,
            0,
            0,
            self.width,
            self.height,
            0,
            strokes.len() as u32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for stroke in strokes {
            bytes.extend_from_slice(&stroke.kind.to_le_bytes());
            for value in stroke.rect {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&[stroke.rgb[0], stroke.rgb[1], stroke.rgb[2], 255]);
        }
        Ok(bytes)
    }

    /// Classify only the proven one-pixel axis and 45-degree strokes. This
    /// does not depend on an OPDL-v2 byte layout.
    pub fn verified_strokes(&self) -> Result<Vec<VerifiedStroke>, Box<dyn Error>> {
        self.validate()?;
        if self.width > 1024 || self.height > 1024 {
            return Err(invalid("v2 bridge page exceeds verified 1024px limit"));
        }
        let mut color = None;
        let mut start = None;
        let mut end = None;
        let mut strokes = Vec::new();
        let exact = |value: f64| -> Result<u32, Box<dyn Error>> {
            if !value.is_finite() || value < 0.0 || value > 1024.0 || value.fract() != 0.0 {
                return Err(invalid("unverified stroke coordinate"));
            }
            Ok(value as u32)
        };
        for command in &self.commands {
            match command {
                Command::StrokeStyle { rgb, width } if *width == 1.0 => color = Some(*rgb),
                Command::Path(Segment::Move(point)) if start.is_none() => start = Some(*point),
                Command::Path(Segment::Line(point)) if start.is_some() && end.is_none() => {
                    end = Some(*point)
                }
                Command::Stroke if start.is_some() && end.is_some() => {
                    let a = start.take().unwrap();
                    let b = end.take().unwrap();
                    let (kind, rect): (u32, [u32; 4]) = if a[1] == b[1] {
                        let left = exact(a[0].min(b[0]))?;
                        let right = exact(a[0].max(b[0]))?;
                        let top = exact(f64::from(self.height) - a[1] - 0.5)?;
                        (3, [left, top, right, top + 1])
                    } else if a[0] == b[0] {
                        let left = exact(a[0] - 0.5)?;
                        let top = exact(f64::from(self.height) - a[1].max(b[1]))?;
                        let bottom = exact(f64::from(self.height) - a[1].min(b[1]))?;
                        (3, [left, top, left + 1, bottom])
                    } else if b[0] > a[0]
                        && b[0] - a[0] == (b[1] - a[1]).abs()
                        && [a[0], a[1], b[0], b[1]]
                            .iter()
                            .all(|value| value.fract() == 0.5)
                    {
                        let left = exact(a[0] - 0.5)?;
                        let right = exact(b[0] + 0.5)?;
                        if b[1] < a[1] {
                            let top = exact(f64::from(self.height) - a[1] - 0.5)?;
                            let bottom = exact(f64::from(self.height) - b[1] + 0.5)?;
                            (4, [left, top, right, bottom])
                        } else {
                            let top = exact(f64::from(self.height) - b[1] - 0.5)?;
                            let bottom = exact(f64::from(self.height) - a[1] + 0.5)?;
                            (5, [left, top, right, bottom])
                        }
                    } else {
                        return Err(invalid("stroke angle is outside verified v2 subset"));
                    };
                    if rect[0] >= rect[2]
                        || rect[1] >= rect[3]
                        || rect[2] > self.width
                        || rect[3] > self.height
                    {
                        return Err(invalid("stroke outside verified page"));
                    }
                    if (kind == 4 || kind == 5)
                        && (rect[2] - rect[0] != rect[3] - rect[1] || rect[2] - rect[0] < 2)
                    {
                        return Err(invalid("invalid 45-degree stroke bounds"));
                    }
                    strokes.push(VerifiedStroke {
                        kind,
                        rect,
                        rgb: color.ok_or_else(|| invalid("missing stroke RGB"))?,
                    });
                }
                _ => return Err(invalid("v4 command outside verified v2 axis subset")),
            }
        }
        Ok(strokes)
    }

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

    #[test]
    fn bridges_two_pixel_aligned_axis_strokes_only() {
        let mut list = DisplayList {
            width: 256,
            height: 192,
            commands: vec![
                Command::StrokeStyle {
                    rgb: [255, 0, 0],
                    width: 1.0,
                },
                Command::Path(Segment::Move([20.0, 151.5])),
                Command::Path(Segment::Line([100.0, 151.5])),
                Command::Stroke,
                Command::StrokeStyle {
                    rgb: [0, 0, 255],
                    width: 1.0,
                },
                Command::Path(Segment::Move([150.5, 160.0])),
                Command::Path(Segment::Line([150.5, 60.0])),
                Command::Stroke,
            ],
        };
        let bytes = list.to_v2_axis_strokes().unwrap();
        assert_eq!(bytes.len(), 40 + 2 * 24);
        assert_eq!(u32::from_le_bytes(bytes[36..40].try_into().unwrap()), 2);
        assert_eq!(
            &bytes[44..60],
            &[20, 0, 0, 0, 40, 0, 0, 0, 100, 0, 0, 0, 41, 0, 0, 0]
        );
        assert_eq!(&bytes[60..64], &[255, 0, 0, 255]);
        list.commands[4] = Command::StrokeStyle {
            rgb: [0, 0, 255],
            width: 2.0,
        };
        assert!(list.to_v2_axis_strokes().is_err());
        list.commands[4] = Command::StrokeStyle {
            rgb: [0, 0, 255],
            width: 1.0,
        };
        list.commands[6] = Command::Path(Segment::Cubic([[1.0, 2.0]; 3]));
        assert!(list.to_v2_axis_strokes().is_err());
    }

    #[test]
    fn bridges_both_verified_45_degree_directions() {
        for (end, expected_kind) in [([80.5, 91.5], 4_u32), ([80.5, 211.5], 5_u32)] {
            let mut list = DisplayList {
                width: 256,
                height: 256,
                commands: vec![
                    Command::StrokeStyle {
                        rgb: [255, 0, 0],
                        width: 1.0,
                    },
                    Command::Path(Segment::Move([20.5, 151.5])),
                    Command::Path(Segment::Line(end)),
                    Command::Stroke,
                ],
            };
            let bytes = list.to_v2_axis_strokes().unwrap();
            assert_eq!(
                u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
                expected_kind
            );
            list.commands[2] = Command::Path(Segment::Line([80.5, 90.5]));
            assert!(list.to_v2_axis_strokes().is_err());
        }
    }
}
