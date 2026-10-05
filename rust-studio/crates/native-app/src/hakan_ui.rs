// SPDX-License-Identifier: GPL-3.0-only
// Adapted from Hakan Seven / OpenCADStudio, commit 2a15a370f6886d3e8695ab81c7d677e0cd484451.
// Source: src/ui/ribbon/widgets.rs and src/ui/ribbon/mod.rs.
// Changes: Iced 0.14 API, PDF command routing, reduced CAD-specific data dependencies.
use super::{
    annotation_layer, inline_text, pdf_save, AnnotationTool, App, Message, PrintDestination,
    PrintRange, PrintSubset, RibbonTab,
};
use iced::widget::{
    button, checkbox, column, container, image, mouse_area, pick_list, row, rule, scrollable, svg,
    text, text_editor, text_input, tooltip, Space,
};
use iced::{Background, Border, Color, Element, Fill, Padding, Theme};
use opds_native_core::annotations::TextAlign;
use std::time::Duration;

pub const ROW_H: f32 = 26.0;
const LARGE_W: f32 = ROW_H * 2.2;
const PDF_RIBBON_H: f32 = 94.0;

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
macro_rules! pdf_icon {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/ribbon/home/", $name, ".svg")) as &'static [u8]
    };
}
macro_rules! annotation_icon {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/ribbon/annotation/", $name, ".svg"))
            as &'static [u8]
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
    large_tool_sized(label, bytes, action, active, LARGE_W)
}

