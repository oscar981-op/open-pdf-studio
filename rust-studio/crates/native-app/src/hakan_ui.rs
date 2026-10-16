// SPDX-License-Identifier: GPL-3.0-only
// Adapted from Hakan Seven / OpenCADStudio, commit 2a15a370f6886d3e8695ab81c7d677e0cd484451.
// Source: src/ui/ribbon/widgets.rs and src/ui/ribbon/mod.rs.
// Changes: Iced 0.14 API, PDF command routing, reduced CAD-specific data dependencies.
use super::{
    annotation_color, annotation_layer, blank_pdf, image_export, inline_text, pdf_save,
    AnnotationTool, App, FitMode, Message, PrintDestination, PrintRange, PrintSubset, RibbonTab,
};
use iced::widget::{
    button, canvas, checkbox, column, container, image, mouse_area, pick_list, progress_bar, row,
    rule, scrollable, slider, svg, text, text_editor, text_input, tooltip, Column, Space,
};
use iced::{Background, Border, Color, Element, Fill, Padding, Theme};
use opds_dynlex_backend::Backend;
use opds_native_core::annotations::TextAlign;
use std::sync::OnceLock;
use std::time::Duration;

pub const ROW_H: f32 = 26.0;
const LARGE_W: f32 = ROW_H * 2.2;
const PDF_RIBBON_H: f32 = 94.0;

fn title_app_icon() -> image::Handle {
    static ICON: OnceLock<image::Handle> = OnceLock::new();
    ICON.get_or_init(|| {
        // The Tauri title bar scales public/icon.png to 18 px. Keep that
        // original source here instead of enlarging a pre-scaled 16 px tile.
        let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
            "../../../assets/ui/app-icon-png.bin"
        )));
        let mut reader = decoder.read_info().expect("bundled title icon header");
        let mut pixels = vec![0; reader.output_buffer_size()];
        let frame = reader
            .next_frame(&mut pixels)
            .expect("bundled title icon pixels");
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        pixels.truncate(frame.buffer_size());
        image::Handle::from_rgba(frame.width, frame.height, pixels)
    })
    .clone()
}

fn kernel_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Hayro => "Hayro",
        Backend::Dynlex => "DynLex",
        Backend::Pdfium => "PDFium",
        Backend::OwnRust => "Eigen Rust",
        Backend::PdfCraft => "PdfCraft (Hayro)",
    }
}

fn available_kernels() -> Vec<&'static str> {
    let mut kernels = vec!["Hayro"];
    if cfg!(feature = "dynlex-experimental") {
        kernels.push("DynLex");
    }
    if opds_dynlex_backend::pdfium_available() {
        kernels.push("PDFium");
    }
    if opds_dynlex_backend::own_rust_available() {
        kernels.push("Eigen Rust");
    }
    if opds_dynlex_backend::pdfcraft_available() {
        kernels.push("PdfCraft (Hayro)");
    }
    kernels
}

fn kernel_from_name(name: &str) -> Backend {
    match name {
        "DynLex" => Backend::Dynlex,
        "PDFium" => Backend::Pdfium,
        "Eigen Rust" => Backend::OwnRust,
        "PdfCraft (Hayro)" => Backend::PdfCraft,
        _ => Backend::Hayro,
    }
}

#[cfg(test)]
mod kernel_choice_tests {
    use super::*;

    #[test]
    fn visible_kernel_choices_round_trip_to_the_selected_backend() {
        for name in available_kernels() {
            assert_eq!(kernel_name(kernel_from_name(name)), name);
        }
        assert!(available_kernels().contains(&"Hayro"));
        if cfg!(feature = "dynlex-experimental")
            && opds_dynlex_backend::pdfium_available()
            && opds_dynlex_backend::own_rust_available()
            && opds_dynlex_backend::pdfcraft_available()
        {
            assert_eq!(
                available_kernels(),
                [
                    "Hayro",
                    "DynLex",
                    "PDFium",
                    "Eigen Rust",
                    "PdfCraft (Hayro)",
                ]
            );
        }
    }
}

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

fn thumbnail_btn_style(is_active: bool, status: button::Status) -> button::Style {
    let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if is_active {
            Some(Background::Color(Color::from_rgb8(68, 68, 76)))
        } else if hover {
            Some(Background::Color(Color::from_rgba8(217, 119, 6, 0.08)))
        } else {
            None
        },
        text_color: Color::from_rgb8(230, 230, 230),
        border: Border {
            radius: 4.0.into(),
            color: if is_active {
                Color::from_rgb8(217, 119, 6)
            } else {
                Color::TRANSPARENT
            },
            width: if is_active { 2.0 } else { 0.0 },
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
macro_rules! drawing_icon {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/ribbon/drawing/", $name, ".svg")) as &'static [u8]
    };
}
macro_rules! palette_icon {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/ui/tool-palette/", $name, ".svg"))
            as &'static [u8]
    };
}
const OPEN: &[u8] = icon_asset!("ui/folder_open.svg");
const SAVE: &[u8] = icon_asset!("ui/save.svg");
const PRINT: &[u8] = icon_asset!("ui/print.svg");
const CLOSE: &[u8] = icon_asset!("ui/close.svg");
const PREV: &[u8] = icon_asset!("ui/tri_left.svg");
const NEXT: &[u8] = icon_asset!("ui/tri_right.svg");
const FIRST_PAGE: &[u8] = pdf_icon!("firstPageIcon");
const PREVIOUS_PAGE: &[u8] = pdf_icon!("prevPageIcon");
const NEXT_PAGE: &[u8] = pdf_icon!("nextPageIcon");
const LAST_PAGE: &[u8] = pdf_icon!("lastPageIcon");
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

fn left_rail_tab(
    bytes: &'static [u8],
    title: &'static str,
    action: Option<Message>,
    active: bool,
) -> Element<'static, Message> {
    let accent = container(Space::new())
        .width(2)
        .height(32)
        .style(move |_| container::Style {
            background: active.then_some(Color::from_rgb8(217, 119, 6).into()),
            ..Default::default()
        });
    let tab = button(svg(svg::Handle::from_memory(bytes)).width(16).height(16))
        .on_press_maybe(action)
        .width(30)
        .height(32)
        .padding(7)
        .style(move |_, status| button::Style {
            background: if active {
                Some(Color::from_rgb8(68, 68, 76).into())
            } else if status == button::Status::Hovered {
                Some(Color::from_rgba8(217, 119, 6, 0.1).into())
            } else {
                None
            },
            text_color: Color::from_rgb8(175, 175, 176),
            ..Default::default()
        });
    tooltip(
        row![accent, tab].height(32),
        text(title).size(11),
        tooltip::Position::Right,
    )
    .into()
}

fn palette_tool(
    bytes: &'static [u8],
    title: &'static str,
    action: Option<Message>,
    active: bool,
) -> Element<'static, Message> {
    let available = action.is_some();
    let tool = button(svg(svg::Handle::from_memory(bytes)).width(22).height(22))
        .on_press_maybe(action)
        .width(36)
        .height(34)
        .padding(6)
        .style(move |_, status| button::Style {
            background: if active {
                Some(Color::from_rgba8(217, 119, 6, 0.20).into())
            } else if available && status == button::Status::Hovered {
                Some(Color::from_rgba8(217, 119, 6, 0.15).into())
            } else {
                None
            },
            border: Border {
                color: if active {
                    Color::from_rgba8(217, 119, 6, 0.40)
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        });
    tooltip(
        tool,
        text(if available {
            title
        } else {
            "Not yet available"
        })
        .size(11),
        tooltip::Position::Right,
    )
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

fn status_nav(
    bytes: &'static [u8],
    label: &'static str,
    action: Option<Message>,
) -> Element<'static, Message> {
    tooltip(
        button(icon_state(bytes, 12.0, action.is_some()))
            .width(20)
            .height(20)
            .padding(4)
            .on_press_maybe(action)
            .style(|_, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(Color::from_rgb8(76, 76, 84))),
                text_color: Color::from_rgb8(250, 250, 249),
                ..Default::default()
            }),
        text(label).size(11),
        tooltip::Position::Top,
    )
    .into()
}

fn title_quick_access(
    bytes: &'static [u8],
    label: &'static str,
    action: Option<Message>,
) -> Element<'static, Message> {
    let btn = button(container(icon_state(bytes, 18.0, action.is_some())).center(Fill))
        .on_press_maybe(action)
        .style(|theme, status| tool_btn_style(theme, false, status))
        .width(22)
        .height(22)
        .padding(0);
    tooltip(btn, text(label).size(11), tooltip::Position::Bottom)
        .gap(6)
        .delay(Duration::from_millis(400))
        .into()
}

fn titlebar_separator() -> Element<'static, Message> {
    container(Space::new())
        .width(1)
        .height(16)
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(88, 88, 94).into()),
            ..Default::default()
        })
        .into()
}

fn titlebar_background() -> Element<'static, Message> {
    // Chromium paints the 29 content rows of the Tauri CSS gradient at pixel
    // centers; the 30th row is its border over the theme's start color.
    let mut stripes = column![].spacing(0).width(Fill).height(30);
    for y in 0..29 {
        let shade = (68.0 - 14.0 * (y as f32 + 0.5) / 29.0).round() as u8;
        stripes = stripes.push(
            container(Space::new())
                .width(Fill)
                .height(1)
                .style(move |_| container::Style {
                    background: Some(Color::from_rgb8(shade, shade, shade + 8).into()),
                    ..Default::default()
                }),
        );
    }
    stripes =
        stripes.push(
            container(Space::new())
                .width(Fill)
                .height(1)
                .style(|_| container::Style {
                    background: Some(Color::from_rgb8(105, 81, 59).into()),
                    ..Default::default()
                }),
        );
    stripes.into()
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

fn drawing_small(
    title: &'static str,
    bytes: &'static [u8],
    action: Option<Message>,
    active: bool,
) -> Element<'static, Message> {
    let enabled = action.is_some();
    tooltip(
        button(icon_state(bytes, 19.0, enabled))
            .on_press_maybe(action)
            .width(25)
            .height(25)
            .padding(3)
            .style(move |_, status| pdf_tool_style(active, status)),
        text(if enabled {
            title.to_owned()
        } else {
            format!("{title} — not yet available")
        })
        .size(11),
        tooltip::Position::Bottom,
    )
    .into()
}

