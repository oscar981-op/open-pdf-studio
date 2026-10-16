//! Native page overlay: PDF pixels stay cached while annotations are drawn separately.
use super::{text_layout, AnnotationTool, Message};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Stroke};
use iced::{keyboard, mouse};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme};
use opds_native_core::annotations::{arrow_wings, polygon_vertices, MAX_POLYLINE_POINTS};
use opds_native_core::annotations::{DocumentModel, DocumentVersion, Kind, Rect, TextAlign};
use opds_native_core::DocumentId;
use std::time::{Duration, Instant};

fn ellipse_path(top_left: Point, size: Size) -> canvas::Path {
    const K: f32 = 0.552_284_8;
    let cx = top_left.x + size.width / 2.0;
    let cy = top_left.y + size.height / 2.0;
    let rx = size.width / 2.0;
    let ry = size.height / 2.0;
    canvas::Path::new(|path| {
        path.move_to(Point::new(cx + rx, cy));
        path.bezier_curve_to(
            Point::new(cx + rx, cy + K * ry),
            Point::new(cx + K * rx, cy + ry),
            Point::new(cx, cy + ry),
        );
        path.bezier_curve_to(
            Point::new(cx - K * rx, cy + ry),
            Point::new(cx - rx, cy + K * ry),
            Point::new(cx - rx, cy),
        );
        path.bezier_curve_to(
            Point::new(cx - rx, cy - K * ry),
            Point::new(cx - K * rx, cy - ry),
            Point::new(cx, cy - ry),
        );
        path.bezier_curve_to(
            Point::new(cx + K * rx, cy - ry),
            Point::new(cx + rx, cy - K * ry),
            Point::new(cx + rx, cy),
        );
        path.close();
    })
}

fn polygon_path(top_left: Point, size: Size) -> canvas::Path {
    let vertices = polygon_vertices(Rect {
        x: f64::from(top_left.x),
        y: f64::from(top_left.y),
        width: f64::from(size.width),
        height: f64::from(size.height),
    });
    canvas::Path::new(|path| {
        path.move_to(Point::new(vertices[0][0] as f32, vertices[0][1] as f32));
        for vertex in &vertices[1..] {
            path.line_to(Point::new(vertex[0] as f32, vertex[1] as f32));
        }
        path.close();
    })
}

// Tauri's Circle tool uses the larger page-space drag component for both
// axes. Keep the square inside the page before showing or committing it.
fn square_drag_end(start: Point, end: Point, bounds: Rectangle, page: (f32, f32)) -> Point {
    let (page_width, page_height) = page;
    if bounds.width <= 0.0 || bounds.height <= 0.0 || page_width <= 0.0 || page_height <= 0.0 {
        return end;
    }
    let scale_x = bounds.width / page_width;
    let scale_y = bounds.height / page_height;
    let sx = start.x / scale_x;
    let sy = start.y / scale_y;
    let dx = (end.x - start.x) / scale_x;
    let dy = (end.y - start.y) / scale_y;
    let available_x = if dx < 0.0 { sx } else { page_width - sx };
    let available_y = if dy < 0.0 { sy } else { page_height - sy };
    let side = dx
        .abs()
        .max(dy.abs())
        .min(available_x)
        .min(available_y)
        .max(0.0);
    Point::new(
        start.x + (if dx < 0.0 { -side } else { side }) * scale_x,
        start.y + (if dy < 0.0 { -side } else { side }) * scale_y,
    )
}

fn arrow_screen_wings(from: Point, to: Point, scale_x: f32, scale_y: f32) -> Option<[Point; 2]> {
    if scale_x <= 0.0 || scale_y <= 0.0 {
        return None;
    }
    let wings = arrow_wings(
        [f64::from(from.x / scale_x), f64::from(from.y / scale_y)],
        [f64::from(to.x / scale_x), f64::from(to.y / scale_y)],
    )?;
    Some(wings.map(|point| Point::new(point[0] as f32 * scale_x, point[1] as f32 * scale_y)))
}

pub struct Layer<'a> {
    pub model: &'a DocumentModel,
    pub page: usize,
    pub selected: Option<u64>,
    pub editing_text: Option<u64>,
    pub tool: AnnotationTool,
    pub tool_epoch: u64,
    pub page_points: (f32, f32),
    pub paint_existing: bool,
}

#[derive(Default)]
pub struct State {
    drawing: Option<DrawState>,
    moving: Option<MoveState>,
    resizing: Option<ResizeState>,
    line_endpoint: Option<LineEndpointState>,
    polyline: Option<PolylineDraft>,
    modifiers: keyboard::Modifiers,
}

#[derive(Clone, Copy)]
struct DrawState {
    start: Point,
    end: Point,
    version: DocumentVersion,
    page: usize,
    tool_epoch: u64,
    bitmap_size: [f32; 2],
}

struct MoveState {
    id: u64,
    version: DocumentVersion,
    start: Point,
    current: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

impl Handle {
    const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
    ];

