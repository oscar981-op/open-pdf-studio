//! Strict, bounded import of native annotation geometry from Tauri XFDF.
//! Unsupported markup fails the whole import before the document is edited.
use opds_native_core::annotations::{
    arrow_wings, Annotation, Kind, Rect, TextAlign, TEXT_FONT_SIZES,
};
use roxmltree::Node;
use std::path::Path;

const MAX_XFDF_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ITEMS: usize = 4096;

fn number(value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "Invalid XFDF coordinate".into())
}

fn pair(value: &str) -> Result<[f64; 2], String> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.len() != 2 {
        return Err("Invalid XFDF point".into());
    }
    Ok([number(parts[0])?, number(parts[1])?])
}

fn rect(value: &str) -> Result<Rect, String> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.len() != 4 {
        return Err("Invalid XFDF rectangle".into());
    }
    let [x0, y0, x1, y1] = [
        number(parts[0])?,
        number(parts[1])?,
        number(parts[2])?,
        number(parts[3])?,
    ];
    let (width, height) = (x1 - x0, y1 - y0);
    if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
        return Err("XFDF rectangle must have positive size".into());
    }
    Ok(Rect {
        x: x0,
        y: y0,
        width,
        height,
    })
}

fn color(raw: Option<&str>) -> Result<[u8; 3], String> {
    let raw = raw.unwrap_or("#000000");
    if let Some(hex) = raw.strip_prefix('#') {
        if hex.len() == 6 {
            return Ok([
                u8::from_str_radix(&hex[0..2], 16).map_err(|_| "Invalid XFDF color")?,
                u8::from_str_radix(&hex[2..4], 16).map_err(|_| "Invalid XFDF color")?,
                u8::from_str_radix(&hex[4..6], 16).map_err(|_| "Invalid XFDF color")?,
            ]);
        }
    }
    Err("Only #RRGGBB XFDF colors are supported".into())
}

fn required<'a>(node: Node<'a, 'a>, attr: &str) -> Result<&'a str, String> {
    node.attribute(attr)
        .ok_or_else(|| format!("Missing XFDF {attr}"))
}

fn parse_item(node: Node<'_, '_>, pages: usize, id: u64) -> Result<Annotation, String> {
    let page = required(node, "page")?
        .parse::<usize>()
        .map_err(|_| "Invalid XFDF page")?;
    if page >= pages {
        return Err("XFDF page is outside the active PDF".into());
    }
    if node.attribute("opstype").is_some()
        || node.attribute("opslayer").is_some()
        || node.attribute("interior-color").is_some()
        || node.attribute("opacity").is_some_and(|value| value != "1")
    {
        return Err(
            "XFDF extension, layer, fill or opacity is not supported by native import".into(),
        );
    }
    if node
        .attribute("width")
        .is_some_and(|value| value != "2" && value != "2.0")
    {
        return Err("XFDF stroke width is not supported by native import".into());
    }
    let mut bounds = rect(required(node, "rect")?)?;
    let rgb = color(node.attribute("color"))?;
    let kind = match node.tag_name().name() {
        "square" => Kind::Rectangle { color_rgb: rgb },
        "circle" => Kind::Ellipse { color_rgb: rgb },
        "line" => {
            let start = pair(required(node, "start")?)?;
            let end = pair(required(node, "end")?)?;
            let head = node.attribute("head").unwrap_or("none");
            let tail = node.attribute("tail").unwrap_or("none");
            if !matches!(head, "none" | "None")
                || !matches!(tail, "none" | "None" | "open" | "OpenArrow")
            {
                return Err("XFDF arrowhead style is not supported".into());
            }
            if matches!(tail, "open" | "OpenArrow") {
                let wings = arrow_wings(start, end).ok_or("XFDF arrow is too short")?;
                let x0 = bounds.x.min(wings[0][0]).min(wings[1][0]);
                let y0 = bounds.y.min(wings[0][1]).min(wings[1][1]);
                let x1 = (bounds.x + bounds.width).max(wings[0][0]).max(wings[1][0]);
                let y1 = (bounds.y + bounds.height).max(wings[0][1]).max(wings[1][1]);
                bounds = Rect {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                };
                Kind::Arrow {
                    start,
                    end,
                    color_rgb: rgb,
                }
            } else {
                Kind::Line {
                    start,
                    end,
                    color_rgb: rgb,
                }
            }
        }
        "freetext" => {
            let value = node
                .children()
                .find(|child| child.is_element() && child.tag_name().name() == "contents")
                .and_then(|child| child.text())
                .unwrap_or_default()
                .to_owned();
            if value.len() > 4096 {
                return Err("XFDF text exceeds the native import limit".into());
            }
            let font_size = node
                .attribute("fontsize")
                .unwrap_or("14")
                .parse::<u16>()
                .map_err(|_| "Invalid XFDF text size")?;
            if !TEXT_FONT_SIZES.contains(&font_size) {
                return Err("XFDF text size is not supported".into());
            }
            Kind::Text {
                value,
                color_rgb: rgb,
                font_size,
                align: TextAlign::Left,
            }
        }
        "ink" => {
            let gesture = node
                .descendants()
                .find(|child| child.is_element() && child.tag_name().name() == "gesture")
                .and_then(|child| child.text())
                .ok_or("Missing XFDF ink gesture")?;
            let points = gesture
                .split(';')
                .map(pair)
                .collect::<Result<Vec<_>, _>>()?;
            if !(2..=512).contains(&points.len()) {
                return Err("XFDF ink point count is outside the native limit".into());
            }
            Kind::Polyline {
                points,
                color_rgb: rgb,
            }
        }
        _ => {
            return Err(format!(
                "Unsupported XFDF annotation: {}",
                node.tag_name().name()
            ))
        }
    };
    Ok(Annotation {
        id,
        page,
        bounds,
        kind,
    })
}