fn drawing_grid_group(
    title: &'static str,
    tools: Vec<Element<'static, Message>>,
    columns: usize,
    width: f32,
) -> Element<'static, Message> {
    let mut rows = Vec::new();
    let mut current = row![].spacing(1);
    for (index, tool) in tools.into_iter().enumerate() {
        if index > 0 && index % columns == 0 {
            rows.push(current.into());
            current = row![].spacing(1);
        }
        current = current.push(tool);
    }
    rows.push(current.into());
    row![
        container(
            column![
                container(Column::with_children(rows).spacing(0)).height(76),
                text(title.to_uppercase())
                    .size(9)
                    .color(Color::from_rgba8(217, 119, 6, 0.8))
            ]
            .align_x(iced::Center)
            .padding(Padding {
                top: 4.0,
                bottom: 0.0,
                left: 6.0,
                right: 6.0,
            })
        )
        .width(width)
        .height(PDF_RIBBON_H),
        rule::vertical(1).style(|_| rule::Style {
            color: Color::from_rgb8(70, 70, 77),
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
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

const MINIMIZE_WINDOW_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><rect y="4.5" width="10" height="1" fill="currentColor"/></svg>"#;
const MAXIMIZE_WINDOW_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1.2"/></svg>"#;
const RESTORE_WINDOW_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="2 2 12 12" fill="currentColor"><path d="M5.08496 4C5.29088 3.4174 5.8465 3 6.49961 3H9.99961C11.6565 3 12.9996 4.34315 12.9996 6V9.5C12.9996 10.1531 12.5822 10.7087 11.9996 10.9146V6C11.9996 4.89543 11.1042 4 9.99961 4H5.08496ZM4.5 5H9.5C10.3284 5 11 5.67157 11 6.5V11.5C11 12.3284 10.3284 13 9.5 13H4.5C3.67157 13 3 12.3284 3 11.5V6.5C3 5.67157 3.67157 5 4.5 5ZM4.5 6C4.22386 6 4 6.22386 4 6.5V11.5C4 11.7761 4.22386 12 4.5 12H9.5C9.77614 12 10 11.7761 10 11.5V6.5C10 6.22386 9.77614 6 9.5 6H4.5Z"/></svg>"#;
const CLOSE_WINDOW_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><path d="M0 0L10 10M10 0L0 10" fill="none" stroke="currentColor" stroke-width="1.2"/></svg>"#;
const RESIZE_GRIP_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d="M7 14l7-7M10 14l4-4M13 14l1-1" fill="none" stroke="#a4a4ad" stroke-width="1.2" stroke-linecap="round"/></svg>"##;

fn window_control(
    icon_bytes: &'static [u8],
    action: Message,
    close: bool,
    enabled: bool,
) -> Element<'static, Message> {
    button(container(icon_state(icon_bytes, 10.0, enabled)).center(Fill))
        .on_press_maybe(enabled.then_some(action))
        .width(40)
        .height(30)
        .padding(0)
        .style(move |_, status| button::Style {
            background: match (close, status) {
                (true, button::Status::Hovered | button::Status::Pressed) => {
                    Some(Color::from_rgb8(232, 17, 35).into())
                }
                (_, button::Status::Hovered | button::Status::Pressed) => {
                    Some(Color::from_rgba8(255, 255, 255, 0.1).into())
                }
                _ => None,
            },
            text_color: Color::from_rgb8(250, 250, 249),
            ..Default::default()
        })
        .into()
}

fn file_menu_item(
    label: &'static str,
    shortcut: &'static str,
    action: Option<Message>,
    selected: bool,
) -> Element<'static, Message> {
    let enabled = action.is_some();
    let glyph = match label {
        "New" => icon_asset!("ui/doc_new.svg"),
        "Open" => OPEN,
        "Save" | "Save As" => SAVE,
        "Print" => PRINT,
        "Import" => icon_asset!("ui/folder.svg"),
        "Export" => icon_asset!("ui/file_export.svg"),
        "Document Properties" => icon_asset!("properties.svg"),
        "Preferences" => icon_asset!("ui/gear.svg"),
        "About" | "What's New" => icon_asset!("ui/help.svg"),
        "Exit" => icon_asset!("ui/arrow_long_right.svg"),
        _ => DOC,
    };
    let body = button(
        row![
            container(icon_state(glyph, 20.0, enabled)).width(24),
            text(label).size(13).width(Fill),
            text(shortcut)
                .size(11)
                .color(Color::from_rgba8(250, 250, 249, 0.5)),
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
    .on_press_maybe(action)
    .width(Fill)
    .height(44)
    .padding([10, 17])
    .style(move |_, status| button::Style {
        background: (selected
            || matches!(status, button::Status::Hovered | button::Status::Pressed))
        .then(|| Color::from_rgba8(217, 119, 6, 0.1).into()),
        text_color: if enabled {
            Color::from_rgb8(250, 250, 249)
        } else {
            Color::from_rgb8(155, 155, 158)
        },
        border: Border {
            radius: 0.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    row![
        container(Space::new())
            .width(3)
            .height(44)
            .style(move |_| container::Style {
                background: selected.then(|| Color::from_rgb8(59, 130, 246).into()),
                ..Default::default()
            }),
        body,
    ]
    .width(Fill)
    .into()
}

fn file_menu_divider() -> Element<'static, Message> {
    container(
        container(Space::new())
            .height(1)
            .width(Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba8(217, 119, 6, 0.15).into()),
                ..Default::default()
            }),
    )
    .padding([4, 16])
    .width(Fill)
    .into()
}

fn recent_file_row(entry: &super::RecentFile) -> Element<'static, Message> {
    let path = entry.path.clone();
    let pinned = entry.pinned;
    let title = std::path::Path::new(&path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let display_path = if path.chars().count() > 62 {
        format!("{}…", path.chars().take(61).collect::<String>())
    } else {
        path.clone()
    };
    let file = button(
        row![
            icon_state(DOC, 20.0, true),
            column![
                text(title).size(13),
                text(display_path)
                    .size(11)
                    .color(Color::from_rgba8(250, 250, 249, 0.5)),
            ]
            .spacing(1),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(Message::SelectPdf(std::path::PathBuf::from(&path)))
    .width(Fill)
    .padding([6, 8])
    .style(|_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| Color::from_rgba8(217, 119, 6, 0.1).into()),
        text_color: Color::from_rgb8(250, 250, 249),
        border: Border {
            radius: 0.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let pin = tooltip(
        button(icon_state(icon_asset!("ui/pin.svg"), 14.0, true))
            .on_press(Message::ToggleRecentPinned(path.clone()))
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then(|| Color::from_rgba8(217, 119, 6, 0.1).into()),
                text_color: if pinned {
                    Color::from_rgb8(217, 119, 6)
                } else {
                    Color::from_rgb8(250, 250, 249)
                },
                ..Default::default()
            }),
        if pinned { "Unpin" } else { "Pin" },
        tooltip::Position::Bottom,
    );
    let remove = tooltip(
        button(icon_state(CLOSE, 14.0, true))
            .on_press(Message::RemoveRecent(path))
            .style(|_, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then(|| Color::from_rgba8(217, 119, 6, 0.1).into()),
                text_color: Color::from_rgb8(250, 250, 249),
                ..Default::default()
            }),
        "Remove from recent files",
        tooltip::Position::Bottom,
    );
    row![file, pin, remove]
        .spacing(2)
        .align_y(iced::Center)
        .into()
}

fn file_open_nav_item(
    label: &'static str,
    glyph: &'static [u8],
    action: Option<Message>,
    selected: bool,
) -> Element<'static, Message> {
    let enabled = action.is_some();
    let glyph = svg(svg::Handle::from_memory(glyph))
        .width(20)
        .height(20)
        .style(move |_, _| svg::Style {
            color: Some(if selected {
                Color::BLACK
            } else if enabled {
                Color::from_rgb8(220, 220, 222)
            } else {
                Color::from_rgb8(150, 150, 156)
            }),
        });
    button(
        row![container(glyph).width(24), text(label).size(13)]
            .spacing(10)
            .align_y(iced::Center),
    )
    .on_press_maybe(action)
    .width(Fill)
    .height(44)
    .padding([10, 16])
    .style(move |_, status| button::Style {
        background: if selected {
            Some(Color::from_rgb8(205, 230, 252).into())
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(Color::from_rgb8(86, 86, 94).into())
        } else {
            None
        },
        text_color: if selected {
            Color::BLACK
        } else if enabled {
            Color::from_rgb8(250, 250, 249)
        } else {
            Color::from_rgb8(155, 155, 158)
        },
        border: Border {
            radius: 0.0.into(),
            ..Default::default()
        },
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
    handing: bool,
) -> Vec<Element<'static, Message>> {
    vec![
        group(
            "Document",
            vec![
                large_tool_sized(
                    "New",
                    pdf_icon!("newDocIcon"),
                    Some(Message::ShowNew),
                    false,
                    44.0,
                ),
                large_tool_sized("IFC-report", pdf_icon!("ifcExportIcon"), None, false, 52.0),
                large_tool_sized("To CAD", pdf_icon!("cadExportIcon"), None, false, 48.0),
                large_tool_sized("E-mail", pdf_icon!("emailIcon"), None, false, 45.0),
                large_tool_sized("Raster-PDF", pdf_icon!("rasterPdfIcon"), None, false, 57.0),
            ],
        ),
        group(
            "Tools",
            vec![
                large_tool_sized(
                    "Hand",
                    pdf_icon!("handIcon"),
                    has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Hand)),
                    handing,
                    44.0,
                ),
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
                    labeled_tool(
                        "Fit Width",
                        pdf_icon!("fitWidthIcon"),
                        has_doc.then_some(Message::FitView(FitMode::Width)),
                        84.0,
                        22.0,
                    ),
                ]),
                stack_tools(vec![
                    labeled_tool(
                        "100%",
                        pdf_icon!("actualSizeIcon"),
                        has_doc.then_some(Message::Zoom(1.0)),
                        78.0,
                        22.0,
                    ),
                    labeled_tool(
                        "Fit Page",
                        pdf_icon!("fitPageIcon"),
                        has_doc.then_some(Message::FitView(FitMode::Page)),
                        78.0,
                        22.0,
                    ),
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
    page: usize,
    can_clear_page: bool,
    can_clear_all: bool,
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
                    can_clear_page.then_some(Message::RequestClearAnnotations(Some(page))),
                    87.0,
                    31.0,
                ),
                labeled_tool(
                    "Clear All",
                    annotation_icon!("clearAllIcon"),
                    can_clear_all.then_some(Message::RequestClearAnnotations(None)),
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

// Keep long names on one row without splitting a UTF-8 character.
fn document_tab_label(name: &str) -> String {
    if name.chars().count() <= 29 {
        name.to_owned()
    } else {
        name.chars().take(28).chain(std::iter::once('…')).collect()
    }
}

fn panel_surface(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb8(68, 68, 76).into()),
        text_color: Some(Color::from_rgb8(250, 250, 249)),
        ..Default::default()
    }
}

fn panel_header(label: &'static str, close: Message) -> Element<'static, Message> {
    panel_header_height(label, close, 28)
}

fn panel_header_height(
    label: &'static str,
    close: Message,
    height: u32,
) -> Element<'static, Message> {
    container(
        row![
            text(label).size(12).width(Fill),
            button(icon(CLOSE, 10.0))
                .width(18)
                .height(18)
                .padding(2)
                .on_press(close)
                .style(|_, status| pdf_tool_style(false, status)),
        ]
        .align_y(iced::Center),
    )
    .height(height)
    .width(Fill)
    .padding([0, 6])
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(62, 62, 70).into()),
        text_color: Some(Color::from_rgb8(250, 250, 249)),
        ..Default::default()
    })
    .into()
}

fn properties_header() -> Element<'static, Message> {
    const COLLAPSE_ICON: &[u8] = br##"<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#AAAAB4" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/><path d="m8 9 3 3-3 3"/></svg>"##;
    container(
        row![
            button(
                svg(svg::Handle::from_memory(COLLAPSE_ICON))
                    .width(14)
                    .height(14)
            )
            .on_press(Message::ToggleProperties)
            .width(28)
            .height(28)
            .padding(7)
            .style(|_, status| button::Style {
                background: (status == button::Status::Hovered)
                    .then_some(Color::from_rgb8(68, 68, 76).into()),
                text_color: Color::from_rgb8(170, 170, 180),
                ..Default::default()
            }),
            text("Properties").size(13).width(Fill),
        ]
        .align_y(iced::Center),
    )
    .height(36)
    .width(Fill)
    .style(|_| container::Style {
        background: Some(
            iced::gradient::Linear::new(std::f32::consts::PI)
                .add_stop(0.0, Color::from_rgb8(54, 54, 62))
                .add_stop(1.0, Color::from_rgb8(68, 68, 76))
                .into(),
        ),
        text_color: Some(Color::from_rgb8(250, 250, 249)),
        ..Default::default()
    })
    .into()
}

struct VerticalPropertiesLabel;

impl canvas::Program<Message> for VerticalPropertiesLabel {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry<iced::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.translate(iced::Vector::new(18.0, 4.0));
        frame.rotate(std::f32::consts::FRAC_PI_2);
        frame.fill_text(canvas::Text {
            content: "Properties".into(),
            position: iced::Point::ORIGIN,
            color: Color::from_rgb8(170, 170, 180),
            size: iced::Pixels(11.0),
            font: iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::DEFAULT
            },
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

fn property_row(label: &'static str, value: String) -> Element<'static, Message> {
    container(
        row![
            text(label)
                .size(11)
                .color(Color::from_rgb8(170, 170, 180))
                .width(70)
                .align_x(iced::Right),
            text(value).size(11).width(Fill),
        ]
        .spacing(4),
    )
    .padding([3, 8])
    .width(Fill)
    .into()
}

fn property_section(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(11))
        .height(24)
        .padding([4, 8])
        .width(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(54, 54, 62).into()),
            text_color: Some(Color::from_rgb8(250, 250, 249)),
            ..Default::default()
        })
        .into()
}

