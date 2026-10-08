//! Original, opaque single-image page plan. No bitmap derived from a previous zoom.
use crate::{
    fail, geometry_v1, image_bytes, image_to_display, inherited_page_resources, jpeg_rgb,
    select_page_id, AxisCtm,
};
use lopdf::{dictionary, Document, Object, Stream};
use std::error::Error;

pub struct RasterPlan {
    image: Stream,
    geometry: geometry_v1::PageGeometry,
    placement: [u32; 4],
    source: [u32; 2],
}

// This deliberately small grammar keeps original f64 numeric tokens. Unknown
// tokens, compound operands, escaped names and partially consumed input fail.
fn tokens(bytes: &[u8]) -> Result<Vec<&str>, Box<dyn Error>> {
    if bytes.len() > 65_536 {
        return Err(fail("raster content limit"));
    }
    let text = std::str::from_utf8(bytes)?;
    let mut result = Vec::new();
    for line in text.split(['\n', '\r']) {
        for token in line
            .split('%')
            .next()
            .unwrap_or("")
            .split(|c: char| matches!(c, '\0' | '\t' | '\x0c' | ' '))
        {
            if token.is_empty() {
                continue;
            }
            if token.len() > 128 || result.len() >= 1024 {
                return Err(fail("raster token limit"));
            }
            result.push(token);
        }
    }
    Ok(result)
}
fn number(s: &str) -> Result<f64, Box<dyn Error>> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    if !body.bytes().any(|b| b.is_ascii_digit())
        || body.bytes().any(|b| !b.is_ascii_digit() && b != b'.')
        || body.bytes().filter(|&b| b == b'.').count() > 1
    {
        return Err(fail("invalid raster number"));
    }
    let n = s.parse::<f64>()?;
    if !n.is_finite() {
        return Err(fail("nonfinite raster number"));
    }
    Ok(n)
}
impl RasterPlan {
    pub fn from_pdf(bytes: &[u8], page: usize) -> Result<Self, Box<dyn Error>> {
        if bytes.len() > 16_000_000 {
            return Err(fail("raster PDF source limit"));
        }
        let doc = Document::load_mem(bytes)?;
        let id = select_page_id(&doc, Some(page))?;
        let geometry = geometry_v1::page_geometry(&doc, id)?;
        let resources = inherited_page_resources(&doc, id)?;
        if resources
            .iter()
            .any(|(k, _)| ![b"XObject".as_slice(), b"ProcSet"].contains(&k.as_slice()))
        {
            return Err(fail("raster resources outside subset"));
        }
        let (_, content) = doc.dereference(doc.get_dictionary(id)?.get(b"Contents")?)?;
        let content = content.as_stream()?;
        if content.dict.has(b"DecodeParms") || content.dict.has(b"F") {
            return Err(fail("unsupported raster content stream state"));
        }
        let data = if content.dict.has(b"Filter") {
            if content.dict.get(b"Filter")?.as_name()? != b"FlateDecode"
                || content.dict.has(b"DecodeParms")
            {
                return Err(fail("raster content filter unsupported"));
            }
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&content.content, 65_536)
                .map_err(|_| fail("raster content decode limit"))?
        } else {
            if content.content.len() > 65_536 {
                return Err(fail("raster content limit"));
            }
            content.content.clone()
        };
        let mut operands: Vec<&str> = Vec::new();
        let mut stack = Vec::new();
        let mut ctm = AxisCtm::IDENTITY;
        let mut draw = None;
        for t in tokens(&data)? {
            match t {
                "q" if operands.is_empty() && stack.len() < 16 => stack.push(ctm),
                "Q" if operands.is_empty() => {
                    ctm = stack.pop().ok_or_else(|| fail("unbalanced raster Q"))?
                }
                "cm" if operands.len() == 6 => {
                    let mut m = [0.; 6];
                    for (n, t) in m.iter_mut().zip(&operands) {
                        *n = number(t)?;
                    }
                    ctm = ctm.concat(m)?;
                    operands.clear();
                }
                "g" if operands.len() == 1 => {
                    let g = number(operands[0])?;
                    if g != 0. && g != 1. {
                        return Err(fail("unproven gray"));
                    }
                    operands.clear();
                }
                "Do" if operands.len() == 1 && draw.is_none() => {
                    let name = operands[0]
                        .strip_prefix('/')
                        .ok_or_else(|| fail("image name required"))?;
                    if name.is_empty()
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    {
                        return Err(fail("unsupported image name"));
                    }
                    // Do not inherit the legacy adapter's near-integer tolerance.
                    if [ctm.tx, ctm.ty, ctm.sx, ctm.sy]
                        .iter()
                        .any(|n| n.fract() != 0.)
                    {
                        return Err(fail("fractional original raster placement"));
                    }
                    let rect = ctm.pixel_rect([0., 0., 1., 1.], 4096)?;
                    if rect[0] + rect[2] > geometry.media[2]
                        || rect[1] + rect[3] > geometry.media[3]
                    {
                        return Err(fail("raster placement outside media"));
                    }
                    draw = Some((name.to_owned(), rect));
                    operands.clear();
                }
                _ if t.starts_with('/') || number(t).is_ok() => {
                    if operands.len() >= 6 {
                        return Err(fail("too many raster operands"));
                    }
                    operands.push(t);
                }
                _ => return Err(fail("unsupported raster operator or operand count")),
            }
        }
        if !operands.is_empty() || !stack.is_empty() {
            return Err(fail("unfinished raster content"));
        }
        let (name, placement) = draw.ok_or_else(|| fail("missing image paint"))?;
        let (_, xobjects) = doc.dereference(resources.get(b"XObject")?)?;
        let (_, image) = doc.dereference(xobjects.as_dict()?.get(name.as_bytes())?)?;
        let image = image.as_stream()?;
        let keys = [
            b"Type".as_slice(),
            b"Subtype",
            b"Name",
            b"Width",
            b"Height",
            b"BitsPerComponent",
            b"ColorSpace",
            b"Length",
            b"Filter",
        ];
        if image
            .dict
            .iter()
            .any(|(k, _)| !keys.contains(&k.as_slice()))
            || image.dict.get(b"Subtype")?.as_name()? != b"Image"
            || image.dict.get(b"ColorSpace")?.as_name()? != b"DeviceRGB"
            || image.dict.get(b"BitsPerComponent")?.as_i64()? != 8
        {
            return Err(fail("unsupported cached image state"));
        }
        if image.dict.has(b"Name") {
            image.dict.get(b"Name")?.as_name()?;
        }
        let filter = image.dict.get(b"Filter")?;
        let filter = match filter {
            Object::Array(a) if a.len() == 1 => a[0].as_name()?,
            Object::Name(n) => n,
            _ => return Err(fail("unsupported cached image filter")),
        };
        if filter != b"DCTDecode" {
            return Err(fail("only JPEG source cache proven"));
        }
        let source = [
            u32::try_from(image.dict.get(b"Width")?.as_i64()?)?,
            u32::try_from(image.dict.get(b"Height")?.as_i64()?)?,
        ];
        if source.contains(&0)
            || source.iter().any(|&n| n > 8192)
            || source[0]
                .checked_mul(source[1])
                .is_none_or(|n| n > jpeg_rgb::MAX_PIXELS)
        {
            return Err(fail("cached JPEG dimensions exceed budget"));
        }
        Ok(Self {
            image: image.clone(),
            geometry,
            placement,
            source,
        })
    }
    pub fn geometry(&self) -> ([u32; 4], [u32; 4], u32) {
        (self.geometry.media, self.geometry.crop, self.geometry.turn)
    }
    pub fn decoded_len(&self) -> usize {
        self.source[0] as usize * self.source[1] as usize * 3
    }
    pub fn decode(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut doc = Document::with_version("1.7");
        let image = doc.add_object(self.image.clone());
        let resources = dictionary! {"XObject"=>dictionary!{"Image"=>image}};
        let mut budget = jpeg_rgb::MAX_PIXELS;
        Ok(image_bytes(
            &doc,
            &resources,
            b"Image",
            self.source[0],
            self.source[1],
            true,
            false,
            &mut budget,
        )?
        .into_owned())
    }
    pub fn scene(&self, rgb: &[u8], scale: f32) -> Result<Vec<u8>, Box<dyn Error>> {
        if rgb.len() != self.decoded_len()
            || !scale.is_finite()
            || ![0.25, 0.5, 1., 2.].contains(&scale)
        {
            return Err(fail("invalid raster source or scale"));
        }
        let scaled = |n: u32| -> Result<u32, Box<dyn Error>> {
            let n = f64::from(n) * f64::from(scale);
            if n.fract() != 0. || !(0. ..=4096.).contains(&n) {
                return Err(fail("fractional raster edges not proven"));
            }
            Ok(n as u32)
        };
        let mut crop = [0; 4];
        let mut placement = [0; 4];
        for i in 0..4 {
            crop[i] = scaled(self.geometry.crop[i])?;
            placement[i] = scaled(self.placement[i])?;
        }
        let (width, height) = if matches!(self.geometry.turn, 90 | 270) {
            (crop[3] - crop[1], crop[2] - crop[0])
        } else {
            (crop[2] - crop[0], crop[3] - crop[1])
        };
        if width == 0
            || height == 0
            || width.checked_mul(height).is_none_or(|n| n > 6_000_000)
            || placement[2] == 0
            || placement[3] == 0
            || placement[2]
                .checked_mul(placement[3])
                .is_none_or(|n| n > 6_000_000)
        {
            return Err(fail("raster output limit"));
        }
        // Hayro's downsample grid uses f32 advances then ceil, which may be
        // one pixel larger than integer PDF edges (e.g. Albany at 125%).
        // Reject until the independent post-resample sampling matches too.
        if placement[2] < self.source[0] || placement[3] < self.source[1] {
            for axis in 0..2 {
                let advance = ((f64::from(self.placement[axis + 2]) / f64::from(self.source[axis]))
                    * f64::from(scale)) as f32;
                let sample_extent = (self.source[axis] as f32 * advance).ceil();
                if sample_extent != placement[axis + 2] as f32 {
                    return Err(fail("unproven intermediate image sampling extent"));
                }
            }
        }
        let pixels = crate::resample_rgb(
            rgb,
            self.source[0],
            self.source[1],
            placement[2],
            placement[3],
            false,
        )?;
        let (rect, pixels) = image_to_display(&pixels, placement, crop, self.geometry.turn)?;
        let mut scene = b"OPDL".to_vec();
        for n in [
            3, width, height, 0, 0, width, height,
            0, // Ordered scenes have no legacy base image.
            1, 6, rect[0], rect[1], rect[2], rect[3], 0,
        ] {
            scene.extend_from_slice(&n.to_le_bytes());
        }
        scene.extend_from_slice(&pixels);
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_limits_and_original_number_precision() {
        assert_eq!(
            tokens(b"q %comment\r\n 1\t0\x0c0 1\0 0 0 cm Q").unwrap(),
            vec!["q", "1", "0", "0", "1", "0", "0", "cm", "Q"]
        );
        assert_eq!(number("128.00001").unwrap(), 128.00001_f64);
        for s in ["NaN", "inf", "1e2", "--2", ".", "+", "1.2.3", "[]"] {
            assert!(number(s).is_err(), "{s}");
        }
        assert!(tokens(&vec![b' '; 65_537]).is_err());
        assert!(tokens("q ".repeat(1025).as_bytes()).is_err());
        assert!(tokens(&vec![b'1'; 129]).is_err());
    }
}
