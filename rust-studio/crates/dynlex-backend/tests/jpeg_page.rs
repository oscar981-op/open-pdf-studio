#![cfg(feature = "kernel")]
use lopdf::{dictionary, Document, Object, Stream};
use opds_dynlex_backend::DynlexDocument;
use opds_engine_probe::ParsedPdf;
use std::sync::Arc;

fn source(jpeg: &[u8], rotate: i64, array_filter: bool, placement: [u32; 4]) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let filter = if array_filter {
        Object::Array(vec![Object::Name(b"DCTDecode".to_vec())])
    } else {
        Object::Name(b"DCTDecode".to_vec())
    };
    let image = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Name"=>"Scan", "Width"=>32, "Height"=>24, "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8, "Filter"=>filter}, jpeg.to_vec()));
    let [x, y, w, h] = placement;
    let contents = format!("0 0 1 rg\n0 0 128 96 re\nf\nq\n{w} 0 0 {h} {x} {y} cm\n1 g\n/Scan Do\nQ\n0 80 16 16 re\nf\n");
    let contents = doc.add_object(Stream::new(dictionary! {}, contents.into_bytes()));
    let page = doc.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages,"Contents"=>contents});
    doc.objects.insert(pages,Object::Dictionary(dictionary!{"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1,"MediaBox"=>vec![0.into(),0.into(),128.into(),96.into()],"CropBox"=>vec![4.into(),4.into(),124.into(),92.into()],"Rotate"=>rotate,"Resources"=>dictionary!{"XObject"=>dictionary!{"Scan"=>image}}}));
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn jpeg_pages_match_hayro_with_layers_rotations_crop_and_gray_restore() {
    for jpeg in [
        include_bytes!("../../opdl-adapter/tests/fixtures/rgb8-small.jpg").as_slice(),
        include_bytes!("../../opdl-adapter/tests/fixtures/rgb8-progressive.jpg").as_slice(),
    ] {
        for turn in [0, 90, 180, 270] {
            for placement in [[16, 16, 32, 24], [16, 16, 64, 48], [16, 16, 16, 12]] {
                for array in [false, true] {
                    let bytes = Arc::new(source(jpeg, turn, array, placement));
                    let oracle = ParsedPdf::open(bytes.clone()).unwrap();
                    let actual = DynlexDocument::new(bytes)
                        .render_strict(&oracle, 1, 1.)
                        .unwrap();
                    let expected = oracle.render_page(1, 1.).unwrap();
                    assert_eq!(
                        (actual.raster.width, actual.raster.height),
                        (expected.width, expected.height)
                    );
                    assert_eq!(
                        actual.raster.rgba, expected.rgba,
                        "turn={turn} placement={placement:?} array={array}"
                    );
                }
            }
        }
    }
}

#[test]
fn unsupported_jpeg_states_produce_no_dynlex_bitmap() {
    let jpeg = include_bytes!("../../opdl-adapter/tests/fixtures/rgb8-small.jpg");
    let original = source(jpeg, 0, false, [16, 16, 32, 24]);
    for (key, value) in [
        ("Width", Object::Integer(33)),
        ("Height", Object::Integer(0)),
        ("ColorSpace", Object::Name(b"DeviceCMYK".to_vec())),
        ("BitsPerComponent", Object::Integer(4)),
        (
            "DecodeParms",
            Object::Dictionary(dictionary! {"ColorTransform"=>0}),
        ),
        ("SMask", Object::Name(b"None".to_vec())),
        ("Interpolate", Object::Boolean(true)),
        (
            "Filter",
            Object::Array(vec!["FlateDecode".into(), "DCTDecode".into()]),
        ),
    ] {
        let mut doc = Document::load_mem(&original).unwrap();
        let image = doc
            .objects
            .values_mut()
            .find_map(|o| {
                o.as_stream_mut().ok().filter(|s| {
                    s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Image")
                })
            })
            .unwrap();
        image.dict.set(key, value);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(
            DynlexDocument::new(Arc::new(bytes))
                .try_render(1, 1.)
                .unwrap()
                .is_none(),
            "{key}"
        );
    }
}
