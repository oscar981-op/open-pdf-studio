//! Experimental, bounded PDF→OPGM-v3 fills and axis-aligned strokes.
use super::{
    fail,
    geometry_v1::{integer, load_page, rectangle_to_display},
    operands,
};
use std::{error::Error, path::Path};

pub fn page_to_opgm_v3(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let page = load_page(path)?;
    let ops = &page.operations;
    let mut records = Vec::new();
    let mut i = 0;
    while i < ops.len() {
        if records.len() / 24 >= 64 {
            return Err(fail("too many OPGM-v3 commands"));
        }
        let (kind, rect, ink, consumed) = match ops[i].operator.as_str() {
            "rg" => {
                if i + 2 >= ops.len()
                    || ops[i + 1].operator != "re"
                    || ops[i + 2].operator != "f"
                    || !ops[i + 2].operands.is_empty()
                {
                    return Err(fail("incomplete fill"));
                }
                let ink = binary_ink(operands::<3>(&ops[i].operands)?)?;
                let [x, y, w, h] = operands::<4>(&ops[i + 1].operands)?;
                let rect = rectangle_to_display(
                    page.crop,
                    page.turn,
                    integer(x)?,
                    integer(y)?,
                    integer(w)?,
                    integer(h)?,
                )?;
                (1, rect, ink, 3)
            }
            "RG" => {
                if i + 4 >= ops.len()
                    || ops[i + 1].operator != "w"
                    || ops[i + 2].operator != "m"
                    || ops[i + 3].operator != "l"
                    || ops[i + 4].operator != "S"
                    || !ops[i + 4].operands.is_empty()
                {
                    return Err(fail("incomplete stroke"));
                }
                let ink = binary_ink(operands::<3>(&ops[i].operands)?)?;
                if operands::<1>(&ops[i + 1].operands)? != [1.0] {
                    return Err(fail("only 1pt stroke"));
                }
                let [x0, y0] = operands::<2>(&ops[i + 2].operands)?;
                let [x1, y1] = operands::<2>(&ops[i + 3].operands)?;
                let (x, y, w, h) = if y0 == y1 && x0 != x1 {
                    (
                        integer(x0.min(x1))?,
                        integer(y0 - 0.5)?,
                        integer((x1 - x0).abs())?,
                        1,
                    )
                } else if x0 == x1 && y0 != y1 {
                    (
                        integer(x0 - 0.5)?,
                        integer(y0.min(y1))?,
                        1,
                        integer((y1 - y0).abs())?,
                    )
                } else {
                    return Err(fail("only pixel-aligned axis stroke"));
                };
                let rect = rectangle_to_display(page.crop, page.turn, x, y, w, h)?;
                (3, rect, ink, 5)
            }
            _ => return Err(fail("unsupported OPGM-v3 operator")),
        };
        for value in [kind, rect[0], rect[1], rect[2], rect[3]] {
            records.extend_from_slice(&value.to_le_bytes());
        }
        records.extend_from_slice(&ink);
        i += consumed;
    }
    if records.is_empty() {
        return Err(fail("empty OPGM-v3 page"));
    }
    let mut out = Vec::with_capacity(40 + records.len());
    out.extend_from_slice(b"OPGM");
    for value in [
        3,
        page.media[2],
        page.media[3],
        page.crop[0],
        page.crop[1],
        page.crop[2],
        page.crop[3],
        page.turn,
        (records.len() / 24) as u32,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&records);
    Ok(out)
}

fn binary_ink(rgb: [f64; 3]) -> Result<[u8; 4], Box<dyn Error>> {
    let mut ink = [0, 0, 0, 255];
    for (dst, src) in ink.iter_mut().take(3).zip(rgb) {
        *dst = match src {
            0.0 => 0,
            1.0 => 255,
            _ => return Err(fail("only binary RGB ink")),
        };
    }
    Ok(ink)
}
