//! Research-only fractional JPEG placement. Never selected by native routing.
//! Its lopdf frontend is for trusted isolated fixtures, not untrusted PDFs.
use lopdf::{content::Content, Document, Object};
use std::{error::Error, fs, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn num(o: &Object) -> Result<f64> {
    let v = match o {
        Object::Integer(n) => *n as f64,
        Object::Real(n) => f64::from(*n),
        _ => return Err("non-numeric operand".into()),
    };
    if !v.is_finite() || v.abs() > 16_384. {
        return Err("unbounded operand".into());
    }
    Ok(v)
}

fn numbers<const N: usize>(operands: &[Object]) -> Result<[f64; N]> {
    if operands.len() != N {
        return Err("wrong operand count".into());
    }
    let mut result = [0.; N];
    for (to, from) in result.iter_mut().zip(operands) {
        *to = num(from)?;
    }
    Ok(result)
}

fn image_info(doc: &Document) -> Result<([f64; 4], [f64; 4], [f64; 6], Vec<u8>, [u32; 2])> {
    let pages = doc.get_pages();
    if pages.len() != 1 {
        return Err("one-page research fixture required".into());
    }
    let id = *pages.values().next().ok_or("missing page")?;
    let page = doc.get_dictionary(id)?;
    // This research renderer has no rotated/CropBox coordinate mapping or
    // annotation/group compositing. Reject inherited state as well as direct
    // state, rather than emitting a plausible but incorrectly oriented page.
    let mut ancestor = Some(id);
    for _ in 0..8 {
        let Some(node_id) = ancestor else { break };
        let node = doc.get_dictionary(node_id)?;
        if [
            b"Rotate".as_slice(),
            b"CropBox",
            b"UserUnit",
            b"Group",
            b"Annots",
        ]
        .iter()
        .any(|key| node.has(key))
        {
            return Err("unsupported page state in image research probe".into());
        }
        ancestor = match node.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            Err(_) => None,
            _ => return Err("malformed page ancestry".into()),
        };
    }
    if ancestor.is_some() {
        return Err("deep or cyclic page ancestry".into());
    }
    let box_obj = page.get(b"MediaBox")?.as_array()?;
    let media = numbers::<4>(box_obj)?;
    if media[0] != 0. || media[1] != 0. || media[2] <= 0. || media[3] <= 0. {
        return Err("unsupported research page box".into());
    }
    let content = Content::decode(&doc.get_page_content(id)?)?;
    let names: Vec<&str> = content
        .operations
        .iter()
        .map(|op| op.operator.as_str())
        .collect();
    if names != ["q", "re", "W*", "n", "q", "cm", "Do", "Q", "Q"] {
        return Err("unexpected image-only grammar".into());
    }
    let rect = numbers::<4>(&content.operations[1].operands)?;
    let matrix = numbers::<6>(&content.operations[5].operands)?;
    if matrix[1] != 0.
        || matrix[2] != 0.
        || matrix[0] <= 0.
        || matrix[3] <= 0.
        || rect[2] <= 0.
        || rect[3] <= 0.
    {
        return Err("unsupported image transform or clip".into());
    }
    let name = content.operations[6].operands.as_slice();
    let [Object::Name(name)] = name else {
        return Err("missing image name".into());
    };
    let (_, resources) = doc.dereference(page.get(b"Resources")?)?;
    let (_, xobjects) = doc.dereference(resources.as_dict()?.get(b"XObject")?)?;
    let (_, image) = doc.dereference(xobjects.as_dict()?.get(name)?)?;
    let image = image.as_stream()?;
    if image.dict.get(b"Subtype")?.as_name()? != b"Image"
        || image.dict.get(b"ColorSpace")?.as_name()? != b"DeviceRGB"
        || image.dict.get(b"BitsPerComponent")?.as_i64()? != 8
        || image.dict.get(b"Filter")?.as_name()? != b"DCTDecode"
    {
        return Err("unsupported image dictionary".into());
    }
    let source = [
        u32::try_from(image.dict.get(b"Width")?.as_i64()?)?,
        u32::try_from(image.dict.get(b"Height")?.as_i64()?)?,
    ];
    if source.contains(&0)
        || source.iter().any(|&v| v > 8192)
        || source[0]
            .checked_mul(source[1])
            .is_none_or(|n| n > 40_000_000)
        || image.content.len() > 16_000_000
    {
        return Err("JPEG source bound".into());
    }
    Ok((media, rect, matrix, image.content.clone(), source))
}

fn render(input: &Path, output: &Path, scale: f32) -> Result<()> {
    let bytes = fs::read(input)?;
    if bytes.len() > 16_000_000 {
        return Err("PDF byte limit".into());
    }
    let doc = Document::load_mem(&bytes)?;
    let (media, rect, matrix, jpeg, source) = image_info(&doc)?;
    let page = opds_opdl_adapter::partial_image::render(&jpeg, source, media, rect, matrix, scale)?;
    fs::write(output, page.rgba)?;
    eprintln!(
        "{}x{} intermediate={}x{}",
        page.width, page.height, page.intermediate[0], page.intermediate[1]
    );
    Ok(())
}

fn render_cached_sequence(input: &Path, directory: &Path, scales: &str) -> Result<()> {
    let bytes = fs::read(input)?;
    if bytes.len() > 16_000_000 {
        return Err("PDF byte limit".into());
    }
    let doc = Document::load_mem(&bytes)?;
    let (media, rect, matrix, jpeg, source) = image_info(&doc)?;
    let decoded = opds_opdl_adapter::partial_image::decode_rgb_research(&jpeg, source)?;
    fs::create_dir_all(directory)?;
    let mut timings = String::new();
    for (index, value) in scales.split(',').enumerate() {
        let scale: f32 = value.parse()?;
        let started = std::time::Instant::now();
        let page = opds_opdl_adapter::partial_image::render_predecoded_research(
            &decoded, source, media, rect, matrix, scale,
        )?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        fs::write(directory.join(format!("{index}.rgba")), page.rgba)?;
        timings.push_str(&format!("{index}\t{scale}\t{elapsed:.3}\n"));
    }
    fs::write(directory.join("timings.tsv"), timings)?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 5 && args[4] == "--cache-research" {
        return render_cached_sequence(Path::new(&args[1]), Path::new(&args[2]), &args[3]);
    }
    if args.len() != 4 {
        return Err("usage: partial_image_probe isolated.pdf output.rgba scale".into());
    }
    render(Path::new(&args[1]), Path::new(&args[2]), args[3].parse()?)
}
