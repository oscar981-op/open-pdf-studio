//! Native page overlay: PDF pixels stay cached while annotations are drawn separately.
use super::{text_layout, AnnotationTool, Message};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Stroke};
use iced::{keyboard, mouse};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme};
use opds_native_core::annotations::{DocumentModel, Kind, Rect, TextAlign};

pub struct Layer<'a> {
    pub model: &'a DocumentModel,
    pub page: usize,
    pub selected: Option<u64>,
    pub editing_text: Option<u64>,
    pub tool: AnnotationTool,
    pub page_points: (f32, f32),
    pub paint_existing: bool,
}

#[derive(Default)]
pub struct State {
    drawing: Option<(Point, Point)>,
    moving: Option<MoveState>,
    resizing: Option<ResizeState>,
    line_endpoint: Option<LineEndpointState>,
    modifiers: keyboard::Modifiers,
}

struct MoveState {
    id: u64,
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
    handle: Handle,
    original: Rect,
    current: Point,
}

struct LineEndpointState {
    id: u64,
    start: bool,
    current: Point,
}

impl Layer<'_> {
    fn page_point(&self, point: Point, bounds: Rectangle) -> [f64; 2] {
        [
            f64::from(point.x * self.page_points.0 / bounds.width),
            f64::from(point.y * self.page_points.1 / bounds.height),
        ]
    }

    fn selected_handle(&self, point: Point, bounds: Rectangle) -> Option<(u64, Handle, Rect)> {
        let item = self.model.get(self.selected?)?;
        if item.page != self.page || matches!(item.kind, Kind::Line { .. }) {
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
        let Kind::Line { start, end, .. } = &item.kind else {
            return None;
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
        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if state.modifiers.control() && cursor.is_over(bounds) =>
            {
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if amount == 0.0 {
                    None
                } else {
                    let position = cursor.position_in(bounds)?;
                    Some(
                        canvas::Action::publish(Message::WheelZoom {
                            amount,
                            anchor: [position.x, position.y],
                        })
                        .and_capture(),
                    )
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                if self.tool == AnnotationTool::Select {
                    if let Some((id, start)) = self.selected_line_endpoint(position, bounds) {
                        state.line_endpoint = Some(LineEndpointState {
                            id,
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
                        start: position,
                        current: position,
                    });
                    Some(canvas::Action::publish(Message::SelectAnnotation(id)).and_capture())
                } else {
                    state.drawing = Some((position, position));
                    Some(canvas::Action::capture())
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let position = cursor.position_in(bounds);
                if let Some((_, end)) = &mut state.drawing {
                    if let Some(position) = position {
                        *end = position;
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
                if let Some(endpoint) = state.line_endpoint.take() {
                    let position = cursor.position_in(bounds).unwrap_or(endpoint.current);
                    let mut target = self.page_point(position, bounds);
                    target[0] = target[0].clamp(0.0, f64::from(self.page_points.0));
                    target[1] = target[1].clamp(0.0, f64::from(self.page_points.1));
                    return Some(
                        canvas::Action::publish(Message::MoveLineEndpoint(
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
                            canvas::Action::publish(Message::ResizeAnnotation(resizing.id, rect))
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
                            canvas::Action::publish(Message::MoveAnnotation(moving.id, delta))
                                .and_capture(),
                        );
                    }
                    return Some(
                        canvas::Action::publish(Message::AnnotationGestureEnded).and_capture(),
                    );
                }
                let (start, last) = state.drawing.take()?;
                let end = cursor.position_in(bounds).unwrap_or(last);
                let width = (end.x - start.x).abs();
                let height = (end.y - start.y).abs();
                if self.tool == AnnotationTool::Line {
                    if width.hypot(height) < 3.0 {
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                    return Some(
                        canvas::Action::publish(Message::AddLine(
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
                Some(canvas::Action::publish(Message::AddAnnotation(rect)).and_capture())
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
        let mut selection = None;
        let mut line_selection = None;
        for annotation in self
            .model
            .items()
            .filter(|item| self.paint_existing && item.page == self.page)
        {
            let rect = if let Some(resizing) = &state.resizing {
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
            if let Some(moving) = &state.moving {
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
                Kind::Line {
                    start,
                    end,
                    color_rgb,
                } => {
                    let offset = if state
                        .moving
                        .as_ref()
                        .is_some_and(|moving| moving.id == annotation.id)
                    {
                        let moving = state.moving.as_ref().expect("checked above");
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
                    if let Some(endpoint) = state
                        .line_endpoint
                        .as_ref()
                        .filter(|endpoint| endpoint.id == annotation.id)
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
                    if self.selected == Some(annotation.id) {
                        line_selection = Some((from, to));
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
            if self.selected == Some(annotation.id) && !matches!(annotation.kind, Kind::Line { .. })
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
        if let Some((start, end)) = state.drawing {
            let top_left = Point::new(start.x.min(end.x), start.y.min(end.y));
            let size = Size::new((end.x - start.x).abs(), (end.y - start.y).abs());
            let color = match self.tool {
                AnnotationTool::Highlight => Color::from_rgba(1.0, 0.89, 0.26, 0.35),
                _ => Color::from_rgba(0.91, 0.30, 0.24, 0.8),
            };
            if self.tool == AnnotationTool::Line {
                frame.stroke(
                    &canvas::Path::line(start, end),
                    Stroke::default().with_color(color).with_width(2.0),
                );
            } else {
                frame.fill_rectangle(top_left, size, color);
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
        if state.moving.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if state.line_endpoint.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(resizing) = &state.resizing {
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
