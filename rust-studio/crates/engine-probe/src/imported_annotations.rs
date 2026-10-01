//! Display fallback for a small set of imported PDF annotations without /AP.
//! This is preview-only: saving and printing preserve the source dictionaries.

use hayro::hayro_syntax::object::{Array, Dict, Name};
use hayro::hayro_syntax::page::Page;

#[derive(Clone, Debug)]
pub(super) enum ImportedAnnotation {
    Square {
        rect: [f64; 4],
        color: [u8; 3],
        width: f64,
    },
    Highlight {
        quads: Vec<[[f64; 2]; 4]>,
        color: [u8; 3],
    },
}

fn numbers(array: Array<'_>) -> Vec<f64> {
    array.iter::<f64>().collect()
}

fn rect(dict: &Dict<'_>) -> Option<[f64; 4]> {
    let values = numbers(dict.get::<Array<'_>>(b"Rect")?);
    let [x0, y0, x1, y1] = values.as_slice() else {
        return None;
    };
    if !values.iter().all(|value| value.is_finite()) || x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some([*x0, *y0, *x1, *y1])
}

fn color(dict: &Dict<'_>, default: [u8; 3]) -> [u8; 3] {
    let Some(values) = dict.get::<Array<'_>>(b"C").map(numbers) else {
        return default;
    };
    let [r, g, b] = values.as_slice() else {
        return default;
    };
    if !values.iter().all(|value| value.is_finite()) {
        return default;
    }
    [r, g, b].map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn border_width(dict: &Dict<'_>) -> f64 {
    let width = dict
        .get::<Dict<'_>>(b"BS")
        .and_then(|border| border.get::<f64>(b"W"))
        .or_else(|| {
            dict.get::<Array<'_>>(b"Border")
                .and_then(|border| numbers(border).get(2).copied())
        })
        .unwrap_or(1.0);
    if width.is_finite() {
        width.clamp(0.0, 20.0)
    } else {
        1.0
    }
}

pub(super) fn read(page: &Page<'_>) -> Vec<ImportedAnnotation> {
    let Some(entries) = page.raw().get::<Array<'_>>(b"Annots") else {
        return Vec::new();
    };
    entries
        .iter::<Dict<'_>>()
        .filter_map(|dict| {
            // Hayro draws an existing appearance itself. Never overlay it twice.
            if dict.contains_key(b"AP") {
                return None;
            }
            let flags = dict.get::<i32>(b"F").unwrap_or(0);
            if flags & (1 | 2 | 32) != 0 {
                return None;
            }
            let bounds = rect(&dict)?;
            let subtype = dict.get::<Name<'_>>(b"Subtype")?;
            match subtype.as_ref() {
                b"Square" => Some(ImportedAnnotation::Square {
                    rect: bounds,
                    color: color(&dict, [0, 0, 0]),
                    width: border_width(&dict),
                }),
                b"Highlight" => {
                    let quads = dict
                        .get::<Array<'_>>(b"QuadPoints")
                        .map(numbers)
                        .filter(|values| values.len() % 8 == 0 && !values.is_empty())
                        .map(|values| {
                            values
                                .chunks_exact(8)
                                .filter(|chunk| chunk.iter().all(|value| value.is_finite()))
                                .map(|chunk| {
                                    [
                                        [chunk[0], chunk[1]],
                                        [chunk[2], chunk[3]],
                                        [chunk[6], chunk[7]],
                                        [chunk[4], chunk[5]],
                                    ]
                                })
                                .collect::<Vec<_>>()
                        })
                        .filter(|quads| !quads.is_empty())
                        .unwrap_or_else(|| {
                            vec![[
                                [bounds[0], bounds[3]],
                                [bounds[2], bounds[3]],
                                [bounds[2], bounds[1]],
                                [bounds[0], bounds[1]],
                            ]]
                        });
                    Some(ImportedAnnotation::Highlight {
                        quads,
                        color: color(&dict, [255, 255, 0]),
                    })
                }
                _ => None,
            }
        })
        .collect()
}

fn mapped(point: [f64; 2], transform: [f64; 6], scale: f64) -> [f64; 2] {
    [
        (transform[0] * point[0] + transform[2] * point[1] + transform[4]) * scale,
        (transform[1] * point[0] + transform[3] * point[1] + transform[5]) * scale,
    ]
}

fn blend(rgba: &mut [u8], width: u32, x: usize, y: usize, color: [u8; 3], opacity: f64) {
    let offset = (y * width as usize + x) * 4;
    for channel in 0..3 {
        rgba[offset + channel] = (f64::from(rgba[offset + channel]) * (1.0 - opacity)
            + f64::from(color[channel]) * opacity)
            .round() as u8;
    }
}

