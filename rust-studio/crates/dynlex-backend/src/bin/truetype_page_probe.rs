//! Shared read-only text staging. Direct diagnostic invocations require trusted
//! research PDFs; native opds-text-worker is launched under separate OS limits.
//! Lopdf itself is not a fully budgeted untrusted-input parser.
#[cfg(feature = "kernel")]
#[path = "truetype_page_probe/type1_worker_glyph.rs"]
mod type1_worker_glyph;
#[cfg(feature = "kernel")]
mod probe {
    use super::type1_worker_glyph;
    use hayro_syntax::{content::UntypedIter, object::Object as Value};
    use lopdf::{Dictionary, Document, Object, Stream};
    use opds_dynlex_backend::{
        page_space::PageSpace, render_validated_glyph_scene_over_premultiplied_at_origin_owned,
        render_validated_polygon_over_premultiplied_at_origin_owned,
    };
    use opds_opdl_adapter::text_quad_glyph::{self as glyph, Source};
    use skrifa::raw::ps::type1::Type1Font;
    use std::{
        collections::{BTreeMap, HashSet},
        error::Error,
        io::{Read, Write},
        sync::Arc,
    };
    type Result<T> = std::result::Result<T, Box<dyn Error>>;
    fn fail<T>(s: &str) -> Result<T> {
        Err(s.to_string().into())
    }
    fn number(v: &Value<'_>) -> Result<f64> {
        let Value::Number(n) = v else {
            return fail("expected number");
        };
        let n = n.as_f64();
        if !n.is_finite() || n.abs() > 1_000_000. {
            return fail("number outside bound");
        };
        Ok(n)
    }
    fn ln(v: &Object) -> Result<f64> {
        match v {
            Object::Integer(n) => Ok(*n as f64),
            Object::Real(n) if n.is_finite() => Ok(f64::from(*n)),
            _ => fail("expected dictionary number"),
        }
    }
    // An ExtGState is inert for this frontend only when every supplied entry
    // leaves its supported opaque fill/text paints unchanged. In particular,
    // non-default alpha, blend modes and soft masks must never be ignored.
    fn neutral_ext_gstate(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<()> {
        let states = doc.dereference(resources.get(b"ExtGState")?)?.1.as_dict()?;
        if states.len() > 32 {
            return fail("graphics state resource count");
        }
        let state = doc.dereference(states.get(name)?)?.1.as_dict()?;
        if state.is_empty() || state.len() > 16 {
            return fail("unsupported graphics state");
        }
        for (key, value) in state.iter() {
            let value = doc.dereference(value)?.1;
            let valid = match key.as_slice() {
                b"Type" => value.as_name().ok() == Some(b"ExtGState"),
                b"CA" | b"ca" => ln(value).ok() == Some(1.),
                b"BM" => value.as_name().ok() == Some(b"Normal"),
                b"SMask" => value.as_name().ok() == Some(b"None"),
                b"AIS" | b"OP" | b"op" => matches!(value, Object::Boolean(false)),
                // Stroke adjustment cannot affect this frontend, which
                // rejects all stroke operators. OPM is inert with OP/op false.
                b"SA" => matches!(value, Object::Boolean(_)),
                b"OPM" => matches!(ln(value).ok(), Some(0. | 1.)),
                // Smoothness only affects gradient shading, which this
                // frontend rejects rather than painting.
                b"SM" => matches!(ln(value).ok(), Some(n) if (0.0..=1.0).contains(&n)),
                _ => false,
            };
            if !valid {
                return fail("unsupported non-neutral graphics state");
            }
        }
        Ok(())
    }
    fn invisible_link_annotations(doc: &Document, page: &Dictionary) -> Result<()> {
        let Ok(value) = page.get(b"Annots") else {
            return Ok(());
        };
        let entries = doc.dereference(value)?.1.as_array()?;
        if entries.len() > 64 {
            return fail("annotation count");
        }
        for entry in entries {
            let annotation = doc.dereference(entry)?.1.as_dict()?;
            if annotation.has(b"AP")
                || annotation.get(b"Subtype")?.as_name()? != b"Link"
                || annotation.iter().any(|(key, _)| {
                    ![
                        b"Type".as_slice(),
                        b"Subtype",
                        b"Rect",
                        b"Border",
                        b"BS",
                        b"C",
                        b"P",
                        b"A",
                        b"StructParent",
                        b"F",
                        b"NM",
                        b"M",
                        b"Contents",
                        b"H",
                        b"Dest",
                    ]
                    .contains(&key.as_slice())
                })
            {
                return fail("visible or unsupported annotation");
            }
            if annotation.has(b"Type") && annotation.get(b"Type")?.as_name()? != b"Annot" {
                return fail("annotation type");
            }
            let rect = annotation.get(b"Rect")?.as_array()?;
            if rect.len() != 4 {
                return fail("annotation rectangle");
            }
            for coordinate in rect {
                if ln(coordinate)?.abs() > 1_000_000. {
                    return fail("annotation rectangle");
                }
            }
            let mut zero_border = false;
            if let Ok(border) = annotation.get(b"Border") {
                let values = doc.dereference(border)?.1.as_array()?;
                if values.len() != 3 || values.iter().any(|v| ln(v).ok() != Some(0.)) {
                    return fail("visible annotation border");
                }
                zero_border = true;
            }
            if let Ok(style) = annotation.get(b"BS") {
                let style = doc.dereference(style)?.1.as_dict()?;
                if style
                    .iter()
                    .any(|(key, _)| ![b"Type".as_slice(), b"S", b"W"].contains(&key.as_slice()))
                    || style.get(b"W").ok().and_then(|v| ln(v).ok()) != Some(0.)
                    || style
                        .get(b"Type")
                        .is_ok_and(|v| v.as_name().ok() != Some(b"Border"))
                    || style
                        .get(b"S")
                        .is_ok_and(|v| v.as_name().ok() != Some(b"S"))
                {
                    return fail("visible annotation border style");
                }
            }
            if !zero_border {
                return fail("link border defaults to visible");
            }
        }
        Ok(())
    }
    fn nums<const N: usize>(v: &[&Value<'_>]) -> Result<[f64; N]> {
        if v.len() != N {
            return fail("wrong operand count");
        };
        let mut r = [0.; N];
        for (a, b) in r.iter_mut().zip(v) {
            *a = number(b)?
        }
        Ok(r)
    }
    // Match the interpreter's documented local numeric boundary: color operands
    // become f32 before 8-bit quantization. Unsupported out-of-range operands
    // reject rather than silently clamp. No rasterizer is called here.
    fn device_channel(value: f64) -> Result<u8> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return fail("device color outside unit range");
        }
        Ok((value as f32 * 255.0 + 0.5) as u8)
    }

