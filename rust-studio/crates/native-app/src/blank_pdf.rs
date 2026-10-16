//! In-memory source for the native New command. It is not written until Save As.
use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Paper {
    A0,
    A1,
    A2,
    A3,
    #[default]
    A4,
    A5,
    A6,
    B3,
    B4,
    B5,
    Letter,
    Legal,
    Tabloid,
    Ledger,
    Custom,
}

impl Paper {
    pub const ALL: [Self; 15] = [
        Self::A0,
        Self::A1,
        Self::A2,
        Self::A3,
        Self::A4,
        Self::A5,
        Self::A6,
        Self::B3,
        Self::B4,
        Self::B5,
        Self::Letter,
        Self::Legal,
        Self::Tabloid,
        Self::Ledger,
        Self::Custom,
    ];

    pub fn points(self) -> Option<(f64, f64)> {
        Some(match self {
            Self::A0 => (2384.0, 3370.0),
            Self::A1 => (1684.0, 2384.0),
            Self::A2 => (1191.0, 1684.0),
            Self::A3 => (842.0, 1191.0),
            Self::A4 => (595.0, 842.0),
            Self::A5 => (420.0, 595.0),
            Self::A6 => (298.0, 420.0),
            Self::B3 => (1001.0, 1417.0),
            Self::B4 => (709.0, 1001.0),
            Self::B5 => (499.0, 709.0),
            Self::Letter => (612.0, 792.0),
            Self::Legal => (612.0, 1008.0),
            Self::Tabloid => (792.0, 1224.0),
            Self::Ledger => (1224.0, 792.0),
            Self::Custom => return None,
        })
    }
}

impl fmt::Display for Paper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::A0 => "A0",
            Self::A1 => "A1",
            Self::A2 => "A2",
            Self::A3 => "A3",
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::A6 => "A6",
            Self::B3 => "B3",
            Self::B4 => "B4",
            Self::B5 => "B5",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
            Self::Tabloid => "Tabloid",
            Self::Ledger => "Ledger",
            Self::Custom => "Custom",
        })
    }
}

pub fn dimensions(
    paper: Paper,
    landscape: bool,
    custom_mm: (f64, f64),
) -> Result<(f64, f64), String> {
    let (mut width, mut height) = match paper.points() {
        Some(points) => points,
        None => {
            let (width_mm, height_mm) = custom_mm;
            if ![width_mm, height_mm]
                .iter()
                .all(|value| value.is_finite() && (10.0..=5000.0).contains(value))
            {
                return Err("Custom width and height must be 10–5000 mm".into());
            }
            (width_mm * 72.0 / 25.4, height_mm * 72.0 / 25.4)
        }
    };
    if landscape {
        std::mem::swap(&mut width, &mut height);
    }
    Ok((width, height))
}

#[cfg(test)]
pub fn a4_portrait() -> Result<Vec<u8>, String> {
    create(595.0, 842.0, 1)
}

pub fn create(width: f64, height: f64, page_count: u16) -> Result<Vec<u8>, String> {
    if !width.is_finite()
        || !height.is_finite()
        || !(10.0..=14_400.0).contains(&width)
        || !(10.0..=14_400.0).contains(&height)
        || !(1..=999).contains(&page_count)
    {
        return Err("Blank PDF dimensions or page count are out of range".into());
    }
    let mut pdf = Document::with_version("1.7");
    let pages = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(Dictionary::new(), Vec::new()));
    let mut kids = Vec::with_capacity(page_count as usize);
    for _ in 0..page_count {
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
            "Contents" => content, "Resources" => Dictionary::new(),
        });
        kids.push(Object::Reference(page));
    }
    pdf.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => i64::from(page_count),
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", Object::Reference(catalog));
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Blank PDF exceeds the source size limit".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_a4_is_one_blank_page_in_memory() {
        let bytes = a4_portrait().unwrap();
        let pdf = Document::load_mem(&bytes).unwrap();
        assert_eq!(pdf.get_pages().len(), 1);
        let page = *pdf.get_pages().values().next().unwrap();
        assert!(pdf.get_page_content(page).unwrap().is_empty());
        let parsed =
            opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(bytes.clone())).unwrap();
        assert_eq!(parsed.page_count(), 1);
        assert_eq!(parsed.page_dimensions(1).unwrap(), (595.0, 842.0));
        let native = crate::render_page_selected(
            &parsed,
            &bytes,
            1,
            1.0,
            opds_dynlex_backend::Backend::Dynlex,
        )
        .unwrap();
        let reference = parsed.render_page(1, 1.0).unwrap();
        assert_eq!(native.raster.rgba, reference.rgba);
    }

    #[test]
    fn dimensions_and_page_counts_are_bounded() {
        assert_eq!(
            dimensions(Paper::A3, true, (0.0, 0.0)).unwrap(),
            (1191.0, 842.0)
        );
        assert_eq!(
            dimensions(Paper::Letter, false, (0.0, 0.0)).unwrap(),
            (612.0, 792.0)
        );
        assert!(dimensions(Paper::Custom, false, (9.0, 297.0)).is_err());
        assert!(dimensions(Paper::Custom, false, (f64::NAN, 297.0)).is_err());
        assert!(create(595.0, 842.0, 0).is_err());
        assert!(create(595.0, 842.0, 1000).is_err());
        let bytes = create(792.0, 612.0, 3).unwrap();
        let pdf = Document::load_mem(&bytes).unwrap();
        assert_eq!(pdf.get_pages().len(), 3);
        for page in pdf.get_pages().values() {
            assert!(pdf.get_page_content(*page).unwrap().is_empty());
        }
    }
}