    fn point(self, top_left: Point, size: Size) -> Point {
        let mid_x = top_left.x + size.width / 2.0;
        let mid_y = top_left.y + size.height / 2.0;
        let right = top_left.x + size.width;
        let bottom = top_left.y + size.height;
        match self {
            Self::NorthWest => top_left,
            Self::North => Point::new(mid_x, top_left.y),
            Self::NorthEast => Point::new(right, top_left.y),
            Self::East => Point::new(right, mid_y),
            Self::SouthEast => Point::new(right, bottom),
            Self::South => Point::new(mid_x, bottom),
            Self::SouthWest => Point::new(top_left.x, bottom),
            Self::West => Point::new(top_left.x, mid_y),
        }
    }

    fn interaction(self) -> mouse::Interaction {
        match self {
            Self::NorthWest | Self::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
            Self::NorthEast | Self::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
            Self::North | Self::South => mouse::Interaction::ResizingVertically,
            Self::East | Self::West => mouse::Interaction::ResizingHorizontally,
        }
    }
}

struct ResizeState {
    id: u64,
    version: DocumentVersion,
    handle: Handle,
    original: Rect,
    current: Point,
}

struct LineEndpointState {
    id: u64,
    version: DocumentVersion,
    start: bool,
    current: Point,
}

struct PolylineDraft {
    points: Vec<Point>,
    cursor: Point,
    last_click: Instant,
    document: DocumentId,
    version: DocumentVersion,
    page: usize,
    tool_epoch: u64,
    bitmap_size: [f32; 2],
}

fn append_freehand_point(points: &mut Vec<Point>, point: Point, minimum_step: f32) {
    if points
        .last()
        .is_some_and(|last| (point.x - last.x).hypot(point.y - last.y) < minimum_step)
    {
        return;
    }
    if points.len() >= MAX_POLYLINE_POINTS {
        let last = points.pop().expect("nonempty freehand stroke");
        let mut reduced: Vec<Point> = points.iter().step_by(2).copied().collect();
        reduced.push(last);
        *points = reduced;
    }
    points.push(point);
}

impl Layer<'_> {
    fn page_point(&self, point: Point, bounds: Rectangle) -> [f64; 2] {
        [
            f64::from(point.x * self.page_points.0 / bounds.width),
            f64::from(point.y * self.page_points.1 / bounds.height),
        ]
    }

    fn finish_polyline(
        &self,
        draft: PolylineDraft,
        bounds: Rectangle,
    ) -> Option<canvas::Action<Message>> {
        (draft.points.len() >= 2).then(|| {
            canvas::Action::publish(Message::AddPolyline(
                draft.version,
                draft
                    .points
                    .into_iter()
                    .map(|point| self.page_point(point, bounds))
                    .collect(),
            ))
            .and_capture()
        })
    }

    fn selected_handle(&self, point: Point, bounds: Rectangle) -> Option<(u64, Handle, Rect)> {
        let item = self.model.get(self.selected?)?;
        if item.page != self.page
            || matches!(
                item.kind,
                Kind::Line { .. } | Kind::Arrow { .. } | Kind::Polyline { .. }
            )
        {
            return None;
        }
        let rect = item.bounds;
        let top_left = Point::new(
            rect.x as f32 * bounds.width / self.page_points.0,
            rect.y as f32 * bounds.height / self.page_points.1,
        );
        let size = Size::new(
            rect.width as f32 * bounds.width / self.page_points.0,
            rect.height as f32 * bounds.height / self.page_points.1,
        );
        Handle::ALL.into_iter().find_map(|handle| {
            let grip = handle.point(top_left, size);
            ((point.x - grip.x).abs() <= 7.0 && (point.y - grip.y).abs() <= 7.0)
                .then_some((item.id, handle, rect))
        })
    }

    fn selected_line_endpoint(&self, point: Point, bounds: Rectangle) -> Option<(u64, bool)> {
        let item = self.model.get(self.selected?)?;
        if item.page != self.page {
            return None;
        }
        let (start, end) = match &item.kind {
            Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => (start, end),
            _ => return None,
        };
        [(*start, true), (*end, false)]
            .into_iter()
            .find_map(|(endpoint, is_start)| {
                let grip = Point::new(
                    endpoint[0] as f32 * bounds.width / self.page_points.0,
                    endpoint[1] as f32 * bounds.height / self.page_points.1,
                );
                ((point.x - grip.x).abs() <= 7.0 && (point.y - grip.y).abs() <= 7.0)
                    .then_some((item.id, is_start))
            })
    }

    fn resized_bounds(
        &self,
        original: Rect,
        handle: Handle,
        position: Point,
        bounds: Rectangle,
    ) -> Rect {
        resize_bounds(
            original,
            handle,
            self.page_point(position, bounds),
            [f64::from(self.page_points.0), f64::from(self.page_points.1)],
            [
                f64::from(3.0 * self.page_points.0 / bounds.width),
                f64::from(3.0 * self.page_points.1 / bounds.height),
            ],
        )
    }
}