    fn decode(s: &Stream, limit: usize) -> Result<Vec<u8>> {
        if s.content.len() > limit {
            return fail("stream limit");
        }
        // Predictor 1 leaves Flate output unchanged. Other decode parameters
        // require transformations this bounded worker does not implement.
        if let Ok(params) = s.dict.get(b"DecodeParms") {
            if s.dict.get(b"Filter")?.as_name()? != b"FlateDecode" {
                return fail("unsupported DecodeParms filter");
            }
            let params = params.as_dict()?;
            if params.len() != 1 || params.get(b"Predictor")?.as_i64()? != 1 {
                return fail("unsupported DecodeParms");
            }
        }
        let b = match s.dict.get(b"Filter") {
            Err(_) => s.content.clone(),
            Ok(Object::Name(n)) if n == b"FlateDecode" => {
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&s.content, limit)
                    .map_err(|_| "bounded Flate failed")?
            }
            _ => return fail("unsupported stream filter"),
        };
        if b.len() > limit {
            return fail("decoded byte limit");
        };
        Ok(b)
    }
    fn inherited(doc: &Document, mut id: lopdf::ObjectId, key: &[u8]) -> Result<Object> {
        let mut seen = HashSet::new();
        for _ in 0..32 {
            if !seen.insert(id) {
                return fail("page cycle");
            };
            let d = doc.get_dictionary(id)?;
            if let Ok(v) = d.get(key) {
                return Ok(doc.dereference(v)?.1.clone());
            };
            id = d.get(b"Parent")?.as_reference()?;
        }
        fail("parent depth")
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FontEncoding {
        WinAnsi,
        MacSymbol,
        IdentityH,
        Type1BuiltIn,
    }
    struct Font {
        bytes: Vec<u8>,
        widths: BTreeMap<u16, f64>,
        gids: BTreeMap<u16, u16>,
        default_width: Option<f64>,
        encoding: FontEncoding,
        glyph_count: u16,
        upem: f64,
    }
    fn font(doc: &Document, d: &Dictionary) -> Result<Font> {
        if !d.type_is(b"Font") {
            return fail("expected embedded WinAnsi TrueType");
        }
        if d.get(b"Subtype")?.as_name()? == b"Type0" {
            return identity_h_font(doc, d);
        }
        if d.get(b"Subtype")?.as_name()? == b"Type1" {
            return type1_font(doc, d);
        }
        if d.get(b"Subtype")?.as_name()? != b"TrueType" {
            return fail("unsupported font subtype");
        }
        let descriptor = doc.dereference(d.get(b"FontDescriptor")?)?.1.as_dict()?;
        let flags = descriptor.get(b"Flags")?.as_i64()?;
        let encoding = match d.get(b"Encoding") {
            Ok(value) if value.as_name()? == b"WinAnsiEncoding" && flags & 4 == 0 => {
                FontEncoding::WinAnsi
            }
            Err(_) if flags & 4 != 0 => FontEncoding::MacSymbol,
            _ => return fail("unsupported TrueType encoding"),
        };
        let bytes = decode(
            doc.dereference(descriptor.get(b"FontFile2")?)?
                .1
                .as_stream()?,
            8_000_000,
        )?;
        let face = ttf_parser::Face::parse(&bytes, 0)?;
        let glyph_count = face.number_of_glyphs();
        let upem = f64::from(face.units_per_em());
        let cmap = face.tables().cmap.ok_or("missing TrueType cmap")?;
        let mapping = cmap
            .subtables
            .into_iter()
            .find(|s| match encoding {
                FontEncoding::WinAnsi => {
                    s.platform_id == ttf_parser::PlatformId::Windows && s.encoding_id == 1
                }
                FontEncoding::MacSymbol => {
                    s.platform_id == ttf_parser::PlatformId::Macintosh
                        && s.encoding_id == 0
                        && matches!(s.format, ttf_parser::cmap::Format::ByteEncodingTable(_))
                }
                _ => false,
            })
            .ok_or("required TrueType cmap missing")?;
        let first = u8::try_from(d.get(b"FirstChar")?.as_i64()?)?;
        let last = u8::try_from(d.get(b"LastChar")?.as_i64()?)?;
        let raw = doc.dereference(d.get(b"Widths")?)?.1.as_array()?;
        if last < first || raw.len() != usize::from(last - first) + 1 {
            return fail("invalid widths");
        }
        // The MacSymbol subset maps the PDF byte directly through an embedded
        // Macintosh format-0 cmap. No Unicode guess or system-font fallback.
        let winansi = if encoding == FontEncoding::WinAnsi {
            Some(d.get_font_encoding(doc)?)
        } else {
            None
        };
        let mut widths = BTreeMap::new();
        let mut gids = BTreeMap::new();
        for (i, w) in raw.iter().enumerate() {
            let code = first + i as u8;
            let w = ln(w)?;
            if !(0.0..=10000.0).contains(&w) {
                return fail("width outside bound");
            };
            widths.insert(u16::from(code), w);
            let mapped = if let Some(winansi) = &winansi {
                let decoded = winansi.bytes_to_string(&[code])?;
                let mut chars = decoded.chars();
                let unicode = chars.next();
                if chars.next().is_some() {
                    return fail("non-scalar WinAnsi mapping");
                }
                unicode.map(u32::from)
            } else {
                Some(u32::from(code))
            };
            if let Some(mapped) = mapped {
                if let Some(g) = mapping.glyph_index(mapped) {
                    if g.0 != 0 {
                        if encoding == FontEncoding::WinAnsi
                            && code == 32
                            && face.glyph_bounding_box(g).is_some()
                        {
                            return fail("space glyph with ink unsupported");
                        }
                        gids.insert(u16::from(code), g.0);
                    }
                }
            }
        }
        Ok(Font {
            bytes,
            widths,
            gids,
            default_width: None,
            encoding,
            glyph_count,
            upem,
        })
    }
    // Validate embedded built-in Type1 code mapping and PDF widths inside the
    // resource-limited worker before bounded cubic staging. Other encodings,
    // external fonts and nonstandard font matrices remain unsupported.
    fn type1_font(doc: &Document, d: &Dictionary) -> Result<Font> {
        if d.has(b"Encoding") || d.has(b"FontMatrix") {
            return fail("Type1 encoding override or font matrix unsupported");
        }
        let descriptor = doc.dereference(d.get(b"FontDescriptor")?)?.1.as_dict()?;
        let bytes = decode(
            doc.dereference(descriptor.get(b"FontFile")?)?
                .1
                .as_stream()?,
            8_000_000,
        )?;
        let font = Type1Font::new(&bytes)?;
        let glyph_count = u16::try_from(font.num_glyphs())?;
        if glyph_count == 0 || glyph_count > 4096 {
            return fail("Type1 glyph count limit");
        }
        let encoding = font.encoding().ok_or("Type1 built-in encoding missing")?;
        let first = u8::try_from(d.get(b"FirstChar")?.as_i64()?)?;
        let last = u8::try_from(d.get(b"LastChar")?.as_i64()?)?;
        let raw = doc.dereference(d.get(b"Widths")?)?.1.as_array()?;
        if first > last || raw.len() != usize::from(last - first) + 1 {
            return fail("invalid Type1 widths");
        }
        let mut widths = BTreeMap::new();
        let mut gids = BTreeMap::new();
        for (index, value) in raw.iter().enumerate() {
            let code = first + index as u8;
            let width = ln(value)?;
            if !(0.0..=10_000.0).contains(&width) || width.fract() != 0. {
                return fail("unproven Type1 width");
            }
            widths.insert(u16::from(code), width);
            if let Some(gid) = encoding.map(code) {
                let id = u16::try_from(gid.to_u32())?;
                if id >= glyph_count {
                    return fail("Type1 code maps outside embedded font");
                }
                gids.insert(u16::from(code), id);
            }
        }
        Ok(Font {
            bytes,
            widths,
            gids,
            default_width: None,
            encoding: FontEncoding::Type1BuiltIn,
            glyph_count,
            upem: 1000.,
        })
    }
    fn identity_h_font(doc: &Document, d: &Dictionary) -> Result<Font> {
        if d.get(b"Encoding")?.as_name()? != b"Identity-H" {
            return fail("unsupported composite font encoding");
        }
        let descendants = doc.dereference(d.get(b"DescendantFonts")?)?.1.as_array()?;
        let [descendant] = descendants.as_slice() else {
            return fail("expected one CIDFont descendant");
        };
        let cid = doc.dereference(descendant)?.1.as_dict()?;
        if cid.get(b"Subtype")?.as_name()? != b"CIDFontType2"
            || cid.get(b"CIDToGIDMap")?.as_name()? != b"Identity"
        {
            return fail("only Identity CIDFontType2 mapping is supported");
        }
        let system = doc.dereference(cid.get(b"CIDSystemInfo")?)?.1.as_dict()?;
        if system.get(b"Registry")?.as_str()? != b"Adobe"
            || system.get(b"Ordering")?.as_str()? != b"Identity"
        {
            return fail("unsupported CID system");
        }
        let descriptor = doc.dereference(cid.get(b"FontDescriptor")?)?.1.as_dict()?;
        let bytes = decode(
            doc.dereference(descriptor.get(b"FontFile2")?)?
                .1
                .as_stream()?,
            8_000_000,
        )?;
        let face = ttf_parser::Face::parse(&bytes, 0)?;
        let glyph_count = face.number_of_glyphs();
        let upem = f64::from(face.units_per_em());
        if glyph_count == 0 || upem == 0. {
            return fail("invalid embedded CIDFontType2 metrics");
        }
        let default_width = match cid.get(b"DW") {
            Ok(value) => ln(doc.dereference(value)?.1)?,
            Err(_) => 1000.,
        };
        // Lopdf stores PDF real numbers as f32. Until original CID-width
        // decimal lexemes are preserved, accept only widths whose value is
        // unaffected by that early narrowing.
        if !(0.0..=10_000.0).contains(&default_width) || default_width.fract() != 0. {
            return fail("invalid default CID width");
        }
        let mut widths = BTreeMap::new();
        if let Ok(w) = cid.get(b"W") {
            let entries = doc.dereference(w)?.1.as_array()?;
            if entries.len() > 8192 {
                return fail("CID width entry limit");
            }
            let mut at = 0;
            while at < entries.len() {
                let first = u16::try_from(entries[at].as_i64()?)?;
                at += 1;
                let next = entries.get(at).ok_or("incomplete CID width array")?;
                if let Ok(values) = next.as_array() {
                    if values.is_empty()
                        || usize::from(first) + values.len() > u16::MAX as usize + 1
                        || widths.len() + values.len() > 8192
                    {
                        return fail("invalid CID width array");
                    }
                    for (offset, value) in values.iter().enumerate() {
                        let width = ln(value)?;
                        if !(0.0..=10_000.0).contains(&width)
                            || width.fract() != 0.
                            || widths.insert(first + offset as u16, width).is_some()
                        {
                            return fail("invalid or overlapping CID width");
                        }
                    }
                    at += 1;
                } else {
                    let last = u16::try_from(next.as_i64()?)?;
                    let value = entries.get(at + 1).ok_or("missing CID range width")?;
                    let width = ln(value)?;
                    if last < first
                        || !(0.0..=10_000.0).contains(&width)
                        || width.fract() != 0.
                        || usize::from(last - first) + 1 + widths.len() > 8192
                    {
                        return fail("invalid CID width range");
                    }
                    for code in first..=last {
                        if widths.insert(code, width).is_some() {
                            return fail("overlapping CID width range");
                        }
                    }
                    at += 2;
                }
            }
        }
        Ok(Font {
            bytes,
            widths,
            gids: BTreeMap::new(),
            default_width: Some(default_width),
            encoding: FontEncoding::IdentityH,
            glyph_count,
            upem,
        })
    }
    #[derive(Clone)]
    struct State {
        tm: [f64; 6],
        line: [f64; 6],
        font: String,
        size: f64,
        tc: f64,
        tw: f64,
        h: f64,
        rise: f64,
        leading: f64,
        color: [u8; 3],
        clip: [u32; 4],
        fractional_clip: Option<[f64; 4]>,
        image_matrix: Option<[f64; 6]>,
        image_clip: Option<[f64; 4]>,
        image_clip_count: u8,
    }
    impl Default for State {
        fn default() -> Self {
            Self {
                tm: [1., 0., 0., 1., 0., 0.],
                line: [1., 0., 0., 1., 0., 0.],
                font: String::new(),
                size: 0.,
                tc: 0.,
                tw: 0.,
                h: 1.,
                rise: 0.,
                leading: 0.,
                color: [0; 3],
                clip: [0; 4],
                fractional_clip: None,
                image_matrix: None,
                image_clip: None,
                image_clip_count: 0,
            }
        }
    }
    fn point(m: [f64; 6], p: [f64; 2]) -> [f64; 2] {
        [
            m[0] * p[0] + m[2] * p[1] + m[4],
            m[1] * p[0] + m[3] * p[1] + m[5],
        ]
    }
    fn shift(m: &mut [f64; 6], x: f64, y: f64) {
        let p = point(*m, [x, y]);
        m[4] = p[0];
        m[5] = p[1];
    }
    enum Paint {
        Image {
            jpeg: Vec<u8>,
            source: [u32; 2],
            media: [f64; 4],
            rect: [f64; 4],
            matrix: [f64; 6],
        },
        Glyph {
            font: String,
            gid: u16,
            matrix: [f64; 6],
            color: [u8; 3],
            clip: [u32; 4],
            fractional_clip: Option<[f64; 4]>,
        },
        Rect {
            points: Vec<[f64; 2]>,
            color: [u8; 3],
            clip: [u32; 4],
            fractional_clip: Option<[f64; 4]>,
        },
    }
    fn page_image_media(doc: &Document, mut id: lopdf::ObjectId) -> Result<[f64; 4]> {
        let mut media = None;
        let mut crops = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..8 {
            if !seen.insert(id) {
                return fail("cyclic image page ancestry");
            }
            let node = doc.get_dictionary(id)?;
            if node.has(b"Group") || node.has(b"Annots") {
                return fail("unsupported image page state");
            }
            // Explicit neutral geometry is common in exported PDFs. Keep
            // nontrivial PageSpace transforms outside this proven image subset.
            if node.has(b"Rotate") && doc.dereference(node.get(b"Rotate")?)?.1.as_i64()? != 0 {
                return fail("unsupported image rotation");
            }
            if node.has(b"UserUnit") && ln(doc.dereference(node.get(b"UserUnit")?)?.1)? != 1. {
                return fail("unsupported image UserUnit");
            }
            let page_box = |key: &[u8]| -> Result<[f64; 4]> {
                let values = doc.dereference(node.get(key)?)?.1.as_array()?;
                if values.len() != 4 {
                    return fail("image page box arity");
                }
                let mut parsed = [0.; 4];
                for (to, from) in parsed.iter_mut().zip(values) {
                    *to = ln(doc.dereference(from)?.1)?;
                }
                Ok(parsed)
            };
            if node.has(b"CropBox") {
                crops.push(page_box(b"CropBox")?);
            }
            if media.is_none() {
                if node.has(b"MediaBox") {
                    media = Some(page_box(b"MediaBox")?);
                }
            }
            id = match node.get(b"Parent") {
                Ok(Object::Reference(parent)) => *parent,
                Err(_) => {
                    let media = media.ok_or("missing image MediaBox")?;
                    if crops.iter().any(|crop| *crop != media) {
                        return fail("unsupported image CropBox");
                    }
                    return Ok(media);
                }
                _ => return fail("malformed image page ancestry"),
            };
        }
        fail("deep image page ancestry")
    }
    fn selected_jpeg(
        doc: &Document,
        resources: &Dictionary,
        name: &[u8],
    ) -> Result<(Vec<u8>, [u32; 2])> {
        let objects = doc.dereference(resources.get(b"XObject")?)?.1.as_dict()?;
        if objects.len() > 32 {
            return fail("image XObject count");
        }
        let image = doc.dereference(objects.get(name)?)?.1.as_stream()?;
        if image.dict.len() > 8
            || image.dict.iter().any(|(key, _)| {
                ![
                    b"Type".as_slice(),
                    b"Subtype",
                    b"Width",
                    b"Height",
                    b"BitsPerComponent",
                    b"ColorSpace",
                    b"Filter",
                    b"Length",
                ]
                .contains(&key.as_slice())
            })
            || image.dict.get(b"Subtype")?.as_name()? != b"Image"
            || image
                .dict
                .get(b"Type")
                .is_ok_and(|value| value.as_name().ok() != Some(b"XObject"))
            || image.dict.get(b"ColorSpace")?.as_name()? != b"DeviceRGB"
            || image.dict.get(b"BitsPerComponent")?.as_i64()? != 8
            || image.dict.get(b"Filter")?.as_name()? != b"DCTDecode"
        {
            return fail("unsupported image resource");
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
            || image.dict.get(b"Length")?.as_i64()? != image.content.len() as i64
        {
            return fail("JPEG source/length bound");
        }
        Ok((image.content.clone(), source))
    }
    fn show(
        bytes: &[u8],
        s: &mut State,
        fonts: &BTreeMap<String, Font>,
        space: &PageSpace,
        out: &mut Vec<Paint>,
        count: &mut usize,
    ) -> Result<()> {
        let f = fonts.get(&s.font).ok_or("missing selected font")?;
        if f.encoding == FontEncoding::IdentityH && bytes.len() % 2 != 0 {
            return fail("odd Identity-H text length");
        }
        let codes: Box<dyn Iterator<Item = u16> + '_> = match f.encoding {
            FontEncoding::WinAnsi | FontEncoding::MacSymbol | FontEncoding::Type1BuiltIn => {
                Box::new(bytes.iter().copied().map(u16::from))
            }
            FontEncoding::IdentityH => Box::new(
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]])),
            ),
        };
        for code in codes {
            *count += 1;
            if *count > 8192 {
                return fail("glyph limit");
            };
            let gid = match f.encoding {
                FontEncoding::WinAnsi | FontEncoding::MacSymbol | FontEncoding::Type1BuiltIn => *f
                    .gids
                    .get(&code)
                    .ok_or_else(|| format!("unproven code {code} in {}", s.font))?,
                FontEncoding::IdentityH if code > 0 && code < f.glyph_count => code,
                FontEncoding::IdentityH => return fail("CID outside embedded glyph set"),
            };
            let base = space.screen_point(point(s.tm, [0., s.rise]))?;
            let x = space.screen_point(point(s.tm, [s.size * s.h / f.upem, s.rise]))?;
            let y = space.screen_point(point(s.tm, [0., s.rise + s.size / f.upem]))?;
            let matrix = [
                x[0] - base[0],
                x[1] - base[1],
                y[0] - base[0],
                y[1] - base[1],
                base[0],
                base[1],
            ];
            // Both WinAnsi space codes select the same empty glyph; only byte
            // 32 receives PDF word spacing below, byte 160 does not.
            if f.encoding != FontEncoding::WinAnsi || (code != 32 && code != 160) {
                out.push(Paint::Glyph {
                    font: s.font.clone(),
                    gid,
                    matrix,
                    color: s.color,
                    clip: s.clip,
                    fractional_clip: s.fractional_clip,
                });
            }
            // Match the oracle's numeric boundary: text advances and spacing
            // use f32, while text matrices and Td/Tm remain f64.
            let width = f
                .widths
                .get(&code)
                .copied()
                .or(f.default_width)
                .ok_or("missing glyph width")?;
            let advance = ((width as f32 / 1000. * s.size as f32
                + s.tc as f32
                + if f.encoding != FontEncoding::IdentityH && code == 32 {
                    s.tw as f32
                } else {
                    0.
                })
                * s.h as f32) as f64;
            shift(&mut s.tm, advance, 0.);
        }
        Ok(())
    }
    fn collect(
        doc: &Document,
        page: usize,
        space: &PageSpace,
    ) -> Result<(Vec<Paint>, BTreeMap<String, Font>, usize, usize)> {
        let id = *doc.get_pages().get(&(page as u32)).ok_or("missing page")?;
        let pd = doc.get_dictionary(id)?;
        // Link annotations with explicitly zero-width borders and no /AP have
        // no page appearance. Other annotation painting remains unsupported.
        if pd.has(b"Group") {
            return fail("annotation/group unsupported");
        }
        invisible_link_annotations(doc, pd)?;
        let resources = inherited(doc, id, b"Resources")?;
        let resources = resources.as_dict()?;
        if resources.len() > 16 {
            return fail("resource count");
        }
        for (name, value) in resources.iter() {
            match name.as_slice() {
                b"Font" | b"ProcSet" => {}
                // These dictionaries have no painting effect until an
                // operator names an entry. The content grammar below rejects
                // gs/Do/sh/SCN/scn and named-property BDC before output.
                b"ExtGState" | b"XObject" | b"Shading" | b"Pattern" | b"Properties" => {}
                b"ColorSpace" => {
                    let entries = doc.dereference(value)?.1.as_dict()?;
                    if entries.len() > 32
                        || entries.iter().any(|(key, _)| {
                            matches!(
                                key.as_slice(),
                                b"DefaultGray" | b"DefaultRGB" | b"DefaultCMYK"
                            )
                        })
                    {
                        // Default device substitutions can alter g/rg even
                        // without a cs/CS operator.
                        return fail("unsupported default color space");
                    }
                }
                _ => return fail("unsupported resource type"),
            }
        }
        // Pages containing only paths or images need not declare /Font. A
        // later Tf still fails unless its name resolves to a staged font.
        let fd = if resources.has(b"Font") {
            Some(doc.dereference(resources.get(b"Font")?)?.1.as_dict()?)
        } else {
            None
        };
        if fd.is_some_and(|fonts| fonts.len() > 8) {
            return fail("font count");
        };
        let ids = doc.get_page_contents(id);
        if ids.len() > 16 {
            return fail("stream count");
        };
        let mut content = Vec::new();
        for id in ids {
            let b = decode(doc.get_object(id)?.as_stream()?, 1_000_000)?;
            if content.len() + b.len() > 1_000_000 {
                return fail("content limit");
            };
            content.extend(b);
            content.push(b'\n');
        }
        // Two parsers must agree on every operator; no typed skipping of unknowns.
        // Native callers execute this parser only inside the resource-bounded
        // text worker; direct diagnostic invocation still requires trusted PDFs.
        // Require both parsers to reach an operand-free sentinel. A malformed
        // suffix must not be silently accepted as a valid operator prefix.
        content.extend_from_slice(b"\nopdsProbeEnd\n");
        let canonical = lopdf::content::Content::decode(&content)?.operations;
        // Only Tf can select a font. A malformed but unreferenced font entry
        // cannot change this page's paints; parsing it eagerly also rejects
        // otherwise valid pages. The interpreter below still verifies Tf's
        // arity/name and requires every selected resource to be present.
        let selected: HashSet<&[u8]> = canonical
            .iter()
            .filter(|op| op.operator == "Tf" && op.operands.len() == 2)
            .filter_map(|op| match &op.operands[0] {
                Object::Name(name) => Some(name.as_slice()),
                _ => None,
            })
            .collect();
        let mut fonts = BTreeMap::new();
        let mut total = 0;
        for (name, obj) in fd.into_iter().flat_map(|fonts| fonts.iter()) {
            if name.is_empty() || name.len() > 31 {
                return fail("font name");
            }
            if !selected.contains(name.as_slice()) {
                continue;
            }
            let f = font(doc, doc.dereference(obj)?.1.as_dict()?)?;
            total += f.bytes.len();
            if total > 16_000_000 {
                return fail("total fonts");
            }
            fonts.insert(String::from_utf8(name.clone())?, f);
        }
        let mut iter = UntypedIter::new(&content);
        let mut ops = 0;
        let mut reached_end = false;
        let mut s = State::default();
        let [page_w, page_h] = space.pixels();
        s.clip = [0, 0, page_w, page_h];
        let mut inside = false;
        let mut marked = 0;
        let mut stack = Vec::new();
        let mut rect = None;
        let mut rect_user = None;
        let mut pending_clip = false;
        let mut paints = Vec::new();
        let mut image_seen = false;
        let mut image_waiting_do = false;
        let mut image_waiting_q = false;
        let mut glyphs = 0;
        while let Some(op) = iter.next() {
            if ops >= 16384 {
                return fail("operator limit");
            };
            let name = &**op.operator;
            if image_waiting_do && name != b"Do" {
                return fail("image CTM must be followed immediately by Do");
            }
            if image_waiting_q && name != b"Q" {
                return fail("image paint must end its graphics-state scope");
            }
            if pending_clip && name != b"n" {
                return fail("clip must be followed by n");
            }
            let a: Vec<_> = op.operands().collect();
            if canonical.get(ops).map(|o| o.operator.as_bytes()) != Some(name) {
                return fail(&format!(
                    "content parser disagreement at {ops}: {:?} vs {:?}",
                    canonical.get(ops).map(|o| &o.operator),
                    String::from_utf8_lossy(name)
                ));
            };
            ops += 1;
            match name {
                b"opdsProbeEnd" if a.is_empty() && ops == canonical.len() => reached_end = true,
                b"BT" if a.is_empty() && !inside => {
                    inside = true;
                    s.tm = [1., 0., 0., 1., 0., 0.];
                    s.line = s.tm;
                }
                b"ET" if a.is_empty() && inside => inside = false,
                b"Tf" if a.len() == 2 => {
                    let Value::Name(n) = a[0] else {
                        return fail("Tf name");
                    };
                    s.font = String::from_utf8(n.as_ref().to_vec())?;
                    s.size = f64::from(number(a[1])? as f32);
                    if !(0.0..=200.).contains(&s.size) || !fonts.contains_key(&s.font) {
                        return fail("font size/resource");
                    }
                }
                b"Tm" if inside => {
                    s.tm = nums(&a)?;
                    s.line = s.tm;
                }
                b"Td" | b"TD" if inside => {
                    let [x, y] = nums(&a)?;
                    shift(&mut s.line, x, y);
                    s.tm = s.line;
                    if name == b"TD" {
                        s.leading = f64::from(-y as f32)
                    }
                }
                b"T*" if inside && a.is_empty() => {
                    shift(&mut s.line, 0., -s.leading);
                    s.tm = s.line;
                }
                b"Tc" => s.tc = f64::from(nums::<1>(&a)?[0] as f32),
                b"Tw" => s.tw = f64::from(nums::<1>(&a)?[0] as f32),
                b"Tz" => {
                    s.h = f64::from(nums::<1>(&a)?[0] as f32 / 100.);
                    if !(0.01..=10.).contains(&s.h) {
                        return fail("horizontal scale");
                    }
                }
                b"Ts" => s.rise = f64::from(nums::<1>(&a)?[0] as f32),
                b"TL" => s.leading = f64::from(nums::<1>(&a)?[0] as f32),
                b"Tr" if nums::<1>(&a)? == [0.] => {}
                b"Tj" if inside && a.len() == 1 => {
                    let Value::String(t) = a[0] else {
                        return fail("Tj string");
                    };
                    show(
                        t.as_bytes(),
                        &mut s,
                        &fonts,
                        space,
                        &mut paints,
                        &mut glyphs,
                    )?;
                }
                b"TJ" if inside && a.len() == 1 => {
                    let Value::Array(arr) = a[0] else {
                        return fail("TJ array");
                    };
                    let mut n = 0;
                    for v in arr.raw_iter() {
                        n += 1;
                        if n > 16384 {
                            return fail("TJ limit");
                        };
                        match v {
                            hayro_syntax::object::MaybeRef::NotRef(Value::String(t)) => show(
                                t.as_bytes(),
                                &mut s,
                                &fonts,
                                space,
                                &mut paints,
                                &mut glyphs,
                            )?,
                            hayro_syntax::object::MaybeRef::NotRef(Value::Number(t)) => {
                                let adjust = t.as_f64();
                                if !adjust.is_finite() || adjust.abs() > 1e6 {
                                    return fail("TJ adjustment");
                                };
                                shift(
                                    &mut s.tm,
                                    f64::from(-adjust as f32 / 1000. * s.size as f32 * s.h as f32),
                                    0.,
                                );
                            }
                            _ => return fail("TJ operand"),
                        }
                    }
                }
                b"g" => {
                    let [v] = nums(&a)?;
                    s.color = [device_channel(v)?; 3];
                }
                b"rg" => {
                    let [r, g, b] = nums(&a)?;
                    s.color = [device_channel(r)?, device_channel(g)?, device_channel(b)?];
                }
                // These update stroke-only state. No stroke paint operator or
                // non-fill text render mode is accepted by this frontend, so
                // validated values cannot change a supported page bitmap.
                b"w" if !inside => {
                    let [width] = nums(&a)?;
                    if !(0.0..=1000.0).contains(&width) {
                        return fail("stroke width bound");
                    }
                }
                b"J" | b"j" if !inside => {
                    let [style] = nums(&a)?;
                    if ![0., 1., 2.].contains(&style) {
                        return fail("stroke cap/join style");
                    }
                }
                b"G" if !inside => {
                    let [gray] = nums(&a)?;
                    device_channel(gray)?;
                }
                b"RG" if !inside => {
                    let [r, g, b] = nums(&a)?;
                    device_channel(r)?;
                    device_channel(g)?;
                    device_channel(b)?;
                }
                b"gs" if !inside && a.len() == 1 => {
                    let Value::Name(name) = a[0] else {
                        return fail("gs resource name");
                    };
                    neutral_ext_gstate(doc, resources, name.as_ref())?;
                }
                b"re" if !inside && rect.is_none() => {
                    let [x, y, w, h] = nums(&a)?;
                    if w <= 0. || h <= 0. {
                        return fail("rectangle size");
                    };
                    rect = Some(vec![
                        space.screen_point([x, y])?,
                        space.screen_point([x + w, y])?,
                        space.screen_point([x + w, y + h])?,
                        space.screen_point([x, y + h])?,
                        space.screen_point([x, y])?,
                    ]);
                    rect_user = Some([x, y, w, h]);
                }
                // The staged path is exactly one simple closed rectangle, so
                // even-odd and nonzero are equivalent. No compound path support.
                b"f" | b"f*" if !inside && a.is_empty() => {
                    rect_user = None;
                    paints.push(Paint::Rect {
                        points: rect.take().ok_or("missing rectangle")?,
                        color: s.color,
                        clip: s.clip,
                        fractional_clip: s.fractional_clip,
                    });
                }
                b"W" | b"W*" if !inside && a.is_empty() && rect.is_some() => {
                    pending_clip = true;
                }
                b"n" if !inside && a.is_empty() && pending_clip => {
                    if s.image_clip_count == u8::MAX {
                        return fail("image clip count");
                    }
                    s.image_clip_count += 1;
                    s.image_clip = rect_user.take();
                    let path = rect.take().ok_or("missing clip rectangle")?;
                    let mut b = bounds(&path);
                    let mut edge = [0_u32; 4];
                    let mut aligned = true;
                    for axis in 0..4 {
                        let page_limit = f64::from([page_w, page_h, page_w, page_h][axis]);
                        if !b[axis].is_finite() || b[axis].abs() > 1_000_000. {
                            return fail("outside clip unsupported");
                        }
                        // The output page is already a hard clip. Intersect
                        // the PDF rectangle with it before representing the
                        // clip in PWPG; a fractional edge beyond the final
                        // page pixel cannot affect any output coverage.
                        b[axis] = b[axis].clamp(0., page_limit);
                        let rounded = b[axis].round();
                        aligned &= b[axis] == rounded;
                        edge[axis] = rounded as u32;
                    }
                    if aligned {
                        s.clip = [
                            s.clip[0].max(edge[0]),
                            s.clip[1].max(edge[1]),
                            s.clip[2].min(edge[2]),
                            s.clip[3].min(edge[3]),
                        ];
                    } else {
                        s.fractional_clip = Some(if let Some(prior) = s.fractional_clip {
                            [
                                prior[0].max(b[0]),
                                prior[1].max(b[1]),
                                prior[2].min(b[2]),
                                prior[3].min(b[3]),
                            ]
                        } else {
                            b
                        });
                    }
                    let f =
                        s.fractional_clip
                            .unwrap_or([0., 0., f64::from(page_w), f64::from(page_h)]);
                    if f[0].max(f64::from(s.clip[0])) >= f[2].min(f64::from(s.clip[2]))
                        || f[1].max(f64::from(s.clip[1])) >= f[3].min(f64::from(s.clip[3]))
                    {
                        return fail("empty clip unsupported");
                    }
                    pending_clip = false;
                }
                b"q" if !inside && rect.is_none() && a.is_empty() => {
                    if stack.len() == 16 {
                        return fail("q depth");
                    };
                    stack.push(s.clone());
                }
                b"Q" if !inside && rect.is_none() && a.is_empty() => {
                    s = stack.pop().ok_or("unbalanced Q")?;
                    image_waiting_q = false;
                }
                b"cm" if !inside && a.len() == 6 && s.image_matrix.is_none() && !image_seen => {
                    let matrix = nums(&a)?;
                    if matrix[0] <= 0.
                        || matrix[3] <= 0.
                        || matrix[1] != 0.
                        || matrix[2] != 0.
                        || stack.is_empty()
                        || !paints.is_empty()
                    {
                        return fail("unsupported image CTM");
                    }
                    s.image_matrix = Some(matrix);
                    image_waiting_do = true;
                }
                b"Do" if !inside && a.len() == 1 && !image_seen && paints.is_empty() => {
                    let Value::Name(name) = a[0] else {
                        return fail("image name");
                    };
                    let matrix = s.image_matrix.take().ok_or("image without CTM")?;
                    let rect = s.image_clip.ok_or("image without rectangle clip")?;
                    if s.image_clip_count != 1 {
                        return fail("image clip subset");
                    }
                    let media = page_image_media(doc, id)?;
                    let (jpeg, source) = selected_jpeg(doc, resources, name.as_ref())?;
                    paints.push(Paint::Image {
                        jpeg,
                        source,
                        media,
                        rect,
                        matrix,
                    });
                    image_seen = true;
                    image_waiting_do = false;
                    image_waiting_q = true;
                }
                b"BMC" if a.len() == 1 => {
                    let Value::Name(tag) = a[0] else {
                        return fail("marked tag");
                    };
                    if tag.as_ref() == b"OC" {
                        return fail("optional content");
                    }
                    marked += 1;
                    if marked > 32 {
                        return fail("marked depth");
                    }
                }
                b"BDC" if a.len() == 2 => {
                    let Value::Name(tag) = a[0] else {
                        return fail("marked tag");
                    };
                    if tag.as_ref() == b"OC" || !matches!(a[1], Value::Dict(_)) {
                        return fail("optional content");
                    };
                    marked += 1;
                    if marked > 32 {
                        return fail("marked depth");
                    }
                }
                b"EMC" if a.is_empty() && marked > 0 => marked -= 1,
                _ => {
                    return fail(&format!(
                        "unsupported operator {} at {}",
                        String::from_utf8_lossy(name),
                        ops - 1
                    ))
                }
            }
        }
        if !reached_end
            || ops != canonical.len()
            || inside
            || marked != 0
            || !stack.is_empty()
            || rect.is_some()
            || rect_user.is_some()
            || pending_clip
            || s.image_matrix.is_some()
            || image_waiting_do
            || image_waiting_q
        {
            return fail("incomplete content");
        };
        Ok((paints, fonts, glyphs, ops - 1))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use lopdf::dictionary;

        fn fixture(content: &[u8]) -> (Document, PageSpace) {
            let mut d = Document::with_version("1.7");
            let pages = d.new_object_id();
            let bytes = include_bytes!("../../../../assets/fonts/LiberationSans-Regular.ttf");
            let file = d.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
            let descriptor = d.add_object(dictionary! {"Flags" => 32, "FontFile2" => file});
            let font = d.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "LiberationSans",
                "Encoding" => "WinAnsiEncoding", "FontDescriptor" => descriptor,
                "FirstChar" => 32, "LastChar" => 255,
                "Widths" => vec![Object::Integer(500); 224]
            });
            let stream = d.add_object(Stream::new(Dictionary::new(), content.to_vec()));
            let page = d.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 256.into(), 192.into()],
                "Resources" => dictionary! {"Font" => dictionary! {"F" => font}},
                "Contents" => stream
            });
            d.objects.insert(
                pages,
                dictionary! {"Type" => "Pages", "Count" => 1, "Kids" => vec![page.into()]}.into(),
            );
            let catalog = d.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
            d.trailer.set("Root", catalog);
            let mut bytes = Vec::new();
            d.save_to(&mut bytes).unwrap();
            let space = PageSpace::from_pdf(Arc::new(bytes), 1, 1.).unwrap();
            (d, space)
        }

        #[test]
        fn fontless_page_can_paint_but_cannot_select_a_font() {
            let (mut d, space) = fixture(b"0 0 30 20 re f");
            let page = *d.get_pages().get(&1).unwrap();
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Resources", Dictionary::new());
            let (paints, fonts, _, _) = collect(&d, 1, &space).unwrap();
            assert_eq!(paints.len(), 1);
            assert!(fonts.is_empty());

            let (mut d, space) = fixture(b"BT /F 12 Tf (A) Tj ET");
            let page = *d.get_pages().get(&1).unwrap();
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Resources", Dictionary::new());
            assert!(collect(&d, 1, &space).is_err());
        }

        #[test]
        fn empty_annots_is_inert_but_annotations_and_groups_reject() {
            let (mut d, space) = fixture(b"BT /F 12 Tf 1 0 0 1 20 100 Tm (A) Tj ET");
            let page = *d.get_pages().get(&1).unwrap();
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Annots", Object::Array(vec![]));
            assert!(collect(&d, 1, &space).is_ok());
            let indirect_empty = d.add_object(Object::Array(vec![]));
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Annots", indirect_empty);
            assert!(collect(&d, 1, &space).is_ok());
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Annots", Object::Array(vec![Object::Null]));
            assert!(collect(&d, 1, &space).is_err());
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Annots", Object::Integer(0));
            assert!(collect(&d, 1, &space).is_err());
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Annots", Object::Array(vec![]));
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Group", Object::Null);
            assert!(collect(&d, 1, &space).is_err());
        }

        #[test]
        fn unused_named_resources_are_inert_but_default_color_and_use_reject() {
            let (mut d, space) = fixture(b"0 0 1 rg BT /F 12 Tf 1 0 0 1 20 100 Tm (A) Tj ET");
            let page = *d.get_pages().get(&1).unwrap();
            let mut resources = d
                .get_dictionary(page)
                .unwrap()
                .get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap()
                .clone();
            resources.set("ExtGState", dictionary! {"GS0" => dictionary! {"CA" => 0}});
            resources.set("XObject", dictionary! {"Unused" => Object::Null});
            resources.set("ColorSpace", dictionary! {"CS0" => "DeviceRGB"});
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Resources", resources.clone());
            assert!(collect(&d, 1, &space).is_ok());
            resources.set("ColorSpace", dictionary! {"DefaultRGB" => "DeviceGray"});
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Resources", resources);
            assert!(collect(&d, 1, &space).is_err());
            for content in [b"/GS0 gs".as_slice(), b"/Unused Do", b"/CS0 cs"] {
                let (mut d, space) = fixture(content);
                let page = *d.get_pages().get(&1).unwrap();
                let mut resources = d
                    .get_dictionary(page)
                    .unwrap()
                    .get(b"Resources")
                    .unwrap()
                    .as_dict()
                    .unwrap()
                    .clone();
                resources.set("ExtGState", dictionary! {"GS0" => dictionary! {"CA" => 0}});
                resources.set("XObject", dictionary! {"Unused" => Object::Null});
                resources.set("ColorSpace", dictionary! {"CS0" => "DeviceRGB"});
                d.get_dictionary_mut(page)
                    .unwrap()
                    .set("Resources", resources);
                assert!(collect(&d, 1, &space).is_err());
            }
        }

        #[test]
        fn unused_font_and_stroke_state_are_inert_but_use_or_stroke_rejects() {
            let content =
                b"0.1 w 1 J 2 j 0.3 G 0.1 0.2 0.3 RG BT /F 12 Tf 1 0 0 1 20 100 Tm (A) Tj ET";
            let (mut d, space) = fixture(content);
            let page = *d.get_pages().get(&1).unwrap();
            let mut resources = d
                .get_dictionary(page)
                .unwrap()
                .get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap()
                .clone();
            let mut fonts = resources.get(b"Font").unwrap().as_dict().unwrap().clone();
            fonts.set("Unused", Object::Null);
            resources.set("Font", fonts);
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Resources", resources);
            let (paints, fonts, _, _) = collect(&d, 1, &space).unwrap();
            assert_eq!(paints.len(), 1);
            assert_eq!(fonts.len(), 1);
            assert!(fonts.contains_key("F"));
            for bad in [
                b"BT /Unused 12 Tf 1 0 0 1 20 100 Tm (A) Tj ET".as_slice(),
                b"-1 w".as_slice(),
                b"3 J".as_slice(),
                b"2 G".as_slice(),
                b"0.1 0.2 2 RG".as_slice(),
                b"0.1 w 10 10 m 20 20 l S".as_slice(),
            ] {
                let (d, space) = fixture(bad);
                assert!(collect(&d, 1, &space).is_err(), "accepted {bad:?}");
            }
        }

        #[test]
        fn only_neutral_named_ext_gstate_is_inert() {
            let (mut d, space) = fixture(b"q /GS0 gs BT /F 12 Tf 1 0 0 1 20 100 Tm (A) Tj ET Q");
            let page = *d.get_pages().get(&1).unwrap();
            let mut resources = d
                .get_dictionary(page)
                .unwrap()
                .get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap()
                .clone();
            let mut state = dictionary! {
                "Type" => "ExtGState", "CA" => 1, "ca" => 1,
                "BM" => "Normal", "SMask" => "None", "OP" => false,
                "op" => false, "AIS" => false, "SA" => true, "OPM" => 1,
                "SM" => Object::Real(0.02),
            };
            resources.set("ExtGState", dictionary! {"GS0" => state.clone()});
            d.get_dictionary_mut(page)
                .unwrap()
                .set("Resources", resources.clone());
            assert_eq!(collect(&d, 1, &space).unwrap().0.len(), 1);
            for (key, value) in [
                ("ca", Object::Real(0.5)),
                ("BM", Object::Name(b"Multiply".to_vec())),
                ("SMask", Object::Null),
                ("OP", Object::Boolean(true)),
                ("SM", Object::Real(-0.1)),
                ("SM", Object::Real(1.1)),
                ("SM", Object::Name(b"bad".to_vec())),
                ("TR2", Object::Name(b"Default".to_vec())),
            ] {
                state.set(key, value);
                resources.set("ExtGState", dictionary! {"GS0" => state.clone()});
                d.get_dictionary_mut(page)
                    .unwrap()
                    .set("Resources", resources.clone());
                assert!(collect(&d, 1, &space).is_err(), "accepted {key}");
                state = dictionary! {
                    "Type" => "ExtGState", "CA" => 1, "ca" => 1,
                    "BM" => "Normal", "SMask" => "None", "OP" => false,
                    "op" => false, "AIS" => false, "SA" => true, "OPM" => 1,
                    "SM" => Object::Real(0.02),
                };
            }
            let (d, space) = fixture(b"/Missing gs");
            assert!(collect(&d, 1, &space).is_err());
        }

        fn identity_fixture(
            content: &[u8],
            cid_map: Object,
            widths: Object,
        ) -> (Document, PageSpace, u16) {
            let mut d = Document::with_version("1.7");
            let pages = d.new_object_id();
            let bytes = include_bytes!("../../../../assets/fonts/LiberationSans-Regular.ttf");
            let gid = ttf_parser::Face::parse(bytes, 0)
                .unwrap()
                .glyph_index('x')
                .unwrap()
                .0;
            let file = d.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
            let descriptor = d.add_object(dictionary! {"FontFile2" => file});
            let system = dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"),
                "Supplement" => 0
            };
            let cid = d.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "CIDFontType2",
                "CIDSystemInfo" => system, "CIDToGIDMap" => cid_map,
                "FontDescriptor" => descriptor, "DW" => 1000, "W" => widths
            });
            let font = d.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "BoundedIdentity",
                "Encoding" => "Identity-H", "DescendantFonts" => vec![cid.into()]
            });
            let stream = d.add_object(Stream::new(Dictionary::new(), content.to_vec()));
            let page = d.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 256.into(), 192.into()],
                "Resources" => dictionary! {"Font" => dictionary! {"C" => font}},
                "Contents" => stream
            });
            d.objects.insert(
                pages,
                dictionary! {"Type" => "Pages", "Count" => 1, "Kids" => vec![page.into()]}.into(),
            );
            let catalog = d.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
            d.trailer.set("Root", catalog);
            let mut source = Vec::new();
            d.save_to(&mut source).unwrap();
            (
                d,
                PageSpace::from_pdf(Arc::new(source), 1, 1.).unwrap(),
                gid,
            )
        }

        #[test]
        fn embedded_identity_h_cid_width_and_framing() {
            let bytes = include_bytes!("../../../../assets/fonts/LiberationSans-Regular.ttf");
            let gid = ttf_parser::Face::parse(bytes, 0)
                .unwrap()
                .glyph_index('x')
                .unwrap()
                .0;
            let content = format!("BT /C 10 Tf 1 0 0 1 20 100 Tm <{gid:04X}{gid:04X}> Tj ET");
            let width = Object::Array(vec![gid.into(), Object::Array(vec![460.into()])]);
            let (d, space, _) =
                identity_fixture(content.as_bytes(), "Identity".into(), width.clone());
            let (paints, _, count, _) = collect(&d, 1, &space).unwrap();
            assert_eq!(count, 2);
            let x: Vec<_> = paints
                .iter()
                .map(|p| match p {
                    Paint::Glyph {
                        gid: actual,
                        matrix,
                        ..
                    } => {
                        assert_eq!(*actual, gid);
                        matrix[4]
                    }
                    _ => panic!("expected glyph"),
                })
                .collect();
            assert_eq!(x, [20., 20. + f64::from(4.6_f32)]);
            for bad in [
                format!("BT /C 10 Tf 1 0 0 1 20 100 Tm <00> Tj ET"),
                format!("BT /C 10 Tf 1 0 0 1 20 100 Tm <FFFF> Tj ET"),
            ] {
                let (d, space, _) =
                    identity_fixture(bad.as_bytes(), "Identity".into(), width.clone());
                assert!(collect(&d, 1, &space).is_err());
            }
            let (d, space, _) = identity_fixture(content.as_bytes(), "NotIdentity".into(), width);
            assert!(collect(&d, 1, &space).is_err());
            for invalid_widths in [
                Object::Array(vec![gid.into()]),
                Object::Array(vec![(gid + 1).into(), gid.into(), 460.into()]),
                Object::Array(vec![
                    gid.into(),
                    Object::Array(vec![460.into()]),
                    gid.into(),
                    Object::Array(vec![460.into()]),
                ]),
                Object::Array(vec![gid.into(), Object::Array(vec![Object::Real(460.5)])]),
            ] {
                let (d, space, _) =
                    identity_fixture(content.as_bytes(), "Identity".into(), invalid_widths);
                assert!(collect(&d, 1, &space).is_err());
            }
        }

        #[test]
        fn spacing_tj_line_motion_and_graphics_restore() {
            let (d, space) = fixture(b"/F 10 Tf 1 Tc 2 Tw BT 1 0 0 1 20 100 Tm [(A ) 100 (B)] TJ 0 -12 TD (C) Tj T* (D) Tj ET q 1 0 0 rg BT 1 0 0 1 50 100 Tm (E) Tj ET Q BT 1 0 0 1 60 100 Tm (F) Tj ET");
            let (paints, _, count, _) = collect(&d, 1, &space).unwrap();
            assert_eq!(count, 7);
            let positions: Vec<_> = paints
                .iter()
                .map(|p| match p {
                    Paint::Glyph { matrix, color, .. } => ([matrix[4], matrix[5]], *color),
                    _ => panic!("unexpected rectangle"),
                })
                .collect();
            assert_eq!(
                positions,
                vec![
                    ([20., 92.], [0; 3]),
                    ([33., 92.], [0; 3]),
                    ([20., 104.], [0; 3]),
                    ([20., 116.], [0; 3]),
                    ([50., 92.], [255, 0, 0]),
                    ([60., 92.], [0; 3])
                ]
            );
        }

        #[test]
        fn malformed_suffixes_and_unsupported_states_reject() {
            for suffix in [
                "123",
                "[",
                "(unclosed",
                "[ (A) TJ",
                "BT",
                "q",
                "Q",
                "2 Tr",
                "/OC <<>> BDC EMC",
                "1 0 0 1 0 0 cm",
                "BT /Missing 10 Tf (A) Tj ET",
                "BT /F 10 Tf <01> Tj ET",
                "opdsProbeEnd",
            ] {
                let input = format!("BT /F 10 Tf 1 0 0 1 20 100 Tm (A) Tj ET\n{suffix}");
                let (d, space) = fixture(input.as_bytes());
                assert!(collect(&d, 1, &space).is_err(), "accepted {suffix:?}");
            }
        }

        #[test]
        fn winansi_aliases_and_word_spacing_remain_explicit() {
            let (d, space) =
                fixture(b"BT /F 10 Tf 3 Tw 1 0 0 1 20 100 Tm <41a0422043d7ad7f80ff> Tj ET");
            let (paints, fonts, count, _) = collect(&d, 1, &space).unwrap();
            assert_eq!(count, 10);
            assert_eq!(paints.len(), 8);
            let face = ttf_parser::Face::parse(&fonts["F"].bytes, 0).unwrap();
            let positions: Vec<_> = paints
                .iter()
                .map(|p| match p {
                    Paint::Glyph { matrix, gid, .. } => (matrix[4], *gid),
                    _ => panic!(),
                })
                .collect();
            for (i, (x, c)) in [
                (20., 'A'),
                (30., 'B'),
                (43., 'C'),
                (48., '×'),
                (53., '-'),
                (58., '•'),
                (63., '€'),
                (68., 'ÿ'),
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(positions[i], (x, face.glyph_index(c).unwrap().0));
            }
            // Undefined controls must never disappear silently during decoding.
            let (d, space) = fixture(b"BT /F 10 Tf <00> Tj ET");
            assert!(collect(&d, 1, &space).is_err());
        }

        #[test]
        fn device_color_state_and_invalid_operands() {
            let (d, space) = fixture(b"0.5 g BT /F 10 Tf 1 0 0 1 20 100 Tm (A) Tj ET q 0.1 0.3 0.7 rg BT 1 0 0 1 40 100 Tm (B) Tj ET Q BT 1 0 0 1 60 100 Tm (C) Tj ET");
            let (paints, _, _, _) = collect(&d, 1, &space).unwrap();
            let colors: Vec<_> = paints
                .iter()
                .map(|p| match p {
                    Paint::Glyph { color, .. } => *color,
                    _ => panic!(),
                })
                .collect();
            assert_eq!(colors, [[128; 3], [26, 77, 179], [128; 3]]);
            for op in [
                "-0.1 g",
                "1.1 g",
                "0.2 0.3 g",
                "0.1 0.2 rg",
                "0 0 1.1 rg",
                "(x) g",
            ] {
                let (d, space) = fixture(op.as_bytes());
                assert!(collect(&d, 1, &space).is_err(), "{op}");
            }
            for n in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
                assert!(device_channel(n).is_err());
            }
        }

        #[test]
        fn simple_rectangle_even_odd_and_balanced_marked_content() {
            let (d, space) = fixture(b"/Artifact BMC /Span <<>> BDC 0.7 g 20 30 40 50 re f* BT /F 10 Tf 1 0 0 1 20 100 Tm (A) Tj ET EMC EMC");
            let (paints, _, _, _) = collect(&d, 1, &space).unwrap();
            assert!(matches!(&paints[0], Paint::Rect {color, ..} if *color == [179; 3]));
            assert_eq!(paints.len(), 2);
            for bad in [
                "/OC BMC EMC",
                "1 BMC EMC",
                "/Artifact BMC",
                "EMC",
                "20 30 40 50 re 2 3 4 5 re f*",
                "f*",
                "20 30 40 50 re 1 f*",
            ] {
                let (d, space) = fixture(bad.as_bytes());
                assert!(collect(&d, 1, &space).is_err(), "{bad}");
            }
            let (d, space) =
                fixture(format!("{}{}", "/Span BMC ".repeat(33), "EMC ".repeat(33)).as_bytes());
            assert!(collect(&d, 1, &space).is_err());
        }

        #[test]
        fn aligned_rectangle_clip_intersects_and_restores_with_graphics_state() {
            let (d, space) = fixture(b"q 20 30 80 60 re W n 30 40 20 20 re f q 40 50 20 20 re W* n BT /F 10 Tf 1 0 0 1 43 56 Tm (A) Tj ET Q BT /F 10 Tf 1 0 0 1 31 56 Tm (B) Tj ET Q BT /F 10 Tf 1 0 0 1 10 56 Tm (C) Tj ET");
            let (paints, _, _, _) = collect(&d, 1, &space).unwrap();
            let clips: Vec<_> = paints
                .iter()
                .map(|p| match p {
                    Paint::Glyph { clip, .. } | Paint::Rect { clip, .. } => *clip,
                    Paint::Image { .. } => panic!("clip fixture unexpectedly staged an image"),
                })
                .collect();
            assert_eq!(
                clips,
                [
                    [20, 102, 100, 162],
                    [40, 122, 60, 142],
                    [20, 102, 100, 162],
                    [0, 0, 256, 192]
                ]
            );
            for bad in [
                "20 30 40 50 re W BT ET n",
                "20 30 40 50 re W",
                "20 30 40 50 re W W n",
                "W n",
                "20 30 40 50 re n",
                "20 30 40 50 re W n 1 1 2 2 re W n",
            ] {
                let (d, space) = fixture(bad.as_bytes());
                assert!(collect(&d, 1, &space).is_err(), "{bad}");
            }
            let (d, space) =
                fixture(b"q 20.25 30.5 80 60 re W n BT /F 10 Tf 1 0 0 1 43 56 Tm (A) Tj ET Q");
            let (paints, _, _, _) = collect(&d, 1, &space).unwrap();
            assert!(matches!(
                &paints[0],
                Paint::Glyph {
                    fractional_clip: Some(_),
                    ..
                }
            ));
            assert!(!inactive_fractional_clip(
                [42., 130., 55., 144.],
                Some([20.25, 101.5, 100.25, 161.5])
            )
            .unwrap());
            assert!(inactive_fractional_clip(
                [1., 130., 17., 144.],
                Some([20.25, 101.5, 100.25, 161.5])
            )
            .unwrap());
            assert!(inactive_fractional_clip(
                [104., 130., 120., 144.],
                Some([20.25, 101.5, 100.25, 161.5])
            )
            .unwrap());
            assert!(inactive_fractional_clip(
                [20., 130., 55., 144.],
                Some([20.25, 101.5, 100.25, 161.5])
            )
            .is_err());
            let f = Some([20.25, 101.5, 100.25, 161.5]);
            assert_eq!(
                fractional_paint_clip([42., 130., 55., 144.], f, true).unwrap(),
                (false, None)
            );
            assert_eq!(
                fractional_paint_clip([1., 130., 17., 144.], f, true).unwrap(),
                (true, None)
            );
            assert_eq!(
                fractional_paint_clip([20., 130., 55., 144.], f, true).unwrap(),
                (false, f)
            );
            assert!(fractional_paint_clip([20., 130., 55., 144.], f, false).is_err());
            assert!(fractional_paint_clip([f64::NAN, 130., 55., 144.], f, true).is_err());
        }

        #[test]
        fn font_resources_and_decode_are_bounded() {
            let (mut d, space) = fixture(b"BT /F 10 Tf 1 0 0 1 20 100 Tm <92> Tj ET");
            let (paints, fonts, count, _) = collect(&d, 1, &space).unwrap();
            assert_eq!(count, 1);
            let face = ttf_parser::Face::parse(&fonts["F"].bytes, 0).unwrap();
            assert!(
                matches!(&paints[0], Paint::Glyph {gid, ..} if *gid == face.glyph_index('\u{2019}').unwrap().0)
            );
            let font_id = d
                .objects
                .iter()
                .find_map(|(id, o)| {
                    o.as_dict()
                        .ok()
                        .filter(|v| {
                            v.get(b"Subtype").and_then(Object::as_name).ok()
                                == Some(b"TrueType".as_slice())
                        })
                        .map(|_| *id)
                })
                .unwrap();
            d.get_object_mut(font_id)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Encoding", "MacRomanEncoding");
            assert!(collect(&d, 1, &space).is_err());
            let packed = miniz_oxide::deflate::compress_to_vec_zlib(&vec![0; 4096], 6);
            let stream = Stream::new(dictionary! {"Filter" => "FlateDecode"}, packed);
            assert!(decode(&stream, 128).is_err());
            assert_eq!(decode(&stream, 4096).unwrap().len(), 4096);
            let mut invalid = stream;
            invalid.dict.set("DecodeParms", Dictionary::new());
            assert!(decode(&invalid, 4096).is_err());
            invalid
                .dict
                .set("DecodeParms", dictionary! {"Predictor" => 1});
            assert_eq!(decode(&invalid, 4096).unwrap().len(), 4096);
            invalid
                .dict
                .set("DecodeParms", dictionary! {"Predictor" => 2});
            assert!(decode(&invalid, 4096).is_err());
        }
    }
    fn bounds(points: &[[f64; 2]]) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for &[x, y] in points {
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x);
            b[3] = b[3].max(y)
        }
        b
    }
    // Return true for a paint wholly outside the clip, false for a paint
    // wholly inside it. Anything close enough to a fractional edge for AA
    // to matter remains unsupported. Two pixels exceed the strip footprint.
    fn inactive_fractional_clip(paint: [f64; 4], clip: Option<[f64; 4]>) -> Result<bool> {
        if let Some(c) = clip {
            if paint.iter().any(|v| !v.is_finite()) {
                return fail("nonfinite paint bounds");
            }
            if paint[2] < c[0] - 2.
                || paint[3] < c[1] - 2.
                || paint[0] > c[2] + 2.
                || paint[1] > c[3] + 2.
            {
                return Ok(true);
            }
            if paint[0] < c[0] + 2.
                || paint[1] < c[1] + 2.
                || paint[2] > c[2] - 2.
                || paint[3] > c[3] - 2.
            {
                return fail("active fractional clip unsupported");
            }
        }
        Ok(false)
    }
    fn fractional_paint_clip(
        paint: [f64; 4],
        clip: Option<[f64; 4]>,
        allow_active: bool,
    ) -> Result<(bool, Option<[f64; 4]>)> {
        if paint.iter().any(|v| !v.is_finite()) {
            return fail("nonfinite paint bounds");
        }
        match inactive_fractional_clip(paint, clip) {
            Ok(true) => Ok((true, None)),
            Ok(false) => Ok((false, None)),
            Err(_) if allow_active => Ok((false, clip)),
            Err(error) => Err(error),
        }
    }
    fn origin(b: [f64; 4], w: u32, h: u32) -> Result<[u32; 2]> {
        if b.iter().any(|v| !v.is_finite())
            || b[0] < 0.
            || b[1] < 0.
            || b[2] > f64::from(w)
            || b[3] > f64::from(h)
            || b[2] - b[0] > 240.
            || b[3] - b[1] > 176.
        {
            return fail("paint bounds unsupported");
        };
        Ok([b[0].floor() as u32, b[1].floor() as u32])
    }
    pub fn main() -> Result<()> {
        // Only direct research runs can set this. The native launcher clears
        // the worker environment before exec.
        let profile_phases = std::env::var_os("OPDS_TEXT_PHASES").is_some();
        let mut phase_start = std::time::Instant::now();
        macro_rules! phase {
            ($name:literal) => {
                if profile_phases {
                    eprintln!(
                        "text phase {}: {:.3} ms",
                        $name,
                        phase_start.elapsed().as_secs_f64() * 1000.
                    );
                }
                phase_start = std::time::Instant::now();
            };
        }
        let a: Vec<_> = std::env::args().collect();
        let worker_mode = a.len() == 6 && a[5] == "--scene-worker" && a[1] == "-" && a[4] == "-";
        if !worker_mode
            && a.len() != 5
            && !(a.len() == 6 && a[5] == "--fractional-research")
            && !(a.len() == 7
                && matches!(
                    a[5].as_str(),
                    "--diagnostics"
                        | "--stage-only"
                        | "--experimental-winding"
                        | "--prepared-winding"
                        | "--fractional-research"
                ))
        {
            return fail("usage: truetype_page_probe INPUT.pdf PAGE SCALE OUTPUT.rgba [--diagnostics DIRECTORY | --stage-only DIRECTORY | --experimental-winding DIRECTORY | --prepared-winding DIRECTORY | --fractional-research DIRECTORY]");
        };
        let fractional_mode = a.get(5).map(String::as_str) == Some("--fractional-research");
        let allow_fractional = worker_mode || fractional_mode;
        let prepared_mode = worker_mode
            || fractional_mode
            || a.get(5).map(String::as_str) == Some("--prepared-winding");
        let winding_mode =
            prepared_mode || a.get(5).map(String::as_str) == Some("--experimental-winding");
        let diagnostic = a.get(6).map(std::path::PathBuf::from);
        let mut b = Vec::new();
        if worker_mode {
            std::io::stdin()
                .lock()
                .take(16_000_001)
                .read_to_end(&mut b)?;
        } else {
            std::fs::File::open(&a[1])?
                .take(16_000_001)
                .read_to_end(&mut b)?;
        }
        if b.len() > 16_000_000 {
            return fail("PDF byte limit");
        };
        phase!("source");
        let page = a[2].parse()?;
        let scale = a[3].parse()?;
        let preflight = opds_dynlex_backend::text_preflight::inspect(&b)?;
        phase!("preflight");
        eprintln!("text preflight: {preflight:?}");
        let b = Arc::new(b);
        let space = PageSpace::from_pdf(b.clone(), page, scale)?;
        phase!("pagespace");
        let [w, h] = space.pixels();
        let doc = Document::load_mem(&b)?;
        phase!("lopdf_load");
        if doc.is_encrypted() {
            return fail("encrypted probe input");
        };
        let (paints, fonts, count, ops) = collect(&doc, page, &space)?;
        phase!("collect");
        eprintln!(
            "staged {count} glyph codes, {} paints, {} fonts, {ops} operators",
            paints.len(),
            fonts.len()
        );
        if let Some(dir) = &diagnostic {
            std::fs::create_dir_all(dir)?;
            let mut text = format!("TPR1\t{w}\t{h}\t{count}\t{ops}\n");
            for (name, f) in &fonts {
                text.push_str(&format!("F\t{name}\t{}\t{}\n", f.upem, f.bytes.len()));
            }
            for (i, p) in paints.iter().enumerate() {
                match p {
                    Paint::Image { source, media, rect, matrix, .. } => text.push_str(&format!(
                        "{i}\tI\t{source:?}\t{media:?}\t{rect:?}\t{matrix:?}\n"
                    )),
                    Paint::Glyph {
                        font,
                        gid,
                        matrix,
                        color,
                        clip,
                        fractional_clip,
                    } => text.push_str(&format!(
                        "{i}\tG\t{font}\t{gid}\t{matrix:?}\t{color:?}\t{clip:?}\t{fractional_clip:?}\n"
                    )),
                    Paint::Rect {
                        points,
                        color,
                        clip,
                        fractional_clip,
                    } => text.push_str(&format!("{i}\tR\t{points:?}\t{color:?}\t{clip:?}\t{fractional_clip:?}\n")),
                }
            }
            std::fs::write(dir.join("staged.tsv"), text)?;
        }
        if a.get(5).map(String::as_str) == Some("--stage-only") {
            return Ok(());
        }
        let preparing = std::time::Instant::now();
        let mut prepared = opds_dynlex_backend::winding_page::PreparedWindingPage::new(w, h)?;
        let mut fractional_clips = Vec::new();
        let mut glyph_cache = BTreeMap::<(String, u16), glyph::PreparedGlyph>::new();
        let mut type1_cache =
            BTreeMap::<(String, u16), type1_worker_glyph::PreparedType1Glyph>::new();
        let mut glyph_bytes = 0_usize;
        let mut layer = if prepared_mode {
            Vec::new()
        } else {
            vec![0; w as usize * h as usize * 4]
        };
        let mut rendered = 0;
        let mut background = None;
        for (paint_index, paint) in paints.into_iter().enumerate() {
            match paint {
                Paint::Image {
                    jpeg,
                    source,
                    media,
                    rect,
                    matrix,
                } => {
                    if !worker_mode || paint_index != 0 || background.is_some() {
                        return fail("hybrid image requires bounded image-first worker");
                    }
                    let image_start = std::time::Instant::now();
                    let image = opds_opdl_adapter::partial_image::render(
                        &jpeg,
                        source,
                        media,
                        rect,
                        matrix,
                        scale as f32,
                    )?;
                    if profile_phases {
                        eprintln!(
                            "text phase image_decode_resample: {:.3} ms",
                            image_start.elapsed().as_secs_f64() * 1000.
                        );
                    }
                    if [image.width, image.height] != [w, h] {
                        return fail("hybrid image/page dimensions disagree");
                    }
                    background = Some(opds_dynlex_backend::Bitmap {
                        width: w,
                        height: h,
                        rgba: image.rgba,
                    });
                }
                Paint::Glyph {
                    font,
                    gid,
                    matrix,
                    color,
                    clip,
                    fractional_clip,
                } => {
                    let f = &fonts[&font];
                    let (mut opm, shift) = if f.encoding == FontEncoding::Type1BuiltIn {
                        if !prepared_mode || !winding_mode || !worker_mode {
                            return fail("Type1 outlines require bounded prepared worker");
                        }
                        let key = (font.clone(), gid);
                        if !type1_cache.contains_key(&key) {
                            if glyph_cache.len() + type1_cache.len() >= 1024 {
                                return fail("prepared glyph cache count");
                            }
                            let outline =
                                type1_worker_glyph::PreparedType1Glyph::new(&f.bytes, gid)?;
                            let bytes = outline.retained_bytes();
                            if glyph_bytes + bytes > 16 * 1024 * 1024 {
                                return fail("prepared glyph cache bytes");
                            }
                            glyph_bytes += bytes;
                            type1_cache.insert(key.clone(), outline);
                        }
                        let Some(staged) = type1_cache[&key].stage(matrix)? else {
                            // An embedded, mapped and fully validated inkless glyph
                            // advances text but contributes no paint.
                            continue;
                        };
                        staged
                    } else if prepared_mode {
                        let key = (font.clone(), gid);
                        if !glyph_cache.contains_key(&key) {
                            if glyph_cache.len() >= 1024 {
                                return fail("prepared glyph cache count");
                            }
                            let outline =
                                glyph::PreparedGlyph::new(&f.bytes, gid, Source::SkrifaAtThousand)?;
                            let bytes = outline.retained_bytes();
                            if glyph_bytes + bytes > 16 * 1024 * 1024 {
                                return fail("prepared glyph cache bytes");
                            }
                            glyph_bytes += bytes;
                            glyph_cache.insert(key.clone(), outline);
                        }
                        let (p, shift, raw) = glyph_cache[&key].stage(matrix)?;
                        if let (Some(dir), Some(raw)) = (&diagnostic, raw) {
                            std::fs::write(dir.join(format!("paint-{paint_index}.opq")), raw)?;
                        }
                        (p, shift)
                    } else {
                        match glyph::stage(&f.bytes, gid, matrix, Source::SkrifaAtThousand) {
                            Ok((p, s, raw)) => {
                                if let Some(dir) = &diagnostic {
                                    std::fs::write(
                                        dir.join(format!("paint-{paint_index}.opq")),
                                        raw,
                                    )?;
                                }
                                (p, s)
                            }
                            Err(_) => glyph::stage_line_only(&f.bytes, gid, matrix)?,
                        }
                    };
                    if let Some(dir) = &diagnostic {
                        std::fs::write(dir.join(format!("paint-{paint_index}.center.opm")), &opm)?;
                    }
                    let b = glyph::absolute_bounds(&opm, shift)?;
                    let (outside, effective_clip) =
                        fractional_paint_clip(b, fractional_clip, allow_fractional)?;
                    if outside {
                        continue;
                    }
                    let mut at = origin(b, w, h)?;
                    if winding_mode {
                        // Preserve the page's 4x4 strip origin in a moved glyph tile.
                        at = at.map(|v| v / 4 * 4);
                    }
                    glyph::rebase_screen_f32(&mut opm, shift, at)?;
                    if let Some(dir) = &diagnostic {
                        std::fs::write(dir.join(format!("paint-{paint_index}.opm")), &opm)?;
                    }
                    if winding_mode {
                        opm[..4].copy_from_slice(b"OPNW");
                        if prepared_mode {
                            prepared.push_fractional_clipped(
                                &opm,
                                color,
                                at,
                                clip,
                                if worker_mode { effective_clip } else { None },
                            )?;
                            if fractional_mode {
                                fractional_clips.push(effective_clip);
                            }
                            rendered += 1;
                            continue;
                        }
                        if clip != [0, 0, w, h] {
                            return fail("clip requires prepared mode");
                        }
                        layer = opds_dynlex_backend::experimental_winding::paint(
                            &opm, color, layer, w, h, at,
                        )
                        .map_err(|e| {
                            format!("research paint {paint_index} font={font} gid={gid}: {e}")
                        })?;
                        rendered += 1;
                        continue;
                    }
                    let mut scene = b"OPGS".to_vec();
                    for n in [1_u32, 256, 192, 1] {
                        scene.extend(n.to_le_bytes())
                    }
                    let mut name = [0; 32];
                    name[..font.len()].copy_from_slice(font.as_bytes());
                    scene.extend(name);
                    scene.extend(gid.to_le_bytes());
                    scene.extend(gid.to_le_bytes());
                    scene.extend(color);
                    scene.push(0);
                    scene.extend((opm.len() as u32).to_le_bytes());
                    scene.extend(opm);
                    layer = render_validated_glyph_scene_over_premultiplied_at_origin_owned(
                        &scene,
                        layer,
                        w,
                        h,
                        [0, 0, w, h],
                        at,
                    )
                    .map_err(|e| {
                        format!("paint {paint_index} font={font} gid={gid} matrix={matrix:?}: {e}")
                    })?
                    .rgba;
                    rendered += 1;
                }
                Paint::Rect {
                    points,
                    color,
                    clip,
                    fractional_clip,
                } => {
                    let (outside, effective_clip) = fractional_paint_clip(
                        bounds(&points).map(|v| f64::from(v as f32)),
                        fractional_clip,
                        allow_fractional,
                    )?;
                    if outside {
                        continue;
                    }
                    if winding_mode {
                        let b = bounds(&points).map(|v| f64::from(v as f32));
                        if b[0] < 0. || b[1] < 0. || b[2] > f64::from(w) || b[3] > f64::from(h) {
                            return fail("research rectangle outside page");
                        }
                        // Disjoint integral tile boundaries introduce no extra
                        // fractional ink edge; retain original outer f32 edges.
                        for ty in (b[1] as u32 / 192)..=(b[3] as u32 / 192) {
                            for tx in (b[0] as u32 / 256)..=(b[2] as u32 / 256) {
                                let at = [tx * 256, ty * 192];
                                let x0 = (b[0] - f64::from(at[0])).max(0.);
                                let y0 = (b[1] - f64::from(at[1])).max(0.);
                                let x1 = (b[2] - f64::from(at[0])).min(256.);
                                let y1 = (b[3] - f64::from(at[1])).min(192.);
                                if x0 >= x1 || y0 >= y1 {
                                    continue;
                                }
                                let mut bytes = b"OPNW".to_vec();
                                for n in [1_u32, 256, 192, 1, 5] {
                                    bytes.extend(n.to_le_bytes());
                                }
                                for p in [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]] {
                                    for v in p {
                                        bytes.extend(v.to_le_bytes());
                                    }
                                }
                                if prepared_mode {
                                    prepared.push_fractional_clipped(
                                        &bytes,
                                        color,
                                        at,
                                        clip,
                                        if worker_mode { effective_clip } else { None },
                                    )?;
                                    if fractional_mode {
                                        fractional_clips.push(effective_clip);
                                    }
                                } else {
                                    if clip != [0, 0, w, h] {
                                        return fail("clip requires prepared mode");
                                    }
                                    layer = opds_dynlex_backend::experimental_winding::paint(
                                        &bytes, color, layer, w, h, at,
                                    )?;
                                }
                            }
                        }
                        continue;
                    }
                    let at = origin(bounds(&points), w, h)?;
                    let mut opm = b"OPM2".to_vec();
                    for n in [1_u32, 256, 192, 1, points.len() as u32] {
                        opm.extend(n.to_le_bytes())
                    }
                    for p in points {
                        for axis in 0..2 {
                            opm.extend(((p[axis] as f32 - at[axis] as f32) as f64).to_le_bytes())
                        }
                    }
                    layer = render_validated_polygon_over_premultiplied_at_origin_owned(
                        &opm,
                        color,
                        layer,
                        w,
                        h,
                        [0, 0, w, h],
                        at,
                    )?
                    .rgba;
                }
            }
        }
        phase!("scene_stage");
        if worker_mode {
            let bytes = if let Some(image) = background {
                opds_dynlex_backend::hybrid_page::PreparedHybridPage::new(image, prepared)?
                    .encode()?
            } else {
                prepared.encode()?
            };
            std::io::stdout().lock().write_all(&bytes)?;
            if profile_phases {
                eprintln!(
                    "text phase encode_write: {:.3} ms",
                    phase_start.elapsed().as_secs_f64() * 1000.
                );
            }
            return Ok(());
        }
        if fractional_mode {
            let bitmap = prepared.render_fractional_research(&fractional_clips)?;
            std::fs::write(&a[4], bitmap.rgba)?;
            eprintln!("{w}x{h} separate fractional clip research; NOT native support");
            return Ok(());
        }
        if prepared_mode {
            let prepare_ms = preparing.elapsed().as_secs_f64() * 1000.;
            let start = std::time::Instant::now();
            layer = prepared.render()?.rgba;
            let render_ms = start.elapsed().as_secs_f64() * 1000.;
            let mut replay_ms = Vec::new();
            let mut sequential_ms = Vec::new();
            // Alternating order, identical prepared geometry and fresh outputs.
            // Pixel comparison is outside both render timers.
            for round in 0..3 {
                for sequential in if round % 2 == 0 {
                    [true, false]
                } else {
                    [false, true]
                } {
                    let start = std::time::Instant::now();
                    let replay = if sequential {
                        prepared.render_sequential_reference()?
                    } else {
                        prepared.render()?
                    }
                    .rgba;
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    if sequential {
                        sequential_ms.push(elapsed);
                    } else {
                        replay_ms.push(elapsed);
                    }
                    if replay != layer {
                        return fail("prepared replay pixels changed");
                    }
                }
            }
            let metrics = format!("{{\"prepare_ms\":{prepare_ms},\"render_ms\":{render_ms},\"replay_ms\":{replay_ms:?},\"sequential_ms\":{sequential_ms:?},\"paints\":{},\"unique_outlines\":{},\"outline_bytes\":{glyph_bytes},\"scene_bytes\":{}}}\n",
                prepared.paint_count(), glyph_cache.len(), prepared.retained_bytes());
            if let Some(dir) = &diagnostic {
                std::fs::write(dir.join("prepared.json"), &metrics)?;
            }
            eprintln!("prepared {metrics}");
        }
        for p in layer.chunks_exact_mut(4) {
            let white = 255 - p[3];
            for c in &mut p[..3] {
                *c = c.saturating_add(white)
            }
            p[3] = 255;
        }
        std::fs::write(&a[4], layer)?;
        eprintln!(
            "{w}x{h} DynLex experimental full page, {rendered} visible glyphs; NOT native support"
        );
        Ok(())
    }
}
#[cfg(feature = "kernel")]
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::main()
}
#[cfg(not(feature = "kernel"))]
fn main() {
    eprintln!("requires kernel feature");
    std::process::exit(1)
}
