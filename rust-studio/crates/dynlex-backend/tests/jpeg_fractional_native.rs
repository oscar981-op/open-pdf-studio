#![cfg(feature = "kernel")]
use lopdf::{dictionary, Document, Object, Stream};
use opds_dynlex_backend::DynlexDocument;
use opds_engine_probe::ParsedPdf;
use std::sync::Arc;

fn source(turn: i64) -> Arc<Vec<u8>> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>81, "Height"=>81,
            "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8, "Filter"=>"DCTDecode"
        },
        include_bytes!("../../opdl-adapter/tests/fixtures/rgb81-grid.jpg").to_vec(),
    ));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 128 0 0 96 0 0 cm /Scan Do Q".to_vec(),
    ));
    let page = doc.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Contents"=>content});
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type"=>"Pages", "Count"=>1, "Kids"=>vec![page.into()],
            "MediaBox"=>vec![0.into(),0.into(),128.into(),96.into()],
            "CropBox"=>vec![4.into(),4.into(),124.into(),92.into()], "Rotate"=>turn,
            "Resources"=>dictionary! {"XObject"=>dictionary! {"Scan"=>image}}
        }),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    Arc::new(bytes)
}

#[test]
fn native_fractional_downsampling_is_exact_and_reuses_document_cache() {
    for turn in [0, 90, 180, 270] {
        let bytes = source(turn);
        let expected = ParsedPdf::open(bytes.clone()).unwrap();
        let actual = DynlexDocument::new(bytes);
        for scale in [0.13, 0.21, 0.33, 0.47, 0.6] {
            let a = actual.try_render(1, scale).unwrap().unwrap();
            let b = expected.render_page(1, scale).unwrap();
            assert_eq!((a.width, a.height), (b.width, b.height));
            assert!(a.rgba == b.rgba, "turn={turn} scale={scale}");
        }
        assert_eq!(actual.raster_cache_stats().unwrap(), (81 * 81 * 3, 1));
    }
}

#[test]
fn concurrent_fractional_zooms_share_one_decode() {
    let document = Arc::new(DynlexDocument::new(source(0)));
    std::thread::scope(|scope| {
        for scale in [0.13, 0.21, 0.33, 0.47, 0.6] {
            let doc = document.clone();
            scope.spawn(move || assert!(doc.try_render(1, scale).unwrap().is_some()));
        }
    });
    assert_eq!(document.raster_cache_stats().unwrap(), (81 * 81 * 3, 1));
}
