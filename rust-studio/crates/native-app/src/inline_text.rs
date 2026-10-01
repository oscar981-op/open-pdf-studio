//! Aligned paint and pointer mapping for Iced's multiline text editor.
//! The underlying editor still handles keyboard and Unicode input; IME preedit
//! rendering needs a separate check because its glyphs are hidden here.
use super::Message;
use iced::advanced::text::editor::{Cursor, Position};
use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::widget::text_editor;
use iced::{keyboard, mouse, Color, Element, Point, Rectangle, Renderer, Size, Theme};
use opds_native_core::annotations::TextAlign;
use std::sync::OnceLock;

const FONT: &[u8] = include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
static FACE: OnceLock<Option<ttf_parser::Face<'static>>> = OnceLock::new();

fn font_face() -> Option<&'static ttf_parser::Face<'static>> {
    FACE.get_or_init(|| ttf_parser::Face::parse(FONT, 0).ok())
        .as_ref()
}

pub struct Aligned<'a> {
    content: &'a text_editor::Content,
    align: TextAlign,
    size: f32,
    line_height: f32,
    color: Color,
}

#[derive(Default)]
pub struct State {
    drag_anchor: Option<Position>,
    modifiers: keyboard::Modifiers,
}

pub fn view<'a>(
    content: &'a text_editor::Content,
    align: TextAlign,
    size: f32,
    line_height: f32,
    color: Color,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(Aligned {
        content,
        align,
        size,
        line_height,
        color,
    })
    .width(width)
    .height(height)
    .into()
}

impl Aligned<'_> {
    fn lines(&self, visible_count: usize) -> Vec<String> {
        (0..self.content.line_count().min(visible_count))
            .filter_map(|index| self.content.line(index))
            .map(|line| line.text.into_owned())
            .collect()
    }

    fn line_start(&self, width: f32, text: &str, face: &ttf_parser::Face<'_>) -> f32 {
        let line_width = text_width(face, text, self.size);
        match self.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (width - line_width) / 2.0,
            TextAlign::Right => width - line_width,
        }
    }

    fn position_at(&self, point: Point, bounds: Rectangle) -> Position {
        let line = ((point.y / self.line_height).floor() as usize)
            .min(self.content.line_count().saturating_sub(1));
        let current = self.content.line(line);
        let text = current.as_ref().map_or("", |current| current.text.as_ref());
        let Some(face) = font_face() else {
            return Position { line, column: 0 };
        };
        let x = point.x - self.line_start(bounds.width, text, face);
        let mut advance = 0.0;
        for (byte, ch) in text.char_indices() {
            let next = advance + glyph_width(face, ch, self.size);
            if x < (advance + next) / 2.0 {
                return Position { line, column: byte };
            }
            advance = next;
        }
        Position {
            line,
            column: text.len(),
        }
    }
}

