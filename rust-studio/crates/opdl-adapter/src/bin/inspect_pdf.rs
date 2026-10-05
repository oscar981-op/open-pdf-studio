//! Read-only inventory of PDF page operators for planning the DynLex subset.
#![forbid(unsafe_code)]

use lopdf::{content::Content, Document, Object, ObjectId};
use std::{collections::BTreeMap, env, error::Error, path::Path};

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

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: inspect_pdf INPUT.pdf [--page N] [--image-ops]")?;
    let mut selected = None;
    let mut image_ops = false;
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
            _ => return Err("usage: inspect_pdf INPUT.pdf [--page N] [--image-ops]".into()),
        }
    }
    let document = Document::load(Path::new(&input))?;
    let pages = document.get_pages();
    if let Some(number) = selected {
        if !pages.contains_key(&number) {
            return Err("page number outside document".into());
        }
    }
    println!("pages={}", pages.len());
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
