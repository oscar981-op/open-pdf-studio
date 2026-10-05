//! Controlled test fixture using two real embedded Type0 fonts from a source PDF.
//! This does not alter the source document or participate in normal rendering.
#![forbid(unsafe_code)]
use lopdf::{Dictionary, Document, Stream};
use std::{env, error::Error, fs, io, path::Path};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: make_type0_run_fixture INPUT.pdf PAGE MODE OUTPUT.pdf")?;
    let page = args
        .next()
        .ok_or("missing page number")?
        .to_string_lossy()
        .parse::<u32>()?;
    let mode = args.next().ok_or("missing fixture geometry")?;
    let output = args.next().ok_or("missing output PDF")?;
    if args.next().is_some()
        || !matches!(
            mode.to_str(),
            Some(
                "normal"
                    | "rotated"
                    | "rotated-180"
                    | "rotated-270"
                    | "bad-rotate"
                    | "bad-crop"
                    | "user-unit"
                    | "bad-alpha"
            )
        )
    {
        return Err(invalid("unsupported fixture mode or extra arguments").into());
    }
    if Path::new(&input).canonicalize()? == Path::new(&output).canonicalize().unwrap_or_default() {
        return Err(invalid("output must not overwrite source PDF").into());
    }
    let bytes = fs::read(&input)?;
    if bytes.len() > 16_000_000 {
        return Err(invalid("source PDF exceeds test fixture limit").into());
    }
    let mut document = Document::load_mem(&bytes)?;
    let page_id = *document.get_pages().get(&page).ok_or("page missing")?;
    let font_names: Vec<_> = document
        .get_page_fonts(page_id)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    if !font_names.iter().any(|name| name == b"F1") || !font_names.iter().any(|name| name == b"F2")
    {
        return Err(invalid("fixture requires embedded F1 and F2 resources").into());
    }
    // Both CIDs occur in the technical drawing's TGR1 inventory. The
    // visible positions are deliberately adjacent so a font switch fits in
    // the bounded 256x192 experimental scene.
    let text = b"q 0 0 0 rg BT /F1 36 Tf 1 0 0 1 60 90 Tm <0025> Tj ET Q\nq 1 0 0 rg BT /F2 36 Tf 1 0 0 1 94 90 Tm <0035> Tj ET Q\n";
    let (media, crop, rotate, contents) = if mode == "rotated" || mode == "bad-rotate" {
        let mut transformed = b"q 0 1 -1 0 222 40 cm\n".to_vec();
        transformed.extend_from_slice(text);
        transformed.extend_from_slice(b"Q\n");
        (
            vec![0.into(), 0.into(), 300.into(), 350.into()],
            vec![30.into(), 40.into(), 222.into(), 296.into()],
            if mode == "bad-rotate" { 45 } else { 90 },
            transformed,
        )
    } else if mode == "rotated-270" {
        let mut transformed = b"q 0 -1 1 0 30 296 cm\n".to_vec();
        transformed.extend_from_slice(text);
        transformed.extend_from_slice(b"Q\n");
        (
            vec![0.into(), 0.into(), 300.into(), 350.into()],
            vec![30.into(), 40.into(), 222.into(), 296.into()],
            270,
            transformed,
        )
    } else if mode == "rotated-180" {
        let mut transformed = b"q -1 0 0 -1 256 192 cm\n".to_vec();
        transformed.extend_from_slice(text);
        transformed.extend_from_slice(b"Q\n");
        (
            vec![0.into(), 0.into(), 256.into(), 192.into()],
            vec![0.into(), 0.into(), 256.into(), 192.into()],
            180,
            transformed,
        )
    } else {
        (
            vec![0.into(), 0.into(), 256.into(), 192.into()],
            vec![0.into(), 0.into(), 256.into(), 192.into()],
            0,
            if mode == "bad-alpha" {
                let mut alpha = b"/BadAlpha gs\n".to_vec();
                alpha.extend_from_slice(text);
                alpha
            } else {
                text.to_vec()
            },
        )
    };
    if mode == "bad-alpha" {
        let (direct, inherited) = document.get_page_resources(page_id)?;
        let mut resources = direct
            .cloned()
            .or_else(|| {
                inherited
                    .first()
                    .and_then(|id| document.get_dictionary(*id).ok())
                    .cloned()
            })
            .ok_or_else(|| invalid("fixture lacks inherited font resources"))?;
        let mut alpha = Dictionary::new();
        alpha.set("Type", "ExtGState");
        alpha.set("ca", 0.5_f32);
        let mut states = Dictionary::new();
        states.set("BadAlpha", alpha);
        resources.set("ExtGState", states);
        document
            .get_object_mut(page_id)?
            .as_dict_mut()?
            .set("Resources", resources);
    }
    let stream = document.add_object(Stream::new(Dictionary::new(), contents));
    let dictionary = document.get_object_mut(page_id)?.as_dict_mut()?;
    dictionary.set("Contents", stream);
    dictionary.set("MediaBox", media);
    dictionary.set("CropBox", crop);
    dictionary.set("Rotate", rotate);
    if mode == "bad-crop" {
        dictionary.set("CropBox", vec![0.into(), 0.into(), 300.into(), 192.into()]);
    }
    if mode == "user-unit" {
        dictionary.set("UserUnit", 2);
    }
    document.save(output)?;
    Ok(())
}