fn property_section_toggle(
    label: &'static str,
    index: usize,
    collapsed: bool,
) -> Element<'static, Message> {
    button(
        row![
            text(label).size(11).width(Fill),
            text(if collapsed { "▶" } else { "▼" })
                .size(8)
                .color(Color::from_rgb8(170, 170, 180)),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::TogglePropertySection(index))
    .width(Fill)
    .height(24)
    .padding([4, 8])
    .style(|_, status| button::Style {
        background: Some(
            Color::from_rgb8(
                if status == button::Status::Hovered {
                    65
                } else {
                    54
                },
                if status == button::Status::Hovered {
                    65
                } else {
                    54
                },
                if status == button::Status::Hovered {
                    73
                } else {
                    62
                },
            )
            .into(),
        ),
        text_color: Color::from_rgb8(250, 250, 249),
        border: Border {
            radius: 0.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let has_doc = app.document.is_some();
    let (page, count, scale) = app.document.as_ref().map_or((0, 0, 1.0), |d| {
        let v = d.model.view();
        (v.page(), v.page_count(), v.viewport().scale())
    });
    let mut top = row![button(text("File").size(12))
        .on_press(Message::ToggleFileMenu)
        .style(|_, status| button::Style {
            background: Some(
                Color::from_rgb8(
                    if app.file_menu_open || status == button::Status::Hovered {
                        234
                    } else {
                        217
                    },
                    if app.file_menu_open || status == button::Status::Hovered {
                        88
                    } else {
                        119
                    },
                    if app.file_menu_open || status == button::Status::Hovered {
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
    let selected_annotation = app.document.as_ref().and_then(|document| {
        document
            .selected_annotation
            .and_then(|id| document.model.get(id))
    });
    let selected_text_style = selected_annotation.and_then(|item| match &item.kind {
        opds_native_core::annotations::Kind::Text {
            color_rgb,
            font_size,
            align,
            ..
        } => Some((*color_rgb, *font_size, *align)),
        _ => None,
    });
    if app.text_dialog || selected_annotation.is_some() {
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
            background: Some(
                iced::gradient::Linear::new(std::f32::consts::PI)
                    .add_stop(0.0, Color::from_rgb8(68, 68, 76))
                    .add_stop(1.0, Color::from_rgb8(62, 62, 70))
                    .into(),
            ),
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
            app.annotation_tool_activated && app.annotation_tool == AnnotationTool::Hand,
        ) {
            groups = groups.push(item);
        }
    } else if app.active_ribbon == RibbonTab::Comment {
        for item in ribbon_annotation(
            has_doc,
            app.annotation_tool_activated && app.annotation_tool == AnnotationTool::Highlight,
            app.annotation_tool_activated && app.annotation_tool == AnnotationTool::Text,
            page,
            app.document
                .as_ref()
                .is_some_and(|document| document.model.items().any(|item| item.page == page)),
            app.document
                .as_ref()
                .is_some_and(|document| document.model.items().next().is_some()),
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
                    let active = (if app.text_dialog {
                        app.text_color
                    } else {
                        selected_annotation
                            .map(|item| annotation_color(&item.kind))
                            .unwrap_or(app.text_color)
                    }) == color
                        && enabled;
                    let preview = container(
                        text(if app.text_dialog || selected_text_style.is_some() {
                            "T"
                        } else {
                            "■"
                        })
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
                    .on_press_maybe(enabled.then_some(Message::SetFormatColor(color)))
                    .style(move |_, status| pdf_tool_style(active, status))
                    .width(50)
                    .height(62)
                    .padding([2, 2])
                    .into()
                })
                .collect(),
        ));
        if app.text_dialog || selected_text_style.is_some() {
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
        }
    } else if !matches!(app.active_ribbon, RibbonTab::View | RibbonTab::Drawing) {
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
        let point_transformable = app.document.as_ref().is_some_and(|document| {
            document.selected_annotation.is_some_and(|id| {
                document.model.get(id).is_some_and(|item| {
                    item.page == document.displayed_page
                        && matches!(
                            item.kind,
                            opds_native_core::annotations::Kind::Line { .. }
                                | opds_native_core::annotations::Kind::Arrow { .. }
                                | opds_native_core::annotations::Kind::Polyline { .. }
                        )
                })
            })
        });
        macro_rules! draw {
            ($title:literal, $icon:literal) => {
                drawing_small($title, drawing_icon!($icon), None, false)
            };
            ($title:literal, $icon:literal, $action:expr, $active:expr) => {
                drawing_small($title, drawing_icon!($icon), $action, $active)
            };
        }
        groups = groups
            .push(group(
                "Selection",
                vec![
                    large_tool_sized(
                        "Select",
                        drawing_icon!("selectCommentsIcon"),
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Select)),
                        app.annotation_tool == AnnotationTool::Select,
                        46.0,
                    ),
                    large_tool_sized(
                        "Hand",
                        drawing_icon!("handIcon"),
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Hand)),
                        app.annotation_tool == AnnotationTool::Hand,
                        46.0,
                    ),
                    stack_tools(vec![
                        labeled_tool(
                            "Select All",
                            drawing_icon!("selectCommentsIcon"),
                            None,
                            96.0,
                            22.0,
                        ),
                        labeled_tool(
                            "Deselect",
                            drawing_icon!("selectCommentsIcon"),
                            has_doc.then_some(Message::SelectAnnotation(None)),
                            96.0,
                            22.0,
                        ),
                        labeled_tool(
                            "Find / Replace",
                            drawing_icon!("findIcon"),
                            None,
                            96.0,
                            22.0,
                        ),
                    ]),
                ],
            ))
            .push(drawing_grid_group(
                "Draw",
                vec![
                    draw!(
                        "Line",
                        "lineIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Line)),
                        app.annotation_tool == AnnotationTool::Line
                    ),
                    draw!(
                        "Arrow",
                        "arrowIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Arrow)),
                        app.annotation_tool == AnnotationTool::Arrow
                    ),
                    draw!("Spline Arrow", "splineArrowIcon"),
                    draw!(
                        "Freehand",
                        "drawIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Freehand)),
                        app.annotation_tool == AnnotationTool::Freehand
                    ),
                    draw!("Eraser", "eraserIcon"),
                    draw!(
                        "Rectangle",
                        "rectIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Rectangle)),
                        app.annotation_tool == AnnotationTool::Rectangle
                    ),
                    draw!("Arc", "arcIcon"),
                    draw!(
                        "Polyline",
                        "polylineIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Polyline)),
                        app.annotation_tool == AnnotationTool::Polyline
                    ),
                    draw!("Filled Area", "hatchIcon"),
                    draw!(
                        "Text Box",
                        "textboxIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Text)),
                        app.annotation_tool == AnnotationTool::Text
                    ),
                    draw!("Note", "noteIcon"),
                    draw!("Spline", "splineIcon"),
                    draw!(
                        "Circle",
                        "circleIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Circle)),
                        app.annotation_tool == AnnotationTool::Circle
                    ),
                    draw!(
                        "Ellipse",
                        "ellipseIcon",
                        has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Ellipse)),
                        app.annotation_tool == AnnotationTool::Ellipse
                    ),
                    draw!("Count", "circleIcon"),
                    draw!("L-Shape", "lShapeIcon"),
                    draw!("Image", "imageIcon"),
                    draw!("Remove Image", "removeImageIcon"),
                ],
                7,
                192.0,
            ))
            .push(group(
                "Snippet",
                vec![
                    large_tool_sized(
                        "Vector Snippet",
                        drawing_icon!("knipselIcon"),
                        None,
                        false,
                        66.0,
                    ),
                    large_tool_sized(
                        "Paste Snippet",
                        drawing_icon!("knipselPlakIcon"),
                        None,
                        false,
                        66.0,
                    ),
                ],
            ))
            .push(group(
                "Scale",
                vec![
                    large_tool_sized(
                        "Scale Region",
                        drawing_icon!("scaleRegionIcon"),
                        None,
                        false,
                        66.0,
                    ),
                    large_tool_sized(
                        "Full-page Scale",
                        drawing_icon!("scaleRegionIcon"),
                        None,
                        false,
                        66.0,
                    ),
                ],
            ))
            .push(group(
                "Annotate",
                vec![
                    stack_tools(vec![
                        labeled_tool("Area", drawing_icon!("measureAreaIcon"), None, 80.0, 22.0),
                        labeled_tool(
                            "Length",
                            drawing_icon!("measureDistanceIcon"),
                            None,
                            80.0,
                            22.0,
                        ),
                    ]),
                    stack_tools(vec![
                        labeled_tool(
                            "Dimension",
                            drawing_icon!("measureDistanceIcon"),
                            None,
                            80.0,
                            22.0,
                        ),
                        labeled_tool(
                            "Angular",
                            drawing_icon!("measureAngleIcon"),
                            None,
                            80.0,
                            22.0,
                        ),
                    ]),
                    stack_tools(vec![
                        labeled_tool(
                            "Spot Coord.",
                            drawing_icon!("placeholderIcon"),
                            None,
                            80.0,
                            22.0,
                        ),
                        labeled_tool("Radius", drawing_icon!("radiusIcon"), None, 80.0, 22.0),
                        labeled_tool("Diameter", drawing_icon!("diameterIcon"), None, 80.0, 22.0),
                    ]),
                    stack_tools(vec![
                        labeled_tool("Leader", drawing_icon!("calloutIcon"), None, 80.0, 22.0),
                        labeled_tool(
                            "Label",
                            drawing_icon!("labelIcon"),
                            has_doc.then_some(Message::SelectAnnotationTool(AnnotationTool::Text)),
                            80.0,
                            22.0,
                        ),
                        labeled_tool("Table", drawing_icon!("tableIcon"), None, 80.0, 22.0),
                    ]),
                    stack_tools(vec![
                        labeled_tool("Cloud", drawing_icon!("cloudIcon"), None, 80.0, 22.0),
                        labeled_tool(
                            "Measure",
                            drawing_icon!("measurePerimeterIcon"),
                            None,
                            80.0,
                            22.0,
                        ),
                    ]),
                ],
            ))
            .push(drawing_grid_group(
                "Modify",
                vec![
                    draw!("Move", "moveIcon"),
                    draw!(
                        "Copy",
                        "copyAnnIcon",
                        (has_doc
                            && app.document.as_ref().is_some_and(|document| {
                                document.selected_annotation.is_some_and(|id| {
                                    document
                                        .model
                                        .get(id)
                                        .is_some_and(|item| item.page == document.displayed_page)
                                })
                            }))
                        .then_some(Message::DuplicateSelectedAnnotation),
                        false
                    ),
                    draw!(
                        "Mirror Horizontally",
                        "flipHIcon",
                        point_transformable
                            .then_some(Message::MirrorSelectedAnnotation { horizontal: true }),
                        false
                    ),
                    draw!(
                        "Mirror Vertically",
                        "flipVIcon",
                        point_transformable
                            .then_some(Message::MirrorSelectedAnnotation { horizontal: false }),
                        false
                    ),
                    draw!("Array", "arrayIcon"),
                    draw!(
                        "Rotate",
                        "rotateCwIcon",
                        point_transformable.then_some(Message::RotateSelectedAnnotation),
                        false
                    ),
                ],
                3,
                89.0,
            ));
        // Tauri only shows Edit while an annotation is selected.
        if app
            .document
            .as_ref()
            .and_then(|document| document.selected_annotation)
            .is_some()
        {
            groups = groups.push(drawing_grid_group(
                "Edit",
                vec![
                    draw!("Trim", "trimIcon"),
                    draw!("Extend", "extendIcon"),
                    draw!("Offset", "placeholderIcon"),
                    draw!("Fillet", "placeholderIcon"),
                    draw!("Chamfer", "placeholderIcon"),
                    draw!("Stretch", "placeholderIcon"),
                    draw!("Split", "splitIcon"),
                    draw!("Align", "alignLeftIcon"),
                    draw!("Explode", "explodeIcon"),
                    draw!("Break", "breakIcon"),
                    draw!("Join", "joinIcon"),
                    draw!("Lengthen", "lengthenIcon"),
                ],
                4,
                130.0,
            ));
        }
        groups = groups
            .push(group(
                "Clipboard",
                vec![
                    large_tool_sized(
                        "Paste",
                        drawing_icon!("pasteIcon"),
                        (app.document.is_some() && app.clipboard_annotation.is_some())
                            .then_some(Message::PasteAnnotation { in_place: false }),
                        false,
                        52.0,
                    ),
                    large_tool_sized(
                        "Paste in Place",
                        drawing_icon!("pasteInPlaceIcon"),
                        (app.document.is_some() && app.clipboard_annotation.is_some())
                            .then_some(Message::PasteAnnotation { in_place: true }),
                        false,
                        60.0,
                    ),
                    stack_tools(vec![
                        draw!(
                            "Cut",
                            "cutIcon",
                            app.document
                                .as_ref()
                                .and_then(|d| d.selected_annotation)
                                .map(|_| Message::CutSelectedAnnotation),
                            false
                        ),
                        draw!(
                            "Copy",
                            "copyAnnIcon",
                            app.document
                                .as_ref()
                                .and_then(|d| d.selected_annotation)
                                .map(|_| Message::CopySelectedAnnotation),
                            false
                        ),
                        draw!(
                            "Delete",
                            "deleteIcon",
                            app.document
                                .as_ref()
                                .and_then(|d| d.selected_annotation)
                                .map(|_| Message::RequestDeleteAnnotation),
                            false
                        ),
                    ]),
                ],
            ))
            .push(group(
                "Collection",
                vec![stack_tools(vec![
                    labeled_tool(
                        "Create",
                        drawing_icon!("collectionCreateIcon"),
                        None,
                        86.0,
                        22.0,
                    ),
                    labeled_tool(
                        "Explode",
                        drawing_icon!("collectionExplodeIcon"),
                        None,
                        86.0,
                        22.0,
                    ),
                ])],
            ));
    }
    if app.active_ribbon == RibbonTab::Help {
        groups = groups.push(group(
            "Help",
            vec![large_tool(
                "About",
                icon_asset!("ui/help.svg"),
                Some(Message::ShowAbout),
            )],
        ));
        groups = groups.push(group(
            "PDF Engine",
            vec![
                large_tool_active(
                    "Hayro",
                    DOC,
                    (!app.opening_document)
                        .then_some(Message::SelectBackend(opds_dynlex_backend::Backend::Hayro)),
                    app.backend == opds_dynlex_backend::Backend::Hayro,
                ),
                large_tool_active(
                    "DynLex",
                    icon_asset!("zoom_ext.svg"),
                    (!app.opening_document && cfg!(feature = "dynlex-experimental"))
                        .then_some(Message::SelectBackend(opds_dynlex_backend::Backend::Dynlex)),
                    app.backend == opds_dynlex_backend::Backend::Dynlex,
                ),
                large_tool_active(
                    "PDFium",
                    DOC,
                    (!app.opening_document && opds_dynlex_backend::pdfium_available())
                        .then_some(Message::SelectBackend(opds_dynlex_backend::Backend::Pdfium)),
                    app.backend == opds_dynlex_backend::Backend::Pdfium,
                ),
                large_tool_active(
                    "Eigen Rust",
                    DOC,
                    (!app.opening_document && opds_dynlex_backend::own_rust_available()).then_some(
                        Message::SelectBackend(opds_dynlex_backend::Backend::OwnRust),
                    ),
                    app.backend == opds_dynlex_backend::Backend::OwnRust,
                ),
                large_tool_active(
                    "PdfCraft (Hayro)",
                    DOC,
                    (!app.opening_document && opds_dynlex_backend::pdfcraft_available()).then_some(
                        Message::SelectBackend(opds_dynlex_backend::Backend::PdfCraft),
                    ),
                    app.backend == opds_dynlex_backend::Backend::PdfCraft,
                ),
            ],
        ));
    }
    if app.active_ribbon == RibbonTab::View {
        groups = groups
            .push(group(
                "Page Display",
                vec![
                    large_tool_active("Single", DOC, None, has_doc),
                    large_tool("Continuous", DOC, None),
                    large_tool("Book", DOC, None),
                    large_tool("Facing", DOC, None),
                    large_tool("Rotate Left", PREV, None),
                    large_tool("Rotate Right", NEXT, None),
                ],
            ))
            .push(group(
                "Display",
                vec![
                    large_tool("Thin Lines", DOC, None),
                    large_tool_active(
                        "Reader Mode",
                        DOC,
                        (has_doc && !app.opening_document).then_some(Message::ToggleReaderMode),
                        app.reader_mode_active,
                    ),
                ],
            ))
            .push(group(
                "Panels",
                vec![
                    large_tool_active(
                        "Navigation",
                        DOC,
                        has_doc.then_some(Message::TogglePages),
                        !app.hide_pages,
                    ),
                    large_tool_active(
                        "Properties",
                        icon_asset!("properties.svg"),
                        has_doc.then_some(Message::ToggleProperties),
                        !app.hide_properties && !app.show_annotation_list,
                    ),
                    large_tool_active(
                        "Annotations",
                        DOC,
                        has_doc.then_some(Message::ToggleAnnotationList),
                        app.show_annotation_list,
                    ),
                    large_tool("Element Visibility", DOC, None),
                    large_tool("Annotation Layers", DOC, None),
                    large_tool_active(
                        "Toolpalette",
                        DOC,
                        Some(Message::ToggleSymbolPalette),
                        !app.hide_symbol_palette,
                    ),
                ],
            ))
            .push(group("Appearance", vec![large_tool("Theme", DOC, None)]))
            .push(group(
                "Compare",
                vec![large_tool("Compare PDFs", DOC, None)],
            ))
            .push(group(
                "Window",
                vec![
                    large_tool_active(
                        "Fullscreen",
                        icon_asset!("zoom_ext.svg"),
                        Some(Message::ToggleFullscreen),
                        app.fullscreen,
                    ),
                    large_tool_active(
                        "Keystrokes",
                        DOC,
                        Some(Message::ToggleKeystrokes),
                        app.keystrokes.visible,
                    ),
                ],
            ));
    }
    if !matches!(
        app.active_ribbon,
        RibbonTab::Home
            | RibbonTab::Comment
            | RibbonTab::Format
            | RibbonTab::View
            | RibbonTab::Drawing
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
    let ribbon_scrollbar = if app.active_ribbon == RibbonTab::Drawing {
        scrollable::Scrollbar::new().width(6).scroller_width(6)
    } else {
        scrollable::Scrollbar::new().width(0).scroller_width(0)
    };
    let ribbon = container(
        scrollable(groups).direction(scrollable::Direction::Horizontal(
            ribbon_scrollbar.margin(0).spacing(0),
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
    let window_controls_enabled = !(app.new_dialog
        || app.confirm_discard.is_some()
        || app.pending_delete_annotation.is_some()
        || app.pending_clear_annotations.is_some()
        || app.extension_import_dialog
        || app.extensions_dialog
        || app.about_dialog
        || app.print_dialog
        || app.save_dialog
        || app.export_dialog
        || app.import_dialog
        || app.xfdf_export_dialog
        || app.open_dialog
        || app.text_dialog);
    let app_icon = image(title_app_icon()).width(18).height(18);
    let title_tools = row![
        app_icon,
        titlebar_separator(),
        title_quick_access(
            OPEN,
            "Open",
            window_controls_enabled.then_some(Message::ShowOpen)
        ),
        title_quick_access(
            SAVE,
            "Save",
            (has_doc && window_controls_enabled).then_some(Message::Save),
        ),
        title_quick_access(
            SAVE,
            "Save As",
            (has_doc && window_controls_enabled).then_some(Message::ShowSave)
        ),
        title_quick_access(
            PRINT,
            "Print",
            (has_doc && window_controls_enabled).then_some(Message::ShowPrint)
        ),
        title_quick_access(DOC, "Raster PDF", None),
        title_quick_access(pdf_icon!("preferencesIcon"), "Preferences", None),
        titlebar_separator(),
        title_quick_access(
            icon_asset!("ui/undo.svg"),
            "Undo",
            app.document
                .as_ref()
                .filter(|d| window_controls_enabled && d.model.can_undo())
                .map(|_| Message::Undo)
        ),
        title_quick_access(
            icon_asset!("ui/redo.svg"),
            "Redo",
            app.document
                .as_ref()
                .filter(|d| window_controls_enabled && d.model.can_redo())
                .map(|_| Message::Redo)
        ),
        titlebar_separator(),
        title_quick_access(PREV, "Previous view", None),
        title_quick_access(NEXT, "Next view", None),
    ]
    .spacing(2)
    .align_y(iced::Center);
    let drag_region = mouse_area(
        container(
            row![
                text("Open PDF Studio v2026.39.0")
                    .size(12)
                    .color(Color::from_rgb8(250, 250, 249)),
                text(if has_doc { name.clone() } else { String::new() })
                    .size(11)
                    .color(Color::from_rgb8(175, 175, 182)),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .center(Fill)
        .height(30),
    );
    let drag_region = if window_controls_enabled {
        drag_region
            .on_press(Message::DragWindow)
            .on_double_click(Message::MaximizeWindow)
    } else {
        drag_region
    };
    let titlebar_controls = container(
        row![
            container(title_tools)
                .width(310)
                .height(30)
                .padding([0, 8])
                .align_y(iced::Center),
            drag_region,
            container(
                row![
                    Space::new().width(Fill),
                    text("Send Feedback")
                        .size(11)
                        .color(Color::from_rgb8(174, 174, 180)),
                    Space::new().width(10),
                    window_control(
                        MINIMIZE_WINDOW_ICON,
                        Message::MinimizeWindow,
                        false,
                        window_controls_enabled
                    ),
                    window_control(
                        if app.window_maximized {
                            RESTORE_WINDOW_ICON
                        } else {
                            MAXIMIZE_WINDOW_ICON
                        },
                        Message::MaximizeWindow,
                        false,
                        window_controls_enabled,
                    ),
                    window_control(
                        CLOSE_WINDOW_ICON,
                        Message::CloseWindow,
                        true,
                        window_controls_enabled
                    ),
                ]
                .align_y(iced::Center),
            )
            .width(310)
            .height(30),
        ]
        .height(30)
        .align_y(iced::Center),
    )
    .width(Fill)
    .height(30);
    let titlebar = iced::widget::stack![titlebar_background(), titlebar_controls]
        .width(Fill)
        .height(30);
    // Match document-tabs.css: bounded single-line tab and a 2px accent.
    // The full filename stays available on hover; the dirty dot is real state.
    let dirty = app.document.as_ref().is_some_and(|d| d.is_dirty());
    let tab: Element<'_, Message> = if has_doc {
        let mut content = row![].spacing(4).align_y(iced::Center);
        if dirty {
            content = content.push(text("●").size(10).color(Color::from_rgb8(217, 119, 6)));
        }
        content = content
            .push(
                text(document_tab_label(&name))
                    .size(12)
                    .width(Fill)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .push(
                button(icon(CLOSE, 12.0))
                    .on_press(Message::Close)
                    .width(16)
                    .height(16)
                    .padding(0)
                    .style(|_, status| pdf_tool_style(false, status)),
            );
        tooltip(
            container(column![
                container(content)
                    .height(26)
                    .padding([0, 5])
                    .align_y(iced::Center),
                rule::horizontal(2).style(|_| rule::Style {
                    color: Color::from_rgb8(217, 119, 6),
                    radius: 0.0.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: true,
                }),
            ])
            .width(240)
            .height(28)
            .style(|_| container::Style {
                background: Some(Color::from_rgb8(54, 54, 62).into()),
                text_color: Some(Color::from_rgb8(250, 250, 249)),
                ..Default::default()
            }),
            text(name.clone()).size(12),
            tooltip::Position::Bottom,
        )
        .delay(Duration::from_millis(400))
        .into()
    } else {
        container(
            text("No documents open")
                .size(12)
                .color(Color::from_rgb8(153, 153, 157)),
        )
        .height(28)
        .padding([0, 10])
        .align_y(iced::Center)
        .into()
    };
    let tabs = container(
        row![
            tab,
            quick_access(icon_asset!("ui/plus.svg"), "Open", Some(Message::ShowOpen))
        ]
        .align_y(iced::Center),
    )
    .height(28)
    .width(Fill)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(46, 46, 54).into()),
        ..Default::default()
    });
    let center: Element<'_, Message> =
        if let Some(document) = app.document.as_ref().filter(|d| d.bitmap.is_some()) {
            let annotation_bitmap = app.dynlex_visible_annotation_bitmap(document);
            let dynlex_annotations = annotation_bitmap.is_some();
            let handle = annotation_bitmap
                .or_else(|| document.bitmap.clone())
                .expect("filtered above");
            // Stretch the last verified bitmap immediately while a new PDF
            // raster is pending. The final render replaces it at full detail.
            let (width, height) = if document.displayed_page == document.model.view().page() {
                super::zoom_preview_size(
                    document.bitmap_page_points,
                    document.bitmap_size,
                    document.model.view().viewport().scale(),
                )
            } else {
                document.bitmap_size
            };
            let mut page: Element<'_, Message> = iced::widget::stack![
                image(handle).width(width as f32).height(height as f32),
                annotation_layer::view(
                    &document.model,
                    document.displayed_page,
                    document.selected_annotation,
                    app.editing_text_id,
                    app.annotation_tool,
                    app.annotation_tool_epoch,
                    document.bitmap_page_points,
                    (width, height),
                    !dynlex_annotations
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
            let page_with_pan_room = column![
                Space::new().height(super::VERTICAL_PAN_MARGIN),
                page_with_pan_room,
                Space::new().height(super::VERTICAL_PAN_MARGIN),
            ];
            super::zoom_area::view(
                scrollable(container(page_with_pan_room).padding(super::PAGE_PADDING))
                    .id(iced::widget::Id::new("pdf-page-scroll"))
                    .on_scroll(Message::PageScrolled)
                    .direction(scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::new(),
                        horizontal: scrollable::Scrollbar::new()
                            .width(0)
                            .scroller_width(0)
                            .margin(0)
                            .spacing(0),
                    }),
                app.keyboard_modifiers.control(),
                app.annotation_tool == AnnotationTool::Hand,
                app.pan_drag.map(|drag| drag.button),
            )
        } else if app.document.is_some() {
            let choose_other_renderer: Element<'_, Message> =
                if !app.busy && app.backend == opds_dynlex_backend::Backend::Dynlex {
                    let (label, backend) = if opds_dynlex_backend::pdfium_available() {
                        ("Try PDFium", opds_dynlex_backend::Backend::Pdfium)
                    } else {
                        ("View with Hayro", opds_dynlex_backend::Backend::Hayro)
                    };
                    button(label)
                        .on_press(Message::SelectBackend(backend))
                        .style(|t, s| tool_btn_style(t, true, s))
                        .into()
                } else {
                    Space::new().height(0).into()
                };
            super::zoom_area::view(
                container(
                    column![
                        text(if app.busy {
                            "Rendering…"
                        } else {
                            "No rendered page"
                        })
                        .size(22),
                        text(&app.message)
                            .size(14)
                            .wrapping(iced::widget::text::Wrapping::Word),
                        text("Select a renderer at the bottom left.")
                            .size(14)
                            .wrapping(iced::widget::text::Wrapping::Word),
                        choose_other_renderer,
                    ]
                    .spacing(16)
                    .max_width(350),
                )
                .center(Fill),
                app.keyboard_modifiers.control(),
                app.annotation_tool == AnnotationTool::Hand,
                app.pan_drag.map(|drag| drag.button),
            )
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
    let rail = column![
        left_rail_tab(
            if app.hide_pages {
                include_bytes!("../../../assets/ui/left-panel/thumbnailsIcon.svg")
            } else {
                include_bytes!("../../../assets/ui/left-panel/thumbnailsActiveIcon.svg")
            },
            "Thumbnails",
            Some(Message::TogglePages),
            !app.hide_pages,
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/bookmarksIcon.svg"),
            "Bookmarks",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/annotationsIcon.svg"),
            "Annotations",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/attachmentsIcon.svg"),
            "Attachments",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/signaturesIcon.svg"),
            "Signatures",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/layersIcon.svg"),
            "Layers",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/formFieldsIcon.svg"),
            "Form fields",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/destinationsIcon.svg"),
            "Destinations",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/tagsIcon.svg"),
            "Tags",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/linksIcon.svg"),
            "Links",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/measurementsIcon.svg"),
            "Measurements",
            None,
            false
        ),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/schedulesIcon.svg"),
            "Schedules",
            None,
            false
        ),
        Space::new().height(Fill),
        left_rail_tab(
            include_bytes!("../../../assets/ui/left-panel/toggleIcon.svg"),
            "Collapse panel",
            Some(Message::TogglePages),
            false
        ),
    ];
    workspace =
        workspace.push(
            container(rail)
                .width(32)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgb8(54, 54, 62).into()),
                    ..Default::default()
                }),
        );
    if !app.hide_pages {
        let focus = app.document.as_ref().map_or(0, |d| d.thumbnail_focus);
        let visible = super::thumbnail_window(focus, count);
        let mut pages = column![].spacing(0).padding(4);
        if visible.start > 0 {
            pages =
                pages.push(Space::new().height(visible.start as f32 * super::THUMBNAIL_ROW_STEP));
        }
        for index in visible.clone() {
            let handle = app.document.as_ref().and_then(|document| {
                document
                    .thumbnails
                    .iter()
                    .find(|(page, _)| *page == index)
                    .map(|(_, handle)| handle.clone())
                    .or_else(|| {
                        (document.displayed_page == index)
                            .then(|| document.bitmap.clone())
                            .flatten()
                    })
            });
            let preview: Element<'_, Message> = if let Some(handle) = handle {
                image(handle)
                    .width(super::THUMBNAIL_WIDTH)
                    .height(iced::Length::Shrink)
                    .content_fit(iced::ContentFit::Contain)
                    .into()
            } else {
                container(
                    text(
                        if app
                            .document
                            .as_ref()
                            .is_some_and(|d| d.thumbnail_failed.contains(&index))
                        {
                            "Unsupported preview"
                        } else {
                            "Page preview"
                        },
                    )
                    .size(11),
                )
                .width(super::THUMBNAIL_WIDTH)
                .height(super::THUMBNAIL_HEIGHT)
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
                    .width(160)
                    .height(super::THUMBNAIL_CARD_HEIGHT)
                    .padding(4)
                    .on_press(Message::GoToPage(index))
                    .style(move |_, status| thumbnail_btn_style(index == page, status)),
                )
                .width(Fill)
                .height(super::THUMBNAIL_ROW_STEP)
                .align_x(iced::Center),
            );
        }
        if visible.end < count {
            pages = pages.push(
                Space::new().height((count - visible.end) as f32 * super::THUMBNAIL_ROW_STEP),
            );
        }
        workspace = workspace.push(
            container(column![
                panel_header("Thumbnails", Message::TogglePages),
                scrollable(pages)
                    .id(iced::widget::Id::new("pdf-thumbnails"))
                    .on_scroll(Message::ThumbnailsScrolled),
            ])
            .width(168)
            .height(Fill)
            .style(panel_surface),
        );
        workspace = workspace.push(container(Space::new()).width(4).height(Fill).style(|_| {
            container::Style {
                background: Some(Color::from_rgb8(98, 78, 67).into()),
                ..Default::default()
            }
        }));
    }
    let palette_width = super::drawing_palette_width(app.window_size.1, app.ribbon_collapsed);
    let two_columns = super::drawing_palette_two_columns(app.window_size.1, app.ribbon_collapsed);
    let tool_row = |left, right| row![left, right].spacing(4).padding([1, 2]);
    let separator = || container(rule::horizontal(1)).height(7).padding([3, 6]);
    let drawing_tools = column![
        container(text("⠿").size(11))
            .height(24)
            .width(palette_width)
            .center_x(Fill),
        tool_row(
            palette_tool(
                palette_icon!("handIcon"),
                "Hand",
                Some(Message::SelectAnnotationTool(AnnotationTool::Hand)),
                app.annotation_tool == AnnotationTool::Hand
            ),
            palette_tool(
                if app.annotation_tool == AnnotationTool::Select {
                    palette_icon!("selectActiveIcon")
                } else {
                    palette_icon!("selectIcon")
                },
                "Select",
                Some(Message::SelectAnnotationTool(AnnotationTool::Select)),
                app.annotation_tool == AnnotationTool::Select
            ),
        ),
        separator(),
        tool_row(
            palette_tool(
                palette_icon!("highlightIcon"),
                "Highlight",
                Some(Message::SelectAnnotationTool(AnnotationTool::Highlight)),
                app.annotation_tool == AnnotationTool::Highlight
            ),
            palette_tool(
                palette_icon!("freehandIcon"),
                "Freehand",
                Some(Message::SelectAnnotationTool(AnnotationTool::Freehand)),
                app.annotation_tool == AnnotationTool::Freehand
            ),
        ),
        tool_row(
            palette_tool(
                palette_icon!("lineIcon"),
                "Line",
                Some(Message::SelectAnnotationTool(AnnotationTool::Line)),
                app.annotation_tool == AnnotationTool::Line
            ),
            palette_tool(
                palette_icon!("arrowIcon"),
                "Arrow",
                Some(Message::SelectAnnotationTool(AnnotationTool::Arrow)),
                app.annotation_tool == AnnotationTool::Arrow
            ),
        ),
        tool_row(
            palette_tool(
                palette_icon!("polylineIcon"),
                "Polyline",
                Some(Message::SelectAnnotationTool(AnnotationTool::Polyline)),
                app.annotation_tool == AnnotationTool::Polyline
            ),
            Space::new().width(36).into(),
        ),
        separator(),
        tool_row(
            palette_tool(
                palette_icon!("rectIcon"),
                "Rectangle",
                Some(Message::SelectAnnotationTool(AnnotationTool::Rectangle)),
                app.annotation_tool == AnnotationTool::Rectangle
            ),
            palette_tool(
                palette_icon!("ellipseIcon"),
                "Ellipse",
                Some(Message::SelectAnnotationTool(AnnotationTool::Circle)),
                app.annotation_tool == AnnotationTool::Circle
            ),
        ),
        tool_row(
            palette_tool(
                palette_icon!("polygonIcon"),
                "Polygon",
                Some(Message::SelectAnnotationTool(AnnotationTool::Polygon)),
                app.annotation_tool == AnnotationTool::Polygon
            ),
            palette_tool(palette_icon!("cloudIcon"), "Cloud", None, false),
        ),
        tool_row(
            palette_tool(
                palette_icon!("cloudPolylineIcon"),
                "Cloud polyline",
                None,
                false
            ),
            Space::new().width(36).into(),
        ),
        separator(),
        tool_row(
            palette_tool(
                palette_icon!("textboxIcon"),
                "Text box",
                Some(Message::SelectAnnotationTool(AnnotationTool::Text)),
                app.annotation_tool == AnnotationTool::Text
            ),
            palette_tool(palette_icon!("calloutIcon"), "Callout", None, false),
        ),
        tool_row(
            palette_tool(palette_icon!("noteIcon"), "Note", None, false),
            Space::new().width(36).into(),
        ),
        separator(),
        tool_row(
            palette_tool(palette_icon!("stampIcon"), "Stamp", None, false),
            palette_tool(palette_icon!("signatureIcon"), "Signature", None, false),
        ),
        separator(),
        tool_row(
            palette_tool(
                palette_icon!("measureDistanceIcon"),
                "Measure distance",
                None,
                false
            ),
            palette_tool(
                palette_icon!("measureAreaIcon"),
                "Measure area",
                None,
                false
            ),
        ),
        tool_row(
            palette_tool(
                palette_icon!("measurePerimeterIcon"),
                "Measure perimeter",
                None,
                false
            ),
            Space::new().width(36).into(),
        ),
    ];
    let drawing_tools: Element<'_, Message> = if two_columns {
        drawing_tools.into()
    } else {
        // Match Tauri's full-height single-column palette. Keep the same tool
        // order and disabled state as the compact two-column layout.
        column![
            container(text("⠿").size(11))
                .height(24)
                .width(palette_width)
                .center_x(Fill),
            palette_tool(
                palette_icon!("handIcon"),
                "Hand",
                Some(Message::SelectAnnotationTool(AnnotationTool::Hand)),
                app.annotation_tool == AnnotationTool::Hand
            ),
            palette_tool(
                if app.annotation_tool == AnnotationTool::Select {
                    palette_icon!("selectActiveIcon")
                } else {
                    palette_icon!("selectIcon")
                },
                "Select",
                Some(Message::SelectAnnotationTool(AnnotationTool::Select)),
                app.annotation_tool == AnnotationTool::Select
            ),
            separator(),
            palette_tool(
                palette_icon!("highlightIcon"),
                "Highlight",
                Some(Message::SelectAnnotationTool(AnnotationTool::Highlight)),
                app.annotation_tool == AnnotationTool::Highlight
            ),
            palette_tool(
                palette_icon!("freehandIcon"),
                "Freehand",
                Some(Message::SelectAnnotationTool(AnnotationTool::Freehand)),
                app.annotation_tool == AnnotationTool::Freehand
            ),
            palette_tool(
                palette_icon!("lineIcon"),
                "Line",
                Some(Message::SelectAnnotationTool(AnnotationTool::Line)),
                app.annotation_tool == AnnotationTool::Line
            ),
            palette_tool(
                palette_icon!("arrowIcon"),
                "Arrow",
                Some(Message::SelectAnnotationTool(AnnotationTool::Arrow)),
                app.annotation_tool == AnnotationTool::Arrow
            ),
            palette_tool(
                palette_icon!("polylineIcon"),
                "Polyline",
                Some(Message::SelectAnnotationTool(AnnotationTool::Polyline)),
                app.annotation_tool == AnnotationTool::Polyline
            ),
            separator(),
            palette_tool(
                palette_icon!("rectIcon"),
                "Rectangle",
                Some(Message::SelectAnnotationTool(AnnotationTool::Rectangle)),
                app.annotation_tool == AnnotationTool::Rectangle
            ),
            palette_tool(
                palette_icon!("ellipseIcon"),
                "Ellipse",
                Some(Message::SelectAnnotationTool(AnnotationTool::Circle)),
                app.annotation_tool == AnnotationTool::Circle
            ),
            palette_tool(
                palette_icon!("polygonIcon"),
                "Polygon",
                Some(Message::SelectAnnotationTool(AnnotationTool::Polygon)),
                app.annotation_tool == AnnotationTool::Polygon
            ),
            palette_tool(palette_icon!("cloudIcon"), "Cloud", None, false),
            palette_tool(
                palette_icon!("cloudPolylineIcon"),
                "Cloud polyline",
                None,
                false
            ),
            separator(),
            palette_tool(
                palette_icon!("textboxIcon"),
                "Text box",
                Some(Message::SelectAnnotationTool(AnnotationTool::Text)),
                app.annotation_tool == AnnotationTool::Text
            ),
            palette_tool(palette_icon!("calloutIcon"), "Callout", None, false),
            palette_tool(palette_icon!("noteIcon"), "Note", None, false),
            separator(),
            palette_tool(palette_icon!("stampIcon"), "Stamp", None, false),
            palette_tool(palette_icon!("signatureIcon"), "Signature", None, false),
            separator(),
            palette_tool(
                palette_icon!("measureDistanceIcon"),
                "Measure distance",
                None,
                false
            ),
            palette_tool(
                palette_icon!("measureAreaIcon"),
                "Measure area",
                None,
                false
            ),
            palette_tool(
                palette_icon!("measurePerimeterIcon"),
                "Measure perimeter",
                None,
                false
            ),
        ]
        .into()
    };
    workspace = workspace.push(
        container(drawing_tools)
            .width(palette_width)
            .height(Fill)
            .style(panel_surface),
    );
    let center: Element<'_, Message> = if app.document.is_some() {
        let mut chips = row![].spacing(8);
        for chip in &app.keystrokes.chips {
            let label = if chip.count > 1 {
                format!("{} ×{}", chip.label, chip.count)
            } else {
                chip.label.clone()
            };
            chips = chips.push(
                container(text(label).size(20).color(Color::WHITE))
                    .padding([8, 14])
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.08, 0.08, 0.08, 0.85).into()),
                        border: iced::Border {
                            color: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
                            width: 1.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            );
        }
        iced::widget::stack![
            center,
            container(chips)
                .width(Fill)
                .height(Fill)
                .padding(16)
                .align_x(iced::Right)
                .align_y(iced::Bottom)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        center
    };
    let rendered_page = app.document.as_ref().is_some_and(|d| d.bitmap.is_some());
    workspace = workspace.push(container(center).width(Fill).height(Fill).style(move |_| {
        container::Style {
            // Same paper surround as Tauri pdf-viewport.js::_render().
            background: Some(
                if rendered_page {
                    Color::from_rgb8(224, 224, 224)
                } else {
                    Color::from_rgb8(46, 46, 54)
                }
                .into(),
            ),
            ..Default::default()
        }
    }));
    if !app.hide_symbol_palette {
        let palette = column![
            row![
                text("⠿  Toolpalette").size(12).width(Fill),
                button("×")
                    .on_press(Message::ToggleSymbolPalette)
                    .padding([0, 5]),
            ]
            .height(29)
            .align_y(iced::Center),
            container(text("⌕  Search symbols...").size(11)).padding(7),
            container(text("Scale     1x       1").size(11)).padding(7),
            rule::horizontal(1),
            text("›  NL IFC Bouw").size(12),
            text("›  NL Elektra").size(12),
            text("›  NL Bouwplaats").size(12),
            text("›  NL Sanitair").size(12),
            text("›  NL Keuken").size(12),
            text("›  NL NEN 1414 — Brandbeveiliging").size(12),
            text("›  NL NEN 1414 — Bussystemen").size(12),
            text("›  NL NEN 1414 — Deuren").size(12),
        ]
        .spacing(9)
        .padding(7);
        workspace = workspace.push(
            container(palette)
                .width(220)
                .height(Fill)
                .style(panel_surface),
        );
    }
    if !app.hide_properties && app.show_annotation_list {
        let mut annotations = column![
            text("Annotations").size(12),
            text(format!("Page {}", page + 1)).size(10),
        ]
        .padding(12)
        .spacing(8);
        let mut count = 0;
        if let Some(document) = &app.document {
            for annotation in document.model.items().filter(|item| item.page == page) {
                count += 1;
                let kind = match &annotation.kind {
                    opds_native_core::annotations::Kind::Rectangle { color_rgb }
                        if *color_rgb == [255, 226, 67] =>
                    {
                        "Highlight"
                    }
                    opds_native_core::annotations::Kind::Rectangle { .. } => "Rectangle",
                    opds_native_core::annotations::Kind::Ellipse { .. } => "Ellipse",
                    opds_native_core::annotations::Kind::Polygon { .. } => "Polygon",
                    opds_native_core::annotations::Kind::Line { .. } => "Line",
                    opds_native_core::annotations::Kind::Arrow { .. } => "Arrow",
                    opds_native_core::annotations::Kind::Polyline { .. } => "Polyline",
                    opds_native_core::annotations::Kind::Text { .. } => "Text Box",
                };
                let selected = document.selected_annotation == Some(annotation.id);
                annotations = annotations.push(
                    button(text(format!("{kind} #{}", annotation.id)).size(12))
                        .on_press(Message::SelectAnnotation(Some(annotation.id)))
                        .style(move |_, status| pdf_tool_style(selected, status))
                        .width(Fill),
                );
            }
        }
        if count == 0 {
            annotations = annotations.push(text("No annotations on this page").size(11));
        }
        workspace = workspace.push(
            container(scrollable(annotations))
                .width(300)
                .height(Fill)
                .style(surface),
        );
    } else if !app.hide_properties {
        let page_size = app
            .document
            .as_ref()
            .and_then(|d| d.pdf.page_dimensions(page + 1).ok())
            .map(|(w, h)| format!("{:.1} × {:.1} mm", w * 25.4 / 72.0, h * 25.4 / 72.0))
            .unwrap_or_else(|| "-".into());
        let document_path = app
            .document
            .as_ref()
            .filter(|document| !document.is_new)
            .map(|document| document.path.clone())
            .unwrap_or_else(|| "-".into());
        let pdf_info = app
            .document
            .as_ref()
            .map(|document| document.pdf.document_info());
        let mut properties = column![
            properties_header(),
            property_section_toggle("DOCUMENT", 0, app.properties_collapsed[0]),
        ]
        .spacing(0);
        if !app.properties_collapsed[0] {
            properties = properties
                .push(property_row(
                    "File",
                    if has_doc {
                        name.clone()
                    } else {
                        "No file open".into()
                    },
                ))
                .push(property_row("Path", document_path))
                .push(property_row(
                    "Pages",
                    if has_doc {
                        format!("{} / {count}", page + 1)
                    } else {
                        "-".into()
                    },
                ))
                .push(property_row("Page Size", page_size));
        }
        properties = properties.push(Space::new().height(4));
        properties = properties.push(property_section_toggle(
            "METADATA",
            1,
            app.properties_collapsed[1],
        ));
        if !app.properties_collapsed[1] {
            properties = properties
                .push(property_row(
                    "Title",
                    pdf_info
                        .and_then(|info| info.title.clone())
                        .unwrap_or_else(|| "-".into()),
                ))
                .push(property_row(
                    "Author",
                    pdf_info
                        .and_then(|info| info.author.clone())
                        .unwrap_or_else(|| "-".into()),
                ))
                .push(property_row(
                    "Subject",
                    pdf_info
                        .and_then(|info| info.subject.clone())
                        .unwrap_or_else(|| "-".into()),
                ))
                .push(property_row(
                    "Creator",
                    pdf_info
                        .and_then(|info| info.creator.clone())
                        .unwrap_or_else(|| "-".into()),
                ))
                .push(property_row(
                    "Producer",
                    pdf_info
                        .and_then(|info| info.producer.clone())
                        .unwrap_or_else(|| "-".into()),
                ))
                .push(property_row(
                    "PDF Version",
                    pdf_info.map_or("-", |info| info.version).into(),
                ));
        }
        properties = properties.push(Space::new().height(4));
        properties = properties.push(property_section_toggle(
            "ANNOTATIONS",
            2,
            app.properties_collapsed[2],
        ));
        if !app.properties_collapsed[2] {
            properties = properties
                .push(property_row(
                    "Total",
                    app.document
                        .as_ref()
                        .map_or(0, |d| d.model.items().count())
                        .to_string(),
                ))
                .push(property_row(
                    "On Page",
                    format!(
                        "{} (page {})",
                        app.document.as_ref().map_or(0, |d| d
                            .model
                            .items()
                            .filter(|a| a.page == page)
                            .count()),
                        page + 1
                    ),
                ));
        }
        properties = properties.push(Space::new().height(4));
        properties = properties.push(
            container(
                text("Select an annotation to edit its properties")
                    .size(11)
                    .font(iced::Font {
                        style: iced::font::Style::Italic,
                        ..iced::Font::DEFAULT
                    })
                    .color(Color::from_rgb8(170, 170, 180)),
            )
            .width(Fill)
            .align_x(iced::Center)
            .padding([12, 4]),
        );
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
                opds_native_core::annotations::Kind::Ellipse { .. } => "Ellipse",
                opds_native_core::annotations::Kind::Polygon { .. } => "Polygon",
                opds_native_core::annotations::Kind::Line { .. } => "Line",
                opds_native_core::annotations::Kind::Arrow { .. } => "Arrow",
                opds_native_core::annotations::Kind::Polyline { .. } => "Polyline",
                opds_native_core::annotations::Kind::Text { .. } => "Text",
            };
            properties = properties
                .push(property_section("SELECTED ANNOTATION"))
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
                )
                .push(
                    text(format!(
                        "Color   #{:02X}{:02X}{:02X}",
                        annotation_color(&annotation.kind)[0],
                        annotation_color(&annotation.kind)[1],
                        annotation_color(&annotation.kind)[2]
                    ))
                    .size(12),
                )
                .push(button("Choose color…").on_press(Message::SelectRibbon(RibbonTab::Format)));
            if matches!(
                annotation.kind,
                opds_native_core::annotations::Kind::Text { .. }
            ) {
                properties =
                    properties.push(button("Edit text").on_press(Message::EditSelectedText));
            }
            properties = properties
                .push(button("Delete annotation").on_press(Message::RequestDeleteAnnotation));
        }
        workspace = workspace.push(
            container(scrollable(properties))
                .width(300)
                .height(Fill)
                .style(panel_surface),
        );
    } else {
        workspace = workspace.push(
            button(
                container(canvas(VerticalPropertiesLabel).width(28).height(95))
                    .width(28)
                    .height(Fill)
                    .align_x(iced::Center)
                    .align_y(iced::Top)
                    .padding([12, 0]),
            )
            .on_press(Message::ToggleProperties)
            .width(28)
            .height(Fill)
            .padding(0)
            .style(|_, status| button::Style {
                background: Some(
                    Color::from_rgb8(
                        if status == button::Status::Hovered {
                            68
                        } else {
                            54
                        },
                        if status == button::Status::Hovered {
                            68
                        } else {
                            54
                        },
                        if status == button::Status::Hovered {
                            76
                        } else {
                            62
                        },
                    )
                    .into(),
                ),
                text_color: Color::from_rgb8(170, 170, 180),
                ..Default::default()
            }),
        );
    }
    let renderer_label = app
        .document
        .as_ref()
        .map(|document| {
            if document.bitmap.is_none() {
                return if app.backend == opds_dynlex_backend::Backend::Dynlex {
                    "DynLex only · no fallback"
                } else {
                    "No rendered page"
                };
            }
            let dynlex_annotations = app.dynlex_visible_annotation_bitmap(document).is_some();
            let has_annotations = document
                .model
                .items()
                .any(|item| item.page == document.displayed_page);
            if document.imported_annotations_dynlex {
                if dynlex_annotations {
                    "PDF: Hayro · Imported: DynLex · Native: DynLex"
                } else if has_annotations {
                    "PDF: Hayro · Imported: DynLex · Native: Iced"
                } else {
                    "PDF: Hayro · Imported: DynLex"
                }
            } else {
                match (document.bitmap_backend, has_annotations, dynlex_annotations) {
                    (opds_dynlex_backend::Backend::Dynlex, _, true) => {
                        "PDF: DynLex · Annotations: DynLex"
                    }
                    (opds_dynlex_backend::Backend::Hayro, _, true) => {
                        "PDF: Hayro fallback · Annotations: DynLex"
                    }
                    (opds_dynlex_backend::Backend::Dynlex, true, false) => {
                        "PDF: DynLex · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::Hayro, true, false)
                        if app.backend == opds_dynlex_backend::Backend::Dynlex =>
                    {
                        "PDF: Hayro fallback · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::Hayro, true, false)
                        if app.backend == opds_dynlex_backend::Backend::Pdfium =>
                    {
                        "PDF: Hayro (PDFium fallback) · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::Hayro, true, false)
                        if app.backend == opds_dynlex_backend::Backend::OwnRust =>
                    {
                        "PDF: Hayro (Eigen Rust fallback) · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::Hayro, true, false) => {
                        "PDF: Hayro · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::Dynlex, false, false) => "Renderer: DynLex",
                    (opds_dynlex_backend::Backend::Pdfium, true, false) => {
                        "PDF: PDFium · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::Pdfium, false, false) => "Renderer: PDFium",
                    (opds_dynlex_backend::Backend::OwnRust, true, false) => {
                        "PDF: Eigen Rust · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::OwnRust, false, false) => "Renderer: Eigen Rust",
                    (opds_dynlex_backend::Backend::PdfCraft, true, false) => {
                        "PDF: PdfCraft (Hayro) · Annotations: Iced"
                    }
                    (opds_dynlex_backend::Backend::PdfCraft, false, false) => {
                        "Renderer: PdfCraft (Hayro)"
                    }
                    (opds_dynlex_backend::Backend::Hayro, false, false)
                        if app.backend == opds_dynlex_backend::Backend::Dynlex =>
                    {
                        "Renderer: Hayro (DynLex fallback)"
                    }
                    (opds_dynlex_backend::Backend::Hayro, _, false)
                        if app.backend == opds_dynlex_backend::Backend::Pdfium =>
                    {
                        "Renderer: Hayro (PDFium fallback)"
                    }
                    (opds_dynlex_backend::Backend::Hayro, _, false)
                        if app.backend == opds_dynlex_backend::Backend::OwnRust =>
                    {
                        "Renderer: Hayro (Eigen Rust fallback)"
                    }
                    _ => "Renderer: Hayro",
                }
            }
        })
        .unwrap_or("");
    let tool_name = match app.annotation_tool {
        AnnotationTool::Select => "Select Tool",
        AnnotationTool::Hand => "Hand Tool",
        AnnotationTool::Rectangle => "Rectangle",
        AnnotationTool::Circle => "Circle",
        AnnotationTool::Ellipse => "Ellipse",
        AnnotationTool::Polygon => "Polygon",
        AnnotationTool::Freehand => "Freehand",
        AnnotationTool::Line => "Line",
        AnnotationTool::Arrow => "Arrow",
        AnnotationTool::Polyline => "Polyline",
        AnnotationTool::Highlight => "Highlight",
        AnnotationTool::Text => "Text Box",
    };
    let (page_annotations, total_annotations) = app.document.as_ref().map_or((0, 0), |document| {
        (
            document
                .model
                .items()
                .filter(|item| item.page == page)
                .count(),
            document.model.items().count(),
        )
    });
    let status_left = row![
        text("Kernel").size(11),
        pick_list(
            available_kernels(),
            Some(kernel_name(app.backend)),
            |name| Message::SelectBackend(kernel_from_name(name)),
        )
        .width(132)
        .text_size(11)
        .padding([1, 4]),
        text(format!("Tool: {tool_name}")).size(11),
        text(format!(
            "Annotations: {page_annotations} ({total_annotations} total)"
        ))
        .size(11),
    ]
    .spacing(7)
    .align_y(iced::Center);
    let status_center = row![
        status_nav(
            FIRST_PAGE,
            "First page",
            (has_doc && page > 0).then_some(Message::GoToPage(0))
        ),
        status_nav(
            PREVIOUS_PAGE,
            "Previous page",
            (has_doc && page > 0).then_some(Message::GoToPage(page.saturating_sub(1)))
        ),
        text(format!(
            "Page {} / {}",
            if has_doc { page + 1 } else { 0 },
            count
        ))
        .size(11),
        status_nav(
            NEXT_PAGE,
            "Next page",
            (has_doc && page + 1 < count).then_some(Message::GoToPage(page + 1))
        ),
        status_nav(
            LAST_PAGE,
            "Last page",
            (has_doc && page + 1 < count).then_some(Message::GoToPage(count.saturating_sub(1)))
        ),
        status_nav(
            ZOOM_OUT,
            "Zoom out",
            has_doc.then_some(Message::Zoom((scale / 1.25).max(0.1)))
        ),
        text(format!("{:.0}%", scale * 100.0)).size(11),
        status_nav(
            ZOOM_IN,
            "Zoom in",
            has_doc.then_some(Message::Zoom((scale * 1.25).min(8.0)))
        ),
    ]
    .spacing(3)
    .align_y(iced::Center);
    let resize_grip: Element<'_, Message> = if !app.window_maximized && window_controls_enabled {
        tooltip(
            mouse_area(
                container(
                    svg(svg::Handle::from_memory(RESIZE_GRIP_ICON))
                        .width(16)
                        .height(16),
                )
                .width(20)
                .height(18)
                .center(Fill),
            )
            .interaction(iced::mouse::Interaction::ResizingDiagonallyDown)
            .on_press(Message::ResizeWindow),
            text("Drag to resize window").size(11),
            tooltip::Position::Top,
        )
        .into()
    } else {
        Space::new().width(20).height(18).into()
    };
    let status = container(
        row![
            container(status_left).width(Fill),
            status_center,
            container(row![text(&app.message).size(11), text(renderer_label).size(11)].spacing(8))
                .width(Fill)
                .align_x(iced::Right),
        ]
        .align_y(iced::Center),
    )
    .padding([2, 8])
    .width(Fill)
    .height(22)
    .style(|_| container::Style {
        background: Some(Color::from_rgb8(54, 54, 62).into()),
        ..Default::default()
    });
    let status = row![
        status,
        container(resize_grip)
            .width(20)
            .height(22)
            .align_y(iced::Center)
            .style(|_| container::Style {
                background: Some(Color::from_rgb8(54, 54, 62).into()),
                ..Default::default()
            })
    ]
    .width(Fill)
    .height(22)
    .align_y(iced::Center);
    let content: Element<'_, Message> = if app.ribbon_collapsed {
        column![titlebar, top, tabs, workspace, status]
    } else {
        column![titlebar, top, ribbon, tabs, workspace, status]
    }
    .width(Fill)
    .height(Fill)
    .into();
    if app.pending_delete_annotation.is_some() {
        let dialog = container(
            column![
                text("Delete Annotation").size(20),
                text("Delete this annotation?").size(12),
                row![
                    button("No").on_press(Message::CancelDeleteAnnotation),
                    button("Yes").on_press(Message::ConfirmDeleteAnnotation),
                ]
                .spacing(8),
            ]
            .spacing(16)
            .padding(24),
        )
        .width(380)
        .style(surface);
        iced::widget::stack![
            content,
            iced::widget::opaque(container(dialog).center(Fill).style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }))
        ]
        .into()
    } else if let Some((_, page)) = app.pending_clear_annotations {
        let label = if page.is_some() {
            "this page"
        } else {
            "the document"
        };
        let action = if page.is_some() {
            "Clear Page"
        } else {
            "Clear All"
        };
        let dialog = container(
            column![
                text("Clear annotations?").size(20),
                text(format!(
                    "Remove native annotations from {label}? You can undo this action."
                ))
                .size(12),
                row![
                    button("Cancel").on_press(Message::CancelClearAnnotations),
                    button(action).on_press(Message::ConfirmClearAnnotations),
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
    } else if let Some(action) = &app.confirm_discard {
        let action_label = match action {
            super::PendingAction::New => "create a blank PDF",
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
    } else if app.new_dialog {
        let mut fields = column![
            row![
                text("New blank PDF").size(20).width(Fill),
                button("×").on_press(Message::CancelNew),
            ],
            text("Paper size").size(12),
            pick_list(
                blank_pdf::Paper::ALL,
                Some(app.new_paper),
                Message::NewPaperSelected,
            ),
            text("Orientation").size(12),
            row![
                button("Portrait")
                    .on_press(Message::NewLandscape(false))
                    .style(move |_, _| button::Style {
                        background: Some(if app.new_landscape {
                            Color::from_rgb8(68, 68, 75).into()
                        } else {
                            Color::from_rgb8(180, 83, 9).into()
                        }),
                        text_color: Color::WHITE,
                        ..Default::default()
                    }),
                button("Landscape")
                    .on_press(Message::NewLandscape(true))
                    .style(move |_, _| button::Style {
                        background: Some(if app.new_landscape {
                            Color::from_rgb8(180, 83, 9).into()
                        } else {
                            Color::from_rgb8(68, 68, 75).into()
                        }),
                        text_color: Color::WHITE,
                        ..Default::default()
                    }),
            ]
            .spacing(8),
            text("Pages (1–999)").size(12),
            text_input("1", &app.new_pages_input).on_input(Message::NewPagesChanged),
        ]
        .spacing(10);
        if app.new_paper == blank_pdf::Paper::Custom {
            fields = fields
                .push(text("Custom size in mm (10–5000)").size(12))
                .push(
                    row![
                        text_input("Width", &app.new_custom_width_mm)
                            .on_input(Message::NewCustomWidthChanged),
                        text_input("Height", &app.new_custom_height_mm)
                            .on_input(Message::NewCustomHeightChanged),
                    ]
                    .spacing(8),
                );
        }
        let dialog = container(
            fields
                .push(
                    text(&app.new_error)
                        .size(12)
                        .color(Color::from_rgb8(240, 120, 105)),
                )
                .push(
                    row![
                        button("Cancel").on_press(Message::CancelNew),
                        button("Create PDF").on_press(Message::NewBlank),
                    ]
                    .spacing(8),
                )
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
    } else if app.extension_import_dialog {
        let dialog = container(
            column![
                row![
                    text("Open extension package").size(20).width(Fill),
                    button("×").on_press(Message::CloseExtensionImport),
                ],
                text("Open a package built by this Rust editor for further HTML and JavaScript editing.").size(12),
                text_input("Absolute path to an existing .oppx", &app.extension_import_path)
                    .on_input(Message::ExtensionImportPathChanged)
                    .on_submit(Message::ImportExtension),
                text(&app.extension_error).size(12).color(Color::from_rgb8(240, 120, 105)),
                row![
                    button("Open package").on_press(Message::ImportExtension),
                    button("Cancel").on_press(Message::CloseExtensionImport),
                ].spacing(8),
            ]
            .spacing(12)
            .padding(20),
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
    } else if app.extensions_dialog {
        let dialog = container(
            column![
                row![
                    text("Build extension").size(20).width(Fill),
                    button("×").on_press(Message::CloseExtensions),
                ],
                text("Create an .oppx package with HTML and JavaScript source. The package is not activated in the Rust app.").size(12),
                row![
                    text_input("Extension ID", &app.extension_id)
                        .on_input(Message::ExtensionIdChanged),
                    text_input("Extension name", &app.extension_name)
                        .on_input(Message::ExtensionNameChanged),
                ].spacing(8),
                text_input("Absolute output path ending in .oppx", &app.extension_output)
                    .on_input(Message::ExtensionOutputChanged),
                text("Source preview and editor; the preview reflects the bytes packaged below.").size(11),
                row![
                    column![
                        text("index.html").size(12),
                        scrollable(text(app.extension_html.text()).size(11)).height(75),
                        text_editor(&app.extension_html)
                            .height(145)
                            .size(12)
                            .style(|theme, status| {
                                let mut style = text_editor::default(theme, status);
                                style.background = Color::from_rgb8(48, 48, 54).into();
                                style.value = Color::WHITE;
                                style.placeholder = Color::from_rgb8(170, 170, 176);
                                style
                            })
                            .on_action(Message::ExtensionHtmlEdited),
                    ].spacing(4).width(Fill),
                    column![
                        text("index.js").size(12),
                        scrollable(text(app.extension_javascript.text()).size(11)).height(75),
                        text_editor(&app.extension_javascript)
                            .height(145)
                            .size(12)
                            .style(|theme, status| {
                                let mut style = text_editor::default(theme, status);
                                style.background = Color::from_rgb8(48, 48, 54).into();
                                style.value = Color::WHITE;
                                style.placeholder = Color::from_rgb8(170, 170, 176);
                                style
                            })
                            .on_action(Message::ExtensionJavascriptEdited),
                    ].spacing(4).width(Fill),
                ].spacing(12),
                text(&app.extension_error).size(12).color(Color::from_rgb8(240, 120, 105)),
                row![
                    button("Build .oppx").on_press(Message::BuildExtension),
                    button("Close").on_press(Message::CloseExtensions),
                ].spacing(8),
            ]
            .spacing(12)
            .padding(20),
        )
        .width(880)
        .style(surface);
        iced::widget::stack![
            content,
            iced::widget::opaque(container(dialog).center(Fill).style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }))
        ]
        .into()
    } else if app.about_dialog {
        let dialog = container(
            column![
                row![
                    text("About Open PDF Studio").size(20).width(Fill),
                    button("×").on_press(Message::CloseAbout),
                ]
                .align_y(iced::Center),
                rule::horizontal(1),
                row![
                    image(title_app_icon()).width(64).height(64),
                    column![
                        text("Open PDF Studio").size(22),
                        text(format!("Rust version {}", env!("CARGO_PKG_VERSION"))).size(12),
                        text("Native PDF editing in development").size(12),
                    ]
                    .spacing(5),
                ]
                .spacing(16)
                .align_y(iced::Center),
                text("Open PDF Studio is developed by the OpenAEC Foundation.").size(13),
                text("Source: github.com/OpenAEC-Foundation/open-pdf-studio").size(12),
                text("License: GPL-3.0-only").size(12),
                button("Close").on_press(Message::CloseAbout),
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
    } else if app.file_menu_open {
        let sidebar = column![
            button(row![text("←").size(18), text("File").size(15)].spacing(10))
                .on_press(Message::CloseFileMenu)
                .width(Fill)
                .height(45)
                .padding([14, 20])
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
                            },
                        )
                        .into()
                    ),
                    text_color: Color::from_rgb8(54, 54, 62),
                    border: Border {
                        radius: 0.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            Space::new().height(8),
            file_menu_item("New", "Ctrl+N", Some(Message::ShowNew), false),
            file_menu_item("Open", "Ctrl+O", Some(Message::SelectFileOpenPanel), true),
            file_menu_item("Save", "Ctrl+S", has_doc.then_some(Message::Save), false),
            file_menu_item(
                "Save As",
                "Ctrl+Shift+S",
                has_doc.then_some(Message::ShowSave),
                false,
            ),
            file_menu_item(
                "Print",
                "Ctrl+P",
                has_doc.then_some(Message::ShowPrint),
                false
            ),
            file_menu_divider(),
            file_menu_item("Annotate Screenshot", "", None, false),
            file_menu_divider(),
            file_menu_item(
                "Import XFDF annotations",
                "",
                has_doc.then_some(Message::ShowImport),
                false
            ),
            file_menu_item(
                "Export XFDF annotations",
                "",
                has_doc.then_some(Message::ShowXfdfExport),
                false
            ),
            file_menu_item(
                "Export pages as images",
                "",
                has_doc.then_some(Message::ShowExport),
                false,
            ),
            file_menu_divider(),
            file_menu_item("Extensions", "", Some(Message::ShowExtensions), false),
            file_menu_item(
                "Open Extension Package",
                "",
                Some(Message::ShowExtensionImport),
                false
            ),
            file_menu_divider(),
            file_menu_item(
                "Document Properties",
                "Ctrl+D",
                has_doc.then_some(Message::ShowProperties),
                false,
            ),
            file_menu_divider(),
            file_menu_item("Preferences", "Ctrl+,", None, false),
            file_menu_divider(),
            file_menu_item("About", "", Some(Message::ShowAbout), false),
            file_menu_item("What's New", "", None, false),
            file_menu_divider(),
            file_menu_item("Exit", "Alt+F4", Some(Message::CloseWindow), false),
        ]
        .spacing(0);
        let open_navigation = column![
            file_open_nav_item(
                "Recent Files",
                icon_asset!("ui/home.svg"),
                Some(Message::SelectFileOpenPanel),
                true
            ),
            file_open_nav_item("Sessions", icon_asset!("ui/grid.svg"), None, false),
            file_open_nav_item("Open from URL", icon_asset!("ui/folder.svg"), None, false),
            file_open_nav_item("Add a Place", icon_asset!("ui/pin.svg"), None, false),
            file_open_nav_item("Browse", OPEN, Some(Message::ShowOpen), false),
        ]
        .padding([12, 0]);
        let matching_recent: Vec<_> = app
            .recent_files
            .iter()
            .filter(|entry| super::recent_file_matches(&entry.path, &app.recent_query))
            .collect();
        let pinned_count = matching_recent.iter().filter(|entry| entry.pinned).count();
        let mut recent_content = column![
            text("Recent Files").size(22),
            text_input("Find", &app.recent_query)
                .on_input(Message::RecentSearchChanged)
                .size(12)
                .padding([6, 8]),
        ]
        .spacing(12)
        .padding([20, 24])
        .width(iced::Length::Fixed(648.0));
        recent_content = recent_content.push(
            button(
                text(format!(
                    "{}  Pinned Recents ({pinned_count})",
                    if app.pinned_recents_collapsed {
                        "›"
                    } else {
                        "⌄"
                    }
                ))
                .size(13),
            )
            .on_press(Message::TogglePinnedRecents)
            .padding(0)
            .style(|_, _| button::Style {
                text_color: Color::from_rgb8(235, 235, 237),
                ..Default::default()
            }),
        );
        if !app.pinned_recents_collapsed {
            for entry in matching_recent.iter().filter(|entry| entry.pinned) {
                recent_content = recent_content.push(recent_file_row(entry));
            }
        }
        recent_content = recent_content.push(
            button(
                text(format!(
                    "{}  Unpinned Recents ({})",
                    if app.unpinned_recents_collapsed {
                        "›"
                    } else {
                        "⌄"
                    },
                    matching_recent.len() - pinned_count
                ))
                .size(13),
            )
            .on_press(Message::ToggleUnpinnedRecents)
            .padding(0)
            .style(|_, _| button::Style {
                text_color: Color::from_rgb8(235, 235, 237),
                ..Default::default()
            }),
        );
        if !app.unpinned_recents_collapsed && matching_recent.len() == pinned_count {
            recent_content = recent_content.push(
                text("No recent files")
                    .size(12)
                    .color(Color::from_rgb8(170, 170, 176)),
            );
        }
        if !app.unpinned_recents_collapsed {
            for entry in matching_recent.iter().filter(|entry| !entry.pinned) {
                recent_content = recent_content.push(recent_file_row(entry));
            }
        }
        let menu = row![
            container(scrollable(sidebar))
                .width(260)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgb8(54, 54, 62).into()),
                    ..Default::default()
                }),
            container(open_navigation)
                .width(200)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgb8(68, 68, 76).into()),
                    ..Default::default()
                }),
            container(scrollable(recent_content))
                .width(Fill)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgb8(68, 68, 76).into()),
                    ..Default::default()
                }),
        ]
        .width(Fill)
        .height(Fill)
        .spacing(0);
        iced::widget::stack![
            content,
            column![Space::new().height(30), iced::widget::opaque(menu)]
                .width(Fill)
                .height(Fill)
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
    } else if app.export_dialog {
        let export_button = if app.export_busy {
            button(text(format!(
                "Exporting {}/{}…",
                app.export_completed, app.export_total
            )))
        } else {
            button(text(format!("Export {}", app.export_format))).on_press(Message::ExportPng)
        };
        let cancel_button = if app.export_busy {
            if app
                .export_cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
            {
                button("Cancelling…")
            } else {
                button("Cancel").on_press(Message::CancelExport)
            }
        } else {
            button("Cancel").on_press(Message::CancelExport)
        };
        let page_control: Element<'_, Message> = match app.export_range {
            image_export::ImagePageRange::Current => row![
                text("Page").size(12),
                text_input("Page number", &app.export_page_input)
                    .on_input(Message::ExportPageChanged)
                    .width(80),
                text(format!(
                    "of {}",
                    app.document
                        .as_ref()
                        .map_or(0, |document| document.pdf.page_count())
                ))
                .size(12)
            ]
            .spacing(12)
            .align_y(iced::Center)
            .into(),
            image_export::ImagePageRange::Custom => {
                text_input("Pages, e.g. 1-3,5", &app.export_custom_pages)
                    .on_input(Message::ExportCustomPagesChanged)
                    .into()
            }
            image_export::ImagePageRange::All => text(format!(
                "All {} pages",
                app.document
                    .as_ref()
                    .map_or(0, |document| document.pdf.page_count())
            ))
            .size(12)
            .into(),
        };
        let quality_control: Element<'_, Message> =
            if app.export_format == image_export::ImageFormat::Jpeg {
                row![
                    text("JPEG Quality").size(12),
                    slider(
                        10..=100,
                        app.export_jpeg_quality.value(),
                        Message::SelectJpegQuality
                    )
                    .width(180),
                    text(format!("{}%", app.export_jpeg_quality.value())).size(12)
                ]
                .spacing(12)
                .align_y(iced::Center)
                .into()
            } else {
                Space::new().height(0).into()
            };
        let export_progress: Element<'_, Message> = if app.export_busy {
            progress_bar(
                0.0..=1.0,
                app.export_completed as f32 / app.export_total.max(1) as f32,
            )
            .into()
        } else {
            Space::new().height(0).into()
        };
        let dialog = container(
            column![
                text("Export pages as images").size(20),
                text("Selected renderer, including supported annotations. Creates a new file or folder; existing destinations are not overwritten.").size(12),
                row![
                    text("Pages").size(12),
                    pick_list(
                        image_export::ImagePageRange::ALL,
                        Some(app.export_range),
                        Message::SelectExportRange
                    )
                ]
                .spacing(12)
                .align_y(iced::Center),
                page_control,
                row![
                    text("Format").size(12),
                    pick_list(
                        image_export::ImageFormat::ALL,
                        Some(app.export_format),
                        Message::SelectExportFormat
                    )
                ]
                .spacing(12)
                .align_y(iced::Center),
                row![
                    text("Resolution").size(12),
                    pick_list(
                        image_export::ImageDpi::ALL,
                        Some(app.export_dpi),
                        Message::SelectExportDpi
                    )
                ]
                .spacing(12)
                .align_y(iced::Center),
                quality_control,
                checkbox(app.export_include_annotations)
                    .label("Include annotations")
                    .on_toggle(Message::SetExportAnnotations)
                    .size(14),
                text_input("New image file or folder path", &app.export_path_input)
                    .on_input(Message::ExportPathChanged)
                    .on_submit(Message::ExportPng),
                export_progress,
                row![export_button, cancel_button].spacing(8)
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
    } else if app.import_dialog {
        let dialog = container(
            column![
                text("Import XFDF annotations").size(20),
                text("Imports supported shapes and text into the active PDF as one undoable edit. Unsupported markup rejects the whole file.").size(12),
                text_input("XFDF file path", &app.import_path_input)
                    .on_input(Message::ImportPathChanged)
                    .on_submit(Message::ImportXfdf),
                row![
                    button("Import").on_press(Message::ImportXfdf),
                    button("Cancel").on_press(Message::CancelImport)
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
    } else if app.xfdf_export_dialog {
        let dialog = container(
            column![
                text("Export XFDF annotations").size(20),
                text("Saves the supported native annotations to a new XFDF file. Unsupported kinds stop the entire export.").size(12),
                text_input("New XFDF file path", &app.xfdf_export_path_input)
                    .on_input(Message::XfdfExportPathChanged)
                    .on_submit(Message::ExportXfdf),
                row![
                    button("Export").on_press(Message::ExportXfdf),
                    button("Cancel").on_press(Message::CancelXfdfExport)
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
        iced::widget::stack![content].into()
    }
}
