//! Capture Ctrl+wheel once, before the scrollable or annotation children.
//! Its anchor is measured in the fixed viewport on the wheel event itself.
use super::Message;
use iced::advanced::{
    layout, mouse as advanced_mouse, overlay, renderer, widget, Clipboard, Layout, Shell, Widget,
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
struct ZoomArea<'a> {
    content: Element<'a, Message>,
    enabled: bool,
}
pub fn view<'a>(content: impl Into<Element<'a, Message>>, enabled: bool) -> Element<'a, Message> {
    Element::new(ZoomArea {
        content: content.into(),
        enabled,
    })
}

impl Widget<Message, Theme, Renderer> for ZoomArea<'_> {
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
        if self.enabled {
            if let Event::Mouse(advanced_mouse::Event::WheelScrolled { delta }) = event {
                if let Some(position) = cursor.position_in(layout.bounds()) {
                    let amount = match *delta {
                        advanced_mouse::ScrollDelta::Lines { y, .. } => y,
                        advanced_mouse::ScrollDelta::Pixels { y, .. } => y / 60.0,
                    };
                    if amount.is_finite() && amount != 0.0 {
                        shell.publish(Message::WheelZoom {
                            amount,
                            anchor: [position.x, position.y],
                        });
                        shell.capture_event();
                        return;
                    }
                }
            }
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
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
