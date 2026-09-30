// SPDX-License-Identifier: GPL-3.0-only
// Adapted from Hakan Seven / OpenCADStudio, commit 2a15a370f6886d3e8695ab81c7d677e0cd484451.
// Source: src/ui/ribbon/widgets.rs and src/ui/ribbon/mod.rs.
// Changes: Iced 0.14 API, PDF command routing, reduced CAD-specific data dependencies.
use super::{annotation_layer, AnnotationTool, App, Message, RibbonTab};
use iced::widget::{
    button, column, container, image, row, scrollable, svg, text, text_input, tooltip, Space,
};
use iced::{Background, Border, Color, Element, Fill, Theme};
use std::time::Duration;

pub const ROW_H: f32 = 26.0;
const LARGE_ICON: f32 = ROW_H * 1.5;
const LARGE_W: f32 = ROW_H * 2.2;
const TOOL_BAR_H: f32 = 3.0 * ROW_H + 18.0;

fn tool_btn_style(theme: &Theme, is_active: bool, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let pair = match (is_active, status) {
        (true, _) => palette.primary.weak,
        (_, button::Status::Hovered) => palette.background.weak,
        (_, button::Status::Pressed) => palette.primary.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: is_active
            .then_some(Background::Color(pair.color))
            .or_else(|| {
                matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(pair.color))
            }),
        text_color: if status == button::Status::Disabled {
            pair.text.scale_alpha(0.4)
        } else {
            pair.text
        },
        border: Border {
            radius: 3.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

macro_rules! icon_asset {
    ($path:literal) => {
        include_bytes!(concat!(
            "../../..",
            "/vendor/opencadstudio/assets/icons/",
            $path
        )) as &'static [u8]
    };
}
const OPEN: &[u8] = icon_asset!("ui/folder_open.svg");
const SAVE: &[u8] = icon_asset!("ui/save.svg");
const PRINT: &[u8] = icon_asset!("ui/print.svg");
const CLOSE: &[u8] = icon_asset!("ui/close.svg");
const PREV: &[u8] = icon_asset!("ui/tri_left.svg");
const NEXT: &[u8] = icon_asset!("ui/tri_right.svg");
const ZOOM_IN: &[u8] = icon_asset!("zoom_in.svg");
const ZOOM_OUT: &[u8] = icon_asset!("zoom_out.svg");
const DOC: &[u8] = icon_asset!("ui/doc.svg");

fn icon(bytes: &'static [u8], size: f32) -> Element<'static, Message> {
    icon_state(bytes, size, true)
}
fn icon_state(bytes: &'static [u8], size: f32, enabled: bool) -> Element<'static, Message> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |theme: &Theme, _| svg::Style {
            color: Some(
                theme
                    .extended_palette()
                    .background
                    .base
                    .text
                    .scale_alpha(if enabled { 1.0 } else { 0.35 }),
            ),
        })
        .into()
}

// Port of OpenCADStudio render_large's plain Tool branch. CAD ToolDef events
// are replaced by the PDF application's typed messages; geometry is retained.
fn large_tool(
    label: &'static str,
    bytes: &'static [u8],
    action: Option<Message>,
) -> Element<'static, Message> {
    large_tool_active(label, bytes, action, false)
}

fn large_tool_active(
    label: &'static str,
    bytes: &'static [u8],
    action: Option<Message>,
    active: bool,
) -> Element<'static, Message> {
    let btn = button(
        column![
            container(icon_state(bytes, LARGE_ICON, action.is_some()))
                .width(Fill)
                .height(Fill)
                .align_x(iced::Center)
                .align_y(iced::Center),
            text(label).size(10).width(Fill).align_x(iced::Center),
        ]
        .align_x(iced::Center)
        .spacing(0)
        .width(Fill)
        .height(Fill),
    )
    .on_press_maybe(action)
    .style(move |theme, status| tool_btn_style(theme, active, status))
    .width(LARGE_W)
    .height(Fill)
    .padding(4);
    tooltip(btn, text(label).size(11), tooltip::Position::Bottom)
        .gap(6)
        .delay(Duration::from_millis(400))
        .into()
}

