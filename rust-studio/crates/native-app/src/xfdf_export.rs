//! Lossless XFDF export for the native annotation subset supported by import.
//! Unsupported native kinds fail the whole export instead of silently vanishing.
use opds_native_core::annotations::{Annotation, Kind, TextAlign};
use std::fmt::Write as _;

const MAX_XFDF_BYTES: usize = 4 * 1024 * 1024;
const MAX_ITEMS: usize = 4096;

fn xml_legal(value: &str) -> bool {
    value.chars().all(|ch| {
        matches!(ch, '\u{9}' | '\u{A}' | '\u{D}')
            || ('\u{20}'..='\u{D7FF}').contains(&ch)
            || ('\u{E000}'..='\u{FFFD}').contains(&ch)
            || ('\u{10000}'..='\u{10FFFF}').contains(&ch)
    })
}

fn xml_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn color(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub(crate) fn serialize(annotations: &[Annotation], file_name: &str) -> Result<Vec<u8>, String> {
    if !xml_legal(file_name) {
        return Err("PDF file name contains an XML-invalid character".into());
    }
    if annotations.is_empty() {
        return Err("No native annotations to export".into());
    }
    if annotations.len() > MAX_ITEMS {
        return Err("XFDF export exceeds 4096 annotations".into());
    }
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n  <f href=\"{}\"/>\n  <annots>\n",
        xml_text(file_name)
    );
    for annotation in annotations {
        let bounds = annotation.bounds;
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|value| value.is_finite())
            || bounds.width <= 0.0
            || bounds.height <= 0.0
            || !(bounds.x + bounds.width).is_finite()
            || !(bounds.y + bounds.height).is_finite()
        {
            return Err("Cannot export invalid native annotation geometry".into());
        }
        let rect = format!(
            "{},{},{},{}",
            bounds.x,
            bounds.y,
            bounds.x + bounds.width,
            bounds.y + bounds.height
        );
        let attrs = format!(
            "page=\"{}\" rect=\"{}\" name=\"{}\"",
            annotation.page, rect, annotation.id
        );
        match &annotation.kind {
            Kind::Rectangle { color_rgb } => {
                writeln!(
                    xml,
                    "    <square {attrs} color=\"{}\" width=\"2\"/>",
                    color(*color_rgb)
                )
                .unwrap();
            }
            Kind::Ellipse { color_rgb } => {
                writeln!(
                    xml,
                    "    <circle {attrs} color=\"{}\" width=\"2\"/>",
                    color(*color_rgb)
                )
                .unwrap();
            }
            Kind::Line {
                start,
                end,
                color_rgb,
            }
            | Kind::Arrow {
                start,
                end,
                color_rgb,
            } => {
                if !start
                    .iter()
                    .chain(end.iter())
                    .all(|value| value.is_finite())
                {
                    return Err("Cannot export invalid native line geometry".into());
                }
                let tail = if matches!(annotation.kind, Kind::Arrow { .. }) {
                    "open"
                } else {
                    "none"
                };
                writeln!(xml, "    <line {attrs} color=\"{}\" start=\"{},{}\" end=\"{},{}\" width=\"2\" head=\"none\" tail=\"{tail}\"/>", color(*color_rgb), start[0], start[1], end[0], end[1]).unwrap();
            }
            Kind::Polyline { points, color_rgb } => {
                if !(2..=512).contains(&points.len())
                    || !points.iter().flatten().all(|value| value.is_finite())
                {
                    return Err("XFDF ink point count is outside the native limit".into());
                }
                write!(
                    xml,
                    "    <ink {attrs} color=\"{}\" width=\"2\"><inklist><gesture>",
                    color(*color_rgb)
                )
                .unwrap();
                for (index, point) in points.iter().enumerate() {
                    if index > 0 {
                        xml.push(';');
                    }
                    write!(xml, "{},{}", point[0], point[1]).unwrap();
                }
                xml.push_str("</gesture></inklist></ink>\n");
            }
            Kind::Text {
                value,
                color_rgb,
                font_size,
                align,
            } => {
                if *align != TextAlign::Left || value.len() > 4096 || !xml_legal(value) {
                    return Err("XFDF cannot preserve this native text annotation".into());
                }
                writeln!(xml, "    <freetext {attrs} color=\"{}\" fontsize=\"{font_size}\"><contents>{}</contents></freetext>", color(*color_rgb), xml_text(value)).unwrap();
            }
            Kind::Polygon { .. } => {
                return Err("XFDF cannot preserve native polygon annotations".into());
            }
        }
        if xml.len() > MAX_XFDF_BYTES {
            return Err("XFDF export exceeds 4 MiB".into());
        }
    }
    xml.push_str("  </annots>\n</xfdf>\n");
    if xml.len() > MAX_XFDF_BYTES {
        return Err("XFDF export exceeds 4 MiB".into());
    }
    Ok(xml.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xfdf_import;
    use opds_native_core::annotations::{Rect, TextAlign};

    #[test]
    fn native_subset_roundtrips_through_importer() {
        let bounds = Rect {
            x: 10.0,
            y: 12.0,
            width: 30.0,
            height: 40.0,
        };
        let items = vec![
            Annotation {
                id: 1,
                page: 0,
                bounds,
                kind: Kind::Rectangle {
                    color_rgb: [255, 152, 0],
                },
            },
            Annotation {
                id: 2,
                page: 0,
                bounds,
                kind: Kind::Ellipse {
                    color_rgb: [1, 2, 3],
                },
            },
            Annotation {
                id: 3,
                page: 0,
                bounds,
                kind: Kind::Line {
                    start: [10.0, 12.0],
                    end: [40.0, 52.0],
                    color_rgb: [4, 5, 6],
                },
            },
            Annotation {
                id: 4,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 12.0,
                    width: 40.0,
                    height: 40.0,
                },
                kind: Kind::Arrow {
                    start: [10.0, 12.0],
                    end: [40.0, 52.0],
                    color_rgb: [7, 8, 9],
                },
            },
            Annotation {
                id: 5,
                page: 0,
                bounds,
                kind: Kind::Polyline {
                    points: vec![[10.0, 12.0], [20.0, 25.0], [40.0, 52.0]],
                    color_rgb: [10, 11, 12],
                },
            },
            Annotation {
                id: 6,
                page: 0,
                bounds,
                kind: Kind::Text {
                    value: "A & <B>".into(),
                    color_rgb: [13, 14, 15],
                    font_size: 16,
                    align: TextAlign::Left,
                },
            },
        ];
        let bytes = serialize(&items, "a&b.pdf").unwrap();
        let xml = std::str::from_utf8(&bytes).unwrap();
        assert!(xml.contains("a&amp;b.pdf"));
        let imported = xfdf_import::parse(xml, 1, 100).unwrap();
        assert_eq!(imported.len(), items.len());
        for (original, restored) in items.iter().zip(imported.iter()) {
            assert_eq!(original.page, restored.page);
            assert_eq!(original.kind, restored.kind);
            if !matches!(original.kind, Kind::Arrow { .. }) {
                assert_eq!(original.bounds, restored.bounds);
            }
        }
    }

    #[test]
    fn unsupported_kind_is_not_silently_omitted() {
        let item = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            },
            kind: Kind::Polygon {
                color_rgb: [0, 0, 0],
            },
        };
        assert!(serialize(&[item], "x.pdf").is_err());
    }
}