fn large_tool_sized(
    label: &'static str,
    bytes: &'static [u8],
    action: Option<Message>,
    active: bool,
    width: f32,
) -> Element<'static, Message> {
    let btn = button(
        column![
            container(icon_state(bytes, 24.0, action.is_some()))
                .width(Fill)
                .height(30)
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
    .style(move |_, status| pdf_tool_style(active, status))
    .width(width)
    .height(66)
    .padding([3, 3]);
    tooltip(btn, text(label).size(11), tooltip::Position::Bottom)
        .gap(6)
        .delay(Duration::from_millis(400))
        .into()
}

// Hakan's labeled-small ribbon item, sized to the PDF app's 22/33 px rows.
fn labeled_tool(
    label: &'static str,
    bytes: &'static [u8],
    action: Option<Message>,
    width: f32,
    height: f32,
) -> Element<'static, Message> {
    let btn = button(
        row![
            icon_state(
                bytes,
                if height > 22.0 { 18.0 } else { 14.0 },
                action.is_some()
            ),
            text(label)
                .size(11)
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press_maybe(action)
    .style(|_, status| pdf_tool_style(false, status))
    .width(width)
    .height(height)
    .padding([2, 4]);
    tooltip(btn, text(label).size(11), tooltip::Position::Bottom)
        .gap(6)
        .delay(Duration::from_millis(400))
        .into()
}

fn stack_tools(tools: Vec<Element<'static, Message>>) -> Element<'static, Message> {
    tools
        .into_iter()
        .fold(column![].spacing(1), |c, tool| c.push(tool))
        .into()
}

fn pdf_tool_style(active: bool, status: button::Status) -> button::Style {
    let (background, border) = if active {
        (
            Some(Color::from_rgba8(217, 119, 6, 0.20)),
            Color::from_rgba8(217, 119, 6, 0.40),
        )
    } else if status == button::Status::Hovered {
        (
            Some(Color::from_rgba8(217, 119, 6, 0.15)),
            Color::from_rgba8(217, 119, 6, 0.30),
        )
    } else {
        (None, Color::TRANSPARENT)
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: if active {
            Color::from_rgb8(217, 119, 6)
        } else {
            Color::from_rgba8(
                250,
                250,
                249,
                if status == button::Status::Disabled {
                    0.4
                } else {
                    0.6
                },
            )
        },
        border: Border {
            color: border,
            width: 1.0,
            radius: 2.0.into(),
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
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
        .fold(row![].spacing(1).height(76).align_y(iced::Top), |r, e| {
            r.push(e)
        });
    row![
        container(
            column![
                tools_el,
                text(title.to_uppercase())
                    .size(9)
                    .color(Color::from_rgba8(217, 119, 6, 0.8))
            ]
            .align_x(iced::Center)
            .spacing(0)
            .padding(Padding {
                top: 4.0,
                bottom: 0.0,
                left: 6.0,
                right: 6.0
            })
            .height(PDF_RIBBON_H),
        ),
        rule::vertical(1).style(|_| rule::Style {
            color: Color::from_rgb8(70, 70, 77),
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true
        })
    ]
    .height(PDF_RIBBON_H)
    .into()
}

fn tab_style(active: bool, status: button::Status) -> button::Style {
    button::Style {
        background: if active {
            Some(Color::from_rgb8(54, 54, 62).into())
        } else if status == button::Status::Hovered {
            Some(Color::from_rgba8(217, 119, 6, 0.10).into())
        } else {
            None
        },
        text_color: if active || status == button::Status::Hovered {
            Color::from_rgb8(250, 250, 249)
        } else {
            Color::from_rgba8(250, 250, 249, 0.6)
        },
        border: Border {
            color: if active {
                Color::from_rgba8(217, 119, 6, 0.25)
            } else {
                Color::TRANSPARENT
            },
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

fn window_control(label: &'static str, action: Message, close: bool) -> Element<'static, Message> {
    button(text(label).size(13))
        .on_press(action)
        .width(30)
        .height(30)
        .padding(0)
        .style(move |_, status| button::Style {
            background: match (close, status) {
                (true, button::Status::Hovered) => Some(Color::from_rgb8(190, 42, 44).into()),
                (_, button::Status::Hovered) => Some(Color::from_rgb8(76, 76, 82).into()),
                _ => None,
            },
            text_color: Color::from_rgb8(250, 250, 249),
            ..Default::default()
        })
        .into()
}

fn ribbon_home(
    has_doc: bool,
    page: usize,
    count: usize,
    scale: f64,
    selecting: bool,
) -> Vec<Element<'static, Message>> {
    vec![
        group(
            "Document",
            vec![
                large_tool_sized("New", pdf_icon!("newDocIcon"), None, false, 44.0),
                large_tool_sized("IFC-report", pdf_icon!("ifcExportIcon"), None, false, 52.0),
                large_tool_sized("To CAD", pdf_icon!("cadExportIcon"), None, false, 48.0),
                large_tool_sized("E-mail", pdf_icon!("emailIcon"), None, false, 45.0),
                large_tool_sized("Raster-PDF", pdf_icon!("rasterPdfIcon"), None, false, 57.0),
            ],
        ),
        group(
            "Tools",
            vec![
                large_tool_sized("Hand", pdf_icon!("handIcon"), None, false, 44.0),
                large_tool_sized(
                    "Select",
                    pdf_icon!("selectTextIcon"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Select)),
                    selecting,
                    44.0,
                ),
                large_tool_sized("Screenshot", pdf_icon!("screenshotIcon"), None, false, 58.0),
            ],
        ),
        group(
            "View",
            vec![
                stack_tools(vec![
                    labeled_tool(
                        "Zoom In",
                        pdf_icon!("zoomInIcon"),
                        has_doc.then_some(Message::Zoom((scale * 1.25).min(8.0))),
                        84.0,
                        22.0,
                    ),
                    labeled_tool(
                        "Zoom Out",
                        pdf_icon!("zoomOutIcon"),
                        has_doc.then_some(Message::Zoom((scale / 1.25).max(0.1))),
                        84.0,
                        22.0,
                    ),
                    labeled_tool("Fit Width", pdf_icon!("fitWidthIcon"), None, 84.0, 22.0),
                ]),
                stack_tools(vec![
                    labeled_tool(
                        "100%",
                        pdf_icon!("actualSizeIcon"),
                        has_doc.then_some(Message::Zoom(1.0)),
                        78.0,
                        22.0,
                    ),
                    labeled_tool("Fit Page", pdf_icon!("fitPageIcon"), None, 78.0, 22.0),
                ]),
            ],
        ),
        group(
            "Navigate",
            vec![
                stack_tools(vec![
                    labeled_tool(
                        "First",
                        pdf_icon!("firstPageIcon"),
                        (has_doc && page > 0).then_some(Message::GoToPage(0)),
                        76.0,
                        33.0,
                    ),
                    labeled_tool(
                        "Previous",
                        pdf_icon!("prevPageIcon"),
                        (has_doc && page > 0).then_some(Message::GoToPage(page.saturating_sub(1))),
                        76.0,
                        33.0,
                    ),
                ]),
                stack_tools(vec![
                    labeled_tool(
                        "Next",
                        pdf_icon!("nextPageIcon"),
                        (has_doc && page + 1 < count).then_some(Message::GoToPage(page + 1)),
                        76.0,
                        33.0,
                    ),
                    labeled_tool(
                        "Last",
                        pdf_icon!("lastPageIcon"),
                        (has_doc && count > 0 && page + 1 < count)
                            .then_some(Message::GoToPage(count.saturating_sub(1))),
                        76.0,
                        33.0,
                    ),
                ]),
            ],
        ),
        group(
            "Find",
            vec![large_tool_sized(
                "Find",
                pdf_icon!("findIcon"),
                None,
                false,
                44.0,
            )],
        ),
        group(
            "Settings",
            vec![
                large_tool_sized(
                    "Preferences",
                    pdf_icon!("preferencesIcon"),
                    None,
                    false,
                    70.0,
                ),
                large_tool_sized(
                    "Previous version",
                    pdf_icon!("previousVersionIcon"),
                    None,
                    false,
                    65.0,
                ),
            ],
        ),
    ]
}

fn annotation_properties() -> Element<'static, Message> {
    let label = |name| text(name).size(9).color(Color::WHITE);
    let color = container(Space::new().width(20).height(10)).style(|_| container::Style {
        background: Some(Color::from_rgb8(239, 30, 31).into()),
        border: Border {
            color: Color::from_rgb8(126, 126, 131),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    });
    let field = container(text("2  ▴").size(10))
        .width(39)
        .height(19)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(75, 75, 81).into()),
            border: Border {
                color: Color::from_rgb8(115, 115, 120),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });
    column![
        row![label("COLOR"), color]
            .spacing(9)
            .align_y(iced::Center)
            .height(23),
        row![label("WIDTH"), field]
            .spacing(9)
            .align_y(iced::Center)
            .height(23),
    ]
    .width(78)
    .into()
}

// Layout and icon source: Open PDF Studio's CommentTab.jsx and ribbonIcons.js.
// Group composition remains Hakan Seven's OpenCADStudio ribbon port above.
fn ribbon_annotation(
    has_doc: bool,
    highlighting: bool,
    typing: bool,
) -> Vec<Element<'static, Message>> {
    vec![
        group(
            "Drawing",
            vec![large_tool_sized(
                "Highlight",
                annotation_icon!("highlightIcon"),
                has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Highlight)),
                highlighting,
                51.0,
            )],
        ),
        group(
            "Text",
            vec![
                large_tool_sized(
                    "Text Box",
                    annotation_icon!("textboxIcon"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Text)),
                    typing,
                    48.0,
                ),
                large_tool_sized(
                    "Callout",
                    annotation_icon!("calloutIcon"),
                    None,
                    false,
                    48.0,
                ),
                large_tool_sized("Note", annotation_icon!("noteIcon"), None, false, 40.0),
            ],
        ),
        group(
            "Stamp",
            vec![
                large_tool_sized("Stamp", annotation_icon!("stampIcon"), None, false, 49.0),
                large_tool_sized(
                    "Signature",
                    annotation_icon!("signatureIcon"),
                    None,
                    false,
                    53.0,
                ),
                large_tool_sized(
                    "Parametric\nSymbol",
                    annotation_icon!("stampIcon"),
                    None,
                    false,
                    60.0,
                ),
            ],
        ),
        group(
            "Redaction",
            vec![
                large_tool_sized(
                    "Redact",
                    annotation_icon!("redactionIcon"),
                    None,
                    false,
                    52.0,
                ),
                large_tool_sized(
                    "Apply",
                    annotation_icon!("applyRedactionsIcon"),
                    None,
                    false,
                    45.0,
                ),
            ],
        ),
        group("Properties", vec![annotation_properties()]),
        group(
            "Edit",
            vec![stack_tools(vec![
                labeled_tool(
                    "Clear Page",
                    annotation_icon!("clearPageIcon"),
                    None,
                    87.0,
                    31.0,
                ),
                labeled_tool(
                    "Clear All",
                    annotation_icon!("clearAllIcon"),
                    None,
                    87.0,
                    31.0,
                ),
            ])],
        ),
        group(
            "Measure",
            vec![
                stack_tools(vec![
                    labeled_tool(
                        "Distance",
                        annotation_icon!("measureDistanceIcon"),
                        None,
                        81.0,
                        31.0,
                    ),
                    labeled_tool(
                        "Area",
                        annotation_icon!("measureAreaIcon"),
                        None,
                        81.0,
                        31.0,
                    ),
                ]),
                stack_tools(vec![
                    labeled_tool(
                        "Perimeter",
                        annotation_icon!("measurePerimeterIcon"),
                        None,
                        81.0,
                        31.0,
                    ),
                    labeled_tool(
                        "Angle",
                        annotation_icon!("measureAngleIcon"),
                        None,
                        81.0,
                        31.0,
                    ),
                ]),
            ],
        ),
        group(
            "Quantities",
            vec![large_tool_sized(
                "Quantities",
                annotation_icon!("scheduleIcon"),
                None,
                false,
                70.0,
            )],
        ),
    ]
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
    let mut top = row![button(text("File").size(12))
        .on_press(Message::ShowOpen)
        .style(|_, status| button::Style {
            background: Some(
                Color::from_rgb8(
                    if status == button::Status::Hovered {
                        234
                    } else {
                        217
                    },
                    if status == button::Status::Hovered {
                        88
                    } else {
                        119
                    },
                    if status == button::Status::Hovered {
                        12
                    } else {
                        6
                    }
                )
                .into()
            ),
            text_color: Color::from_rgb8(54, 54, 62),
            border: Border {
                radius: 0.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .width(58)
        .height(28)
        .padding([5, 16])]
    .spacing(2)
    .align_y(iced::Alignment::End);
    for tab in RibbonTab::ALL {
        let active = tab == app.active_ribbon;
        top = top.push(
            button(text(tab.label()).size(12))
                .on_press(Message::SelectRibbon(tab))
                .style(move |_, status| tab_style(active, status))
                .height(28)
                .padding([5, 16]),
        );
    }
    let selected_text_style = app
        .document
        .as_ref()
        .and_then(|document| {
            document
                .selected_annotation
                .and_then(|id| document.model.get(id))
        })
        .and_then(|item| match &item.kind {
            opds_native_core::annotations::Kind::Text {
                color_rgb,
                font_size,
                align,
                ..
            } => Some((*color_rgb, *font_size, *align)),
            _ => None,
        });
    if app.text_dialog || selected_text_style.is_some() {
        top = top.push(
            button(text("Format").size(12))
                .on_press(Message::SelectRibbon(RibbonTab::Format))
                .style(move |_, status| tab_style(app.active_ribbon == RibbonTab::Format, status))
                .height(28)
                .padding([5, 16]),
        );
    }
    top = top.push(Space::new().width(Fill)).push(
        button(text("⌃").size(12))
            .on_press(Message::ToggleRibbonCollapsed)
            .style(|_, status| tab_style(false, status))
            .width(22)
            .height(28)
            .padding(0),
    );
    let top = container(top)
        .width(Fill)
        .height(28)
        .padding([0, 2])
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(62, 62, 70).into()),
            ..Default::default()
        });
    let mut groups = row![].spacing(0);
    if app.active_ribbon == RibbonTab::Home {
        for item in ribbon_home(
            has_doc,
            page,
            count,
            scale,
            app.annotation_tool_activated && app.annotation_tool == AnnotationTool::Select,
        ) {
            groups = groups.push(item);
        }
    } else if app.active_ribbon == RibbonTab::Comment {
        for item in ribbon_annotation(
            has_doc,
            app.annotation_tool_activated && app.annotation_tool == AnnotationTool::Highlight,
            app.annotation_tool_activated && app.annotation_tool == AnnotationTool::Text,
        ) {
            groups = groups.push(item);
        }
    } else if app.active_ribbon == RibbonTab::Format {
        // Keep the source gallery order and footprint (FormatTab.jsx). The
        // three cloudy presets remain disabled until their PDF appearance is
        // represented in the native annotation model.
        let colors: [(&str, [u8; 3], bool); 10] = [
            ("Red", [255, 0, 0], true),
            ("Purple", [128, 0, 128], true),
            ("Indigo", [75, 0, 130], true),
            ("Blue", [0, 102, 204], true),
            ("Green", [0, 128, 0], true),
            ("Yellow", [230, 168, 23], true),
            ("Black", [0, 0, 0], true),
            ("Red Cloudy", [255, 0, 0], false),
            ("Purple Cloudy", [128, 0, 128], false),
            ("Indigo Cloudy", [123, 104, 238], false),
        ];
        groups = groups.push(group(
            "",
            colors
                .into_iter()
                .map(|(name, color, enabled)| {
                    let active = if app.text_dialog {
                        app.text_color
                    } else {
                        selected_text_style
                            .map(|style| style.0)
                            .unwrap_or(app.text_color)
                    } == color
                        && enabled;
                    let preview = container(
                        text("T")
                            .size(20)
                            .color(Color::from_rgb8(color[0], color[1], color[2])),
                    )
                    .width(39)
                    .height(35)
                    .align_x(iced::Center)
                    .align_y(iced::Center)
                    .style(move |_| container::Style {
                        border: Border {
                            color: Color::from_rgb8(color[0], color[1], color[2]),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    });
                    button(
                        column![
                            preview,
                            text(name)
                                .size(9)
                                .shaping(iced::widget::text::Shaping::Basic),
                        ]
                        .align_x(iced::Center),
                    )
                    .on_press_maybe(enabled.then_some(Message::SetTextColor(color)))
                    .style(move |_, status| pdf_tool_style(active, status))
                    .width(50)
                    .height(62)
                    .padding([2, 2])
                    .into()
                })
                .collect(),
        ));
        let selected_size = if app.text_dialog {
            app.text_font_size
        } else {
            selected_text_style
                .map(|style| style.1)
                .unwrap_or(app.text_font_size)
        };
        groups = groups.push(group(
            "Font",
            vec![column![
                text("SIZE").size(9),
                pick_list(
                    opds_native_core::annotations::TEXT_FONT_SIZES,
                    Some(selected_size),
                    Message::SetTextFontSize,
                )
                .width(72),
                text("pt").size(9),
            ]
            .spacing(2)
            .align_x(iced::Center)
            .into()],
        ));
        let selected_align = if app.text_dialog {
            app.text_align
        } else {
            selected_text_style
                .map(|style| style.2)
                .unwrap_or(app.text_align)
        };
        groups = groups.push(group(
            "Paragraph",
            [
                ("Left", TextAlign::Left),
                ("Center", TextAlign::Center),
                ("Right", TextAlign::Right),
            ]
            .into_iter()
            .map(|(label, align)| {
                button(text(label).size(11))
                    .on_press(Message::SetTextAlign(align))
                    .style(move |_, status| pdf_tool_style(selected_align == align, status))
                    .width(54)
                    .height(66)
                    .padding([4, 2])
                    .into()
            })
            .collect(),
        ));
    } else {
        groups = groups.push(group(
            "Document",
            vec![
                large_tool("Open", OPEN, Some(Message::ShowOpen)),
                large_tool("Save As", SAVE, has_doc.then_some(Message::ShowSave)),
                large_tool("Print", PRINT, has_doc.then_some(Message::ShowPrint)),
            ],
        ));
    }
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
    }
    if app.active_ribbon == RibbonTab::Help {
        groups = groups.push(group(
            "PDF Engine",
            vec![
                large_tool_active(
                    "Hayro",
                    DOC,
                    (!app.busy)
                        .then_some(Message::SelectBackend(opds_dynlex_backend::Backend::Hayro)),
                    app.backend == opds_dynlex_backend::Backend::Hayro,
                ),
                large_tool_active(
                    "DynLex",
                    icon_asset!("zoom_ext.svg"),
                    (!app.busy && cfg!(feature = "dynlex-experimental"))
                        .then_some(Message::SelectBackend(opds_dynlex_backend::Backend::Dynlex)),
                    app.backend == opds_dynlex_backend::Backend::Dynlex,
                ),
            ],
        ));
    }
    if !matches!(
        app.active_ribbon,
        RibbonTab::Home | RibbonTab::Comment | RibbonTab::Format
    ) {
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
    }
    let ribbon = container(
        scrollable(groups).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(0)
                .scroller_width(0)
                .margin(0)
                .spacing(0),
        )),
    )
    .width(Fill)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(54, 54, 62).into()),
        ..Default::default()
    });
    let name = app
        .document
        .as_ref()
        .and_then(|d| std::path::Path::new(&d.path).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Start".into());
    let app_icon = image(image::Handle::from_rgba(
        16,
        16,
        include_bytes!("../../../assets/ribbon/title-icon.rgba").to_vec(),
    ))
    .width(16)
    .height(16);
    let title_tools = row![
        app_icon,
        quick_access(OPEN, "Open", Some(Message::ShowOpen)),
        quick_access(SAVE, "Save As", has_doc.then_some(Message::ShowSave)),
        quick_access(PRINT, "Print", has_doc.then_some(Message::ShowPrint)),
        quick_access(pdf_icon!("preferencesIcon"), "Settings", None),
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
    let title_text = format!("Open PDF Studio v2026.39.0  {name}");
    let drag_region = mouse_area(container(text(title_text).size(12)).center(Fill).height(30))
        .on_press(Message::DragWindow)
        .on_double_click(Message::MaximizeWindow);
    let titlebar = container(
        row![
            container(title_tools)
                .width(272)
                .height(30)
                .padding([0, 8])
                .align_y(iced::Center),
            drag_region,
            container(
                row![
                    Space::new().width(Fill),
                    window_control("−", Message::MinimizeWindow, false),
                    window_control("□", Message::MaximizeWindow, false),
                    window_control("×", Message::CloseWindow, true),
                ]
                .align_y(iced::Center),
            )
            .width(272)
            .height(30),
        ]
        .height(30)
        .align_y(iced::Center),
    )
    .width(Fill)
    .height(30)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(53, 53, 61).into()),
        ..Default::default()
    });
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
            let mut page: Element<'_, Message> = iced::widget::stack![
                image(handle).width(width as f32).height(height as f32),
                annotation_layer::view(
                    &document.model,
                    document.displayed_page,
                    document.selected_annotation,
                    app.editing_text_id,
                    app.annotation_tool,
                    document.bitmap_page_points,
                    document.bitmap_size
                )
            ]
            .into();
            if app.text_dialog {
                let edit_bounds = app.pending_text_bounds.or_else(|| {
                    app.editing_text_id
                        .and_then(|id| document.model.get(id).map(|item| item.bounds))
                });
                if let Some(bounds) = edit_bounds {
                    let scale_x = width as f32 / document.bitmap_page_points.0;
                    let scale_y = height as f32 / document.bitmap_page_points.1;
                    let x = (bounds.x as f32 * scale_x).max(0.0);
                    let y = (bounds.y as f32 * scale_y).max(0.0);
                    let w = (bounds.width as f32 * scale_x).max(80.0);
                    let h = (bounds.height as f32 * scale_y).max(40.0);
                    let editor = text_editor(&app.text_draft)
                        .id(iced::widget::Id::new("inline-text-editor"))
                        .placeholder("Type text here")
                        .height(h)
                        .font(iced::Font::with_name("Liberation Sans"))
                        .size(iced::Pixels(f32::from(app.text_font_size) * scale_y))
                        .line_height(iced::Pixels(f32::from(app.text_font_size + 4) * scale_y))
                        .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
                        .padding(0)
                        .style(move |theme, status| {
                            let mut style = text_editor::default(theme, status);
                            style.background = Color::WHITE.into();
                            style.value = Color::from_rgb8(
                                app.text_color[0],
                                app.text_color[1],
                                app.text_color[2],
                            );
                            style.placeholder = Color::from_rgb8(110, 110, 110);
                            if app.text_align != TextAlign::Left {
                                style.value = Color::TRANSPARENT;
                                style.placeholder = Color::TRANSPARENT;
                                style.selection = Color::TRANSPARENT;
                            }
                            style
                        })
                        .key_binding(|press| {
                            if press.modifiers.control()
                                && matches!(
                                    press.key,
                                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
                                )
                            {
                                Some(text_editor::Binding::Custom(Message::CommitText))
                            } else {
                                text_editor::Binding::from_key_press(press)
                            }
                        })
                        .on_action(Message::TextEdited);
                    let editor: Element<'_, Message> = if app.text_align == TextAlign::Left {
                        editor.into()
                    } else {
                        inline_text::with_aligned_ime(
                            iced::widget::stack![
                                editor,
                                inline_text::view(
                                    &app.text_draft,
                                    app.text_align,
                                    f32::from(app.text_font_size) * scale_y,
                                    f32::from(app.text_font_size + 4) * scale_y,
                                    Color::from_rgb8(
                                        app.text_color[0],
                                        app.text_color[1],
                                        app.text_color[2]
                                    ),
                                    w,
                                    h,
                                ),
                            ]
                            .into(),
                            &app.text_draft,
                            app.text_align,
                            f32::from(app.text_font_size) * scale_y,
                            f32::from(app.text_font_size + 4) * scale_y,
                        )
                    };
                    let input = column![
                        container(editor).width(w),
                        row![
                            button("Save text").on_press(Message::CommitText),
                            button("Cancel").on_press(Message::CancelText),
                        ]
                        .spacing(4),
                    ]
                    .spacing(2);
                    page = iced::widget::stack![
                        page,
                        column![Space::new().height(y), row![Space::new().width(x), input],]
                    ]
                    .into();
                }
            }
            let page_with_pan_room = row![
                Space::new().width(super::HORIZONTAL_PAN_MARGIN),
                page,
                Space::new().width(super::HORIZONTAL_PAN_MARGIN),
            ];
            scrollable(container(page_with_pan_room).padding(24))
                .id(iced::widget::Id::new("pdf-page-scroll"))
                .on_scroll(Message::PageScrolled)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::new(),
                    horizontal: scrollable::Scrollbar::new()
                        .width(0)
                        .scroller_width(0)
                        .margin(0)
                        .spacing(0),
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
        let focus = app.document.as_ref().map_or(0, |d| d.thumbnail_focus);
        let visible = super::thumbnail_window(focus, count);
        let mut pages = column![container(text("Thumbnails").size(12))
            .height(22)
            .align_y(iced::Center)]
        .spacing(0)
        .padding(8);
        if visible.start > 0 {
            pages =
                pages.push(Space::new().height(visible.start as f32 * super::THUMBNAIL_ROW_STEP));
        }
        for index in visible.clone() {
            let handle = app.document.as_ref().and_then(|document| {
                if document.displayed_page == index {
                    document.bitmap.clone()
                } else {
                    document
                        .thumbnails
                        .iter()
                        .find(|(page, _)| *page == index)
                        .map(|(_, handle)| handle.clone())
                }
            });
            let preview: Element<'_, Message> = if let Some(handle) = handle {
                image(handle).width(128).height(170).into()
            } else {
                container(text("Page preview").size(11))
                    .width(128)
                    .height(170)
                    .align_x(iced::Center)
                    .align_y(iced::Center)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgb8(238, 238, 238).into()),
                        text_color: Some(Color::from_rgb8(100, 100, 100)),
                        ..Default::default()
                    })
                    .into()
            };
            pages = pages.push(
                container(
                    button(
                        column![preview, text(format!("{}", index + 1)).size(11)]
                            .align_x(iced::Center)
                            .spacing(4),
                    )
                    .width(Fill)
                    .height(super::THUMBNAIL_CARD_HEIGHT)
                    .padding(7)
                    .on_press(Message::GoToPage(index))
                    .style(move |t, s| tool_btn_style(t, index == page, s)),
                )
                .width(Fill)
                .height(super::THUMBNAIL_ROW_STEP),
            );
        }
        if visible.end < count {
            pages = pages.push(
                Space::new().height((count - visible.end) as f32 * super::THUMBNAIL_ROW_STEP),
            );
        }
        workspace = workspace.push(
            container(
                scrollable(pages)
                    .id(iced::widget::Id::new("pdf-thumbnails"))
                    .on_scroll(Message::ThumbnailsScrolled),
            )
            .width(200)
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
                .push(
                    text(format!(
                        "W / H   {:.1} / {:.1} pt",
                        annotation.bounds.width, annotation.bounds.height
                    ))
                    .size(12),
                );
            if matches!(
                annotation.kind,
                opds_native_core::annotations::Kind::Text { .. }
            ) {
                properties =
                    properties.push(button("Edit text").on_press(Message::EditSelectedText));
            }
            properties = properties
                .push(button("Delete annotation").on_press(Message::RemoveSelectedAnnotation));
        }
        workspace = workspace.push(container(properties).width(230).height(Fill).style(surface));
    }
    let renderer_label = app
        .document
        .as_ref()
        .map(|document| match (app.backend, document.bitmap_backend) {
            (_, opds_dynlex_backend::Backend::Dynlex) => "Renderer: DynLex",
            (opds_dynlex_backend::Backend::Dynlex, opds_dynlex_backend::Backend::Hayro) => {
                "Renderer: Hayro (DynLex fallback)"
            }
            _ => "Renderer: Hayro",
        })
        .unwrap_or("");
    let status = container(
        row![
            text(&app.message).size(11),
            text(renderer_label).size(11),
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
    let content: Element<'_, Message> = if app.ribbon_collapsed {
        column![titlebar, top, tabs, workspace, status]
    } else {
        column![titlebar, top, ribbon, tabs, workspace, status]
    }
    .width(Fill)
    .height(Fill)
    .into();
    if let Some(action) = &app.confirm_discard {
        let action_label = match action {
            super::PendingAction::Open => "open another PDF",
            super::PendingAction::Close => "close this PDF",
            super::PendingAction::Exit => "exit",
        };
        let dialog = container(
            column![
                text("Unsaved annotations").size(20),
                text(format!(
                    "Save As first to keep your changes. Discard them and {action_label}?"
                ))
                .size(12),
                row![
                    button("Keep editing").on_press(Message::CancelDiscard),
                    button("Discard changes").on_press(Message::ConfirmDiscard),
                ]
                .spacing(8),
            ]
            .spacing(16)
            .padding(24),
        )
        .width(480)
        .style(surface);
        iced::widget::stack![
            content,
            iced::widget::opaque(container(dialog).center(Fill).style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }))
        ]
        .into()
    } else if app.print_dialog {
        let current = app
            .document
            .as_ref()
            .map(|d| d.model.view().page() + 1)
            .unwrap_or(1);
        let total = app
            .document
            .as_ref()
            .map(|d| d.pdf.page_count())
            .unwrap_or(0);
        let selection = super::print_pages(
            app.print_range,
            app.print_subset,
            app.print_reverse,
            current.saturating_sub(1),
            &app.print_custom_pages,
            total,
        );
        let selected_count = selection.as_ref().map(|pages| pages.len()).unwrap_or(0);
        let left = column![
            text("Printer").size(14),
            row![
                button("PDF file")
                    .on_press(Message::SelectPrintDestination(PrintDestination::PdfFile))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_destination == PrintDestination::PdfFile,
                        status
                    )),
                button("Local IPP test")
                    .on_press(Message::SelectPrintDestination(
                        PrintDestination::VirtualIpp
                    ))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_destination == PrintDestination::VirtualIpp,
                        status
                    )),
            ]
            .spacing(8),
            text(if app.print_destination == PrintDestination::VirtualIpp {
                "Sends only to a temporary 127.0.0.1 test server; saves its capture."
            } else {
                "Creates a new PDF; existing files are not overwritten."
            })
            .size(12),
            text_input("Output PDF path", &app.print_path_input)
                .on_input(Message::PrintPathChanged)
                .on_submit(Message::PrintToPdf),
            rule::horizontal(1),
            text("Page Range").size(14),
            row![
                button("All")
                    .on_press(Message::SelectPrintRange(PrintRange::All))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_range == PrintRange::All,
                        status
                    )),
                button(text(format!("Current: {current}")))
                    .on_press(Message::SelectPrintRange(PrintRange::Current))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_range == PrintRange::Current,
                        status
                    )),
                button("Custom")
                    .on_press(Message::SelectPrintRange(PrintRange::Custom))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_range == PrintRange::Custom,
                        status
                    )),
            ]
            .spacing(6),
            text_input("Pages, e.g. 1, 3-5", &app.print_custom_pages)
                .on_input(Message::PrintCustomPagesChanged),
            row![
                text("Subset").size(12),
                button("All")
                    .on_press(Message::SelectPrintSubset(PrintSubset::All))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_subset == PrintSubset::All,
                        status
                    )),
                button("Odd")
                    .on_press(Message::SelectPrintSubset(PrintSubset::Odd))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_subset == PrintSubset::Odd,
                        status
                    )),
                button("Even")
                    .on_press(Message::SelectPrintSubset(PrintSubset::Even))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_subset == PrintSubset::Even,
                        status
                    )),
                checkbox(app.print_reverse)
                    .label("Reverse Order")
                    .on_toggle(Message::SetPrintReverse)
                    .size(14),
            ]
            .spacing(8)
            .align_y(iced::Center),
            rule::horizontal(1),
            text("Page Placement and Scaling").size(14),
            row![
                text("Paper").size(12),
                button("Original")
                    .on_press(Message::SelectPrintPaper(pdf_save::PrintPaper::Original))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_paper == pdf_save::PrintPaper::Original,
                        status
                    )),
                button("A4")
                    .on_press(Message::SelectPrintPaper(pdf_save::PrintPaper::A4))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_paper == pdf_save::PrintPaper::A4,
                        status
                    )),
                button("Letter")
                    .on_press(Message::SelectPrintPaper(pdf_save::PrintPaper::Letter))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_paper == pdf_save::PrintPaper::Letter,
                        status
                    )),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text("Type").size(12),
                button("Fit")
                    .on_press(Message::SelectPrintScale(pdf_save::PrintScale::Fit))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_scale == pdf_save::PrintScale::Fit,
                        status
                    )),
                button("Actual")
                    .on_press(Message::SelectPrintScale(pdf_save::PrintScale::Actual))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_scale == pdf_save::PrintScale::Actual,
                        status
                    )),
                button("Shrink")
                    .on_press(Message::SelectPrintScale(pdf_save::PrintScale::Shrink))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_scale == pdf_save::PrintScale::Shrink,
                        status
                    )),
                button("Custom")
                    .on_press(Message::SelectPrintScale(pdf_save::PrintScale::Custom))
                    .style(|theme, status| tool_btn_style(
                        theme,
                        app.print_scale == pdf_save::PrintScale::Custom,
                        status
                    )),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text("Page Zoom").size(12),
                text_input("100", &app.print_zoom_draft)
                    .on_input(Message::PrintZoomDraftChanged)
                    .on_submit(Message::ApplyPrintZoom)
                    .width(70),
                text("%").size(12),
                button("Apply").on_press_maybe(
                    app.print_preview_base_bytes
                        .is_some()
                        .then_some(Message::ApplyPrintZoom)
                ),
            ]
            .spacing(8)
            .align_y(iced::Center),
            text("Centered on the selected sheet · 10–400%").size(12),
            text(format!(
                "Selected: {}",
                match &selection {
                    Ok(pages) => pages.len().to_string(),
                    Err(error) => error.clone(),
                }
            ))
            .size(12),
            rule::horizontal(1),
            row![
                button(if app.print_destination == PrintDestination::VirtualIpp {
                    "Test virtual printer"
                } else {
                    "Create print PDF"
                })
                .on_press_maybe(
                    (selection.is_ok() && app.print_preview_pdf.is_some())
                        .then_some(Message::PrintToPdf)
                ),
                button("Cancel").on_press(Message::CancelPrint)
            ]
            .spacing(8),
        ]
        .spacing(12)
        .width(430);
        let preview: Element<'_, Message> = if let Some(bitmap) = &app.print_preview {
            image(bitmap.clone()).width(300).into()
        } else {
            text(if app.print_preview_error.is_empty() {
                "Rendering preview…"
            } else {
                &app.print_preview_error
            })
            .size(12)
            .into()
        };
        let right = column![
            text(if selection.is_ok() {
                let placement = match app.print_scale {
                    pdf_save::PrintScale::Fit => "Fit".to_owned(),
                    pdf_save::PrintScale::Actual => "Actual".to_owned(),
                    pdf_save::PrintScale::Shrink => "Shrink".to_owned(),
                    pdf_save::PrintScale::Custom => format!("{}%", app.print_zoom_percent),
                };
                format!(
                    "Preview · page {} of {} · {}",
                    app.print_preview_page + 1,
                    total,
                    placement
                )
            } else {
                "Preview unavailable".to_owned()
            })
            .size(12),
            container(preview)
                .width(320)
                .height(400)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(surface),
            row![
                text(format!(
                    "{} of {} selected",
                    app.print_preview_index + 1,
                    selected_count
                ))
                .size(12),
                Space::new().width(Fill),
                button("‹").on_press_maybe(
                    (app.print_preview_index > 0).then_some(Message::PrintPreviewStep(-1))
                ),
                button("›").on_press_maybe(
                    (app.print_preview_index + 1 < selected_count)
                        .then_some(Message::PrintPreviewStep(1))
                ),
            ]
            .spacing(5)
            .align_y(iced::Center),
        ]
        .spacing(8);
        let dialog = container(
            column![text("Print").size(20), row![left, right].spacing(20),]
                .spacing(12)
                .padding(16),
        )
        .width(820)
        .style(surface);
        iced::widget::stack![
            content,
            iced::widget::opaque(container(dialog).center(Fill).style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }))
        ]
        .into()
    } else if app.save_dialog {
        let dialog = container(
            column![
                text("Save PDF As").size(20),
                text("Creates a new PDF; existing files are not overwritten.").size(12),
                text_input("Destination PDF path", &app.save_path_input)
                    .on_input(Message::SavePathChanged)
                    .on_submit(Message::SaveAs),
                row![
                    button("Save As").on_press(Message::SaveAs),
                    button("Cancel").on_press(Message::CancelSave)
                ]
                .spacing(8)
            ]
            .spacing(16)
            .padding(24),
        )
        .width(600)
        .style(surface);
        iced::widget::stack![
            content,
            iced::widget::opaque(container(dialog).center(Fill).style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }))
        ]
        .into()
    } else if app.open_dialog {
        let indices = super::visible_browser_indices(&app.browser_entries, &app.browser_query);
        let visible = super::browser_window(app.browser_scroll_y, indices.len());
        let mut files = column![].spacing(0);
        if visible.start > 0 {
            files = files.push(Space::new().height(visible.start as f32 * super::BROWSER_ROW_STEP));
        }
        for position in visible.clone() {
            let entry = &app.browser_entries[indices[position]];
            let action = if entry.is_dir {
                Message::BrowseDirectory(entry.path.clone())
            } else {
                Message::SelectPdf(entry.path.clone())
            };
            files = files.push(
                container(
                    button(
                        row![
                            text(if entry.is_dir { "▸" } else { "PDF" }).size(11),
                            text(&entry.name).size(12),
                        ]
                        .spacing(10),
                    )
                    .on_press(action)
                    .width(Fill)
                    .height(34)
                    .padding([5, 9]),
                )
                .width(Fill)
                .height(super::BROWSER_ROW_STEP),
            );
        }
        if visible.end < indices.len() {
            files = files.push(
                Space::new().height((indices.len() - visible.end) as f32 * super::BROWSER_ROW_STEP),
            );
        }
        if app.browser_busy {
            files = files.push(text("Loading folder…").size(12));
        } else if indices.is_empty() && app.browser_error.is_empty() {
            files = files.push(text("No matching folders or PDF files").size(12));
        }
        let parent = app
            .browser_dir
            .parent()
            .map(|path| Message::BrowseDirectory(path.to_path_buf()));
        let dialog = container(
            column![
                text("Open PDF").size(20),
                text(app.browser_dir.to_string_lossy()).size(11),
                row![
                    button("↑ Parent").on_press_maybe(parent),
                    button("Refresh").on_press(Message::BrowseDirectory(app.browser_dir.clone()))
                ]
                .spacing(8),
                text_input("Search this folder", &app.browser_query)
                    .on_input(Message::BrowserSearchChanged),
                container(
                    scrollable(files)
                        .id(iced::widget::Id::new("pdf-file-browser"))
                        .on_scroll(Message::BrowserScrolled)
                        .height(320),
                )
                .height(320),
                text(&app.browser_error)
                    .size(11)
                    .color(Color::from_rgb8(240, 125, 110)),
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
        .width(620)
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
