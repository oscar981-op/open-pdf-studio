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
    Ellipse {
        color_rgb: [u8; 3],
    },
    /// The Tauri shape tool's default six-sided polygon, fitted to bounds.
    Polygon {
        color_rgb: [u8; 3],
    },
    Line {
        start: [f64; 2],
        end: [f64; 2],
        color_rgb: [u8; 3],
    },
    Arrow {
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
pub const MAX_POLYLINE_POINTS: usize = 512;

pub fn polygon_vertices(bounds: Rect) -> [[f64; 2]; 6] {
    let cx = bounds.x + bounds.width / 2.0;
    let cy = bounds.y + bounds.height / 2.0;
    let rx = bounds.width / 2.0;
    let ry = bounds.height / 2.0;
    std::array::from_fn(|index| {
        let angle = index as f64 * std::f64::consts::PI / 3.0 - std::f64::consts::FRAC_PI_2;
        [cx + rx * angle.cos(), cy + ry * angle.sin()]
    })
}

/// Open arrowhead in page coordinates; shared by hit testing, UI and PDF export.
pub fn arrow_wings(start: [f64; 2], end: [f64; 2]) -> Option<[[f64; 2]; 2]> {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = dx.hypot(dy);
    if !length.is_finite() || length < 6.0 {
        return None;
    }
    let ux = dx / length;
    let uy = dy / length;
    let head = length.mul_add(0.4, 0.0).min(10.0);
    let base = [end[0] - ux * head, end[1] - uy * head];
    let spread = head * 0.5;
    Some([
        [base[0] - uy * spread, base[1] + ux * spread],
        [base[0] + uy * spread, base[1] - ux * spread],
    ])
}

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
    /// Add a bounded group as one atomic, undoable document edit.
    AddMany(Vec<Annotation>),
    Replace(Annotation),
    Remove(u64),
    /// Internal inverse of AddMany; callers may not submit this as a new edit.
    RemoveMany(Vec<u64>),
    /// Atomically remove native annotations from one page or the whole document.
    Clear {
        page: Option<usize>,
    },
    /// Undo data produced by Clear; callers may not submit this as a new edit.
    Restore {
        page: Option<usize>,
        items: Vec<Annotation>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    DuplicateId,
    UnknownId,
    InvalidPage,
    InvalidGeometry,
    InvalidTextSize,
    InvalidRestore,
    InvalidBatch,
    GenerationExhausted,
}

/// A document command may run only against the state that was observed when
/// it was made. The render generation and annotation revision are separate:
/// editing an overlay must not invalidate an in-flight page bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentVersion {
    pub id: DocumentId,
    pub generation: u64,
    pub annotation_revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DocumentCommand {
    Edit(Edit),
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    StaleDocument,
    Edit(EditError),
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
    annotation_revision: u64,
    items: BTreeMap<u64, Annotation>,
    used_ids: std::collections::BTreeSet<u64>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl DocumentModel {
    pub fn new(id: DocumentId, pages: usize) -> Result<Self, ViewError> {
        Ok(Self {
            view: DocumentView::new(id, pages)?,
            annotation_revision: 0,
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
    /// Changes on every committed annotation edit, including undo and redo.
    /// Overlay edits deliberately do not invalidate the PDF render ticket.
    pub fn annotation_revision(&self) -> u64 {
        self.annotation_revision
    }
    pub fn version(&self) -> DocumentVersion {
        DocumentVersion {
            id: self.view.document_id(),
            generation: self.view.generation(),
            annotation_revision: self.annotation_revision,
        }
    }
    /// Shared transaction entry point for UI and future external commands.
    /// A stale command has no effect on annotations or undo history.
    pub fn execute(
        &mut self,
        expected: DocumentVersion,
        command: DocumentCommand,
    ) -> Result<bool, CommandError> {
        if self.version() != expected {
            return Err(CommandError::StaleDocument);
        }
        match command {
            DocumentCommand::Edit(edit) => {
                self.apply(edit).map_err(CommandError::Edit)?;
                Ok(true)
            }
            DocumentCommand::Undo => self.undo().map_err(CommandError::Edit),
            DocumentCommand::Redo => self.redo().map_err(CommandError::Edit),
        }
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
                Kind::Ellipse { .. } => {
                    let rx = bounds.width / 2.0;
                    let ry = bounds.height / 2.0;
                    let dx = point[0] - bounds.x - rx;
                    let dy = point[1] - bounds.y - ry;
                    let normalized = (dx / rx).hypot(dy / ry);
                    (normalized > 0.0
                        && ((1.0 - 1.0 / normalized) * dx.hypot(dy)).abs() <= tolerance + 1.0)
                        .then_some(*id)
                }
                Kind::Polygon { .. } => {
                    let vertices = polygon_vertices(bounds);
                    vertices
                        .iter()
                        .zip(vertices.iter().cycle().skip(1))
                        .take(vertices.len())
                        .any(|(start, end)| {
                            distance_to_segment(point, *start, *end) <= tolerance + 1.0
                        })
                        .then_some(*id)
                }
                Kind::Line { start, end, .. } => {
                    (distance_to_segment(point, *start, *end) <= tolerance + 1.0).then_some(*id)
                }
                Kind::Arrow { start, end, .. } => arrow_wings(*start, *end).and_then(|wings| {
                    (distance_to_segment(point, *start, *end) <= tolerance + 1.0
                        || wings
                            .iter()
                            .any(|wing| distance_to_segment(point, *wing, *end) <= tolerance + 1.0))
                    .then_some(*id)
                }),
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
        if let Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } = &item.kind {
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
            if matches!(item.kind, Kind::Arrow { .. }) {
                let Some(wings) = arrow_wings(*start, *end) else {
                    return Err(EditError::InvalidGeometry);
                };
                if wings.iter().any(|point| {
                    point[0] < item.bounds.x
                        || point[0] > item.bounds.x + item.bounds.width
                        || point[1] < item.bounds.y
                        || point[1] > item.bounds.y + item.bounds.height
                }) {
                    return Err(EditError::InvalidGeometry);
                }
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
            Edit::AddMany(items) => {
                if items.is_empty() || items.len() > 4096 {
                    return Err(EditError::InvalidBatch);
                }
                let mut ids = std::collections::BTreeSet::new();
                for item in items {
                    self.check(item)?;
                    if !ids.insert(item.id)
                        || self.items.contains_key(&item.id)
                        || (self.used_ids.contains(&item.id) && !replay)
                    {
                        return Err(EditError::DuplicateId);
                    }
                }
                Edit::RemoveMany(items.iter().map(|item| item.id).collect())
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
            Edit::RemoveMany(ids) => {
                if !replay || ids.is_empty() || ids.len() > 4096 {
                    return Err(EditError::InvalidBatch);
                }
                let mut seen = std::collections::BTreeSet::new();
                let mut items = Vec::with_capacity(ids.len());
                for id in ids {
                    if !seen.insert(*id) {
                        return Err(EditError::InvalidBatch);
                    }
                    items.push(self.items.get(id).ok_or(EditError::UnknownId)?.clone());
                }
                Edit::AddMany(items)
            }
            Edit::Clear { page } => {
                if page.is_some_and(|page| page >= self.view.page_count()) {
                    return Err(EditError::InvalidPage);
                }
                Edit::Restore {
                    page: *page,
                    items: self
                        .items
                        .values()
                        .filter(|item| page.is_none_or(|page| item.page == page))
                        .cloned()
                        .collect(),
                }
            }
            Edit::Restore { page, items } => {
                if !replay || page.is_some_and(|page| page >= self.view.page_count()) {
                    return Err(EditError::InvalidRestore);
                }
                let mut ids = std::collections::BTreeSet::new();
                for item in items {
                    self.check(item)?;
                    if page.is_some_and(|page| item.page != page)
                        || !ids.insert(item.id)
                        || self.items.contains_key(&item.id)
                        || !self.used_ids.contains(&item.id)
                    {
                        return Err(EditError::InvalidRestore);
                    }
                }
                Edit::Clear { page: *page }
            }
        };
        let next_revision = self
            .annotation_revision
            .checked_add(1)
            .ok_or(EditError::GenerationExhausted)?;
        // The PDF bitmap and the native annotation overlay are separate.
        // Editing the overlay must not discard an in-flight page render.
        match edit {
            Edit::Add(item) => {
                self.used_ids.insert(item.id);
                self.items.insert(item.id, item);
            }
            Edit::AddMany(items) => {
                for item in items {
                    self.used_ids.insert(item.id);
                    self.items.insert(item.id, item);
                }
            }
            Edit::Replace(item) => {
                self.items.insert(item.id, item);
            }
            Edit::Remove(id) => {
                self.items.remove(&id);
            }
            Edit::RemoveMany(ids) => {
                for id in ids {
                    self.items.remove(&id);
                }
            }
            Edit::Clear { page } => {
                self.items
                    .retain(|_, item| page.is_some_and(|page| item.page != page));
            }
            Edit::Restore { items, .. } => {
                for item in items {
                    self.items.insert(item.id, item);
                }
            }
        }
        self.annotation_revision = next_revision;
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
    fn ellipse_is_a_real_editable_annotation_with_outline_hit_testing() {
        let mut doc = DocumentModel::new(DocumentId(42), 1).unwrap();
        let mut item = annotation(1);
        item.kind = Kind::Ellipse {
            color_rgb: [231, 76, 60],
        };
        doc.execute(doc.version(), DocumentCommand::Edit(Edit::Add(item)))
            .unwrap();
        assert_eq!(doc.hit_test(0, [12.0, 13.0], 0.0), Some(1));
        assert_eq!(doc.hit_test(0, [7.0, 13.0], 0.0), None);
        assert!(doc.execute(doc.version(), DocumentCommand::Undo).unwrap());
        assert!(doc.get(1).is_none());
        assert!(doc.execute(doc.version(), DocumentCommand::Redo).unwrap());
        assert!(matches!(doc.get(1).unwrap().kind, Kind::Ellipse { .. }));
    }
    #[test]
    fn arrow_requires_its_open_head_inside_bounds_and_is_undoable() {
        let mut doc = DocumentModel::new(DocumentId(43), 1).unwrap();
        let start = [20.0, 20.0];
        let end = [80.0, 20.0];
        let wings = arrow_wings(start, end).unwrap();
        let mut item = annotation(1);
        item.bounds = Rect {
            x: 18.0,
            y: 13.0,
            width: 64.0,
            height: 14.0,
        };
        item.kind = Kind::Arrow {
            start,
            end,
            color_rgb: [231, 76, 60],
        };
        doc.execute(
            doc.version(),
            DocumentCommand::Edit(Edit::Add(item.clone())),
        )
        .unwrap();
        assert_eq!(doc.hit_test(0, wings[0], 0.0), Some(1));
        assert!(doc.execute(doc.version(), DocumentCommand::Undo).unwrap());
        assert!(doc.get(1).is_none());
        assert!(doc.execute(doc.version(), DocumentCommand::Redo).unwrap());
        let mut clipped = item;
        clipped.id = 2;
        clipped.bounds = Rect {
            x: 18.0,
            y: 18.0,
            width: 64.0,
            height: 4.0,
        };
        assert_eq!(
            doc.apply(Edit::Add(clipped)),
            Err(EditError::InvalidGeometry)
        );
    }

    #[test]
    fn add_many_is_atomic_stale_checked_and_one_undo_step() {
        let mut doc = DocumentModel::new(DocumentId(31), 2).unwrap();
        let before = doc.version();
        let first = annotation(10);
        let second = annotation(11);
        doc.execute(
            before,
            DocumentCommand::Edit(Edit::AddMany(vec![first.clone(), second.clone()])),
        )
        .unwrap();
        assert_eq!(doc.items().count(), 2);
        assert_eq!(
            doc.execute(
                before,
                DocumentCommand::Edit(Edit::AddMany(vec![annotation(12)]))
            ),
            Err(CommandError::StaleDocument)
        );
        assert!(doc.execute(doc.version(), DocumentCommand::Undo).unwrap());
        assert_eq!(doc.items().count(), 0);
        assert!(!doc.can_undo());
        assert!(doc.execute(doc.version(), DocumentCommand::Redo).unwrap());
        assert_eq!(doc.get(10), Some(&first));
        assert_eq!(doc.get(11), Some(&second));

        let version = doc.version();
        let mut invalid = annotation(13);
        invalid.page = 2;
        assert_eq!(
            doc.execute(
                version,
                DocumentCommand::Edit(Edit::AddMany(vec![annotation(12), invalid]))
            ),
            Err(CommandError::Edit(EditError::InvalidPage))
        );
        assert_eq!(doc.version(), version);
        assert!(doc.get(12).is_none());
        assert_eq!(
            doc.execute(
                version,
                DocumentCommand::Edit(Edit::AddMany(vec![annotation(12), annotation(12)]))
            ),
            Err(CommandError::Edit(EditError::DuplicateId))
        );
        assert_eq!(doc.version(), version);
        assert_eq!(
            doc.execute(
                version,
                DocumentCommand::Edit(Edit::RemoveMany(vec![10, 11]))
            ),
            Err(CommandError::Edit(EditError::InvalidBatch))
        );
    }
    #[test]
    fn delayed_document_commands_cannot_change_a_newer_document_or_undo_history() {
        let mut doc = DocumentModel::new(DocumentId(41), 2).unwrap();
        let before_page_change = doc.version();
        doc.go_to_page(1).unwrap();
        assert_eq!(
            doc.execute(
                before_page_change,
                DocumentCommand::Edit(Edit::Add(annotation(7)))
            ),
            Err(CommandError::StaleDocument)
        );
        let before_edit = doc.version();
        assert!(doc
            .execute(before_edit, DocumentCommand::Edit(Edit::Add(annotation(7))))
            .unwrap());
        assert_eq!(
            doc.execute(before_edit, DocumentCommand::Undo),
            Err(CommandError::StaleDocument)
        );
        assert!(doc.get(7).is_some());
        assert!(doc.can_undo());
        let newer = doc.version();
        assert!(doc.execute(newer, DocumentCommand::Undo).unwrap());
        assert!(doc.get(7).is_none());
        assert!(doc.can_redo());
        let other_document = DocumentVersion {
            id: DocumentId(40),
            ..doc.version()
        };
        assert_eq!(
            doc.execute(other_document, DocumentCommand::Redo),
            Err(CommandError::StaleDocument)
        );
        assert!(doc.can_redo());
        let current = doc.version();
        assert!(doc.execute(current, DocumentCommand::Redo).unwrap());
        assert!(doc.get(7).is_some());
    }
    #[test]
    fn clear_page_and_all_are_single_undoable_stale_checked_transactions() {
        let mut doc = DocumentModel::new(DocumentId(42), 2).unwrap();
        for (id, page) in [(1, 0), (2, 0), (3, 1)] {
            let mut item = annotation(id);
            item.page = page;
            doc.execute(doc.version(), DocumentCommand::Edit(Edit::Add(item)))
                .unwrap();
        }
        let stale = doc.version();
        doc.execute(stale, DocumentCommand::Edit(Edit::Clear { page: Some(0) }))
            .unwrap();
        assert!(doc.get(1).is_none() && doc.get(2).is_none());
        assert!(doc.get(3).is_some());
        assert_eq!(
            doc.execute(stale, DocumentCommand::Undo),
            Err(CommandError::StaleDocument)
        );
        doc.execute(doc.version(), DocumentCommand::Undo).unwrap();
        assert_eq!(doc.items().count(), 3);
        doc.execute(doc.version(), DocumentCommand::Redo).unwrap();
        assert_eq!(doc.items().count(), 1);
        doc.execute(
            doc.version(),
            DocumentCommand::Edit(Edit::Clear { page: None }),
        )
        .unwrap();
        assert_eq!(doc.items().count(), 0);
        doc.execute(doc.version(), DocumentCommand::Undo).unwrap();
        assert!(doc.get(3).is_some());
        assert_eq!(
            doc.execute(
                doc.version(),
                DocumentCommand::Edit(Edit::Restore {
                    page: None,
                    items: vec![annotation(1)],
                })
            ),
            Err(CommandError::Edit(EditError::InvalidRestore))
        );
        assert_eq!(doc.items().count(), 1);
        assert_eq!(
            doc.execute(
                doc.version(),
                DocumentCommand::Edit(Edit::Clear { page: Some(2) })
            ),
            Err(CommandError::Edit(EditError::InvalidPage))
        );
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
    fn polygon_hit_test_uses_edges_and_rejects_the_empty_center() {
        let mut doc = DocumentModel::new(DocumentId(12), 1).unwrap();
        let bounds = Rect {
            x: 10.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
        };
        doc.apply(Edit::Add(Annotation {
            id: 42,
            page: 0,
            bounds,
            kind: Kind::Polygon {
                color_rgb: [231, 76, 60],
            },
        }))
        .unwrap();
        assert_eq!(polygon_vertices(bounds)[0], [50.0, 20.0]);
        assert_eq!(doc.hit_test(0, [50.0, 20.0], 1.0), Some(42));
        assert_eq!(doc.hit_test(0, [50.0, 50.0], 1.0), None);
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