pub(crate) fn parse(xml: &str, pages: usize, first_id: u64) -> Result<Vec<Annotation>, String> {
    if xml.len() as u64 > MAX_XFDF_BYTES {
        return Err("XFDF exceeds the 4 MiB import limit".into());
    }
    if xml.contains("<!DOCTYPE") || xml.contains("<!ENTITY") {
        return Err("XFDF document types and entities are not supported".into());
    }
    if xml.bytes().filter(|byte| *byte == b'<').count() > 32_768 {
        return Err("XFDF has too many XML elements".into());
    }
    let tree = roxmltree::Document::parse(xml).map_err(|error| format!("Invalid XFDF: {error}"))?;
    if tree.root_element().tag_name().name() != "xfdf" {
        return Err("XFDF root element is missing".into());
    }
    let annots = tree
        .root_element()
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "annots")
        .ok_or("XFDF annotation list is missing")?;
    let mut items = Vec::new();
    for node in annots.children().filter(|node| node.is_element()) {
        if items.len() == MAX_ITEMS {
            return Err("XFDF exceeds the 4096-annotation native limit".into());
        }
        let id = first_id
            .checked_add(items.len() as u64)
            .ok_or("XFDF annotation IDs exhausted")?;
        items.push(parse_item(node, pages, id)?);
    }
    if items.is_empty() {
        return Err("XFDF has no supported annotations".into());
    }
    Ok(items)
}

pub(crate) fn read(path: &Path, pages: usize, first_id: u64) -> Result<Vec<Annotation>, String> {
    if path.extension().and_then(|value| value.to_str()) != Some("xfdf") {
        return Err("Import path must end in .xfdf".into());
    }
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_XFDF_BYTES {
        return Err("XFDF exceeds the 4 MiB import limit".into());
    }
    let mut xml = String::new();
    use std::io::Read;
    file.take(MAX_XFDF_BYTES + 1)
        .read_to_string(&mut xml)
        .map_err(|error| error.to_string())?;
    parse(&xml, pages, first_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tauri_shape_and_ink_geometry_with_zero_based_pages() {
        let xml = r##"<?xml version="1.0"?><xfdf xmlns="http://ns.adobe.com/xfdf/"><annots>
          <square page="0" rect="10,20,50,60" color="#ff9800" width="2"><contents>box</contents></square>
          <line page="1" rect="2,3,40,50" start="2,3" end="40,50" color="#102030" tail="open"/>
          <ink page="1" rect="1,1,5,5" color="#ffffff"><inklist><gesture>1,1;3,4;5,5</gesture></inklist></ink>
        </annots></xfdf>"##;
        let items = parse(xml, 2, 71).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, 71);
        assert_eq!(items[1].page, 1);
        assert!(matches!(items[1].kind, Kind::Arrow { .. }));
        assert!(matches!(&items[2].kind, Kind::Polyline { points, .. } if points.len() == 3));
    }

    #[test]
    fn rejects_unsupported_content_without_returning_partial_items() {
        let xml = r##"<xfdf><annots><square page="0" rect="0,0,10,10" color="#000000"/>
          <stamp page="0" rect="0,0,10,10"/></annots></xfdf>"##;
        assert!(parse(xml, 1, 1).is_err());
        let xml = r##"<xfdf><annots><square page="1" rect="0,0,10,10" color="#000000"/></annots></xfdf>"##;
        assert!(parse(xml, 1, 1).is_err());
        let xml = r##"<xfdf><annots><square page="0" rect="0,0,10,10" interior-color="#ff0000"/></annots></xfdf>"##;
        assert!(parse(xml, 1, 1).is_err());
    }
}
