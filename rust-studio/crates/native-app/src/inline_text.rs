//! Aligned paint and pointer mapping for Iced's multiline text editor.
//! The underlying editor still handles keyboard and Unicode input; IME preedit
//! rendering needs a separate check because its glyphs are hidden here.
use super::{text_layout, Message};
use iced::advanced::text::editor::{Cursor, Position};
use iced::advanced::{
    layout, mouse as advanced_mouse, overlay, renderer, widget, Clipboard, Layout, Shell, Widget,
};
use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::widget::text_editor;
use iced::{
    keyboard, mouse, Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
};
use opds_native_core::annotations::TextAlign;

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

/// Forward the editor's IME request with the cursor used by the aligned
/// canvas. Iced/winit paints preedit outside the widget at this rectangle.
pub fn with_aligned_ime<'a>(
    content: Element<'a, Message>,
    draft: &'a text_editor::Content,
    align: TextAlign,
    size: f32,
    line_height: f32,
) -> Element<'a, Message> {
    Element::new(AlignedIme {
        content,
        draft,
        align,
        size,
        line_height,
    })
}

struct AlignedIme<'a> {
    content: Element<'a, Message>,
    draft: &'a text_editor::Content,
    align: TextAlign,
    size: f32,
    line_height: f32,
}

impl AlignedIme<'_> {
    fn cursor_rectangle(&self, bounds: Rectangle) -> Option<Rectangle> {
        let face = text_layout::font_face()?;
        let value = self
            .draft
            .lines()
            .map(|line| line.text)
            .collect::<Vec<_>>()
            .join("\n");
        let lines =
            text_layout::wrap_text(&value, f64::from(bounds.width), f64::from(self.size), face);
        let cursor = self.draft.cursor().position;
        for (index, line) in lines.iter().enumerate() {
            let next_same_source = lines
                .get(index + 1)
                .is_some_and(|next| next.source_line == line.source_line);
            if line.source_line != cursor.line
                || cursor.column < line.start_byte
                || (cursor.column >= line.end_byte && next_same_source)
            {
                continue;
            }
            let prefix = line
                .text
                .get(
                    ..cursor
                        .column
                        .saturating_sub(line.start_byte)
                        .min(line.text.len()),
                )
                .unwrap_or("");
            let start = match self.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (bounds.width - line.width as f32) / 2.0,
                TextAlign::Right => bounds.width - line.width as f32,
            };
            return Some(Rectangle::new(
                Point::new(
                    bounds.x
                        + start
                        + text_layout::text_width(face, prefix, f64::from(self.size)) as f32,
                    bounds.y + index as f32 * self.line_height,
                ),
                Size::new(1.0, self.line_height),
            ));
        }
        None
    }
}

