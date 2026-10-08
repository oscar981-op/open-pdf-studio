//! Bounded pure-Rust JPEG codec; PDF placement/composition remains separate.
//! No Hayro/Vello rendering. Header/dictionary agreement precedes pixel allocation.
use zune_jpeg::{
    zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
    JpegDecoder,
};

pub(crate) const MAX_PIXELS: u32 = 40_000_000;

pub(crate) fn decode(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, &'static str> {
    if data.len() > 16_000_000
        || !data.starts_with(&[0xff, 0xd8])
        || !data.ends_with(&[0xff, 0xd9])
        || width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS)
    {
        return Err("JPEG source exceeds bounds or lacks complete markers");
    }
    let expected = width as usize * height as usize * 3;
    let options = DecoderOptions::default()
        .set_max_width(8192)
        .set_max_height(8192)
        .jpeg_set_max_scans(16)
        .set_strict_mode(true)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    decoder
        .decode_headers()
        .map_err(|_| "invalid bounded JPEG headers")?;
    let info = decoder.info().ok_or("missing JPEG info")?;
    if u32::from(info.width) != width
        || u32::from(info.height) != height
        || info.components != 3
        || !matches!(
            decoder.input_colorspace(),
            Some(ColorSpace::RGB | ColorSpace::YCbCr)
        )
        || decoder.output_buffer_size() != Some(expected)
    {
        return Err("JPEG dimensions/components disagree with DeviceRGB8 dictionary");
    }
    let pixels = decoder.decode().map_err(|_| "invalid bounded JPEG data")?;
    if pixels.len() != expected {
        return Err("wrong JPEG RGB output length");
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_rgb_decode_and_invalid_headers() {
        for bytes in [
            include_bytes!("../tests/fixtures/rgb8-small.jpg").as_slice(),
            include_bytes!("../tests/fixtures/rgb8-progressive.jpg").as_slice(),
        ] {
            assert_eq!(decode(bytes, 32, 24).unwrap().len(), 32 * 24 * 3);
            for (w, h) in [(0, 24), (33, 24), (32, 23), (8193, 24), (8192, 8192)] {
                assert!(decode(bytes, w, h).is_err());
            }
            for end in [0, 2, 10, bytes.len() / 2, bytes.len() - 2] {
                assert!(decode(&bytes[..end], 32, 24).is_err());
            }
            let mut truncated = bytes[..bytes.len() / 2].to_vec();
            truncated.extend_from_slice(&[0xff, 0xd9]);
            assert!(decode(&truncated, 32, 24).is_err());
        }
    }
}

#[cfg(test)]
mod budget_tests {
    #[test]
    fn repeated_jpeg_draws_share_one_page_budget() {
        use lopdf::{dictionary, Document, Stream};
        let mut doc = Document::with_version("1.7");
        let image = doc.add_object(Stream::new(dictionary! {"Subtype"=>"Image","Width"=>32,"Height"=>24,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8,"Filter"=>"DCTDecode"}, include_bytes!("../tests/fixtures/rgb8-small.jpg").to_vec()));
        let resources = dictionary! {"XObject"=>dictionary!{"Scan"=>image}};
        let mut remaining = 2 * 32 * 24;
        for _ in 0..2 {
            assert!(crate::image_bytes(
                &doc,
                &resources,
                b"Scan",
                32,
                24,
                true,
                false,
                &mut remaining
            )
            .is_ok());
        }
        assert_eq!(remaining, 0);
        let error = crate::image_bytes(
            &doc,
            &resources,
            b"Scan",
            32,
            24,
            true,
            false,
            &mut remaining,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cumulative JPEG"));
    }
}