impl canvas::Program<Message> for Aligned<'_> {
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
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = cursor.position_in(bounds)?;
                let position = self.position_at(point, bounds);
                let previous = self.content.cursor();
                let anchor = if state.modifiers.shift() {
                    previous.selection.unwrap_or(previous.position)
                } else {
                    position
                };
                state.drag_anchor = Some(anchor);
                Some(
                    canvas::Action::publish(Message::TextPointer(
                        Cursor {
                            position,
                            selection: (anchor != position).then_some(anchor),
                        },
                        true,
                    ))
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let anchor = state.drag_anchor?;
                let point = cursor.position_in(bounds).unwrap_or_else(|| {
                    let absolute = cursor.position().unwrap_or(bounds.position());
                    Point::new(
                        (absolute.x - bounds.x).clamp(0.0, bounds.width),
                        (absolute.y - bounds.y).clamp(0.0, bounds.height),
                    )
                });
                let position = self.position_at(point, bounds);
                Some(
                    canvas::Action::publish(Message::TextPointer(
                        Cursor {
                            position,
                            selection: (anchor != position).then_some(anchor),
                        },
                        false,
                    ))
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag_anchor.take().map(|_| canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let Some(face) = font_face() else {
            return vec![frame.into_geometry()];
        };
        let visible_count = (bounds.height / self.line_height).ceil() as usize + 1;
        let lines = self.lines(visible_count);
        let cursor = self.content.cursor();
        frame.with_clip(Rectangle::new(Point::ORIGIN, bounds.size()), |frame| {
            if let Some(anchor) = cursor.selection {
                let (from, to) = if (anchor.line, anchor.column)
                    <= (cursor.position.line, cursor.position.column)
                {
                    (anchor, cursor.position)
                } else {
                    (cursor.position, anchor)
                };
                for line_index in from.line..=to.line.min(lines.len().saturating_sub(1)) {
                    if let Some(line) = lines.get(line_index) {
                        let start_byte = if line_index == from.line {
                            from.column
                        } else {
                            0
                        };
                        let end_byte = if line_index == to.line {
                            to.column
                        } else {
                            line.len()
                        };
                        let start = self.line_start(bounds.width, line, face);
                        let x1 = start
                            + text_width(face, line.get(..start_byte).unwrap_or(""), self.size);
                        let x2 = start
                            + text_width(face, line.get(..end_byte).unwrap_or(line), self.size);
                        frame.fill_rectangle(
                            Point::new(x1, line_index as f32 * self.line_height),
                            Size::new((x2 - x1).max(1.0), self.line_height),
                            Color::from_rgba8(64, 130, 220, 0.32),
                        );
                    }
                }
            }
            for (index, line) in lines.iter().enumerate() {
                frame.fill_text(canvas::Text {
                    content: line.clone(),
                    position: Point::new(
                        self.line_start(bounds.width, line, face),
                        index as f32 * self.line_height,
                    ),
                    max_width: f32::INFINITY,
                    color: self.color,
                    size: iced::Pixels(self.size),
                    font: iced::Font::with_name("Liberation Sans"),
                    ..Default::default()
                });
            }
            if self.content.is_empty() {
                frame.fill_text(canvas::Text {
                    content: "Type text here".into(),
                    position: Point::ORIGIN,
                    max_width: f32::INFINITY,
                    color: Color::from_rgb8(110, 110, 110),
                    size: iced::Pixels(self.size),
                    font: iced::Font::with_name("Liberation Sans"),
                    ..Default::default()
                });
            }
            if cursor.selection.is_none() {
                if let Some(line) = lines.get(cursor.position.line) {
                    let prefix = line.get(..cursor.position.column).unwrap_or("");
                    let x = self.line_start(bounds.width, line, face)
                        + text_width(face, prefix, self.size);
                    frame.fill_rectangle(
                        Point::new(x, cursor.position.line as f32 * self.line_height),
                        Size::new(1.0, self.line_height),
                        self.color,
                    );
                }
            }
        });
        vec![frame.into_geometry()]
    }
}

fn glyph_width(face: &ttf_parser::Face<'_>, ch: char, size: f32) -> f32 {
    face.glyph_index(ch)
        .and_then(|glyph| face.glyph_hor_advance(glyph))
        .map_or(0.0, |advance| {
            f32::from(advance) * size / f32::from(face.units_per_em())
        })
}

fn text_width(face: &ttf_parser::Face<'_>, text: &str, size: f32) -> f32 {
    text.chars().map(|ch| glyph_width(face, ch, size)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_hit_test_returns_utf8_boundaries() {
        let content = text_editor::Content::with_text("Café Ω\nЖ");
        let aligned = Aligned {
            content: &content,
            align: TextAlign::Right,
            size: 24.0,
            line_height: 28.0,
            color: Color::BLACK,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(240.0, 40.0));
        let face = font_face().unwrap();
        let start = aligned.line_start(bounds.width, "Café Ω", face);
        assert_eq!(
            aligned
                .position_at(Point::new(start + 0.1, 4.0), bounds)
                .column,
            0
        );
        assert_eq!(
            aligned.position_at(Point::new(239.9, 4.0), bounds).column,
            "Café Ω".len()
        );
        let prefix = "Café ";
        let x = start + text_width(face, prefix, aligned.size) + 0.1;
        assert_eq!(
            aligned.position_at(Point::new(x, 4.0), bounds),
            Position {
                line: 0,
                column: prefix.len(),
            }
        );
        let second_start = aligned.line_start(bounds.width, "Ж", face);
        assert_eq!(
            aligned.position_at(Point::new(second_start + 0.1, 30.0), bounds),
            Position { line: 1, column: 0 }
        );
    }
}
