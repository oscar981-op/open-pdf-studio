//! Native page overlay: PDF pixels stay cached while annotations are drawn separately.
use super::{AnnotationTool, Message};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Stroke};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme};
use opds_native_core::annotations::{DocumentModel, Kind, Rect};

pub struct Layer<'a> {
    pub model: &'a DocumentModel,
    pub page: usize,
    pub selected: Option<u64>,
    pub tool: AnnotationTool,
    pub page_points: (f32, f32),
}

#[derive(Default)]
pub struct State {
    drawing: Option<(Point, Point)>,
    moving: Option<MoveState>,
}

struct MoveState {
    id: u64,
    start: Point,
    current: Point,
}

impl Layer<'_> {
    fn page_point(&self, point: Point, bounds: Rectangle) -> [f64; 2] {
        [
            f64::from(point.x * self.page_points.0 / bounds.width),
            f64::from(point.y * self.page_points.1 / bounds.height),
        ]
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
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                if self.tool == AnnotationTool::Select {
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
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
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
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                let (start, last) = state.drawing.take()?;
                let end = cursor.position_in(bounds).unwrap_or(last);
                let width = (end.x - start.x).abs();
                let height = (end.y - start.y).abs();
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
        for annotation in self.model.items().filter(|item| item.page == self.page) {
            let rect = annotation.bounds;
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
                Kind::Text { value } => {
                    frame.fill_text(canvas::Text {
                        content: value.clone(),
                        position: top_left,
                        max_width: size.width,
                        color: Color::from_rgb8(20, 20, 20),
                        size: iced::Pixels(14.0 * scale_y),
                        ..Default::default()
                    });
                }
            }
            if self.selected == Some(annotation.id) {
                selection = Some((top_left, size));
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
            for point in [
                top_left,
                Point::new(top_left.x + size.width, top_left.y),
                Point::new(top_left.x, top_left.y + size.height),
                Point::new(top_left.x + size.width, top_left.y + size.height),
            ] {
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
            frame.fill_rectangle(top_left, size, color);
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
        if self.tool == AnnotationTool::Select {
            if let Some(point) = cursor.position_in(bounds) {
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
    tool: AnnotationTool,
    page_points: (f32, f32),
    bitmap_size: (u32, u32),
) -> Element<'a, Message> {
    Canvas::new(Layer {
        model,
        page,
        selected,
        tool,
        page_points,
    })
    .width(bitmap_size.0 as f32)
    .height(bitmap_size.1 as f32)
    .into()
}