fn resize_bounds(
    original: Rect,
    handle: Handle,
    target: [f64; 2],
    page_size: [f64; 2],
    min_size: [f64; 2],
) -> Rect {
    let (mut left, mut top) = (original.x, original.y);
    let (mut right, mut bottom) = (original.x + original.width, original.y + original.height);
    let min_width = min_size[0].min(original.width);
    let min_height = min_size[1].min(original.height);
    if matches!(handle, Handle::NorthWest | Handle::West | Handle::SouthWest) {
        left = target[0].clamp(0.0, (right - min_width).max(0.0));
    }
    if matches!(handle, Handle::NorthEast | Handle::East | Handle::SouthEast) {
        right = target[0].clamp(left + min_width, page_size[0].max(left + min_width));
    }
    if matches!(
        handle,
        Handle::NorthWest | Handle::North | Handle::NorthEast
    ) {
        top = target[1].clamp(0.0, (bottom - min_height).max(0.0));
    }
    if matches!(
        handle,
        Handle::SouthWest | Handle::South | Handle::SouthEast
    ) {
        bottom = target[1].clamp(top + min_height, page_size[1].max(top + min_height));
    }
    Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

impl canvas::Program<Message> for Layer<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let version = self.model.version();
        if state.drawing.is_some_and(|drawing| {
            drawing.version != version
                || drawing.page != self.page
                || drawing.tool_epoch != self.tool_epoch
                || drawing.bitmap_size != [bounds.width, bounds.height]
        }) {
            state.drawing = None;
        }
        if state
            .moving
            .as_ref()
            .is_some_and(|moving| moving.version != version)
        {
            state.moving = None;
        }
        if state
            .resizing
            .as_ref()
            .is_some_and(|resizing| resizing.version != version)
        {
            state.resizing = None;
        }
        if state
            .line_endpoint
            .as_ref()
            .is_some_and(|endpoint| endpoint.version != version)
        {
            state.line_endpoint = None;
        }
        if !matches!(
            self.tool,
            AnnotationTool::Polyline | AnnotationTool::Freehand
        ) || state.polyline.as_ref().is_some_and(|draft| {
            draft.document != self.model.view().document_id()
                || draft.version != version
                || draft.page != self.page
                || draft.tool_epoch != self.tool_epoch
                || draft.bitmap_size != [bounds.width, bounds.height]
        }) {
            state.polyline = None;
        }
        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if self.tool == AnnotationTool::Polyline && state.polyline.is_some() =>
            {
                match key {
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        self.finish_polyline(state.polyline.take()?, bounds)
                    }
                    keyboard::Key::Named(keyboard::key::Named::Escape) => {
                        state.polyline = None;
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    _ => None,
                }
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if matches!(self.tool, AnnotationTool::Line | AnnotationTool::Arrow)
                && state.drawing.is_some() =>
            {
                state.drawing = None;
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if self.tool == AnnotationTool::Polyline && state.polyline.is_some() =>
            {
                self.finish_polyline(state.polyline.take()?, bounds)
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if self.tool == AnnotationTool::Hand {
                    return None;
                }
                let position = cursor.position_in(bounds)?;
                if self.tool == AnnotationTool::Freehand {
                    state.polyline = Some(PolylineDraft {
                        points: vec![position],
                        cursor: position,
                        last_click: Instant::now(),
                        document: self.model.view().document_id(),
                        version: self.model.version(),
                        page: self.page,
                        tool_epoch: self.tool_epoch,
                        bitmap_size: [bounds.width, bounds.height],
                    });
                    return Some(canvas::Action::capture());
                }
                if self.tool == AnnotationTool::Polyline {
                    if let Some(draft) = &mut state.polyline {
                        let previous = *draft.points.last()?;
                        if draft.points.len() >= 2
                            && draft.last_click.elapsed() <= Duration::from_millis(400)
                            && (position.x - previous.x).hypot(position.y - previous.y) <= 8.0
                        {
                            return self.finish_polyline(state.polyline.take()?, bounds);
                        }
                        if (position.x - previous.x).hypot(position.y - previous.y) >= 3.0 {
                            draft.points.push(position);
                            draft.cursor = position;
                            draft.last_click = Instant::now();
                            if draft.points.len() >= MAX_POLYLINE_POINTS {
                                return self.finish_polyline(state.polyline.take()?, bounds);
                            }
                        }
                    } else {
                        state.polyline = Some(PolylineDraft {
                            points: vec![position],
                            cursor: position,
                            last_click: Instant::now(),
                            document: self.model.view().document_id(),
                            version: self.model.version(),
                            page: self.page,
                            tool_epoch: self.tool_epoch,
                            bitmap_size: [bounds.width, bounds.height],
                        });
                    }
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                if matches!(self.tool, AnnotationTool::Line | AnnotationTool::Arrow) {
                    if let Some(drawing) = state.drawing.take() {
                        if (position.x - drawing.start.x).hypot(position.y - drawing.start.y) < 3.0
                        {
                            state.drawing = Some(drawing);
                            return Some(canvas::Action::capture());
                        }
                        return Some(
                            canvas::Action::publish(Message::AddLine(
                                drawing.version,
                                self.page_point(drawing.start, bounds),
                                self.page_point(position, bounds),
                            ))
                            .and_capture(),
                        );
                    }
                }
                if self.tool == AnnotationTool::Select {
                    if let Some((id, start)) = self.selected_line_endpoint(position, bounds) {
                        state.line_endpoint = Some(LineEndpointState {
                            id,
                            version: self.model.version(),
                            start,
                            current: position,
                        });
                        return Some(
                            canvas::Action::publish(Message::AnnotationGestureStarted)
                                .and_capture(),
                        );
                    }
                    if let Some((id, handle, original)) = self.selected_handle(position, bounds) {
                        state.resizing = Some(ResizeState {
                            id,
                            version: self.model.version(),
                            handle,
                            original,
                            current: position,
                        });
                        return Some(
                            canvas::Action::publish(Message::AnnotationGestureStarted)
                                .and_capture(),
                        );
                    }
                    let point = self.page_point(position, bounds);
                    let tolerance = f64::from(4.0 * self.page_points.0 / bounds.width);
                    let id = self.model.hit_test(self.page, point, tolerance);
                    state.moving = id.map(|id| MoveState {
                        id,
                        version: self.model.version(),
                        start: position,
                        current: position,
                    });
                    Some(canvas::Action::publish(Message::SelectAnnotation(id)).and_capture())
                } else {
                    state.drawing = Some(DrawState {
                        start: position,
                        end: position,
                        version: self.model.version(),
                        page: self.page,
                        tool_epoch: self.tool_epoch,
                        bitmap_size: [bounds.width, bounds.height],
                    });
                    Some(canvas::Action::capture())
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let position = cursor.position_in(bounds);
                if let Some(draft) = &mut state.polyline {
                    if let Some(position) = position {
                        draft.cursor = position;
                        if self.tool == AnnotationTool::Freehand {
                            let step = (bounds.width / self.page_points.0)
                                .max(bounds.height / self.page_points.1)
                                .max(3.0);
                            append_freehand_point(&mut draft.points, position, step);
                        }
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
                if let Some(drawing) = &mut state.drawing {
                    if let Some(position) = position {
                        drawing.end = position;
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
                if let Some(moving) = &mut state.moving {
                    if let Some(position) = position {
                        moving.current = position;
                        let original = self.model.get(moving.id)?.bounds;
                        let [dx, dy] = self.page_point(
                            Point::new(position.x - moving.start.x, position.y - moving.start.y),
                            bounds,
                        );
                        return Some(
                            canvas::Action::publish(Message::AnnotationGesturePreview(
                                moving.version,
                                moving.id,
                                Rect {
                                    x: original.x + dx,
                                    y: original.y + dy,
                                    ..original
                                },
                            ))
                            .and_capture(),
                        );
                    }
                }
                if let Some(resizing) = &mut state.resizing {
                    if let Some(position) = position {
                        resizing.current = position;
                        let rect = self.resized_bounds(
                            resizing.original,
                            resizing.handle,
                            position,
                            bounds,
                        );
                        return Some(
                            canvas::Action::publish(Message::AnnotationGesturePreview(
                                resizing.version,
                                resizing.id,
                                rect,
                            ))
                            .and_capture(),
                        );
                    }
                }
                if let Some(endpoint) = &mut state.line_endpoint {
                    if let Some(position) = position {
                        endpoint.current = position;
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if self.tool == AnnotationTool::Freehand {
                    let mut draft = state.polyline.take()?;
                    if let Some(position) = cursor.position_in(bounds) {
                        let step = (bounds.width / self.page_points.0)
                            .max(bounds.height / self.page_points.1)
                            .max(3.0);
                        append_freehand_point(&mut draft.points, position, step);
                    }
                    return self
                        .finish_polyline(draft, bounds)
                        .or_else(|| Some(canvas::Action::request_redraw().and_capture()));
                }
                if let Some(endpoint) = state.line_endpoint.take() {
                    let position = cursor.position_in(bounds).unwrap_or(endpoint.current);
                    let mut target = self.page_point(position, bounds);
                    target[0] = target[0].clamp(0.0, f64::from(self.page_points.0));
                    target[1] = target[1].clamp(0.0, f64::from(self.page_points.1));
                    return Some(
                        canvas::Action::publish(Message::MoveLineEndpoint(
                            endpoint.version,
                            endpoint.id,
                            endpoint.start,
                            target,
                        ))
                        .and_capture(),
                    );
                }
                if let Some(resizing) = state.resizing.take() {
                    let position = cursor.position_in(bounds).unwrap_or(resizing.current);
                    let rect =
                        self.resized_bounds(resizing.original, resizing.handle, position, bounds);
                    if rect != resizing.original {
                        return Some(
                            canvas::Action::publish(Message::ResizeAnnotation(
                                resizing.version,
                                resizing.id,
                                rect,
                            ))
                            .and_capture(),
                        );
                    }
                    return Some(
                        canvas::Action::publish(Message::AnnotationGestureEnded).and_capture(),
                    );
                }
                if let Some(moving) = state.moving.take() {
                    let end = cursor.position_in(bounds).unwrap_or(moving.current);
                    let delta = self.page_point(
                        Point::new(end.x - moving.start.x, end.y - moving.start.y),
                        bounds,
                    );
                    if (end.x - moving.start.x).abs() >= 2.0
                        || (end.y - moving.start.y).abs() >= 2.0
                    {
                        return Some(
                            canvas::Action::publish(Message::MoveAnnotation(
                                moving.version,
                                moving.id,
                                delta,
                            ))
                            .and_capture(),
                        );
                    }
                    return Some(
                        canvas::Action::publish(Message::AnnotationGestureEnded).and_capture(),
                    );
                }
                let drawing = state.drawing.take()?;
                let start = drawing.start;
                let raw_end = cursor.position_in(bounds).unwrap_or(drawing.end);
                let end = if self.tool == AnnotationTool::Circle {
                    square_drag_end(start, raw_end, bounds, self.page_points)
                } else {
                    raw_end
                };
                let width = (end.x - start.x).abs();
                let height = (end.y - start.y).abs();
                if matches!(self.tool, AnnotationTool::Line | AnnotationTool::Arrow) {
                    if width.hypot(height) < 3.0 {
                        state.drawing = Some(drawing);
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                    return Some(
                        canvas::Action::publish(Message::AddLine(
                            drawing.version,
                            self.page_point(start, bounds),
                            self.page_point(end, bounds),
                        ))
                        .and_capture(),
                    );
                }
                if width < 3.0 || height < 3.0 {
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                let [x, y] =
                    self.page_point(Point::new(start.x.min(end.x), start.y.min(end.y)), bounds);
                let [w, h] = self.page_point(Point::new(width, height), bounds);
                let rect = Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                };
                Some(
                    canvas::Action::publish(Message::AddAnnotation(drawing.version, rect))
                        .and_capture(),
                )
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let scale_x = bounds.width / self.page_points.0;
        let scale_y = bounds.height / self.page_points.1;
        let version = self.model.version();
        let moving = state.moving.as_ref().filter(|item| item.version == version);
        let resizing = state
            .resizing
            .as_ref()
            .filter(|item| item.version == version);
        let line_endpoint = state
            .line_endpoint
            .as_ref()
            .filter(|item| item.version == version);
        let mut selection = None;
        let mut line_selection = None;
        for annotation in self
            .model
            .items()
            .filter(|item| self.paint_existing && item.page == self.page)
        {
            let rect = if let Some(resizing) = resizing {
                if resizing.id == annotation.id {
                    self.resized_bounds(
                        resizing.original,
                        resizing.handle,
                        resizing.current,
                        bounds,
                    )
                } else {
                    annotation.bounds
                }
            } else {
                annotation.bounds
            };
            let mut top_left = Point::new(rect.x as f32 * scale_x, rect.y as f32 * scale_y);
            if let Some(moving) = moving {
                if moving.id == annotation.id {
                    top_left.x += moving.current.x - moving.start.x;
                    top_left.y += moving.current.y - moving.start.y;
                }
            }
            let size = Size::new(rect.width as f32 * scale_x, rect.height as f32 * scale_y);
            match &annotation.kind {
                Kind::Rectangle { color_rgb } => {
                    let color = Color::from_rgb8(color_rgb[0], color_rgb[1], color_rgb[2]);
                    if *color_rgb == [255, 226, 67] {
                        frame.fill_rectangle(top_left, size, color.scale_alpha(0.35));
                    } else {
                        frame.stroke_rectangle(
                            top_left,
                            size,
                            Stroke::default().with_color(color).with_width(2.0),
                        );
                    }
                }
                Kind::Ellipse { color_rgb } => {
                    frame.stroke(
                        &ellipse_path(top_left, size),
                        Stroke::default()
                            .with_color(Color::from_rgb8(color_rgb[0], color_rgb[1], color_rgb[2]))
                            .with_width(2.0),
                    );
                }
                Kind::Polygon { color_rgb } => {
                    frame.stroke(
                        &polygon_path(top_left, size),
                        Stroke::default()
                            .with_color(Color::from_rgb8(color_rgb[0], color_rgb[1], color_rgb[2]))
                            .with_width(2.0),
                    );
                }
                Kind::Line {
                    start,
                    end,
                    color_rgb,
                }
                | Kind::Arrow {
                    start,
                    end,
                    color_rgb,
                } => {
                    let offset =
                        if let Some(moving) = moving.filter(|moving| moving.id == annotation.id) {
                            Point::new(
                                moving.current.x - moving.start.x,
                                moving.current.y - moving.start.y,
                            )
                        } else {
                            Point::ORIGIN
                        };
                    let mut from = Point::new(
                        start[0] as f32 * scale_x + offset.x,
                        start[1] as f32 * scale_y + offset.y,
                    );
                    let mut to = Point::new(
                        end[0] as f32 * scale_x + offset.x,
                        end[1] as f32 * scale_y + offset.y,
                    );
                    if let Some(endpoint) =
                        line_endpoint.filter(|endpoint| endpoint.id == annotation.id)
                    {
                        if endpoint.start {
                            from = endpoint.current;
                        } else {
                            to = endpoint.current;
                        }
                    }
                    frame.stroke(
                        &canvas::Path::line(from, to),
                        Stroke::default()
                            .with_color(Color::from_rgb8(color_rgb[0], color_rgb[1], color_rgb[2]))
                            .with_width(2.0),
                    );
                    if matches!(&annotation.kind, Kind::Arrow { .. }) {
                        if let Some(wings) = arrow_screen_wings(from, to, scale_x, scale_y) {
                            for wing in wings {
                                frame.stroke(
                                    &canvas::Path::line(to, wing),
                                    Stroke::default()
                                        .with_color(Color::from_rgb8(
                                            color_rgb[0],
                                            color_rgb[1],
                                            color_rgb[2],
                                        ))
                                        .with_width(2.0),
                                );
                            }
                        }
                    }
                    if self.selected == Some(annotation.id) {
                        line_selection = Some((from, to));
                    }
                }
                Kind::Polyline { points, color_rgb } => {
                    let offset = moving
                        .filter(|moving| moving.id == annotation.id)
                        .map(|moving| {
                            Point::new(
                                moving.current.x - moving.start.x,
                                moving.current.y - moving.start.y,
                            )
                        })
                        .unwrap_or(Point::ORIGIN);
                    for segment in points.windows(2) {
                        let from = Point::new(
                            segment[0][0] as f32 * scale_x + offset.x,
                            segment[0][1] as f32 * scale_y + offset.y,
                        );
                        let to = Point::new(
                            segment[1][0] as f32 * scale_x + offset.x,
                            segment[1][1] as f32 * scale_y + offset.y,
                        );
                        frame.stroke(
                            &canvas::Path::line(from, to),
                            Stroke::default()
                                .with_color(Color::from_rgb8(
                                    color_rgb[0],
                                    color_rgb[1],
                                    color_rgb[2],
                                ))
                                .with_width(2.0),
                        );
                    }
                }
                Kind::Text {
                    value,
                    color_rgb,
                    font_size,
                    align,
                } => {
                    if self.editing_text != Some(annotation.id) {
                        frame.with_clip(Rectangle::new(top_left, size), |frame| {
                            let (x, align_x) = match align {
                                TextAlign::Left => {
                                    (top_left.x, iced::advanced::text::Alignment::Left)
                                }
                                TextAlign::Center => (
                                    top_left.x + size.width / 2.0,
                                    iced::advanced::text::Alignment::Center,
                                ),
                                TextAlign::Right => (
                                    top_left.x + size.width,
                                    iced::advanced::text::Alignment::Right,
                                ),
                            };
                            if let Some(face) = text_layout::font_face() {
                                let lines = text_layout::wrap_text(
                                    value,
                                    annotation.bounds.width,
                                    f64::from(*font_size),
                                    face,
                                );
                                for (index, line) in lines.iter().enumerate() {
                                    frame.fill_text(canvas::Text {
                                        content: line.text.to_owned(),
                                        position: Point::new(
                                            x,
                                            top_left.y
                                                + index as f32
                                                    * f32::from(*font_size + 4)
                                                    * scale_y,
                                        ),
                                        max_width: f32::INFINITY,
                                        color: Color::from_rgb8(
                                            color_rgb[0],
                                            color_rgb[1],
                                            color_rgb[2],
                                        ),
                                        size: iced::Pixels(f32::from(*font_size) * scale_y),
                                        font: iced::Font::with_name("Liberation Sans"),
                                        align_x,
                                        ..Default::default()
                                    });
                                }
                            }
                        });
                    }
                }
            }
            if self.selected == Some(annotation.id)
                && !matches!(
                    annotation.kind,
                    Kind::Line { .. } | Kind::Arrow { .. } | Kind::Polyline { .. }
                )
            {
                selection = Some((top_left, size));
            }
        }
        if let Some((from, to)) = line_selection {
            for point in [from, to] {
                frame.fill_rectangle(
                    Point::new(point.x - 3.0, point.y - 3.0),
                    Size::new(6.0, 6.0),
                    Color::from_rgb8(31, 136, 255),
                );
            }
        }
        if let Some((top_left, size)) = selection {
            frame.stroke_rectangle(
                top_left,
                size,
                Stroke::default()
                    .with_color(Color::from_rgb8(31, 136, 255))
                    .with_width(1.5),
            );
            for handle in Handle::ALL {
                let point = handle.point(top_left, size);
                frame.fill_rectangle(
                    Point::new(point.x - 3.0, point.y - 3.0),
                    Size::new(6.0, 6.0),
                    Color::from_rgb8(31, 136, 255),
                );
            }
        }
        if let Some(DrawState { start, end, .. }) =
            state.drawing.filter(|drawing| drawing.version == version)
        {
            let end = if self.tool == AnnotationTool::Circle {
                square_drag_end(start, end, bounds, self.page_points)
            } else {
                end
            };
            let top_left = Point::new(start.x.min(end.x), start.y.min(end.y));
            let size = Size::new((end.x - start.x).abs(), (end.y - start.y).abs());
            let color = match self.tool {
                AnnotationTool::Highlight => Color::from_rgba(1.0, 0.89, 0.26, 0.35),
                _ => Color::from_rgba(0.91, 0.30, 0.24, 0.8),
            };
            if matches!(self.tool, AnnotationTool::Line | AnnotationTool::Arrow) {
                frame.stroke(
                    &canvas::Path::line(start, end),
                    Stroke::default().with_color(color).with_width(2.0),
                );
                if self.tool == AnnotationTool::Arrow {
                    if let Some(wings) = arrow_screen_wings(start, end, scale_x, scale_y) {
                        for wing in wings {
                            frame.stroke(
                                &canvas::Path::line(end, wing),
                                Stroke::default().with_color(color).with_width(2.0),
                            );
                        }
                    }
                }
            } else if matches!(self.tool, AnnotationTool::Circle | AnnotationTool::Ellipse) {
                frame.stroke(
                    &ellipse_path(top_left, size),
                    Stroke::default().with_color(color).with_width(2.0),
                );
            } else if self.tool == AnnotationTool::Polygon {
                frame.stroke(
                    &polygon_path(top_left, size),
                    Stroke::default().with_color(color).with_width(2.0),
                );
            } else {
                frame.fill_rectangle(top_left, size, color);
            }
        }
        if matches!(
            self.tool,
            AnnotationTool::Polyline | AnnotationTool::Freehand
        ) {
            if let Some(draft) = state.polyline.as_ref().filter(|draft| {
                draft.document == self.model.view().document_id()
                    && draft.version == version
                    && draft.page == self.page
                    && draft.tool_epoch == self.tool_epoch
                    && draft.bitmap_size == [bounds.width, bounds.height]
            }) {
                let red = Color::from_rgba(0.91, 0.30, 0.24, 0.8);
                for segment in draft.points.windows(2) {
                    frame.stroke(
                        &canvas::Path::line(segment[0], segment[1]),
                        Stroke::default().with_color(red).with_width(2.0),
                    );
                }
                if let Some(last) = draft.points.last() {
                    frame.stroke(
                        &canvas::Path::line(*last, draft.cursor),
                        Stroke::default().with_color(red).with_width(2.0),
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let version = self.model.version();
        if state
            .moving
            .as_ref()
            .is_some_and(|item| item.version == version)
        {
            return mouse::Interaction::Grabbing;
        }
        if state
            .line_endpoint
            .as_ref()
            .is_some_and(|item| item.version == version)
        {
            return mouse::Interaction::Grabbing;
        }
        if let Some(resizing) = state
            .resizing
            .as_ref()
            .filter(|item| item.version == version)
        {
            return resizing.handle.interaction();
        }
        if self.tool == AnnotationTool::Select {
            if let Some(point) = cursor.position_in(bounds) {
                if self.selected_line_endpoint(point, bounds).is_some() {
                    return mouse::Interaction::Grab;
                }
                if let Some((_, handle, _)) = self.selected_handle(point, bounds) {
                    return handle.interaction();
                }
                let tolerance = f64::from(4.0 * self.page_points.0 / bounds.width);
                if self
                    .model
                    .hit_test(self.page, self.page_point(point, bounds), tolerance)
                    .is_some()
                {
                    return mouse::Interaction::Grab;
                }
            }
            mouse::Interaction::default()
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

pub fn view<'a>(
    model: &'a DocumentModel,
    page: usize,
    selected: Option<u64>,
    editing_text: Option<u64>,
    tool: AnnotationTool,
    tool_epoch: u64,
    page_points: (f32, f32),
    bitmap_size: (u32, u32),
    paint_existing: bool,
) -> Element<'a, Message> {
    Canvas::new(Layer {
        model,
        page,
        selected,
        editing_text,
        tool,
        tool_epoch,
        page_points,
        paint_existing,
    })
    .width(bitmap_size.0 as f32)
    .height(bitmap_size.1 as f32)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use opds_native_core::annotations::{Annotation, Edit};
    use opds_native_core::DocumentId;

    #[test]
    fn circle_drag_stays_square_in_page_space_and_within_bounds() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(300.0, 200.0));
        let page = (150.0, 200.0);
        let start = Point::new(60.0, 40.0);
        let end = square_drag_end(start, Point::new(90.0, 80.0), bounds, page);
        assert_eq!(end, Point::new(140.0, 80.0));
        let reverse = square_drag_end(start, Point::new(40.0, 15.0), bounds, page);
        assert_eq!(reverse, Point::new(10.0, 15.0));
        let at_edge = square_drag_end(
            Point::new(290.0, 40.0),
            Point::new(299.0, 80.0),
            bounds,
            page,
        );
        assert_eq!(at_edge, Point::new(300.0, 45.0));
    }

    #[test]
    fn freehand_drag_is_bounded_and_finishes_as_a_polyline_command() {
        use canvas::Program;
        let model = DocumentModel::new(DocumentId(100), 1).unwrap();
        let layer = Layer {
            model: &model,
            page: 0,
            selected: None,
            editing_text: None,
            tool: AnnotationTool::Freehand,
            tool_epoch: 1,
            page_points: (256.0, 192.0),
            paint_existing: true,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(256.0, 192.0));
        let mut state = State::default();
        let press = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let start = mouse::Cursor::Available(Point::new(10.0, 20.0));
        assert!(layer.update(&mut state, &press, bounds, start).is_some());
        for x in 11..220 {
            let point = Point::new(x as f32, 20.0 + (x % 7) as f32);
            let cursor = mouse::Cursor::Available(point);
            layer.update(
                &mut state,
                &canvas::Event::Mouse(mouse::Event::CursorMoved { position: point }),
                bounds,
                cursor,
            );
            assert!(state.polyline.as_ref().unwrap().points.len() <= MAX_POLYLINE_POINTS);
        }
        // Ordinary page-width strokes should retain their sampled vertices;
        // decimation is reserved for drawings beyond the bounded 512-point cap.
        assert!(state.polyline.as_ref().unwrap().points.len() > 64);
        let end = mouse::Cursor::Available(Point::new(220.0, 24.0));
        assert!(layer.update(&mut state, &release, bounds, end).is_some());
        assert!(state.polyline.is_none());
    }

    #[test]
    fn line_accepts_two_clicks_and_discards_draft_after_tool_change() {
        use canvas::Program;
        let model = DocumentModel::new(DocumentId(1), 1).unwrap();
        let layer = Layer {
            model: &model,
            page: 0,
            selected: None,
            editing_text: None,
            tool: AnnotationTool::Line,
            tool_epoch: 1,
            page_points: (256.0, 192.0),
            paint_existing: true,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(256.0, 192.0));
        let mut state = State::default();
        let press = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let start = mouse::Cursor::Available(Point::new(20.0, 30.0));
        let end = mouse::Cursor::Available(Point::new(90.0, 80.0));
        assert!(layer.update(&mut state, &press, bounds, start).is_some());
        assert!(layer.update(&mut state, &release, bounds, start).is_some());
        assert!(state.drawing.is_some());
        assert!(layer.update(&mut state, &press, bounds, end).is_some());
        assert!(state.drawing.is_none());

        layer.update(&mut state, &press, bounds, start);
        layer.update(&mut state, &release, bounds, start);
        assert!(state.drawing.is_some());
        let changed_tool = Layer {
            tool_epoch: 2,
            ..layer
        };
        changed_tool.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(90.0, 80.0),
            }),
            bounds,
            end,
        );
        assert!(state.drawing.is_none());
    }

    #[test]
    fn annotation_canvas_leaves_ctrl_wheel_to_the_single_page_zoom_handler() {
        use canvas::Program;
        let model = DocumentModel::new(DocumentId(1), 1).unwrap();
        let layer = Layer {
            model: &model,
            page: 0,
            selected: None,
            editing_text: None,
            tool: AnnotationTool::Select,
            tool_epoch: 0,
            page_points: (256.0, 192.0),
            paint_existing: true,
        };
        let mut state = State {
            modifiers: keyboard::Modifiers::CTRL,
            ..State::default()
        };
        assert!(layer
            .update(
                &mut state,
                &canvas::Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }
                }),
                Rectangle::new(Point::ORIGIN, Size::new(256.0, 192.0)),
                mouse::Cursor::Available(Point::new(45.0, 157.0))
            )
            .is_none());
    }

    #[test]
    fn selected_line_grips_track_page_scale_and_ignore_other_points() {
        let mut model = DocumentModel::new(DocumentId(1), 1).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 7,
                page: 0,
                bounds: Rect {
                    x: 18.0,
                    y: 28.0,
                    width: 84.0,
                    height: 54.0,
                },
                kind: Kind::Line {
                    start: [20.0, 30.0],
                    end: [100.0, 80.0],
                    color_rgb: [231, 76, 60],
                },
            }))
            .unwrap();
        let layer = Layer {
            model: &model,
            page: 0,
            selected: Some(7),
            editing_text: None,
            tool: AnnotationTool::Select,
            tool_epoch: 0,
            page_points: (200.0, 150.0),
            paint_existing: true,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0));
        assert_eq!(
            layer.selected_line_endpoint(Point::new(40.0, 60.0), bounds),
            Some((7, true))
        );
        assert_eq!(
            layer.selected_line_endpoint(Point::new(200.0, 160.0), bounds),
            Some((7, false))
        );
        assert_eq!(
            layer.selected_line_endpoint(Point::new(100.0, 80.0), bounds),
            None
        );
    }

    #[test]
    fn corner_resize_stays_on_page_and_keeps_positive_size() {
        let original = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let page = [100.0, 100.0];
        let min = [3.0, 3.0];
        assert_eq!(
            resize_bounds(original, Handle::SouthEast, [200.0, 200.0], page, min),
            Rect {
                x: 10.0,
                y: 20.0,
                width: 90.0,
                height: 80.0,
            }
        );
        assert_eq!(
            resize_bounds(original, Handle::NorthWest, [200.0, 200.0], page, min),
            Rect {
                x: 37.0,
                y: 57.0,
                width: 3.0,
                height: 3.0,
            }
        );
    }

    #[test]
    fn side_resize_changes_only_one_axis() {
        let original = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        assert_eq!(
            resize_bounds(
                original,
                Handle::East,
                [70.0, -10.0],
                [100.0, 100.0],
                [3.0, 3.0],
            ),
            Rect {
                x: 10.0,
                y: 20.0,
                width: 60.0,
                height: 40.0,
            }
        );
    }
}
