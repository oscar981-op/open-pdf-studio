//! Native PDF editor foundations. No UI, PDF engine, FFI or third-party dependencies.
#![forbid(unsafe_code)]

/// Stable identity; the application must never reuse an ID for another document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentId(pub u64);

pub mod annotations;

/// A render result can only be installed in the exact view that requested it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTicket {
    document: DocumentId,
    generation: u64,
}

#[derive(Clone, Debug)]
pub struct DocumentView {
    id: DocumentId,
    page_count: usize,
    page: usize,
    generation: u64,
    viewport: Viewport,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    scale: f64,
    offset: [f64; 2],
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset: [0.0, 0.0],
        }
    }
}

impl Viewport {
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn offset(&self) -> [f64; 2] {
        self.offset
    }
    pub fn page_to_screen(&self, point: [f64; 2]) -> [f64; 2] {
        [
            point[0] * self.scale + self.offset[0],
            point[1] * self.scale + self.offset[1],
        ]
    }
    pub fn screen_to_page(&self, point: [f64; 2]) -> [f64; 2] {
        [
            (point[0] - self.offset[0]) / self.scale,
            (point[1] - self.offset[1]) / self.scale,
        ]
    }
    fn zoom_at(&mut self, scale: f64, anchor: [f64; 2]) -> Result<(), ViewError> {
        if !scale.is_finite()
            || !(0.05..=32.0).contains(&scale)
            || !anchor.iter().all(|v| v.is_finite())
        {
            return Err(ViewError::InvalidZoom);
        }
        let point = self.screen_to_page(anchor);
        let offset = [anchor[0] - point[0] * scale, anchor[1] - point[1] * scale];
        if !offset.iter().all(|v| v.is_finite()) {
            return Err(ViewError::InvalidZoom);
        }
        self.scale = scale;
        self.offset = offset;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewError {
    EmptyDocument,
    InvalidPage,
    InvalidZoom,
    GenerationExhausted,
}

impl DocumentView {
    pub fn new(id: DocumentId, page_count: usize) -> Result<Self, ViewError> {
        if page_count == 0 {
            return Err(ViewError::EmptyDocument);
        }
        Ok(Self {
            id,
            page_count,
            page: 0,
            generation: 0,
            viewport: Viewport::default(),
        })
    }
    /// Pages are zero-based in the domain model; UI labels add one.
    pub fn page_count(&self) -> usize {
        self.page_count
    }
    pub fn page(&self) -> usize {
        self.page
    }
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }
    pub fn ticket(&self) -> RenderTicket {
        RenderTicket {
            document: self.id,
            generation: self.generation,
        }
    }
    pub fn accepts(&self, ticket: RenderTicket) -> bool {
        ticket == self.ticket()
    }
    /// Call on a content edit/reload, even when page and zoom remain unchanged.
    pub fn invalidate(&mut self) -> Result<(), ViewError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ViewError::GenerationExhausted)?;
        Ok(())
    }
    pub fn go_to_page(&mut self, page: usize) -> Result<(), ViewError> {
        if page >= self.page_count {
            return Err(ViewError::InvalidPage);
        }
        if page != self.page {
            self.invalidate()?;
            self.page = page;
        }
        Ok(())
    }
    pub fn zoom_at(&mut self, scale: f64, anchor: [f64; 2]) -> Result<(), ViewError> {
        let mut next = self.viewport;
        next.zoom_at(scale, anchor)?;
        if next != self.viewport {
            self.invalidate()?;
            self.viewport = next;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_keeps_the_page_point_under_the_cursor() {
        let mut view = DocumentView::new(DocumentId(1), 3).unwrap();
        let anchor = [143.5, 281.25];
        let point = view.viewport().screen_to_page(anchor);
        for scale in [0.05, 0.5, 2.0, 32.0, 1.0] {
            view.zoom_at(scale, anchor).unwrap();
            let actual = view.viewport().page_to_screen(point);
            assert!((actual[0] - anchor[0]).abs() < 1e-9);
            assert!((actual[1] - anchor[1]).abs() < 1e-9);
        }
    }
    #[test]
    fn page_zoom_and_edits_reject_stale_results_even_after_returning() {
        let mut view = DocumentView::new(DocumentId(1), 3).unwrap();
        let first = view.ticket();
        view.go_to_page(1).unwrap();
        view.go_to_page(0).unwrap();
        assert!(!view.accepts(first));
        let before_zoom = view.ticket();
        view.zoom_at(2.0, [10.0, 20.0]).unwrap();
        assert!(!view.accepts(before_zoom));
        let before_edit = view.ticket();
        view.invalidate().unwrap();
        assert!(!view.accepts(before_edit));
        assert!(view.accepts(view.ticket()));
    }
    #[test]
    fn results_cannot_cross_document_boundaries() {
        let a = DocumentView::new(DocumentId(1), 1).unwrap();
        let b = DocumentView::new(DocumentId(2), 1).unwrap();
        assert!(!b.accepts(a.ticket()));
    }
    #[test]
    fn invalid_requests_do_not_change_view_or_ticket() {
        assert!(matches!(
            DocumentView::new(DocumentId(1), 0),
            Err(ViewError::EmptyDocument)
        ));
        let mut view = DocumentView::new(DocumentId(1), 2).unwrap();
        let ticket = view.ticket();
        let viewport = *view.viewport();
        assert_eq!(view.go_to_page(2), Err(ViewError::InvalidPage));
        for scale in [f64::NAN, f64::INFINITY, -1.0, 0.0, 33.0] {
            assert_eq!(view.zoom_at(scale, [0.0, 0.0]), Err(ViewError::InvalidZoom));
        }
        assert_eq!(
            view.zoom_at(2.0, [f64::NAN, 0.0]),
            Err(ViewError::InvalidZoom)
        );
        assert_eq!(view.ticket(), ticket);
        assert_eq!(*view.viewport(), viewport);
    }
    #[test]
    fn generation_exhaustion_cannot_accept_an_old_ticket() {
        let mut view = DocumentView::new(DocumentId(1), 2).unwrap();
        view.generation = u64::MAX;
        assert_eq!(view.go_to_page(1), Err(ViewError::GenerationExhausted));
        assert_eq!(view.page(), 0);
        assert_eq!(
            view.zoom_at(2.0, [0.0, 0.0]),
            Err(ViewError::GenerationExhausted)
        );
        assert_eq!(view.viewport().scale(), 1.0);
    }
}
