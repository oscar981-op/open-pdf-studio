//! Native page overlay: PDF pixels stay cached while annotations are drawn separately.
use super::{AnnotationTool, Message};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Stroke};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme};
use opds_native_core::annotations::{Annotation, Kind, Rect};

pub struct Layer<'a> {
    pub annotations: Vec<&'a Annotation>,
    pub tool: AnnotationTool,
    pub page_points: (f32, f32),
}

#[derive(Default)]
pub struct State {
    drag: Option<(Point, Point)>,
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
        if self.tool == AnnotationTool::Select {
            state.drag = None;
            return None;
        }
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                state.drag = Some((position, position));
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some((_, end)) = &mut state.drag {
                    if let Some(position) = cursor.position_in(bounds) {
                        *end = position;
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let (start, last) = state.drag.take()?;
                let end = cursor.position_in(bounds).unwrap_or(last);
                let width = (end.x - start.x).abs();
                let height = (end.y - start.y).abs();
                if width < 3.0 || height < 3.0 {
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                let ratio_x = f64::from(self.page_points.0 / bounds.width);
                let ratio_y = f64::from(self.page_points.1 / bounds.height);
                let rect = Rect {
                    x: f64::from(start.x.min(end.x)) * ratio_x,
                    y: f64::from(start.y.min(end.y)) * ratio_y,
                    width: f64::from(width) * ratio_x,
                    height: f64::from(height) * ratio_y,
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
        for annotation in &self.annotations {
            let rect = annotation.bounds;
            let top_left = Point::new(rect.x as f32 * scale_x, rect.y as f32 * scale_y);
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
        }
        if let Some((start, end)) = state.drag {
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
        _state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.tool != AnnotationTool::Select && cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

pub fn view<'a>(
    annotations: Vec<&'a Annotation>,
    tool: AnnotationTool,
    page_points: (f32, f32),
    bitmap_size: (u32, u32),
) -> Element<'a, Message> {
    Canvas::new(Layer {
        annotations,
        tool,
        page_points,
    })
    .width(bitmap_size.0 as f32)
    .height(bitmap_size.1 as f32)
    .into()
}
