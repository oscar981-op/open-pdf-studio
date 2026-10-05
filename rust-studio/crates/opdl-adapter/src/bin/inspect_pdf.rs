//! Read-only inventory of PDF page operators for planning the DynLex subset.
#![forbid(unsafe_code)]

use lopdf::{content::Content, Document, Object, ObjectId};
use std::{collections::BTreeMap, env, error::Error, io::Write, path::Path};

fn inherited<'a>(document: &'a Document, mut page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        let dictionary = document.get_dictionary(page).ok()?;
        if let Ok(value) = dictionary.get(key) {
            return Some(value);
        }
        page = dictionary.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn pdf_number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

fn describe(document: &Document, object: &Object) -> String {
    describe_bounded(document, object, 0)
}

fn describe_bounded(document: &Document, object: &Object, depth: usize) -> String {
    if depth >= 4 {
        return "...".into();
    }
    let Ok((_, object)) = document.dereference(object) else {
        return "unresolved".into();
    };
    match object {
        Object::Name(name) => String::from_utf8_lossy(name).into_owned(),
        Object::Integer(value) => value.to_string(),
        Object::Real(value) => value.to_string(),
        Object::Array(items) => {
            let parts = items
                .iter()
                .take(3)
                .map(|item| describe_bounded(document, item, depth + 1))
                .collect::<Vec<_>>();
            format!(
                "[{}{}]",
                parts.join(","),
                if items.len() > 3 { ",..." } else { "" }
            )
        }
        Object::Dictionary(dict) => {
            let parts = dict
                .iter()
                .take(16)
                .map(|(key, value)| {
                    format!(
                        "{}={}",
                        String::from_utf8_lossy(key),
                        describe_bounded(document, value, depth + 1)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(","))
        }
        Object::Null => "null".into(),
        _ => "other".into(),
    }
}

fn contour_summary(operations: &[lopdf::content::Operation]) -> String {
    let mut contours = Vec::<Vec<[f64; 2]>>::new();
    let mut current = Vec::<[f64; 2]>::new();
    let mut other_segments = 0_usize;
    for operation in operations {
        match operation.operator.as_str() {
            "m" if operation.operands.len() == 2 => {
                if !current.is_empty() {
                    contours.push(std::mem::take(&mut current));
                }
                if let (Some(x), Some(y)) = (
                    pdf_number(&operation.operands[0]),
                    pdf_number(&operation.operands[1]),
                ) {
                    current.push([x, y]);
                }
            }
            "l" if operation.operands.len() == 2 => {
                if let (Some(x), Some(y)) = (
                    pdf_number(&operation.operands[0]),
                    pdf_number(&operation.operands[1]),
                ) {
                    current.push([x, y]);
                }
            }
            "h" => {
                if !current.is_empty() {
                    contours.push(std::mem::take(&mut current));
                }
            }
            "c" | "v" | "y" | "re" => other_segments += 1,
            _ => {}
        }
    }
    if !current.is_empty() {
        contours.push(current);
    }
    let details = contours
        .iter()
        .take(8)
        .map(|points| {
            let signed_area2: f64 = points
                .iter()
                .enumerate()
                .map(|(index, a)| {
                    let b = points[(index + 1) % points.len()];
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum();
            format!("{}pts/area2={signed_area2:.1}", points.len())
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "contours={} [{}] other_segments={other_segments}",
        contours.len(),
        details
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or(
        "usage: inspect_pdf INPUT.pdf [--page N] [--image-ops] [--clip-ops] [--head N] [--around INDEX] [--dump-clip INDEX]",
    )?;
    let mut selected = None;
    let mut image_ops = false;
    let mut clip_ops = false;
    let mut head = 0_usize;
    let mut around = None;
    let mut dump_clip = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--page") if selected.is_none() => {
                selected = Some(
                    args.next()
                        .ok_or("--page requires a number")?
                        .to_string_lossy()
                        .parse::<u32>()?,
                );
            }
            Some("--image-ops") if !image_ops => image_ops = true,
            Some("--clip-ops") if !clip_ops => clip_ops = true,
            Some("--head") if head == 0 => {
                head = args
                    .next()
                    .ok_or("--head requires a number")?
                    .to_string_lossy()
                    .parse()?;
                if head > 128 {
                    return Err("--head is limited to 128 operators".into());
                }
            }
            Some("--around") if around.is_none() => {
                around = Some(
                    args.next()
                        .ok_or("--around requires an index")?
                        .to_string_lossy()
                        .parse::<usize>()?,
                );
            }
            Some("--dump-clip") if dump_clip.is_none() => {
                dump_clip = Some(
                    args.next()
                        .ok_or("--dump-clip requires an index")?
                        .to_string_lossy()
                        .parse::<usize>()?,
                );
            }
            _ => return Err(
                "usage: inspect_pdf INPUT.pdf [--page N] [--image-ops] [--clip-ops] [--head N] [--around INDEX] [--dump-clip INDEX]"
                    .into(),
            ),
        }
    }
    let document = Document::load(Path::new(&input))?;
    if dump_clip.is_some()
        && (selected.is_none() || image_ops || clip_ops || head > 0 || around.is_some())
    {
        return Err("--dump-clip needs --page and no other display options".into());
    }
    let pages = document.get_pages();
    if let Some(number) = selected {
        if !pages.contains_key(&number) {
            return Err("page number outside document".into());
        }
    }
    if dump_clip.is_none() {
        println!("pages={}", pages.len());
    }
    for (number, id) in pages {
        if selected.is_some_and(|selected| selected != number) {
            continue;
        }
        let media = inherited(&document, id, b"MediaBox")
            .and_then(|entry| document.dereference(entry).ok())
            .and_then(|(_, entry)| entry.as_array().ok())
            .and_then(|array| {
                (array.len() == 4)
                    .then(|| array.iter().map(pdf_number).collect::<Option<Vec<_>>>())
                    .flatten()
            });
        let rotate = inherited(&document, id, b"Rotate")
            .and_then(|entry| document.dereference(entry).ok())
            .and_then(|(_, entry)| entry.as_i64().ok())
            .unwrap_or(0);
        let stream = document.get_page_content(id)?;
        let content = Content::decode(&stream)?;
        if let Some(index) = dump_clip {
            let clip = content
                .operations
                .get(index)
                .ok_or("clip operator index outside page")?;
            if !matches!(clip.operator.as_str(), "W" | "W*")
                || content
                    .operations
                    .get(index + 1)
                    .is_none_or(|next| next.operator != "n")
            {
                return Err("index must name a W/W* followed by n".into());
            }
            let start = content.operations[..index]
                .iter()
                .rposition(|previous| {
                    matches!(
                        previous.operator.as_str(),
                        "n" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                    )
                })
                .map_or(0, |previous| previous + 1);
            let mut selected_ops = Vec::new();
            for operation in &content.operations[start..index] {
                if matches!(operation.operator.as_str(), "m" | "l" | "h") {
                    selected_ops.push(operation.clone());
                } else if !selected_ops.is_empty() {
                    return Err("clip path contains an interleaved graphics operation".into());
                }
            }
            if selected_ops.is_empty() || selected_ops[0].operator != "m" {
                return Err("clip path has no initial move".into());
            }
            selected_ops.push(clip.clone());
            selected_ops.push(content.operations[index + 1].clone());
            let bytes = Content {
                operations: selected_ops,
            }
            .encode()?;
            std::io::stdout().write_all(&bytes)?;
            std::io::stdout().write_all(b"\n")?;
            return Ok(());
        }
        let mut counts = BTreeMap::<String, usize>::new();
        for operation in &content.operations {
            *counts.entry(operation.operator.clone()).or_default() += 1;
        }
        let mut frequent: Vec<_> = counts.into_iter().collect();
        frequent.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let leaders = frequent
            .iter()
            .take(16)
            .map(|(name, count)| format!("{name}:{count}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "page={number} media={media:?} rotate={rotate} stream_bytes={} operators={} top={leaders}",
            stream.len(), content.operations.len()
        );
        for (index, operation) in content.operations.iter().take(head).enumerate() {
            let operands = operation
                .operands
                .iter()
                .map(|value| describe(&document, value))
                .collect::<Vec<_>>();
            println!("op={index} {} {}", operands.join(" "), operation.operator);
        }
        if let Some(center) = around {
            for index in
                center.saturating_sub(8)..center.saturating_add(9).min(content.operations.len())
            {
                let operation = &content.operations[index];
                let operands = operation
                    .operands
                    .iter()
                    .map(|value| describe(&document, value))
                    .collect::<Vec<_>>();
                println!("op={index} {} {}", operands.join(" "), operation.operator);
            }
        }
        if clip_ops {
            for (index, operation) in content.operations.iter().enumerate() {
                if !matches!(operation.operator.as_str(), "W" | "W*") {
                    continue;
                }
                let mut path_counts = BTreeMap::<String, usize>::new();
                for previous in content.operations[..index].iter().rev() {
                    match previous.operator.as_str() {
                        "m" | "l" | "c" | "v" | "y" | "h" | "re" => {
                            *path_counts.entry(previous.operator.clone()).or_default() += 1;
                        }
                        "n" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => break,
                        _ => {}
                    }
                }
                let counts = path_counts
                    .iter()
                    .map(|(name, count)| format!("{name}:{count}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let nearby = content.operations[index.saturating_sub(5)..index]
                    .iter()
                    .map(|op| op.operator.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let path_start = content.operations[..index]
                    .iter()
                    .rposition(|previous| {
                        matches!(
                            previous.operator.as_str(),
                            "n" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                        )
                    })
                    .map_or(0, |previous| previous + 1);
                println!(
                    "clip_op={index} rule={} path={counts} recent={nearby} {}",
                    operation.operator,
                    contour_summary(&content.operations[path_start..index])
                );
            }
        }
        if image_ops {
            let (resources, _) = document.get_page_resources(id)?;
            if let Some(resources) = resources {
                if let Ok(xobjects) = resources.get(b"XObject") {
                    let (_, xobjects) = document.dereference(xobjects)?;
                    for (name, reference) in xobjects.as_dict()?.iter() {
                        let (_, object) = document.dereference(reference)?;
                        let Ok(image) = object.as_stream() else {
                            continue;
                        };
                        if image.dict.get(b"Subtype").and_then(Object::as_name).ok()
                            != Some(b"Image")
                        {
                            continue;
                        }
                        let label = |key: &[u8]| -> String {
                            image
                                .dict
                                .get(key)
                                .map(|value| describe(&document, value))
                                .unwrap_or_else(|_| "-".into())
                        };
                        println!("image={} size={}x{} bpc={} colorspace={} filter={} decode_parms={} mask={}",
                            String::from_utf8_lossy(name), label(b"Width"), label(b"Height"), label(b"BitsPerComponent"),
                            label(b"ColorSpace"), label(b"Filter"), label(b"DecodeParms"),
                            image.dict.has(b"Mask") || image.dict.has(b"SMask"));
                        if let Ok((_, Object::Array(array))) = image
                            .dict
                            .get(b"ColorSpace")
                            .and_then(|value| document.dereference(value))
                        {
                            let [indexed, base, high, lookup] = array.as_slice() else {
                                continue;
                            };
                            if indexed.as_name().ok() == Some(b"Indexed") {
                                let lookup = document.dereference(lookup)?.1;
                                let (kind, bytes) = match lookup {
                                    Object::String(bytes, _) => ("string", bytes.len()),
                                    Object::Stream(stream) => ("stream", stream.content.len()),
                                    _ => ("unsupported", 0),
                                };
                                println!(
                                    "indexed_image={} base={} high={} lookup={} bytes={}",
                                    String::from_utf8_lossy(name),
                                    describe(&document, base),
                                    describe(&document, high),
                                    kind,
                                    bytes
                                );
                            }
                        }
                    }
                }
            }
            for (index, operation) in content.operations.iter().enumerate() {
                if operation.operator == "cm" {
                    let values = operation
                        .operands
                        .iter()
                        .map(pdf_number)
                        .collect::<Option<Vec<_>>>();
                    println!("op={index} cm={values:?}");
                } else if operation.operator == "Do" {
                    let name = operation
                        .operands
                        .first()
                        .and_then(|object| object.as_name().ok());
                    println!(
                        "op={index} Do={}",
                        name.map(String::from_utf8_lossy)
                            .map(|name| name.into_owned())
                            .unwrap_or_else(|| "?".into())
                    );
                }
            }
        }
    }
    Ok(())
}