// Port of OpenCADStudio quick_access_btn: same sizing and tooltip placement.
fn quick_access(
    bytes: &'static [u8],
    label: &'static str,
    action: Option<Message>,
) -> Element<'static, Message> {
    let btn = button(
        container(icon_state(bytes, 16.0, action.is_some()))
            .width(Fill)
            .height(Fill)
            .align_x(iced::Center)
            .align_y(iced::Center),
    )
    .on_press_maybe(action)
    .style(|theme, status| tool_btn_style(theme, false, status))
    .width(28)
    .height(24)
    .padding([2, 0]);
    tooltip(btn, text(label).size(11), tooltip::Position::Bottom)
        .gap(6)
        .delay(Duration::from_millis(400))
        .into()
}

// Port of render_group's assembled tools/label layout; PDF tools supplied by caller.
fn group(title: &'static str, tools: Vec<Element<'static, Message>>) -> Element<'static, Message> {
    let tools_el = tools
        .into_iter()
        .fold(row![].spacing(2).height(Fill).align_y(iced::Top), |r, e| {
            r.push(e)
        });
    container(
        column![
            tools_el,
            text(title).size(10).color(Color::from_rgb8(160, 160, 165))
        ]
        .align_x(iced::Center)
        .spacing(0)
        .padding([3u16, 4])
        .height(TOOL_BAR_H),
    )
    .style(|theme: &Theme| container::Style {
        border: Border {
            color: theme.extended_palette().background.strong.color,
            width: 0.5,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn surface(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.extended_palette().background.base.color.into()),
        ..Default::default()
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let has_doc = app.document.is_some();
    let (page, count, scale) = app.document.as_ref().map_or((0, 0, 1.0), |d| {
        let v = d.model.view();
        (v.page(), v.page_count(), v.viewport().scale())
    });
    let mut top = row![
        quick_access(OPEN, "Open", Some(Message::ShowOpen)),
        quick_access(SAVE, "Save", None),
        quick_access(PRINT, "Print", None),
        quick_access(
            icon_asset!("ui/undo.svg"),
            "Undo",
            app.document
                .as_ref()
                .filter(|d| d.model.can_undo())
                .map(|_| Message::Undo)
        ),
        quick_access(
            icon_asset!("ui/redo.svg"),
            "Redo",
            app.document
                .as_ref()
                .filter(|d| d.model.can_redo())
                .map(|_| Message::Redo)
        ),
    ]
    .spacing(2)
    .align_y(iced::Center);
    for tab in RibbonTab::ALL {
        let active = tab == app.active_ribbon;
        top = top.push(
            button(text(tab.label()).size(12))
                .on_press(Message::SelectRibbon(tab))
                .style(move |theme, status| tool_btn_style(theme, active, status))
                .padding([5, 14]),
        );
    }
    let top = container(top).width(Fill).padding([2, 4]).style(surface);
    let mut groups = row![group(
        "Document",
        vec![
            large_tool("Open", OPEN, Some(Message::ShowOpen)),
            large_tool("Save", SAVE, None),
            large_tool("Print", PRINT, None),
        ]
    )]
    .spacing(0);
    if app.active_ribbon == RibbonTab::Drawing {
        groups = groups.push(group(
            "Draw",
            vec![
                large_tool("Line", icon_asset!("line.svg"), None),
                large_tool("Polyline", icon_asset!("polyline.svg"), None),
                large_tool_active(
                    "Rectangle",
                    icon_asset!("shapes/rect.svg"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Rectangle)),
                    app.annotation_tool == AnnotationTool::Rectangle,
                ),
                large_tool("Circle", icon_asset!("circle/circle.svg"), None),
            ],
        ));
    } else if app.active_ribbon == RibbonTab::Comment {
        groups = groups.push(group(
            "Annotation",
            vec![
                large_tool_active(
                    "Select",
                    icon_asset!("blocks/select_objects.svg"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Select)),
                    app.annotation_tool == AnnotationTool::Select,
                ),
                large_tool_active(
                    "Rectangle",
                    icon_asset!("shapes/rect.svg"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Rectangle)),
                    app.annotation_tool == AnnotationTool::Rectangle,
                ),
                large_tool_active(
                    "Highlight",
                    icon_asset!("blocks/quick_select.svg"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Highlight)),
                    app.annotation_tool == AnnotationTool::Highlight,
                ),
                large_tool("Text", icon_asset!("text.svg"), None),
                large_tool("Measure", icon_asset!("measure.svg"), None),
            ],
        ));
        groups = groups.push(group(
            "Edit",
            vec![large_tool(
                "Delete",
                icon_asset!("ui/trash.svg"),
                app.document
                    .as_ref()
                    .and_then(|d| d.selected_annotation)
                    .map(|_| Message::RemoveSelectedAnnotation),
            )],
        ));
    }
    groups = groups
        .push(group(
            "View",
            vec![
                large_tool(
                    "Zoom in",
                    ZOOM_IN,
                    has_doc.then_some(Message::Zoom((scale * 1.25).min(8.0))),
                ),
                large_tool(
                    "Zoom out",
                    ZOOM_OUT,
                    has_doc.then_some(Message::Zoom((scale / 1.25).max(0.1))),
                ),
                large_tool(
                    "100%",
                    icon_asset!("zoom_ext.svg"),
                    has_doc.then_some(Message::Zoom(1.0)),
                ),
            ],
        ))
        .push(group(
            "Navigate",
            vec![
                large_tool(
                    "Previous",
                    PREV,
                    (has_doc && page > 0).then_some(Message::GoToPage(page.saturating_sub(1))),
                ),
                large_tool(
                    "Next",
                    NEXT,
                    (has_doc && page + 1 < count).then_some(Message::GoToPage(page + 1)),
                ),
            ],
        ))
        .push(group(
            "Panels",
            vec![
                large_tool("Pages", DOC, Some(Message::TogglePages)),
                large_tool(
                    "Properties",
                    icon_asset!("properties.svg"),
                    Some(Message::ToggleProperties),
                ),
            ],
        ));
    let ribbon = container(
        scrollable(groups).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        )),
    )
    .width(Fill)
    .style(surface);
    let name = app
        .document
        .as_ref()
        .and_then(|d| std::path::Path::new(&d.path).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Start".into());
    let tab = container(
        row![
            icon(DOC, 14.0),
            text(name.clone()).size(12),
            quick_access(CLOSE, "Close", has_doc.then_some(Message::Close))
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([1, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(theme.extended_palette().primary.weak.color.into()),
        text_color: Some(theme.extended_palette().primary.weak.text),
        ..Default::default()
    });
    let tabs = container(row![
        tab,
        quick_access(icon_asset!("ui/plus.svg"), "Open", Some(Message::ShowOpen))
    ])
    .padding(2)
    .width(Fill)
    .style(surface);
    let center: Element<'_, Message> =
        if let Some(document) = app.document.as_ref().filter(|d| d.bitmap.is_some()) {
            let handle = document.bitmap.clone().expect("filtered above");
            let (width, height) = document.bitmap_size;
            let page = iced::widget::stack![
                image(handle).width(width as f32).height(height as f32),
                annotation_layer::view(
                    &document.model,
                    document.displayed_page,
                    document.selected_annotation,
                    app.annotation_tool,
                    document.bitmap_page_points,
                    document.bitmap_size
                )
            ];
            scrollable(container(page).center_x(Fill).padding(24))
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::new(),
                    horizontal: scrollable::Scrollbar::new(),
                })
                .into()
        } else {
            container(
                column![
                    icon(OPEN, 44.0),
                    text("Open a PDF").size(22),
                    button("Open…")
                        .on_press(Message::ShowOpen)
                        .style(|t, s| tool_btn_style(t, true, s))
                ]
                .align_x(iced::Center)
                .spacing(16),
            )
            .center(Fill)
            .into()
        };
    let mut workspace = row![].height(Fill);
    if !app.hide_pages {
        let mut pages = column![text("Pages").size(12)].spacing(4).padding(8);
        for index in 0..count {
            pages = pages.push(
                button(
                    row![
                        icon(DOC, 16.0),
                        text(format!("Page {}", index + 1)).size(12)
                    ]
                    .spacing(10),
                )
                .width(Fill)
                .padding(8)
                .on_press(Message::GoToPage(index))
                .style(move |t, s| tool_btn_style(t, index == page, s)),
            );
        }
        workspace = workspace.push(
            container(scrollable(pages))
                .width(180)
                .height(Fill)
                .style(surface),
        );
    }
    workspace = workspace.push(container(center).width(Fill).height(Fill));
    if !app.hide_properties {
        let mut properties = column![
            text("Properties").size(12),
            text("DOCUMENT").size(10),
            text(name).size(12),
            text(format!("Pages   {count}")).size(12),
            text(format!("Zoom    {:.0}%", scale * 100.0)).size(12)
        ]
        .padding(12)
        .spacing(14);
        if let Some(annotation) = app
            .document
            .as_ref()
            .and_then(|d| d.selected_annotation.and_then(|id| d.model.get(id)))
        {
            let kind = match &annotation.kind {
                opds_native_core::annotations::Kind::Rectangle { color_rgb }
                    if *color_rgb == [255, 226, 67] =>
                {
                    "Highlight"
                }
                opds_native_core::annotations::Kind::Rectangle { .. } => "Rectangle",
                opds_native_core::annotations::Kind::Text { .. } => "Text",
            };
            properties = properties
                .push(text("ANNOTATION").size(10))
                .push(text(format!("{kind} #{}", annotation.id)).size(12))
                .push(text(format!("Page   {}", annotation.page + 1)).size(12))
                .push(
                    text(format!(
                        "X / Y   {:.1} / {:.1} pt",
                        annotation.bounds.x, annotation.bounds.y
                    ))
                    .size(12),
                )
                .push(button("Delete annotation").on_press(Message::RemoveSelectedAnnotation));
        }
        workspace = workspace.push(container(properties).width(230).height(Fill).style(surface));
    }
    let status = container(
        row![
            text(&app.message).size(11),
            Space::new().width(Fill),
            quick_access(
                PREV,
                "Previous",
                (has_doc && page > 0).then_some(Message::GoToPage(page.saturating_sub(1)))
            ),
            text(format!(
                "{} / {}",
                if has_doc { page + 1 } else { 0 },
                count
            ))
            .size(11),
            quick_access(
                NEXT,
                "Next",
                (has_doc && page + 1 < count).then_some(Message::GoToPage(page + 1))
            ),
            text(format!("{:.0}%", scale * 100.0)).size(11)
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([2, 8])
    .width(Fill)
    .style(surface);
    let content: Element<'_, Message> = column![top, ribbon, tabs, workspace, status]
        .width(Fill)
        .height(Fill)
        .into();
    if app.open_dialog {
        let dialog = container(
            column![
                text("Open PDF").size(20),
                text_input("Path to PDF file", &app.path_input)
                    .on_input(Message::PathChanged)
                    .on_submit(Message::Open),
                row![
                    button("Open").on_press(Message::Open),
                    button("Cancel").on_press(Message::CancelOpen)
                ]
                .spacing(8)
            ]
            .spacing(16)
            .padding(24),
        )
        .width(520)
        .style(surface);
        iced::widget::stack![
            content,
            iced::widget::opaque(container(dialog).center(Fill).style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }))
        ]
        .into()
    } else {
        content
    }
}
