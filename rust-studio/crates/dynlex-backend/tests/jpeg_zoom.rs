#![cfg(feature = "kernel")]
use lopdf::{dictionary, Document, Object, Stream};
use opds_dynlex_backend::DynlexDocument;
use opds_engine_probe::ParsedPdf;
use std::sync::Arc;

fn source(turn: i64, contents: &str, page_count: usize) -> Vec<u8> {
    source_image(
        turn,
        contents,
        page_count,
        include_bytes!("../../opdl-adapter/tests/fixtures/rgb8-small.jpg"),
        32,
        24,
    )
}
fn source_image(
    turn: i64,
    contents: &str,
    page_count: usize,
    jpeg: &[u8],
    width: i64,
    height: i64,
) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let image = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>width,"Height"=>height,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8,"Filter"=>"DCTDecode"}, jpeg.to_vec()));
    let contents = doc.add_object(Stream::new(dictionary! {}, contents.as_bytes().to_vec()));
    let mut kids = Vec::new();
    for _ in 0..page_count {
        kids.push(Object::Reference(doc.add_object(
            dictionary! {"Type"=>"Page","Parent"=>pages,"Contents"=>contents},
        )));
    }
    doc.objects.insert(pages,Object::Dictionary(dictionary!{"Type"=>"Pages","Kids"=>kids,"Count"=>page_count as i64,"MediaBox"=>vec![0.into(),0.into(),128.into(),96.into()],"CropBox"=>vec![4.into(),4.into(),124.into(),92.into()],"Rotate"=>turn,"Resources"=>dictionary!{"XObject"=>dictionary!{"Scan"=>image}}}));
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
const CONTENT: &str = "q 128 0 0 96 0 0 cm 1 g /Scan Do Q";

#[test]
fn original_jpeg_reused_across_zoom_rotation_crop_and_margins() {
    for turn in [0, 90, 180, 270] {
        for content in [
            CONTENT,
            "q 64 0 0 48 16 16 cm /Scan Do Q",
            "q 32 0 0 24 16 16 cm /Scan Do Q",
            "q 16 0 0 12 16 16 cm /Scan Do Q",
        ] {
            let bytes = Arc::new(source(turn, content, 1));
            let oracle = ParsedPdf::open(bytes.clone()).unwrap();
            let actual = DynlexDocument::new(bytes);
            for scale in [0.25, 1., 2., 0.5, 1.25, 1.] {
                let expected = oracle.render_page(1, scale).unwrap();
                if scale == 1.25 && content != "q 16 0 0 12 16 16 cm /Scan Do Q" {
                    assert!(actual.try_render(1, scale).unwrap().is_none());
                    continue;
                }
                let bitmap = actual
                    .try_render(1, scale)
                    .unwrap()
                    .expect("supported JPEG zoom");
                assert_eq!(
                    (bitmap.width, bitmap.height),
                    (expected.width, expected.height)
                );
                assert_eq!(
                    bitmap.rgba, expected.rgba,
                    "turn={turn} scale={scale} content={content}"
                );
            }
            assert_eq!(actual.raster_cache_stats().unwrap(), (32 * 24 * 3, 1));
        }
    }
}

#[test]
fn document_cache_evicts_previous_page_and_serializes_concurrent_decode() {
    let doc = Arc::new(DynlexDocument::new(Arc::new(source(0, CONTENT, 5))));
    std::thread::scope(|s| {
        for scale in [0.25, 1., 2., 0.5] {
            let doc = doc.clone();
            s.spawn(move || assert!(doc.try_render(1, scale).unwrap().is_some()));
        }
    });
    assert_eq!(doc.raster_cache_stats().unwrap(), (2304, 1));
    for (page, count) in [(2, 2), (3, 3), (4, 4), (5, 5), (1, 6)] {
        assert!(doc.try_render(page, 1.).unwrap().is_some());
        assert_eq!(doc.raster_cache_stats().unwrap(), (2304, count));
    }
}

#[test]
fn unsafe_scale_and_unproven_content_never_produce_zoom_bitmap() {
    let doc = DynlexDocument::new(Arc::new(source(0, CONTENT, 1)));
    for scale in [f32::NAN, f32::INFINITY, 0., -1., 0.3, 9.] {
        assert!(doc.try_render(1, scale).unwrap().is_none());
    }
    for content in [
        "q 128.00001 0 0 96 0 0 cm /Scan Do Q",
        "q 128 0 0 96 0 0 cm /Scan Do",
        "q 128 0 0 96 0 0 cm /Scan Do Q Q",
        "q 128 0 0 96 0 0 cm /Scan Do /Scan Do Q",
        "q 128 0 0 96 0 0 cm /Scan Do Q 0 0 8 8 re f",
        "q 128 0 0 96 0 0 cm /Scan Do Q BT ET",
        "q 128 0 0 96 0 0 cm .5 g /Scan Do Q",
        "q 128 0 0 96 0 0 cm /Scan Do Q []",
    ] {
        let doc = DynlexDocument::new(Arc::new(source(0, content, 1)));
        assert!(doc.try_render(1, 2.).unwrap().is_none(), "{content}");
    }
}

#[test]
fn ceil_intermediate_grid_and_second_sampling_match_with_crop_and_rotation() {
    let jpeg = include_bytes!("../../opdl-adapter/tests/fixtures/rgb81-grid.jpg");
    for turn in [0, 90, 180, 270] {
        for x in [0, 16, 44] {
            for y in [0, 16, 44] {
                let content = format!("q 36 0 0 36 {x} {y} cm /Scan Do Q");
                let bytes = Arc::new(source_image(turn, &content, 1, jpeg, 81, 81));
                let oracle = ParsedPdf::open(bytes.clone()).unwrap();
                let actual = DynlexDocument::new(bytes)
                    .try_render(1, 1.25)
                    .unwrap()
                    .unwrap();
                let expected = oracle.render_page(1, 1.25).unwrap();
                assert!(
                    actual.rgba == expected.rgba,
                    "turn={turn} x={x} y={y}: {} differing channels",
                    actual
                        .rgba
                        .iter()
                        .zip(&expected.rgba)
                        .filter(|(a, b)| a != b)
                        .count()
                );
            }
        }
    }
}

#[test]
fn fractional_sampling_probe_rotation_crop_diagnosis() {
    use opds_dynlex_backend::fractional_probe::FractionalProbe;
    let jpeg = include_bytes!("../../opdl-adapter/tests/fixtures/rgb81-grid.jpg");
    for turn in [0, 90, 180, 270] {
        let bytes = Arc::new(source_image(turn, CONTENT, 1, jpeg, 81, 81));
        let expected = ParsedPdf::open(bytes.clone()).unwrap();
        let probe = FractionalProbe::new(bytes, 1).unwrap();
        for scale in [0.13, 0.21, 0.33, 0.47, 0.6] {
            let oracle = expected.render_page(1, scale).unwrap();
            let actual = probe.render(scale, true).unwrap();
            assert_eq!((actual.width, actual.height), (oracle.width, oracle.height));
            let differences = actual
                .rgba
                .iter()
                .zip(&oracle.rgba)
                .filter(|(a, b)| a != b)
                .count();
            println!("fractional turn={turn} scale={scale} differing_channels={differences}");
            assert_eq!(differences, 0, "turn={turn} scale={scale}");
        }
        assert_eq!(probe.decoded_bytes(), 81 * 81 * 3);
        for scale in [0., f32::NAN, f32::INFINITY, -1., 8., 1.25] {
            assert!(probe.render(scale, true).is_err());
        }
    }
}
