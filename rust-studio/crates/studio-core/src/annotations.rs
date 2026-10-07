//! Document-scoped annotations with reversible edits and no active-tab globals.
use crate::{DocumentId, DocumentView, ViewError};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite())
            && self.width > 0.0
            && self.height > 0.0
            && (self.x + self.width).is_finite()
            && (self.y + self.height).is_finite()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Rectangle {
        color_rgb: [u8; 3],
    },
    Line {
        start: [f64; 2],
        end: [f64; 2],
        color_rgb: [u8; 3],
    },
    Polyline {
        points: Vec<[f64; 2]>,
        color_rgb: [u8; 3],
    },
    Text {
        value: String,
        color_rgb: [u8; 3],
        font_size: u16,
        align: TextAlign,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

pub const TEXT_FONT_SIZES: [u16; 16] =
    [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72];
pub const DEFAULT_TEXT_FONT_SIZE: u16 = 16;
pub const MAX_POLYLINE_POINTS: usize = 64;

fn distance_to_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length_squared = dx * dx + dy * dy;
    if !length_squared.is_finite() || length_squared == 0.0 {
        return f64::INFINITY;
    }
    let t = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length_squared)
        .clamp(0.0, 1.0);
    (point[0] - start[0] - t * dx).hypot(point[1] - start[1] - t * dy)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub id: u64,
    pub page: usize,  // zero-based
    pub bounds: Rect, // page points, top-left origin
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Add(Annotation),
    Replace(Annotation),
    Remove(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    DuplicateId,
    UnknownId,
    InvalidPage,
    InvalidGeometry,
    InvalidTextSize,
    GenerationExhausted,
}

impl From<ViewError> for EditError {
    fn from(value: ViewError) -> Self {
        match value {
            ViewError::GenerationExhausted => Self::GenerationExhausted,
            _ => Self::InvalidPage,
        }
    }
}

