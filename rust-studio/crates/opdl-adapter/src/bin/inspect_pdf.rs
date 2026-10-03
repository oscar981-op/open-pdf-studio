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

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: inspect_pdf INPUT.pdf [--page N]")?;
    let selected = match (args.next(), args.next(), args.next()) {
        (None, None, None) => None,
        (Some(flag), Some(value), None) if flag == "--page" => {
            Some(value.to_string_lossy().parse::<u32>()?)
        }
        _ => return Err("usage: inspect_pdf INPUT.pdf [--page N]".into()),
    };
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
    }
    Ok(())
}