fn inside_quad(point: [f64; 2], quad: [[f64; 2]; 4]) -> bool {
    let mut sign = 0_i8;
    for index in 0..4 {
        let a = quad[index];
        let b = quad[(index + 1) % 4];
        let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        if cross.abs() < 1e-8 {
            continue;
        }
        let current = if cross > 0.0 { 1 } else { -1 };
        if sign != 0 && sign != current {
            return false;
        }
        sign = current;
    }
    true
}

pub(super) fn paint(
    annotations: &[ImportedAnnotation],
    rgba: &mut [u8],
    width: u32,
    height: u32,
    transform: [f64; 6],
    scale: f64,
) {
    for annotation in annotations {
        let (points, color, stroke) = match annotation {
            ImportedAnnotation::Square { rect, color, width } => (
                vec![
                    mapped([rect[0], rect[1]], transform, scale),
                    mapped([rect[2], rect[3]], transform, scale),
                ],
                *color,
                Some(*width * scale),
            ),
            ImportedAnnotation::Highlight { quads, color } => {
                for quad in quads {
                    let quad = quad.map(|point| mapped(point, transform, scale));
                    let min_x = quad.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                    let max_x = quad.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
                    let min_y = quad.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                    let max_y = quad.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
                    if ![min_x, max_x, min_y, max_y].iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    for y in min_y.floor().max(0.0) as usize
                        ..max_y.ceil().min(f64::from(height)) as usize
                    {
                        for x in min_x.floor().max(0.0) as usize
                            ..max_x.ceil().min(f64::from(width)) as usize
                        {
                            if inside_quad([x as f64 + 0.5, y as f64 + 0.5], quad) {
                                blend(rgba, width, x, y, *color, 1.0);
                            }
                        }
                    }
                }
                continue;
            }
        };
        let x0 = points[0][0].min(points[1][0]);
        let x1 = points[0][0].max(points[1][0]);
        let y0 = points[0][1].min(points[1][1]);
        let y1 = points[0][1].max(points[1][1]);
        if ![x0, x1, y0, y1].iter().all(|v| v.is_finite()) {
            continue;
        }
        let stroke = stroke.unwrap_or(1.0).max(1.0);
        for y in y0.floor().max(0.0) as usize..y1.ceil().min(f64::from(height)) as usize {
            for x in x0.floor().max(0.0) as usize..x1.ceil().min(f64::from(width)) as usize {
                let center = [x as f64 + 0.5, y as f64 + 0.5];
                if center[0] - x0 < stroke
                    || x1 - center[0] < stroke
                    || center[1] - y0 < stroke
                    || y1 - center[1] < stroke
                {
                    blend(rgba, width, x, y, color, 1.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParsedPdf;
    use lopdf::{dictionary, Dictionary, Document, Object, Stream};
    use std::sync::Arc;

    fn fixture() -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
        let square_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![60.into(),80.into(),100.into(),120.into()],
            "C" => vec![0.into(),1.into(),0.into()],
            "BS" => dictionary! { "W" => 3 },
        });
        let highlight_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Highlight",
            "Rect" => vec![150.into(),150.into(),180.into(),170.into()],
            "QuadPoints" => vec![150.into(),170.into(),180.into(),170.into(),150.into(),150.into(),180.into(),150.into()],
            "C" => vec![1.into(),1.into(),0.into()],
        });
        let appearance_id = doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),20.into(),20.into()] },
            b"q 0 0 1 RG 3 w 1 1 18 18 re S Q".to_vec(),
        ));
        let existing_appearance_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![175.into(),180.into(),195.into(),200.into()],
            "C" => vec![1.into(),0.into(),0.into()],
            "AP" => dictionary! { "N" => Object::Reference(appearance_id) },
        });
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "Contents" => content_id,
            "Annots" => vec![Object::Reference(square_id),Object::Reference(highlight_id),Object::Reference(existing_appearance_id)],
            "MediaBox" => vec![20.into(),30.into(),220.into(),330.into()],
            "CropBox" => vec![40.into(),60.into(),200.into(),300.into()],
            "Rotate" => 90,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn rgb(raster: &crate::Raster, x: usize, y: usize) -> [u8; 3] {
        let offset = (y * raster.width as usize + x) * 4;
        raster.rgba[offset..offset + 3].try_into().unwrap()
    }

    #[test]
    fn fallback_draws_only_missing_appearances_on_rotated_crop() {
        let pdf = ParsedPdf::open(Arc::new(fixture())).unwrap();
        let page = &pdf.pdf.pages()[0];
        assert_eq!(read(page).len(), 2);
        let raster = pdf.render_page(1, 1.0).unwrap();
        assert_eq!((raster.width, raster.height), (240, 160));
        assert_eq!(rgb(&raster, 20, 40), [0, 255, 0]);
        assert_eq!(rgb(&raster, 40, 40), [255, 255, 255]);
        assert_eq!(rgb(&raster, 100, 125), [255, 255, 0]);
        assert_ne!(rgb(&raster, 130, 145), [255, 0, 0]);
    }
}