/// The identity belongs to the document, never to the selected UI tab.
/// IDs must not be reused within a document, even after undo or deletion.
#[derive(Clone, Debug)]
pub struct DocumentModel {
    view: DocumentView,
    items: BTreeMap<u64, Annotation>,
    used_ids: std::collections::BTreeSet<u64>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl DocumentModel {
    pub fn new(id: DocumentId, pages: usize) -> Result<Self, ViewError> {
        Ok(Self {
            view: DocumentView::new(id, pages)?,
            items: BTreeMap::new(),
            used_ids: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
    pub fn zoom_at(&mut self, scale: f64, anchor: [f64; 2]) -> Result<(), ViewError> {
        self.view.zoom_at(scale, anchor)
    }
    pub fn go_to_page(&mut self, page: usize) -> Result<(), ViewError> {
        self.view.go_to_page(page)
    }
    pub fn view(&self) -> &DocumentView {
        &self.view
    }
    pub fn items(&self) -> impl Iterator<Item = &Annotation> {
        self.items.values()
    }
    pub fn get(&self, id: u64) -> Option<&Annotation> {
        self.items.get(&id)
    }
    /// The newest annotation at a page point, allowing a small screen-sized
    /// tolerance around thin rectangle borders and text boxes.
    pub fn hit_test(&self, page: usize, point: [f64; 2], tolerance: f64) -> Option<u64> {
        if !point.iter().all(|value| value.is_finite()) || !tolerance.is_finite() || tolerance < 0.0
        {
            return None;
        }
        self.items.iter().rev().find_map(|(id, annotation)| {
            let bounds = annotation.bounds;
            let in_bounds = annotation.page == page
                && point[0] >= bounds.x - tolerance
                && point[0] <= bounds.x + bounds.width + tolerance
                && point[1] >= bounds.y - tolerance
                && point[1] <= bounds.y + bounds.height + tolerance;
            if !in_bounds {
                return None;
            }
            match &annotation.kind {
                Kind::Line { start, end, .. } => {
                    (distance_to_segment(point, *start, *end) <= tolerance + 1.0).then_some(*id)
                }
                Kind::Polyline { points, .. } => points
                    .windows(2)
                    .any(|pair| distance_to_segment(point, pair[0], pair[1]) <= tolerance + 1.0)
                    .then_some(*id),
                _ => Some(*id),
            }
        })
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn apply(&mut self, edit: Edit) -> Result<(), EditError> {
        let inverse = self.apply_raw(edit, false)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }
    pub fn undo(&mut self) -> Result<bool, EditError> {
        let Some(edit) = self.undo.pop() else {
            return Ok(false);
        };
        match self.apply_raw(edit.clone(), true) {
            Ok(inverse) => {
                self.redo.push(inverse);
                Ok(true)
            }
            Err(error) => {
                self.undo.push(edit);
                Err(error)
            }
        }
    }
    pub fn redo(&mut self) -> Result<bool, EditError> {
        let Some(edit) = self.redo.pop() else {
            return Ok(false);
        };
        match self.apply_raw(edit.clone(), true) {
            Ok(inverse) => {
                self.undo.push(inverse);
                Ok(true)
            }
            Err(error) => {
                self.redo.push(edit);
                Err(error)
            }
        }
    }
    fn check(&self, item: &Annotation) -> Result<(), EditError> {
        if item.page >= self.view.page_count() {
            return Err(EditError::InvalidPage);
        }
        if !item.bounds.valid() {
            return Err(EditError::InvalidGeometry);
        }
        if let Kind::Text { font_size, .. } = item.kind {
            if !TEXT_FONT_SIZES.contains(&font_size) {
                return Err(EditError::InvalidTextSize);
            }
        }
        if let Kind::Line { start, end, .. } = &item.kind {
            if !start
                .iter()
                .chain(end.iter())
                .all(|value| value.is_finite())
                || start == end
                || [start, end].iter().any(|point| {
                    point[0] < item.bounds.x
                        || point[0] > item.bounds.x + item.bounds.width
                        || point[1] < item.bounds.y
                        || point[1] > item.bounds.y + item.bounds.height
                })
            {
                return Err(EditError::InvalidGeometry);
            }
        }
        if let Kind::Polyline { points, .. } = &item.kind {
            if !(2..=MAX_POLYLINE_POINTS).contains(&points.len())
                || points.iter().any(|point| {
                    !point.iter().all(|value| value.is_finite())
                        || point[0] < item.bounds.x
                        || point[0] > item.bounds.x + item.bounds.width
                        || point[1] < item.bounds.y
                        || point[1] > item.bounds.y + item.bounds.height
                })
                || points
                    .windows(2)
                    .any(|pair| (pair[0][0] - pair[1][0]).hypot(pair[0][1] - pair[1][1]) < 1.0)
            {
                return Err(EditError::InvalidGeometry);
            }
        }
        Ok(())
    }
    fn apply_raw(&mut self, edit: Edit, replay: bool) -> Result<Edit, EditError> {
        let inverse = match &edit {
            Edit::Add(item) => {
                self.check(item)?;
                if self.items.contains_key(&item.id)
                    || (self.used_ids.contains(&item.id) && !replay)
                {
                    return Err(EditError::DuplicateId);
                }
                Edit::Remove(item.id)
            }
            Edit::Replace(item) => {
                self.check(item)?;
                Edit::Replace(
                    self.items
                        .get(&item.id)
                        .ok_or(EditError::UnknownId)?
                        .clone(),
                )
            }
            Edit::Remove(id) => Edit::Add(self.items.get(id).ok_or(EditError::UnknownId)?.clone()),
        };
        // The PDF bitmap and the native annotation overlay are separate.
        // Editing the overlay must not discard an in-flight page render.
        match edit {
            Edit::Add(item) => {
                self.used_ids.insert(item.id);
                self.items.insert(item.id, item);
            }
            Edit::Replace(item) => {
                self.items.insert(item.id, item);
            }
            Edit::Remove(id) => {
                self.items.remove(&id);
            }
        }
        Ok(inverse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn annotation(id: u64) -> Annotation {
        Annotation {
            id,
            page: 0,
            bounds: Rect {
                x: 2.0,
                y: 3.0,
                width: 10.0,
                height: 20.0,
            },
            kind: Kind::Text {
                value: "first".into(),
                color_rgb: [0, 0, 0],
                font_size: DEFAULT_TEXT_FONT_SIZE,
                align: TextAlign::Left,
            },
        }
    }
    #[test]
    fn undo_redo_and_new_edit_clear_redo() {
        let mut doc = DocumentModel::new(DocumentId(1), 2).unwrap();
        doc.apply(Edit::Add(annotation(7))).unwrap();
        let mut changed = annotation(7);
        changed.bounds.x = 50.0;
        doc.apply(Edit::Replace(changed.clone())).unwrap();
        assert_eq!(doc.get(7).unwrap().bounds.x, 50.0);
        assert!(doc.undo().unwrap());
        assert_eq!(doc.get(7).unwrap().bounds.x, 2.0);
        assert!(doc.redo().unwrap());
        assert_eq!(doc.get(7).unwrap().bounds.x, 50.0);
        assert!(doc.undo().unwrap());
        doc.apply(Edit::Remove(7)).unwrap();
        assert!(!doc.can_redo());
        assert!(doc.undo().unwrap());
        assert_eq!(doc.get(7), Some(&annotation(7)));
    }
    #[test]
    fn invalid_edits_leave_contents_and_render_ticket_unchanged() {
        let mut doc = DocumentModel::new(DocumentId(1), 1).unwrap();
        doc.apply(Edit::Add(annotation(7))).unwrap();
        let ticket = doc.view().ticket();
        let mut invalid = annotation(8);
        invalid.bounds.x = f64::NAN;
        assert_eq!(
            doc.apply(Edit::Add(invalid)),
            Err(EditError::InvalidGeometry)
        );
        assert_eq!(
            doc.apply(Edit::Add(annotation(7))),
            Err(EditError::DuplicateId)
        );
        invalid = annotation(9);
        invalid.page = 1;
        assert_eq!(doc.apply(Edit::Add(invalid)), Err(EditError::InvalidPage));
        invalid = annotation(10);
        if let Kind::Text { font_size, .. } = &mut invalid.kind {
            *font_size = 0;
        }
        assert_eq!(
            doc.apply(Edit::Add(invalid)),
            Err(EditError::InvalidTextSize)
        );
        assert_eq!(doc.apply(Edit::Remove(50)), Err(EditError::UnknownId));
        assert_eq!(doc.view().ticket(), ticket);
        assert_eq!(doc.items().count(), 1);
    }
    #[test]
    fn overlay_edits_keep_in_flight_page_render_valid() {
        let mut doc = DocumentModel::new(DocumentId(1), 1).unwrap();
        let ticket = doc.view().ticket();
        doc.apply(Edit::Add(annotation(1))).unwrap();
        assert!(doc.view().accepts(ticket));
        doc.undo().unwrap();
        assert!(doc.view().accepts(ticket));
    }
    #[test]
    fn ids_are_not_reused_after_delete_or_undo() {
        let mut doc = DocumentModel::new(DocumentId(1), 1).unwrap();
        doc.apply(Edit::Add(annotation(1))).unwrap();
        doc.undo().unwrap();
        assert_eq!(
            doc.apply(Edit::Add(annotation(1))),
            Err(EditError::DuplicateId)
        );
    }
    #[test]
    fn hit_test_selects_topmost_annotation_on_correct_page() {
        let mut doc = DocumentModel::new(DocumentId(1), 2).unwrap();
        doc.apply(Edit::Add(annotation(1))).unwrap();
        let mut top = annotation(2);
        top.bounds.x = 4.0;
        top.page = 1;
        doc.apply(Edit::Add(top.clone())).unwrap();
        assert_eq!(doc.hit_test(0, [5.0, 5.0], 0.0), Some(1));
        assert_eq!(doc.hit_test(1, [5.0, 5.0], 0.0), Some(2));
        top.page = 0;
        doc.apply(Edit::Replace(top)).unwrap();
        assert_eq!(doc.hit_test(0, [5.0, 5.0], 0.0), Some(2));
        assert_eq!(doc.hit_test(1, [5.0, 5.0], 0.0), None);
        assert_eq!(doc.hit_test(0, [f64::NAN, 5.0], 1.0), None);
    }

    #[test]
    fn line_hit_test_uses_stroke_instead_of_bounding_box() {
        let mut doc = DocumentModel::new(DocumentId(3), 1).unwrap();
        let line = Annotation {
            id: 8,
            page: 0,
            bounds: Rect {
                x: 8.0,
                y: 18.0,
                width: 104.0,
                height: 84.0,
            },
            kind: Kind::Line {
                start: [10.0, 20.0],
                end: [110.0, 100.0],
                color_rgb: [231, 76, 60],
            },
        };
        doc.apply(Edit::Add(line.clone())).unwrap();
        assert_eq!(doc.hit_test(0, [60.0, 60.0], 2.0), Some(8));
        assert_eq!(doc.hit_test(0, [60.0, 30.0], 2.0), None);
        let mut invalid = line;
        invalid.id = 9;
        if let Kind::Line { end, .. } = &mut invalid.kind {
            *end = [115.0, 100.0];
        }
        assert_eq!(
            doc.apply(Edit::Add(invalid)),
            Err(EditError::InvalidGeometry)
        );
    }

    #[test]
    fn polyline_hit_test_checks_each_segment_and_rejects_invalid_points() {
        let mut doc = DocumentModel::new(DocumentId(4), 1).unwrap();
        let polyline = Annotation {
            id: 9,
            page: 0,
            bounds: Rect {
                x: 8.0,
                y: 8.0,
                width: 104.0,
                height: 104.0,
            },
            kind: Kind::Polyline {
                points: vec![[10.0, 10.0], [110.0, 10.0], [110.0, 110.0]],
                color_rgb: [231, 76, 60],
            },
        };
        doc.apply(Edit::Add(polyline.clone())).unwrap();
        assert_eq!(doc.hit_test(0, [50.0, 11.0], 2.0), Some(9));
        assert_eq!(doc.hit_test(0, [109.0, 50.0], 2.0), Some(9));
        assert_eq!(doc.hit_test(0, [50.0, 50.0], 2.0), None);
        let mut invalid = polyline;
        invalid.id = 10;
        if let Kind::Polyline { points, .. } = &mut invalid.kind {
            points.push([110.0, 110.0]);
        }
        assert_eq!(
            doc.apply(Edit::Add(invalid)),
            Err(EditError::InvalidGeometry)
        );
    }

    #[test]
    fn text_edit_is_reversible_without_losing_geometry() {
        let mut doc = DocumentModel::new(DocumentId(1), 1).unwrap();
        let original = annotation(1);
        doc.apply(Edit::Add(original.clone())).unwrap();
        let mut revised = original.clone();
        revised.kind = Kind::Text {
            value: "revised".into(),
            color_rgb: [0, 102, 204],
            font_size: 24,
            align: TextAlign::Center,
        };
        doc.apply(Edit::Replace(revised.clone())).unwrap();
        assert_eq!(doc.get(1), Some(&revised));
        assert!(doc.undo().unwrap());
        assert_eq!(doc.get(1), Some(&original));
        assert!(doc.redo().unwrap());
        assert_eq!(doc.get(1), Some(&revised));
    }
}
