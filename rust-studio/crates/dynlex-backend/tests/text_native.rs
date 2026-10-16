#![cfg(all(feature = "kernel", target_os = "linux"))]
use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use opds_dynlex_backend::{Backend, DynlexDocument};
use opds_engine_probe::ParsedPdf;
use std::{path::PathBuf, sync::Arc};

fn source() -> Arc<Vec<u8>> {
    let mut d = Document::with_version("1.7");
    let pages = d.new_object_id();
    let font_bytes = include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
    let file = d.add_object(Stream::new(Dictionary::new(), font_bytes.to_vec()));
    let descriptor=d.add_object(dictionary!{"Type"=>"FontDescriptor","FontName"=>"LiberationSans","Flags"=>32,"FontBBox"=>vec![Object::from(-543),Object::from(-303),Object::from(1301),Object::from(980)],"ItalicAngle"=>0,"Ascent"=>905,"Descent"=>-212,"CapHeight"=>688,"StemV"=>80,"FontFile2"=>file});
    let font=d.add_object(dictionary!{"Type"=>"Font","Subtype"=>"TrueType","BaseFont"=>"LiberationSans","Encoding"=>"WinAnsiEncoding","FontDescriptor"=>descriptor,"FirstChar"=>32,"LastChar"=>151,"Widths"=>vec![Object::from(500);120]});
    let mut kids = Vec::new();
    for content in [
        b"0 0 1 rg BT /F 16 Tf 1 0 0 1 20 100 Tm (A A) Tj ET".as_slice(),
        b"BT /F 16 Tf 1 Tr 1 0 0 1 20 100 Tm (A) Tj ET".as_slice(),
    ] {
        let stream = d.add_object(Stream::new(Dictionary::new(), content.to_vec()));
        let page=d.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![Object::from(0),Object::from(0),Object::from(256),Object::from(192)],"Resources"=>dictionary!{"Font"=>dictionary!{"F"=>font}},"Contents"=>stream});
        kids.push(Object::from(page));
    }
    d.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Count"=>2,"Kids"=>kids}.into(),
    );
    let catalog = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    d.trailer.set("Root", catalog);
    let mut b = Vec::new();
    d.save_to(&mut b).unwrap();
    Arc::new(b)
}
fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_opds-text-worker"))
}
#[test]
fn no_op_flate_predictor_preserves_strict_text_pixels() {
    let original = source();
    let mut pdf = Document::load_mem(original.as_slice()).unwrap();
    let page = *pdf.get_pages().get(&1).unwrap();
    let content_id = pdf.get_page_contents(page)[0];
    let stream = pdf
        .get_object_mut(content_id)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = miniz_oxide::deflate::compress_to_vec_zlib(&stream.content, 6);
    stream.dict.set("Filter", "FlateDecode");
    stream
        .dict
        .set("DecodeParms", dictionary! {"Predictor" => 1});
    stream.dict.set("Length", stream.content.len() as i64);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let bytes = Arc::new(bytes);
    let reference = ParsedPdf::open(bytes.clone()).unwrap();
    let document = DynlexDocument::with_text_worker(bytes, worker());
    for scale in [0.25, 1.0, 2.0] {
        let actual = document.render_strict(&reference, 1, scale).unwrap();
        assert_eq!(actual.used, Backend::Dynlex);
        assert_eq!(
            actual.raster.rgba,
            reference.render_page(1, scale).unwrap().rgba
        );
    }
    pdf.get_object_mut(content_id)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("DecodeParms", dictionary! {"Predictor" => 2});
    let mut invalid = Vec::new();
    pdf.save_to(&mut invalid).unwrap();
    let invalid = DynlexDocument::with_text_worker(Arc::new(invalid), worker());
    assert!(invalid.try_render(1, 1.0).is_err());
}
#[test]
fn invisible_link_annotation_keeps_text_pixels_and_visible_link_rejects() {
    let original = source();
    let mut pdf = Document::load_mem(original.as_slice()).unwrap();
    let page = *pdf.get_pages().get(&1).unwrap();
    let link = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![Object::from(15),Object::from(90),Object::from(95),Object::from(115)],
        "Border" => vec![Object::from(0),Object::from(0),Object::from(0)],
        "BS" => dictionary! {"Type"=>"Border","S"=>"S","W"=>0},
        "C" => vec![Object::from(0),Object::from(1),Object::from(0)],
        "A" => dictionary! {"S"=>"URI","URI"=>Object::string_literal("https://example.org")}
    });
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![Object::from(link)]);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let bytes = Arc::new(bytes);
    let reference = ParsedPdf::open(bytes.clone()).unwrap();
    let document = DynlexDocument::with_text_worker(bytes, worker());
    for scale in [0.25, 1.0, 2.0] {
        let actual = document.render_strict(&reference, 1, scale).unwrap();
        assert_eq!(actual.used, Backend::Dynlex);
        assert_eq!(
            actual.raster.rgba,
            reference.render_page(1, scale).unwrap().rgba
        );
    }

    let mut visible = pdf;
    visible.get_dictionary_mut(link).unwrap().set(
        "Border",
        vec![Object::from(0), Object::from(0), Object::from(1)],
    );
    let mut bytes = Vec::new();
    visible.save_to(&mut bytes).unwrap();
    let unsupported = DynlexDocument::with_text_worker(Arc::new(bytes), worker());
    assert!(unsupported
        .try_render(1, 1.)
        .unwrap_err()
        .contains("No fallback rendered"));
}
#[test]
fn strict_text_cache_zoom_page_rejection_and_recovery() {
    let bytes = source();
    let reference = ParsedPdf::open(bytes.clone()).unwrap();
    let document = DynlexDocument::with_text_worker(bytes, worker());
    for scale in [1., 1., 1.25, 0.87] {
        let actual = document.render_strict(&reference, 1, scale).unwrap();
        assert_eq!(actual.used, Backend::Dynlex);
        assert_eq!(
            actual.raster.rgba,
            reference.render_page(1, scale).unwrap().rgba
        );
    }
    assert_eq!(document.text_cache_stats().unwrap().unwrap().0, 3);
    assert!(document
        .render_strict(&reference, 2, 1.)
        .unwrap_err()
        .contains("No fallback rendered"));
    let (preparations, retained) = document.text_cache_stats().unwrap().unwrap();
    assert_eq!(preparations, 4);
    assert!(retained > 0 && retained <= 32 * 1024 * 1024);
    assert!(document.render_strict(&reference, 2, 1.).is_err());
    assert_eq!(document.text_cache_stats().unwrap().unwrap().0, 4);
    assert_eq!(
        document
            .render_strict(&reference, 1, 1.)
            .unwrap()
            .raster
            .rgba,
        reference.render_page(1, 1.).unwrap().rgba
    );
    assert_eq!(document.text_cache_stats().unwrap().unwrap().0, 4);
    let before = document.text_cache_stats().unwrap();
    assert!(document.try_render(1, f32::NAN).unwrap().is_none());
    assert_eq!(document.text_cache_stats().unwrap(), before);
}
#[test]
fn concurrent_native_requests_prepare_once_and_missing_helper_rejects() {
    let bytes = source();
    let document = DynlexDocument::with_text_worker(bytes.clone(), worker());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| document.try_render(1, 1.).unwrap().unwrap().rgba))
            .collect();
        let output = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert!(output.windows(2).all(|p| p[0] == p[1]));
    });
    assert_eq!(document.text_cache_stats().unwrap().unwrap().0, 1);
    let missing = DynlexDocument::with_text_worker(
        bytes,
        worker().with_file_name("does-not-exist-opds-worker"),
    );
    assert!(missing
        .try_render(1, 1.)
        .unwrap_err()
        .contains("No fallback rendered"));
}
