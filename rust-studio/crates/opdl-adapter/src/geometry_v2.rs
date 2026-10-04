//! Bounded multi-rectangle PDF→OPGM-v2 fixture export; not an app route.
use super::{
    fail,
    geometry_v1::{integer, load_page, rectangle_to_display},
    operands,
};
use std::{error::Error, path::Path};

pub fn page_to_opgm_v2(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let page = load_page(path)?;
    let operations = &page.operations;
    if operations.is_empty() || operations.len() % 3 != 0 || operations.len() / 3 > 64 {
        return Err(fail("OPGM-v2 requires 1..64 complete rectangle fills"));
    }
    let mut records = Vec::with_capacity(operations.len() / 3 * 24);
    for group in operations.chunks_exact(3) {
        if group[0].operator != "rg"
            || group[1].operator != "re"
            || group[2].operator != "f"
            || !group[2].operands.is_empty()
        {
            return Err(fail("OPGM-v2 requires rg/re/f triples"));
        }
        let rgb = operands::<3>(&group[0].operands)?;
        let mut ink = [0_u8; 3];
        for (channel, value) in ink.iter_mut().zip(rgb) {
            *channel = match value {
                0.0 => 0,
                1.0 => 255,
                _ => return Err(fail("OPGM-v2 accepts only binary RGB channels")),
            };
        }
        let [x, y, w, h] = operands::<4>(&group[1].operands)?;
        let rect = rectangle_to_display(
            page.crop,
            page.turn,
            integer(x)?,
            integer(y)?,
            integer(w)?,
            integer(h)?,
        )?;
        for value in [1, rect[0], rect[1], rect[2], rect[3]] {
            records.extend_from_slice(&value.to_le_bytes());
        }
        records.extend_from_slice(&[ink[0], ink[1], ink[2], 255]);
    }
    let mut output = Vec::with_capacity(40 + records.len());
    output.extend_from_slice(b"OPGM");
    for value in [
        2,
        page.media[2],
        page.media[3],
        page.crop[0],
        page.crop[1],
        page.crop[2],
        page.crop[3],
        page.turn,
        (records.len() / 24) as u32,
    ] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend_from_slice(&records);
    Ok(output)
}