impl Widget<Message, Theme, Renderer> for AlignedIme<'_> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<widget::Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut widget::Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: advanced_mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        if matches!(
            event,
            Event::Window(iced::window::Event::RedrawRequested(_))
        ) {
            if let iced::advanced::InputMethod::Enabled { cursor, .. } = shell.input_method_mut() {
                if let Some(aligned_cursor) = self.cursor_rectangle(layout.bounds()) {
                    *cursor = aligned_cursor;
                }
            }
        }
    }
    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: advanced_mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> advanced_mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: advanced_mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut widget::Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl Aligned<'_> {
    fn value(&self) -> String {
        let mut value = String::new();
        for (index, line) in self.content.lines().enumerate() {
            if index > 0 {
                value.push('\n');
            }
            value.push_str(&line.text);
        }
        value
    }

    fn line_start(&self, width: f32, line_width: f64) -> f32 {
        let line_width = line_width as f32;
        match self.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (width - line_width) / 2.0,
            TextAlign::Right => width - line_width,
        }
    }

    fn position_at(&self, point: Point, bounds: Rectangle) -> Position {
        let Some(face) = text_layout::font_face() else {
            return Position { line: 0, column: 0 };
        };
        let value = self.value();
        let lines =
            text_layout::wrap_text(&value, f64::from(bounds.width), f64::from(self.size), face);
        let index =
            ((point.y / self.line_height).floor() as usize).min(lines.len().saturating_sub(1));
        let Some(line) = lines.get(index) else {
            return Position { line: 0, column: 0 };
        };
        let x = point.x - self.line_start(bounds.width, line.width);
        let mut advance = 0.0_f64;
        for (byte, ch) in line.text.char_indices() {
            let next = advance
                + text_layout::text_width(
                    face,
                    &line.text[byte..byte + ch.len_utf8()],
                    f64::from(self.size),
                );
            if f64::from(x) < (advance + next) / 2.0 {
                return Position {
                    line: line.source_line,
                    column: line.start_byte + byte,
                };
            }
            advance = next;
        }
        Position {
            line: line.source_line,
            column: line.end_byte,
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
        let Some(face) = text_layout::font_face() else {
            return vec![frame.into_geometry()];
        };
        let value = self.value();
        let lines =
            text_layout::wrap_text(&value, f64::from(bounds.width), f64::from(self.size), face);
        let visible_count = (bounds.height / self.line_height).ceil() as usize + 1;
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
                for (index, line) in lines.iter().take(visible_count).enumerate() {
                    if line.source_line < from.line || line.source_line > to.line {
                        continue;
                    }
                    let start_byte = if line.source_line == from.line {
                        from.column.max(line.start_byte)
                    } else {
                        line.start_byte
                    };
                    let end_byte = if line.source_line == to.line {
                        to.column.min(line.end_byte)
                    } else {
                        line.end_byte
                    };
                    if start_byte >= end_byte {
                        continue;
                    }
                    let start = self.line_start(bounds.width, line.width);
                    let prefix_start = line
                        .text
                        .get(..(start_byte - line.start_byte).min(line.text.len()))
                        .unwrap_or("");
                    let prefix_end = line
                        .text
                        .get(..(end_byte - line.start_byte).min(line.text.len()))
                        .unwrap_or(line.text);
                    let x1 = start
                        + text_layout::text_width(face, prefix_start, f64::from(self.size)) as f32;
                    let x2 = start
                        + text_layout::text_width(face, prefix_end, f64::from(self.size)) as f32;
                    frame.fill_rectangle(
                        Point::new(x1, index as f32 * self.line_height),
                        Size::new((x2 - x1).max(1.0), self.line_height),
                        Color::from_rgba8(64, 130, 220, 0.32),
                    );
                }
            }
            for (index, line) in lines.iter().take(visible_count).enumerate() {
                frame.fill_text(canvas::Text {
                    content: line.text.to_owned(),
                    position: Point::new(
                        self.line_start(bounds.width, line.width),
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
                for (index, line) in lines.iter().take(visible_count).enumerate() {
                    let next_same_source = lines
                        .get(index + 1)
                        .is_some_and(|next| next.source_line == line.source_line);
                    if line.source_line == cursor.position.line
                        && cursor.position.column >= line.start_byte
                        && (cursor.position.column < line.end_byte || !next_same_source)
                    {
                        let prefix_bytes = cursor
                            .position
                            .column
                            .saturating_sub(line.start_byte)
                            .min(line.text.len());
                        let prefix = line.text.get(..prefix_bytes).unwrap_or("");
                        let x = self.line_start(bounds.width, line.width)
                            + text_layout::text_width(face, prefix, f64::from(self.size)) as f32;
                        frame.fill_rectangle(
                            Point::new(x, index as f32 * self.line_height),
                            Size::new(1.0, self.line_height),
                            self.color,
                        );
                        break;
                    }
                }
            }
        });
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_cursor_follows_wrapped_aligned_unicode_caret() {
        let mut content = text_editor::Content::with_text("Café Ω longword");
        content.move_to(Cursor {
            position: Position {
                line: 0,
                column: "Café Ω longword".len(),
            },
            selection: None,
        });
        let bounds = Rectangle::new(Point::new(20.0, 30.0), Size::new(90.0, 100.0));
        let face = text_layout::font_face().unwrap();
        let value = content.text();
        let lines = text_layout::wrap_text(&value, 90.0, 16.0, face);
        assert!(lines.len() > 1);
        let last = lines.last().unwrap();
        for align in [TextAlign::Center, TextAlign::Right] {
            let wrapper = AlignedIme {
                content: iced::widget::text("").into(),
                draft: &content,
                align,
                size: 16.0,
                line_height: 20.0,
            };
            let caret = wrapper.cursor_rectangle(bounds).unwrap();
            let expected_x = match align {
                TextAlign::Center => bounds.x + (bounds.width + last.width as f32) / 2.0,
                TextAlign::Right => bounds.x + bounds.width,
                TextAlign::Left => unreachable!(),
            };
            assert!((caret.x - expected_x).abs() < 0.5);
            assert_eq!(caret.y, bounds.y + (lines.len() - 1) as f32 * 20.0);
        }
    }

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
        let face = text_layout::font_face().unwrap();
        let start = aligned.line_start(bounds.width, text_layout::text_width(face, "Café Ω", 24.0));
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
        let x = start + text_layout::text_width(face, prefix, f64::from(aligned.size)) as f32 + 0.1;
        assert_eq!(
            aligned.position_at(Point::new(x, 4.0), bounds),
            Position {
                line: 0,
                column: prefix.len(),
            }
        );
        let second_start =
            aligned.line_start(bounds.width, text_layout::text_width(face, "Ж", 24.0));
        assert_eq!(
            aligned.position_at(Point::new(second_start + 0.1, 30.0), bounds),
            Position { line: 1, column: 0 }
        );
    }

    #[test]
    fn wrapped_line_hit_test_keeps_logical_line_and_byte_offset() {
        let content = text_editor::Content::with_text("Café Ω longword");
        let aligned = Aligned {
            content: &content,
            align: TextAlign::Center,
            size: 16.0,
            line_height: 20.0,
            color: Color::BLACK,
        };
        let face = text_layout::font_face().unwrap();
        let width = text_layout::text_width(face, "longword", 16.0).max(text_layout::text_width(
            face,
            "Café Ω ",
            16.0,
        )) as f32
            + 0.1;
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(width, 60.0));
        let wrapped = text_layout::wrap_text("Café Ω longword", f64::from(width), 16.0, face);
        assert_eq!(wrapped.len(), 2);
        let x = aligned.line_start(width, wrapped[1].width) + 0.1;
        assert_eq!(
            aligned.position_at(Point::new(x, 25.0), bounds),
            Position {
                line: 0,
                column: "Café Ω ".len(),
            }
        );
    }

    #[test]
    fn draft_layout_normalizes_line_endings_without_changing_editor_lines() {
        let content = text_editor::Content::with_text("A\r\nB");
        let aligned = Aligned {
            content: &content,
            align: TextAlign::Center,
            size: 16.0,
            line_height: 20.0,
            color: Color::BLACK,
        };
        assert_eq!(content.line_count(), 2);
        assert_eq!(aligned.value(), "A\nB");
    }
}
