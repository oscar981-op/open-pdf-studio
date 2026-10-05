//! Isolated PDF→OPGM-v1 fixture export. This is not a general PDF parser.
use super::{fail, has_compound_operand, number, operands};
use lopdf::{
    content::{Content, Operation},
    Document, Object,
};
use std::{collections::HashSet, error::Error, path::Path};

pub(crate) fn integer(value: f64) -> Result<u32, Box<dyn Error>> {
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=4096.0).contains(&value) {
        return Err(fail("OPGM requires integer coordinates within 4096"));
    }
    Ok(value as u32)
}

fn box_value(value: &Object) -> Result<[u32; 4], Box<dyn Error>> {
    let raw = operands::<4>(value.as_array()?)?;
    Ok([
        integer(raw[0])?,
        integer(raw[1])?,
        integer(raw[2])?,
        integer(raw[3])?,
    ])
}

pub(crate) struct PageFixture {
    pub media: [u32; 4],
    pub crop: [u32; 4],
    pub turn: u32,
    pub operations: Vec<Operation>,
}

pub(crate) fn load_page(path: &Path) -> Result<PageFixture, Box<dyn Error>> {
    let source = std::fs::read(path)?;
    load_page_bytes(&source)
}

pub(crate) fn load_page_bytes(source: &[u8]) -> Result<PageFixture, Box<dyn Error>> {
    if source.len() > 16_000_000 {
        return Err(fail("PDF fixture is too large"));
    }
    let doc = Document::load_mem(&source)?;
    let pages = doc.get_pages();
    if pages.len() != 1 {
        return Err(fail("OPGM requires one page"));
    }
    let page_id = *pages.values().next().ok_or_else(|| fail("missing page"))?;
    let page = doc.get_dictionary(page_id)?;
    if page.has(b"Annots") || page.has(b"UserUnit") {
        return Err(fail("unsupported page state"));
    }
    let resources = page.get(b"Resources")?.as_dict()?;
    if !resources.is_empty() {
        return Err(fail("OPGM requires empty resources"));
    }
    let mut media = None;
    let mut crop = None;
    let mut rotation = None;
    let mut current = Some(page_id);
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id) || seen.len() > 16 {
            return Err(fail("cyclic or deep page tree"));
        }
        let node = doc.get_dictionary(id)?;
        if node.has(b"UserUnit") {
            return Err(fail("unsupported inherited UserUnit"));
        }
        if media.is_none() {
            media = node.get(b"MediaBox").ok().map(box_value).transpose()?;
        }
        if crop.is_none() {
            crop = node.get(b"CropBox").ok().map(box_value).transpose()?;
        }
        if rotation.is_none() {
            rotation = node
                .get(b"Rotate")
                .ok()
                .map(|value| integer(number(value)?))
                .transpose()?;
        }
        current = node
            .get(b"Parent")
            .ok()
            .map(Object::as_reference)
            .transpose()?;
    }
    let media = media.ok_or_else(|| fail("missing MediaBox"))?;
    let crop = crop.unwrap_or(media);
    let turn = rotation.unwrap_or(0);
    if media[0] != 0
        || media[1] != 0
        || media[2] == 0
        || media[3] == 0
        || crop[0] >= crop[2]
        || crop[1] >= crop[3]
        || crop[2] > media[2]
        || crop[3] > media[3]
        || !matches!(turn, 0 | 90 | 180 | 270)
    {
        return Err(fail("unsupported page geometry"));
    }
    let width = crop[2] - crop[0];
    let height = crop[3] - crop[1];
    if width
        .checked_mul(height)
        .is_none_or(|pixels| pixels > 6_000_000)
    {
        return Err(fail("OPGM page exceeds pixel bound"));
    }
    let bytes = doc.get_page_content(page_id)?;
    if bytes.len() > 2_000_000 || has_compound_operand(&bytes) {
        return Err(fail("unsupported PDF content syntax"));
    }
    let operations = Content::decode(&bytes)?.operations;
    Ok(PageFixture {
        media,
        crop,
        turn,
        operations,
    })
}

pub(crate) fn rectangle_to_display(
    crop: [u32; 4],
    turn: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Result<[u32; 4], Box<dyn Error>> {
    let right = x.checked_add(w).ok_or_else(|| fail("rectangle overflow"))?;
    let top = y.checked_add(h).ok_or_else(|| fail("rectangle overflow"))?;
    if w == 0 || h == 0 || x < crop[0] || y < crop[1] || right > crop[2] || top > crop[3] {
        return Err(fail("rectangle exceeds visible CropBox"));
    }
    Ok(match turn {
        0 => [x - crop[0], crop[3] - top, right - crop[0], crop[3] - y],
        90 => [y - crop[1], x - crop[0], top - crop[1], right - crop[0]],
        180 => [crop[2] - right, y - crop[1], crop[2] - x, top - crop[1]],
        270 => [crop[3] - top, crop[2] - right, crop[3] - y, crop[2] - x],
        _ => unreachable!(),
    })
}

pub fn page_to_opgm(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let PageFixture {
        media,
        crop,
        turn,
        operations,
    } = load_page(path)?;
    if operations.len() != 3
        || operations[0].operator != "rg"
        || operations[1].operator != "re"
        || operations[2].operator != "f"
        || !operations[2].operands.is_empty()
        || operands::<3>(&operations[0].operands)? != [1.0, 0.0, 0.0]
    {
        return Err(fail("OPGM requires one explicit red rectangle fill"));
    }
    let [x, y, w, h] = operands::<4>(&operations[1].operands)?;
    let [x, y, w, h] = [integer(x)?, integer(y)?, integer(w)?, integer(h)?];
    let rect = rectangle_to_display(crop, turn, x, y, w, h)?;
    let mut output = Vec::with_capacity(56);
    output.extend_from_slice(b"OPGM");
    for value in [
        1, media[2], media[3], crop[0], crop[1], crop[2], crop[3], turn, rect[0], rect[1], rect[2],
        rect[3],
    ] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend_from_slice(&[255, 0, 0, 255]);
    Ok(output)
}
