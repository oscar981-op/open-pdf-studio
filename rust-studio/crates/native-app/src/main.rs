//! First native window; UI structure follows the current Open PDF Studio app.
#![forbid(unsafe_code)]
mod annotation_layer;
mod blank_pdf;
mod extension_builder;
mod hakan_ui;
mod image_export;
mod inline_text;
mod keystrokes;
mod mcp_server;
#[cfg(feature = "dynlex-experimental")]
mod native_text_scene;
mod pdf_save;
mod reader_mode;
mod recent_files;
use recent_files::RecentFile;
mod text_layout;
mod virtual_ipp;
mod xfdf_export;
mod xfdf_import;
mod zoom_area;
use iced::widget::{image, scrollable};
use iced::{color, window, Element, Task, Theme};
use opds_dynlex_backend::{Backend, RenderOutcome};
use opds_engine_probe::{ParsedPdf, Raster};
use opds_native_core::annotations::{
    Annotation, CommandError, DocumentCommand, DocumentModel, DocumentVersion, Edit, Kind, Rect,
    TextAlign, DEFAULT_TEXT_FONT_SIZE, MAX_POLYLINE_POINTS, TEXT_FONT_SIZES,
};
use opds_native_core::{DocumentId, RenderTicket};
use std::cell::RefCell;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn render_page_selected(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
) -> Result<RenderOutcome, String> {
    opds_dynlex_backend::render_selected_strict(pdf, source, page, scale, backend)
}

// A fit scale can be outside DynLex's proven subset even when the same page
// renders at 100%. Keep startup on the selected native backend in both cases.
fn render_initial_dynlex_page(
    pdf: &ParsedPdf,
    document: &opds_dynlex_backend::DynlexDocument,
    page: usize,
    fit: f32,
) -> (f32, Result<RenderOutcome, String>) {
    match document.render_strict(pdf, page, fit) {
        Ok(outcome) => (fit, Ok(outcome)),
        Err(fit_error) if fit != 1.0 => match document.render_strict(pdf, page, 1.0) {
            Ok(outcome) => (1.0, Ok(outcome)),
            Err(_) => (fit, Err(fit_error)),
        },
        Err(error) => (fit, Err(error)),
    }
}

fn render_initial_selected_page(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
) -> (f32, Result<RenderOutcome, String>) {
    match render_page_selected(pdf, source, page, scale, backend) {
        Ok(outcome) => (scale, Ok(outcome)),
        Err(first_error) if scale != 1.0 && backend != Backend::Hayro => {
            match render_page_selected(pdf, source, page, 1.0, backend) {
                Ok(outcome) => (1.0, Ok(outcome)),
                Err(_) => (scale, Err(first_error)),
            }
        }
        Err(error) => (scale, Err(error)),
    }
}

#[derive(Clone)]
struct DynlexOpen(Arc<opds_dynlex_backend::DynlexDocument>);

impl std::fmt::Debug for DynlexOpen {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DynlexOpen")
    }
}

#[cfg(all(test, feature = "dynlex-experimental"))]
/// Only call after `eligible_mixed_page` has proved that the source uses
/// normal opaque paints without resources, groups, or imported annotations.
/// The visible page is reconstructed from Hayro's premultiplied content.
fn render_page_with_content_layer(
    pdf: &ParsedPdf,
    page: usize,
    scale: f32,
) -> Result<(RenderOutcome, Raster), String> {
    let layer = pdf
        .render_page_content_layer(page, scale)
        .map_err(|error| error.to_string())?;
    let composition_started = std::time::Instant::now();
    let mut visible = layer.clone();
    for pixel in visible.rgba.chunks_exact_mut(4) {
        let paper = 255 - pixel[3];
        for channel in &mut pixel[..3] {
            *channel = channel.saturating_add(paper);
        }
        pixel[3] = 255;
    }
    visible.render_ms += composition_started.elapsed().as_millis();
    Ok((
        RenderOutcome {
            raster: visible,
            used: Backend::Hayro,
            imported_annotations_dynlex: false,
        },
        layer,
    ))
}

const MAX_PAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_THUMBNAILS: usize = 12;
const THUMBNAIL_WIDTH: f32 = 140.0;
const THUMBNAIL_HEIGHT: f32 = 128.0;
const THUMBNAIL_ROW_STEP: f32 = 144.0;
const THUMBNAIL_CARD_HEIGHT: f32 = 140.0;
const THUMBNAIL_HEADER_HEIGHT: f32 = 30.0;
const THUMBNAIL_WINDOW_SIZE: usize = 12;
const BROWSER_ROW_STEP: f32 = 36.0;
const BROWSER_WINDOW_SIZE: usize = 20;
const HORIZONTAL_PAN_MARGIN: f32 = 512.0;
const VERTICAL_PAN_MARGIN: f32 = 512.0;
const PAGE_PADDING: f32 = 24.0;

fn thumbnail_scale(width: f32, height: f32) -> Result<f32, String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("Invalid PDF page dimensions".into());
    }
    let scale = (THUMBNAIL_WIDTH / width)
        .min(THUMBNAIL_HEIGHT / height)
        .min(1.0);
    if scale < 0.01 {
        return Err("Page is too large for a thumbnail".into());
    }
    Ok(scale)
}

fn line_bounds(start: [f64; 2], end: [f64; 2], page: (f32, f32)) -> Option<Rect> {
    let size = [f64::from(page.0), f64::from(page.1)];
    if !start
        .iter()
        .chain(end.iter())
        .all(|value| value.is_finite())
        || !size.iter().all(|value| value.is_finite() && *value > 0.0)
        || (start[0] - end[0]).hypot(start[1] - end[1]) < 1.0
        || [start, end].iter().any(|point| {
            point[0] < 0.0 || point[1] < 0.0 || point[0] > size[0] || point[1] > size[1]
        })
    {
        return None;
    }
    let left = (start[0].min(end[0]) - 2.0).max(0.0);
    let top = (start[1].min(end[1]) - 2.0).max(0.0);
    let right = (start[0].max(end[0]) + 2.0).min(size[0]);
    let bottom = (start[1].max(end[1]) + 2.0).min(size[1]);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn arrow_bounds(start: [f64; 2], end: [f64; 2], page: (f32, f32)) -> Option<Rect> {
    let wings = opds_native_core::annotations::arrow_wings(start, end)?;
    let page = [f64::from(page.0), f64::from(page.1)];
    if [start, end, wings[0], wings[1]].iter().any(|point| {
        !point.iter().all(|value| value.is_finite())
            || point[0] < 0.0
            || point[1] < 0.0
            || point[0] > page[0]
            || point[1] > page[1]
    }) {
        return None;
    }
    let points = [start, end, wings[0], wings[1]];
    let left = (points
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min)
        - 4.0)
        .max(0.0);
    let top = (points
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min)
        - 4.0)
        .max(0.0);
    let right = (points
        .iter()
        .map(|point| point[0])
        .fold(f64::NEG_INFINITY, f64::max)
        + 4.0)
        .min(page[0]);
    let bottom = (points
        .iter()
        .map(|point| point[1])
        .fold(f64::NEG_INFINITY, f64::max)
        + 4.0)
        .min(page[1]);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn polyline_bounds(points: &[[f64; 2]], page: (f32, f32)) -> Option<Rect> {
    let size = [f64::from(page.0), f64::from(page.1)];
    if !(2..=MAX_POLYLINE_POINTS).contains(&points.len())
        || !size.iter().all(|value| value.is_finite() && *value > 0.0)
        || points.iter().any(|point| {
            !point.iter().all(|value| value.is_finite())
                || point[0] < 0.0
                || point[1] < 0.0
                || point[0] > size[0]
                || point[1] > size[1]
        })
        || points
            .windows(2)
            .any(|pair| (pair[0][0] - pair[1][0]).hypot(pair[0][1] - pair[1][1]) < 1.0)
    {
        return None;
    }
    let left = (points
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min)
        - 2.0)
        .max(0.0);
    let top = (points
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min)
        - 2.0)
        .max(0.0);
    let right = (points
        .iter()
        .map(|point| point[0])
        .fold(f64::NEG_INFINITY, f64::max)
        + 2.0)
        .min(size[0]);
    let bottom = (points
        .iter()
        .map(|point| point[1])
        .fold(f64::NEG_INFINITY, f64::max)
        + 2.0)
        .min(size[1]);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn thumbnail_page_at_scroll(y: f32, page_count: usize) -> usize {
    if page_count == 0 {
        return 0;
    }
    ((y - THUMBNAIL_HEADER_HEIGHT).max(0.0) / THUMBNAIL_ROW_STEP)
        .floor()
        .min((page_count - 1) as f32) as usize
}

fn thumbnail_window(focus: usize, page_count: usize) -> std::ops::Range<usize> {
    let start = focus
        .saturating_sub(3)
        .min(page_count.saturating_sub(THUMBNAIL_WINDOW_SIZE));
    start..start.saturating_add(THUMBNAIL_WINDOW_SIZE).min(page_count)
}

#[derive(Debug, Clone)]
struct BrowserEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

fn list_pdf_directory(path: &Path) -> Result<Vec<BrowserEntry>, String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let is_dir = file_type.is_dir();
        let is_pdf = file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
        if is_dir || is_pdf {
            entries.push(BrowserEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                is_dir,
            });
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

fn visible_browser_indices(entries: &[BrowserEntry], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (query.is_empty() || entry.name.to_lowercase().contains(&query)).then_some(index)
        })
        .collect()
}

fn browser_window(scroll_y: f32, visible_count: usize) -> std::ops::Range<usize> {
    let focus = (scroll_y.max(0.0) / BROWSER_ROW_STEP).floor() as usize;
    let start = focus
        .saturating_sub(3)
        .min(visible_count.saturating_sub(BROWSER_WINDOW_SIZE));
    start..start.saturating_add(BROWSER_WINDOW_SIZE).min(visible_count)
}

fn annotations_changed(model: &DocumentModel, saved: &[Annotation]) -> bool {
    !model.items().eq(saved.iter())
}

#[cfg(feature = "dynlex-experimental")]
fn single_text_index(annotations: &[Annotation]) -> Option<(usize, &Annotation)> {
    let mut texts = annotations
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item.kind, Kind::Text { .. }));
    let text = texts.next()?;
    if texts.next().is_some() {
        return None;
    }
    Some(text)
}

#[cfg(feature = "dynlex-experimental")]
fn single_disjoint_text(annotations: &[Annotation]) -> Option<&Annotation> {
    let (_, text) = single_text_index(annotations)?;
    for item in annotations.iter().filter(|item| item.id != text.id) {
        if !(item.bounds.x + item.bounds.width <= text.bounds.x
            || text.bounds.x + text.bounds.width <= item.bounds.x
            || item.bounds.y + item.bounds.height <= text.bounds.y
            || text.bounds.y + text.bounds.height <= item.bounds.y)
        {
            return None;
        }
    }
    Some(text)
}

fn theme(_: &App) -> Theme {
    Theme::custom(
        "Open PDF Studio",
        iced::theme::Palette {
            background: color!(0x28282b),
            text: color!(0xd7d7db),
            primary: color!(0xd97706),
            success: color!(0x4caf50),
            warning: color!(0xffb300),
            danger: color!(0xe53935),
        },
    )
}

fn initial_window_size() -> iced::Size {
    if std::env::var_os("OPDS_UI_SCREENSHOT_RGBA").is_some() {
        if let Ok(value) = std::env::var("OPDS_UI_SCREENSHOT_SIZE") {
            if let Some((width, height)) = value.split_once('x') {
                if let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>()) {
                    if (760..=4096).contains(&width) && (500..=2160).contains(&height) {
                        return iced::Size::new(width as f32, height as f32);
                    }
                }
            }
        }
    }
    iced::Size::new(1200.0, 832.0)
}

fn boot() -> (App, Task<Message>) {
    let mut app = App::default();
    if !cfg!(test) && std::env::var_os("OPDS_UI_SCREENSHOT_RGBA").is_none() {
        app.recent_store_path = recent_files::default_path();
        if let Some(path) = &app.recent_store_path {
            app.recent_files = recent_files::read(path);
        }
    }
    let screenshot_task = if std::env::var_os("OPDS_UI_SCREENSHOT_RGBA").is_some() {
        app.file_menu_open = std::env::var("OPDS_UI_SCREENSHOT_FILE_MENU")
            .ok()
            .as_deref()
            == Some("1");
        if std::env::var("OPDS_UI_SCREENSHOT_DRAWING_TAB")
            .ok()
            .as_deref()
            == Some("1")
        {
            app.active_ribbon = RibbonTab::Drawing;
        }
        if std::env::var("OPDS_UI_SCREENSHOT_ANNOTATION_TAB")
            .ok()
            .as_deref()
            == Some("1")
        {
            app.active_ribbon = RibbonTab::Comment;
        }
        app.about_dialog = std::env::var("OPDS_UI_SCREENSHOT_ABOUT").ok().as_deref() == Some("1");
        if std::env::var("OPDS_UI_SCREENSHOT_EXTENSIONS")
            .ok()
            .as_deref()
            == Some("1")
        {
            app.extensions_dialog = true;
            app.extension_id = "example_extension".into();
            app.extension_name = "Example extension".into();
            app.extension_output = "/tmp/example_extension.oppx".into();
            app.extension_html =
                iced::widget::text_editor::Content::with_text(extension_builder::default_html());
            app.extension_javascript = iced::widget::text_editor::Content::with_text(
                extension_builder::default_javascript(),
            );
        }
        if std::env::var("OPDS_UI_SCREENSHOT_EXTENSION_IMPORT")
            .ok()
            .as_deref()
            == Some("1")
        {
            app.extension_import_dialog = true;
        }
        app.hide_properties = std::env::var("OPDS_UI_SCREENSHOT_COLLAPSED_PROPERTIES")
            .ok()
            .as_deref()
            == Some("1");
        Task::perform(
            async { std::thread::sleep(Duration::from_millis(800)) },
            |()| Message::CaptureUiScreenshot,
        )
    } else {
        Task::none()
    };
    if cfg!(feature = "dynlex-experimental")
        && std::env::var("OPDS_RENDER_BACKEND").ok().as_deref() == Some("dynlex")
    {
        app.backend = Backend::Dynlex;
    }
    if cfg!(feature = "pdfium-experimental")
        && opds_dynlex_backend::pdfium_available()
        && std::env::var("OPDS_RENDER_BACKEND").ok().as_deref() == Some("pdfium")
    {
        app.backend = Backend::Pdfium;
    }
    if cfg!(feature = "own-rust-experimental")
        && std::env::var("OPDS_RENDER_BACKEND").ok().as_deref() == Some("own-rust")
    {
        app.backend = Backend::OwnRust;
    }
    if opds_dynlex_backend::pdfcraft_available()
        && std::env::var("OPDS_RENDER_BACKEND").ok().as_deref() == Some("pdfcraft")
    {
        app.backend = Backend::PdfCraft;
    }
    mcp_server::publish_backend(app.backend);
    app.text_font_size = DEFAULT_TEXT_FONT_SIZE;
    let initial_size = initial_window_size();
    app.window_size = (initial_size.width, initial_size.height);
    if let Some(path) = mcp_server::document_argument() {
        app.path_input = path;
        let task = app.update(Message::Open);
        (app, Task::batch([task, screenshot_task]))
    } else {
        (app, screenshot_task)
    }
}

fn main() -> iced::Result {
    if let Some(port) = mcp_server::requested_port() {
        if let Err(error) = mcp_server::start(port) {
            eprintln!("MCP server: {error}");
            std::process::exit(2);
        }
    }
    let app_icon = {
        let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
            "../../../assets/ui/app-icon-png.bin"
        )));
        decoder.read_info().ok().and_then(|mut reader| {
            let mut rgba = vec![0; reader.output_buffer_size()];
            let frame = reader.next_frame(&mut rgba).ok()?;
            (frame.color_type == png::ColorType::Rgba)
                .then(|| iced::window::icon::from_rgba(rgba, frame.width, frame.height).ok())
                .flatten()
        })
    };
    iced::application(boot, App::update, App::view)
        .title("Open PDF Studio Rust")
        .theme(theme)
        .subscription(|_| {
            // Keyboard commands are needed even while the optional screencast
            // labels are hidden. The label recorder itself remains opt-in.
            iced::Subscription::batch([
                iced::event::listen_with(window_event_to_message),
                iced::event::listen_with(keystroke_event_to_message),
                iced::Subscription::run(mcp_server::request_stream).map(Message::ExternalRequest),
            ])
        })
        .exit_on_close_request(false)
        .font(include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf").as_slice())
        .window(window::Settings {
            size: initial_window_size(),
            min_size: Some(iced::Size::new(760.0, 500.0)),
            decorations: false,
            icon: app_icon,
            ..Default::default()
        })
        .run()
}

fn window_event_to_message(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::CloseRequested) => Some(Message::CloseWindow),
        iced::Event::Window(window::Event::Unfocused) => Some(Message::KeyboardModifiersChanged(
            iced::keyboard::Modifiers::default(),
        )),
        iced::Event::Window(window::Event::Opened { size, .. })
        | iced::Event::Window(window::Event::Resized(size)) => {
            Some(Message::WindowResized((size.width, size.height)))
        }
        iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::KeyboardModifiersChanged(modifiers))
        }
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            repeat: false,
            ..
        }) => Some(Message::EscapeOverlay),
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat: false,
            ..
        }) if matches!(
            key,
            iced::keyboard::Key::Named(iced::keyboard::key::Named::F11)
        ) || (modifiers.control()
            && matches!(key, iced::keyboard::Key::Character(ref c) if c.eq_ignore_ascii_case("l"))) =>
        {
            Some(Message::ToggleFullscreen)
        }
        _ => None,
    }
}

fn keystroke_event_to_message(
    event: iced::Event,
    status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat: false,
            ..
        }) => Some(Message::KeystrokePressed(
            key,
            modifiers,
            status == iced::event::Status::Captured,
        )),
        _ => None,
    }
}

fn keystroke_timeout(delay: Duration) -> Task<Message> {
    // One timer at a time, even when keys are pressed rapidly. The thread-pool
    // Iced backend does not provide a time::every subscription.
    Task::perform(async move { std::thread::sleep(delay) }, |()| {
        Message::KeystrokesExpired
    })
}

fn zoom_timeout(delay: Duration) -> Task<Message> {
    Task::perform(async move { std::thread::sleep(delay) }, |()| {
        Message::ZoomRenderDue
    })
}

fn zoom_render_delay(timer_active: bool) -> Duration {
    Duration::from_millis(if timer_active { 160 } else { 24 })
}

fn zoom_preview_size(
    page_points: (f32, f32),
    fallback: (u32, u32),
    wanted_scale: f64,
) -> (u32, u32) {
    if fallback.0 == 0
        || fallback.1 == 0
        || !wanted_scale.is_finite()
        || wanted_scale <= 0.0
        || !page_points.0.is_finite()
        || !page_points.1.is_finite()
        || page_points.0 <= 0.0
        || page_points.1 <= 0.0
    {
        return fallback;
    }
    // Page geometry determines display size independently of the pending or
    // final raster dimensions. Otherwise rounding when a sharp bitmap arrives
    // moves the anchor and accumulates drift over repeated zoom-in/out cycles.
    let scaled = |length: f32| (length * wanted_scale as f32).floor().clamp(1.0, 32_768.0) as u32;
    (scaled(page_points.0), scaled(page_points.1))
}

// The wheel handler converts its viewport anchor to a PDF-image-local point.
// Use rounded preview dimensions so repeated events do not accumulate drift.
fn zoom_scroll_offset(
    current: scrollable::AbsoluteOffset,
    anchor: [f32; 2],
    old: (u32, u32),
    new: (u32, u32),
) -> scrollable::AbsoluteOffset {
    let axis = |offset: f32, point: f32, before: u32, after: u32| {
        if before == 0 {
            offset
        } else {
            (offset + point * (after as f32 / before as f32 - 1.0)).max(0.0)
        }
    };
    scrollable::AbsoluteOffset {
        x: axis(current.x, anchor[0], old.0, new.0),
        y: axis(current.y, anchor[1], old.1, new.1),
    }
}

fn thumbnail_candidates(focus: usize, count: usize) -> impl Iterator<Item = usize> {
    let window = thumbnail_window(focus, count);
    let focus = focus.clamp(window.start, window.end.saturating_sub(1).max(window.start));
    (focus..window.end).chain((window.start..focus).rev())
}

fn next_thumbnail(document: &OpenDocument) -> Option<usize> {
    let window = thumbnail_window(document.thumbnail_focus, document.pdf.page_count());
    // Fill the complete virtual window, not only two neighbours of its first
    // visible row: at the bottom those neighbours omit the final pages.
    if window.is_empty() {
        return None;
    }
    thumbnail_candidates(document.thumbnail_focus, document.pdf.page_count()).find(|&page| {
        !(page == document.displayed_page && document.bitmap.is_some())
            && !document
                .thumbnails
                .iter()
                .any(|(cached, _)| *cached == page)
            && !document.thumbnail_failed.contains(&page)
    })
}

fn scroll_page_to(offset: scrollable::AbsoluteOffset) -> Task<Message> {
    iced::widget::operation::scroll_to(iced::widget::Id::new("pdf-page-scroll"), offset)
}

fn scroll_thumbnail_to(page: usize) -> Task<Message> {
    iced::widget::operation::scroll_to(
        iced::widget::Id::new("pdf-thumbnails"),
        scrollable::AbsoluteOffset {
            x: 0.0,
            y: THUMBNAIL_HEADER_HEIGHT + page as f32 * THUMBNAIL_ROW_STEP,
        },
    )
}

fn scroll_browser_to(y: f32) -> Task<Message> {
    iced::widget::operation::scroll_to(
        iced::widget::Id::new("pdf-file-browser"),
        scrollable::AbsoluteOffset { x: 0.0, y },
    )
}

fn remember_recent(recent_files: &mut VecDeque<RecentFile>, path: &str) {
    if path.is_empty() || path.len() > crate::recent_files::MAX_PATH_BYTES || path.contains('\0') {
        return;
    }
    let pinned = recent_files
        .iter()
        .find(|recent| recent.path == path)
        .is_some_and(|recent| recent.pinned);
    recent_files.retain(|recent| recent.path != path);
    recent_files.push_front(RecentFile {
        path: path.to_owned(),
        pinned,
    });
    while recent_files.len() > crate::recent_files::MAX_ENTRIES {
        if let Some(index) = recent_files.iter().rposition(|entry| !entry.pinned) {
            recent_files.remove(index);
        } else {
            recent_files.pop_back();
        }
    }
}

fn recent_file_matches(path: &str, query: &str) -> bool {
    path.to_lowercase().contains(&query.trim().to_lowercase())
}

// Tauri's Drawing > Copy duplicates the selected annotation 20 screen pixels
// down and right. Keep the offset in page coordinates and ensure a new native
// annotation can still be exported when the selection touches a page edge.
fn duplicate_annotation(
    source: &Annotation,
    id: u64,
    scale: f64,
    page_size: (f32, f32),
) -> Option<Annotation> {
    copy_annotation_with_offset(source, id, scale, page_size, 20.0)
}

fn copy_annotation_with_offset(
    source: &Annotation,
    id: u64,
    scale: f64,
    page_size: (f32, f32),
    offset_px: f64,
) -> Option<Annotation> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let step = offset_px / scale;
    let (width, height) = (f64::from(page_size.0), f64::from(page_size.1));
    let bounds = source.bounds;
    if ![
        step,
        width,
        height,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
    ]
    .iter()
    .all(|value| value.is_finite())
        || step < 0.0
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.x + bounds.width > width
        || bounds.y + bounds.height > height
    {
        return None;
    }
    let shift = |start: f64, extent: f64, limit: f64| {
        if start + extent + step <= limit {
            Some(step)
        } else if start >= step {
            Some(-step)
        } else {
            None
        }
    };
    let dx = shift(bounds.x, bounds.width, width)?;
    let dy = shift(bounds.y, bounds.height, height)?;
    let mut copy = source.clone();
    copy.id = id;
    copy.bounds.x += dx;
    copy.bounds.y += dy;
    match &mut copy.kind {
        Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => {
            for point in [start, end] {
                point[0] += dx;
                point[1] += dy;
            }
        }
        Kind::Polyline { points, .. } => {
            for point in points {
                point[0] += dx;
                point[1] += dy;
            }
        }
        _ => {}
    }
    Some(copy)
}

// The GUI drag and external move must use identical page-point geometry.
// Keep line/polyline points and their bounds in sync before the shared
// DocumentCommand validation and undo history see the replacement.
fn translated_annotation(
    source: &Annotation,
    delta: [f64; 2],
    page_size: (f32, f32),
) -> Option<Annotation> {
    let (width, height) = (f64::from(page_size.0), f64::from(page_size.1));
    if !delta.iter().all(|value| value.is_finite())
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }
    let mut item = source.clone();
    let previous = item.bounds;
    let max_x = (width - previous.width).max(0.0);
    let max_y = (height - previous.height).max(0.0);
    item.bounds.x = (previous.x + delta[0]).clamp(0.0, max_x);
    item.bounds.y = (previous.y + delta[1]).clamp(0.0, max_y);
    if item.bounds == previous {
        return None;
    }
    let actual = [item.bounds.x - previous.x, item.bounds.y - previous.y];
    match &mut item.kind {
        Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => {
            for point in [start, end] {
                point[0] += actual[0];
                point[1] += actual[1];
            }
        }
        Kind::Polyline { points, .. } => {
            for point in points {
                point[0] += actual[0];
                point[1] += actual[1];
            }
        }
        _ => {}
    }
    Some(item)
}

// The resize handles and external API share one bounded replacement. Point
// geometry cannot be resized by changing the rectangle alone.
fn resized_annotation(
    source: &Annotation,
    bounds: Rect,
    page_size: (f32, f32),
) -> Option<Annotation> {
    let (page_width, page_height) = (f64::from(page_size.0), f64::from(page_size.1));
    if ![
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        page_width,
        page_height,
    ]
    .iter()
    .all(|value| value.is_finite())
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.x + bounds.width > page_width + 1e-6
        || bounds.y + bounds.height > page_height + 1e-6
        || bounds == source.bounds
        || matches!(
            source.kind,
            Kind::Line { .. } | Kind::Arrow { .. } | Kind::Polyline { .. }
        )
    {
        return None;
    }
    let mut item = source.clone();
    item.bounds = bounds;
    Some(item)
}

fn annotation_color(kind: &Kind) -> [u8; 3] {
    match kind {
        Kind::Rectangle { color_rgb }
        | Kind::Ellipse { color_rgb }
        | Kind::Polygon { color_rgb }
        | Kind::Line { color_rgb, .. }
        | Kind::Arrow { color_rgb, .. }
        | Kind::Polyline { color_rgb, .. }
        | Kind::Text { color_rgb, .. } => *color_rgb,
    }
}

fn recolored_annotation(source: &Annotation, color: [u8; 3]) -> Option<Annotation> {
    if annotation_color(&source.kind) == color {
        return None;
    }
    let mut item = source.clone();
    match &mut item.kind {
        Kind::Rectangle { color_rgb }
        | Kind::Ellipse { color_rgb }
        | Kind::Polygon { color_rgb }
        | Kind::Line { color_rgb, .. }
        | Kind::Arrow { color_rgb, .. }
        | Kind::Polyline { color_rgb, .. }
        | Kind::Text { color_rgb, .. } => *color_rgb = color,
    }
    Some(item)
}

// Reflect native point geometry about its own bounds. The current annotation
// schema has no flip flags for stamps/text/shapes, so only point-based kinds
// can be mirrored without claiming a transform that export cannot preserve.
fn mirror_annotation(
    source: &Annotation,
    horizontal: bool,
    page_size: (f32, f32),
) -> Option<Annotation> {
    let coordinate = usize::from(!horizontal);
    let axis_sum = match &source.kind {
        Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => {
            start[coordinate] + end[coordinate]
        }
        Kind::Polyline { points, .. } if points.len() >= 2 => {
            let min = points
                .iter()
                .map(|point| point[coordinate])
                .reduce(f64::min)?;
            let max = points
                .iter()
                .map(|point| point[coordinate])
                .reduce(f64::max)?;
            min + max
        }
        _ => return None,
    };
    if !axis_sum.is_finite() {
        return None;
    }
    let mut result = source.clone();
    match &mut result.kind {
        Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => {
            for point in [start, end] {
                point[coordinate] = axis_sum - point[coordinate];
            }
        }
        Kind::Polyline { points, .. } => {
            for point in points {
                point[coordinate] = axis_sum - point[coordinate];
            }
        }
        _ => return None,
    }
    if result.kind == source.kind {
        return None;
    }
    result.bounds = match &result.kind {
        Kind::Line { start, end, .. } => line_bounds(*start, *end, page_size)?,
        Kind::Arrow { start, end, .. } => arrow_bounds(*start, *end, page_size)?,
        Kind::Polyline { points, .. } => polyline_bounds(points, page_size)?,
        _ => unreachable!(),
    };
    Some(result)
}

fn rotate_annotation_clockwise_90(
    source: &Annotation,
    page_size: (f32, f32),
) -> Option<Annotation> {
    let center = match &source.kind {
        Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => {
            [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0]
        }
        Kind::Polyline { points, .. } if points.len() >= 2 => {
            let min_x = points.iter().map(|point| point[0]).reduce(f64::min)?;
            let max_x = points.iter().map(|point| point[0]).reduce(f64::max)?;
            let min_y = points.iter().map(|point| point[1]).reduce(f64::min)?;
            let max_y = points.iter().map(|point| point[1]).reduce(f64::max)?;
            [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0]
        }
        _ => return None,
    };
    if !center.iter().all(|value| value.is_finite()) {
        return None;
    }
    let rotate = |point: &mut [f64; 2]| {
        let dx = point[0] - center[0];
        let dy = point[1] - center[1];
        *point = [center[0] - dy, center[1] + dx];
    };
    let mut result = source.clone();
    match &mut result.kind {
        Kind::Line { start, end, .. } | Kind::Arrow { start, end, .. } => {
            rotate(start);
            rotate(end);
        }
        Kind::Polyline { points, .. } => {
            for point in points {
                rotate(point);
            }
        }
        _ => unreachable!(),
    }
    result.bounds = match &result.kind {
        Kind::Line { start, end, .. } => line_bounds(*start, *end, page_size)?,
        Kind::Arrow { start, end, .. } => arrow_bounds(*start, *end, page_size)?,
        Kind::Polyline { points, .. } => polyline_bounds(points, page_size)?,
        _ => unreachable!(),
    };
    Some(result)
}

fn export_page_path(
    document_path: &str,
    page_number: usize,
    format: image_export::ImageFormat,
) -> String {
    let source_path = Path::new(document_path);
    let stem = source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    source_path
        .with_file_name(format!("{stem}-page-{page_number}.{}", format.extension()))
        .to_string_lossy()
        .into_owned()
}

fn export_pages_folder(document_path: &str, format: image_export::ImageFormat) -> String {
    let source_path = Path::new(document_path);
    let stem = source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let suffix = match format {
        image_export::ImageFormat::Png => "images",
        image_export::ImageFormat::Jpeg => "jpeg-images",
        image_export::ImageFormat::Tiff => "tiff-images",
    };
    source_path
        .with_file_name(format!("{stem}-{suffix}"))
        .to_string_lossy()
        .into_owned()
}

#[derive(Default)]
struct App {
    backend: Backend,
    backend_generation: u64,
    dynlex_switch_retry: Option<(u64, RenderTicket)>,
    path_input: String,
    recent_files: VecDeque<RecentFile>,
    recent_query: String,
    pinned_recents_collapsed: bool,
    unpinned_recents_collapsed: bool,
    recent_store_path: Option<PathBuf>,
    browser_dir: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    browser_busy: bool,
    browser_error: String,
    browser_query: String,
    browser_scroll_y: f32,
    next_browse: u64,
    active_ribbon: RibbonTab,
    document: Option<OpenDocument>,
    clipboard_annotation: Option<Annotation>,
    clipboard_paste_count: u32,
    next_load: u64,
    busy: bool,
    opening_document: bool,
    render_queued: bool,
    zoom_render_due: Option<Instant>,
    zoom_timer_active: bool,
    message: String,
    file_menu_open: bool,
    about_dialog: bool,
    extensions_dialog: bool,
    extension_import_dialog: bool,
    extension_id: String,
    extension_name: String,
    extension_output: String,
    extension_import_path: String,
    extension_html: iced::widget::text_editor::Content,
    extension_javascript: iced::widget::text_editor::Content,
    extension_error: String,
    open_dialog: bool,
    new_dialog: bool,
    new_paper: blank_pdf::Paper,
    new_landscape: bool,
    new_pages_input: String,
    new_custom_width_mm: String,
    new_custom_height_mm: String,
    new_error: String,
    confirm_discard: Option<PendingAction>,
    pending_delete_annotation: Option<(DocumentVersion, u64)>,
    pending_clear_annotations: Option<(DocumentVersion, Option<usize>)>,
    allow_discard_once: bool,
    save_dialog: bool,
    save_path_input: String,
    saved_target: Option<(String, u64)>,
    save_busy: bool,
    save_scroll_offset: Option<scrollable::AbsoluteOffset>,
    import_dialog: bool,
    import_path_input: String,
    xfdf_export_dialog: bool,
    xfdf_export_path_input: String,
    export_dialog: bool,
    export_path_input: String,
    export_page: usize,
    export_page_input: String,
    export_range: image_export::ImagePageRange,
    export_custom_pages: String,
    export_dpi: image_export::ImageDpi,
    export_format: image_export::ImageFormat,
    export_jpeg_quality: image_export::JpegQuality,
    export_include_annotations: bool,
    export_busy: bool,
    export_cancel: Option<Arc<AtomicBool>>,
    export_completed: usize,
    export_total: usize,
    export_scroll_offset: Option<scrollable::AbsoluteOffset>,
    print_dialog: bool,
    print_path_input: String,
    print_busy: bool,
    print_scroll_offset: Option<scrollable::AbsoluteOffset>,
    print_destination: PrintDestination,
    print_range: PrintRange,
    print_subset: PrintSubset,
    print_reverse: bool,
    print_custom_pages: String,
    print_paper: pdf_save::PrintPaper,
    print_scale: pdf_save::PrintScale,
    print_zoom_percent: u16,
    print_zoom_draft: String,
    print_preview: Option<image::Handle>,
    print_preview_base_bytes: Option<Arc<Vec<u8>>>,
    print_preview_base_pdf: Option<Arc<ParsedPdf>>,
    print_preview_pdf: Option<Arc<ParsedPdf>>,
    print_preview_page: usize,
    print_preview_index: usize,
    print_preview_revision: u64,
    print_preview_document_revision: u64,
    print_preview_error: String,
    text_dialog: bool,
    text_draft: iced::widget::text_editor::Content,
    text_color: [u8; 3],
    text_font_size: u16,
    text_align: TextAlign,
    pending_text_bounds: Option<Rect>,
    editing_text_id: Option<u64>,
    text_dialog_version: Option<DocumentVersion>,
    text_scroll_offset: Option<scrollable::AbsoluteOffset>,
    hide_pages: bool,
    hide_properties: bool,
    hide_symbol_palette: bool,
    properties_collapsed: [bool; 3],
    show_annotation_list: bool,
    reader_mode_active: bool,
    reader_shutdown: ReaderShutdown,
    fullscreen: bool,
    keystrokes: keystrokes::Keystrokes,
    ribbon_collapsed: bool,
    annotation_tool: AnnotationTool,
    annotation_tool_epoch: u64,
    annotation_gesture_active: bool,
    annotation_gesture_preview: Option<AnnotationGesturePreview>,
    annotation_tool_activated: bool,
    next_annotation_id: u64,
    page_scroll_offset: scrollable::AbsoluteOffset,
    pan_drag: Option<PanDrag>,
    pending_zoom_offset: Option<scrollable::AbsoluteOffset>,
    keyboard_modifiers: iced::keyboard::Modifiers,
    window_size: (f32, f32),
    window_maximized: bool,
}

#[derive(Default)]
struct ReaderShutdown {
    position: Option<(PathBuf, reader_mode::Position)>,
}

impl Drop for ReaderShutdown {
    fn drop(&mut self) {
        // Some window managers close the window without delivering our
        // CloseRequested message. Preserve the last validated view then.
        if let Some((path, position)) = &self.position {
            let _ = reader_mode::write(path, *position);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum AnnotationTool {
    #[default]
    Select,
    Hand,
    Rectangle,
    Circle,
    Ellipse,
    Polygon,
    Freehand,
    Line,
    Arrow,
    Polyline,
    Highlight,
    Text,
}

impl AnnotationTool {
    fn from_mcp_name(name: &str) -> Option<Self> {
        Some(match name {
            "select" => Self::Select,
            "hand" => Self::Hand,
            "box" => Self::Rectangle,
            "circle" => Self::Circle,
            "ellipse" => Self::Ellipse,
            "polygon" => Self::Polygon,
            "freehand" => Self::Freehand,
            "line" => Self::Line,
            "arrow" => Self::Arrow,
            "polyline" => Self::Polyline,
            "highlight" => Self::Highlight,
            "textbox" => Self::Text,
            _ => return None,
        })
    }

    fn mcp_name(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Hand => "hand",
            Self::Rectangle => "box",
            Self::Circle => "circle",
            Self::Ellipse => "ellipse",
            Self::Polygon => "polygon",
            Self::Freehand => "freehand",
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::Polyline => "polyline",
            Self::Highlight => "highlight",
            Self::Text => "textbox",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PanDrag {
    button: iced::mouse::Button,
    start: [f32; 2],
    scroll_start: scrollable::AbsoluteOffset,
}

fn dragged_scroll_offset(drag: PanDrag, pointer: [f32; 2]) -> scrollable::AbsoluteOffset {
    scrollable::AbsoluteOffset {
        x: (drag.scroll_start.x - (pointer[0] - drag.start[0])).max(0.0),
        y: (drag.scroll_start.y - (pointer[1] - drag.start[1])).max(0.0),
    }
}

#[derive(Debug, Clone, Copy)]
enum FitMode {
    Width,
    Page,
}

// Tauri's docked ToolPalette switches to one column when its content area
// reaches the 635 px small-icon threshold. Keep page fitting and centering in
// sync with the actual palette width when the window is maximized.
fn drawing_palette_two_columns(window_height: f32, ribbon_collapsed: bool) -> bool {
    let workspace_height = window_height
        - 30.0 // title bar
        - 28.0 // ribbon tabs
        - if ribbon_collapsed { 0.0 } else { 94.0 }
        - 28.0 // document tabs
        - 22.0; // status bar
    workspace_height - 46.0 < 635.0
}

fn drawing_palette_width(window_height: f32, ribbon_collapsed: bool) -> f32 {
    if drawing_palette_two_columns(window_height, ribbon_collapsed) {
        80.0
    } else {
        44.0
    }
}

fn fit_scale(
    mode: FitMode,
    page_size: (f32, f32),
    window_size: (f32, f32),
    pages_visible: bool,
    properties_visible: bool,
    symbol_palette_visible: bool,
    ribbon_collapsed: bool,
) -> Option<f64> {
    let available_width = window_size.0
        - 32.0 // left icon rail
        - if pages_visible { 172.0 } else { 0.0 } // pages + resize handle
        - if properties_visible { 300.0 } else { 28.0 }
        - drawing_palette_width(window_size.1, ribbon_collapsed)
        - if symbol_palette_visible { 220.0 } else { 0.0 };
    let available_height = window_size.1
        - 30.0 // title bar
        - 28.0 // ribbon tabs
        - if ribbon_collapsed { 0.0 } else { 94.0 }
        - 28.0 // document tabs
        - 22.0; // status bar; Tauri's fit math uses zero page padding
    if page_size.0 <= 0.0 || page_size.1 <= 0.0 || available_width <= 0.0 || available_height <= 0.0
    {
        return None;
    }
    let width_scale = available_width / page_size.0;
    let scale = match mode {
        FitMode::Width => width_scale,
        FitMode::Page => width_scale.min(available_height / page_size.1),
    };
    scale
        .is_finite()
        .then_some(f64::from(scale).clamp(0.1, 8.0))
}

fn centered_page_offset(
    window_size: (f32, f32),
    bitmap_size: (u32, u32),
    pages_visible: bool,
    properties_visible: bool,
    symbol_palette_visible: bool,
    ribbon_collapsed: bool,
) -> scrollable::AbsoluteOffset {
    let viewport_width = window_size.0
        - 32.0 // left icon rail
        - if pages_visible { 172.0 } else { 0.0 } // pages + resize handle
        - drawing_palette_width(window_size.1, ribbon_collapsed)
        - if symbol_palette_visible { 220.0 } else { 0.0 }
        - if properties_visible { 300.0 } else { 28.0 };
    let viewport_height = window_size.1
        - 30.0 // title bar
        - 28.0 // ribbon tabs
        - if ribbon_collapsed { 0.0 } else { 94.0 }
        - 28.0 // document tabs
        - 22.0; // status bar
    scrollable::AbsoluteOffset {
        x: (HORIZONTAL_PAN_MARGIN + PAGE_PADDING
            - ((viewport_width - bitmap_size.0 as f32) / 2.0).max(0.0))
        .max(0.0),
        y: (VERTICAL_PAN_MARGIN + PAGE_PADDING
            - ((viewport_height - bitmap_size.1 as f32) / 2.0).max(0.0))
        .max(0.0),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PrintRange {
    #[default]
    All,
    Current,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PrintSubset {
    #[default]
    All,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PrintDestination {
    #[default]
    PdfFile,
    VirtualIpp,
}

#[derive(Debug, Clone)]
enum PendingAction {
    New,
    Open,
    Close,
    Exit,
}

fn print_pages(
    range: PrintRange,
    subset: PrintSubset,
    reverse: bool,
    current: usize,
    custom: &str,
    total: usize,
) -> Result<Vec<usize>, String> {
    if total == 0 {
        return Err("No PDF pages available".into());
    }
    let pages: Vec<usize> = match range {
        PrintRange::All => (0..total).collect(),
        PrintRange::Current => vec![current.min(total - 1)],
        PrintRange::Custom => {
            let mut pages = std::collections::BTreeSet::new();
            if custom.trim().is_empty() {
                return Err("Enter page numbers such as 1, 3-5".into());
            }
            for part in custom.split(',') {
                let part = part.trim();
                let (first, last) = if let Some((first, last)) = part.split_once('-') {
                    (first.trim(), last.trim())
                } else {
                    (part, part)
                };
                let parse = |value: &str| {
                    value
                        .parse::<usize>()
                        .ok()
                        .filter(|&page| page > 0 && page <= total)
                };
                let (Some(first), Some(last)) = (parse(first), parse(last)) else {
                    return Err(format!("Invalid page range: {part}"));
                };
                if first > last {
                    return Err(format!("Page range runs backwards: {part}"));
                }
                pages.extend((first - 1)..last);
            }
            pages.into_iter().collect()
        }
    };
    let mut selected: Vec<_> = pages
        .into_iter()
        .filter(|&page| match subset {
            PrintSubset::All => true,
            PrintSubset::Odd => (page + 1) % 2 == 1,
            PrintSubset::Even => (page + 1) % 2 == 0,
        })
        .collect();
    if selected.is_empty() {
        Err("No pages match this range and subset".into())
    } else {
        if reverse {
            selected.reverse();
        }
        Ok(selected)
    }
}

#[cfg(test)]
mod print_range_tests {
    use super::*;

    #[test]
    fn custom_page_ranges_are_sorted_deduplicated_and_bounded() {
        assert_eq!(
            print_pages(
                PrintRange::Custom,
                PrintSubset::All,
                false,
                0,
                "5, 2-3, 2",
                5
            )
            .unwrap(),
            vec![1, 2, 4]
        );
        assert_eq!(
            print_pages(PrintRange::Current, PrintSubset::All, false, 3, "", 5).unwrap(),
            vec![3]
        );
        assert_eq!(
            print_pages(PrintRange::All, PrintSubset::Odd, false, 0, "", 5).unwrap(),
            vec![0, 2, 4]
        );
        assert_eq!(
            print_pages(PrintRange::All, PrintSubset::Even, false, 0, "", 5).unwrap(),
            vec![1, 3]
        );
        assert!(print_pages(PrintRange::Current, PrintSubset::Even, false, 0, "", 5).is_err());
        assert_eq!(
            print_pages(PrintRange::All, PrintSubset::Odd, true, 0, "", 5).unwrap(),
            vec![4, 2, 0]
        );
        for invalid in ["", "0", "6", "3-2", "1,,2", "1-2-3"] {
            assert!(
                print_pages(PrintRange::Custom, PrintSubset::All, false, 0, invalid, 5).is_err(),
                "{invalid}"
            );
        }
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::{
        thumbnail_page_at_scroll, thumbnail_scale, thumbnail_window, THUMBNAIL_HEADER_HEIGHT,
        THUMBNAIL_HEIGHT, THUMBNAIL_ROW_STEP, THUMBNAIL_WIDTH,
    };

    #[test]
    fn fits_large_landscape_and_portrait_pages_without_upscaling() {
        assert!((thumbnail_scale(3000.0, 2000.0).unwrap() - THUMBNAIL_WIDTH / 3000.0).abs() < 1e-6);
        assert!((thumbnail_scale(600.0, 1200.0).unwrap() - THUMBNAIL_HEIGHT / 1200.0).abs() < 1e-6);
        assert_eq!(thumbnail_scale(50.0, 50.0).unwrap(), 1.0);
        assert!(thumbnail_scale(20000.0, 200.0).is_err());
        assert!(thumbnail_scale(f32::NAN, 100.0).is_err());
    }

    #[test]
    fn scroll_focus_tracks_visible_thumbnail_card() {
        assert_eq!(thumbnail_page_at_scroll(0.0, 100), 0);
        assert_eq!(
            thumbnail_page_at_scroll(THUMBNAIL_HEADER_HEIGHT + THUMBNAIL_ROW_STEP * 9.0, 100),
            9
        );
        assert_eq!(thumbnail_page_at_scroll(100_000.0, 100), 99);
        assert_eq!(thumbnail_page_at_scroll(100.0, 0), 0);
        assert_eq!(thumbnail_window(0, 100), 0..12);
        assert_eq!(thumbnail_window(50, 100), 47..59);
        assert_eq!(thumbnail_window(99, 100), 88..100);
        assert_eq!(thumbnail_window(10, 4), 0..4);
        for focus in 0..100 {
            let window = thumbnail_window(focus, 100);
            assert!(window.contains(&focus));
            assert!(window.len() <= 12);
        }
    }
}

#[cfg(test)]
mod fit_view_tests {
    use super::{
        centered_page_offset, fit_scale, zoom_preview_size, zoom_render_delay, App, FitMode,
        Message,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn fit_uses_document_area_and_reacts_to_panels() {
        let page = (1000.0, 2000.0);
        let window = (1200.0, 832.0);
        let width = fit_scale(FitMode::Width, page, window, true, true, true, false).unwrap();
        let full = fit_scale(FitMode::Page, page, window, true, true, true, false).unwrap();
        assert!((width - 0.396).abs() < 1e-5);
        assert!((full - 0.315).abs() < 1e-5);
        assert!(
            fit_scale(FitMode::Width, page, window, false, false, true, false).unwrap() > width
        );
    }

    #[test]
    fn initial_page_is_centered_in_the_docked_tauri_layout() {
        let offset = centered_page_offset((1200.0, 832.0), (256, 192), true, true, true, false);
        assert_eq!(offset.x, 466.0);
        assert_eq!(offset.y, 317.0);
    }

    #[test]
    fn pending_zoom_stretches_existing_bitmap_without_a_new_raster() {
        assert_eq!(
            zoom_preview_size((1000.0, 800.0), (1000, 800), 1.25),
            (1250, 1000)
        );
        assert_eq!(
            zoom_preview_size((2000.0, 1600.0), (1000, 800), 0.25),
            (500, 400)
        );
        assert_eq!(
            zoom_preview_size((0.0, 0.0), (1000, 800), 1.25),
            (1000, 800)
        );
        assert_eq!(zoom_preview_size((1000.0, 800.0), (0, 0), 1.25), (0, 0));
    }

    #[test]
    fn repeated_zoom_requests_keep_only_the_latest_render_deadline() {
        let mut app = App::default();
        let _ = app.schedule_zoom_render();
        let first = app.zoom_render_due.unwrap();
        let _ = app.schedule_zoom_render();
        assert!(app.zoom_timer_active);
        assert!(app.zoom_render_due.unwrap() >= first);
        app.zoom_render_due = Some(Instant::now() - Duration::from_millis(1));
        let _ = app.update(Message::ZoomRenderDue);
        assert!(!app.zoom_timer_active);
        assert!(app.zoom_render_due.is_none());
    }

    #[test]
    fn first_zoom_step_sharpens_before_repeated_wheel_steps() {
        assert_eq!(zoom_render_delay(false), Duration::from_millis(24));
        assert_eq!(zoom_render_delay(true), Duration::from_millis(160));
    }
}

#[cfg(test)]
mod zoom_interaction_tests {
    use super::*;

    #[test]
    fn held_mouse_drag_moves_scroll_opposite_to_pointer_and_clamps() {
        let drag = PanDrag {
            button: iced::mouse::Button::Middle,
            start: [120.0, 80.0],
            scroll_start: scrollable::AbsoluteOffset { x: 300.0, y: 500.0 },
        };
        assert_eq!(
            dragged_scroll_offset(drag, [170.0, 50.0]),
            scrollable::AbsoluteOffset { x: 250.0, y: 530.0 }
        );
        assert_eq!(
            dragged_scroll_offset(drag, [600.0, 900.0]),
            scrollable::AbsoluteOffset { x: 0.0, y: 0.0 }
        );
    }

    #[test]
    fn pointer_stays_fixed_with_pan_margins_and_scrolled_page() {
        for (offset, point, old, new) in [
            ([512.0, 512.0], [40.0, 140.0], (256, 192), (294, 221)),
            ([812.0, 912.0], [640.0, 720.0], (1000, 1400), (1150, 1610)),
            ([812.0, 912.0], [640.0, 720.0], (1000, 1400), (500, 700)),
        ] {
            let next = zoom_scroll_offset(
                scrollable::AbsoluteOffset {
                    x: offset[0],
                    y: offset[1],
                },
                point,
                old,
                new,
            );
            let before = [
                HORIZONTAL_PAN_MARGIN + PAGE_PADDING + point[0] - offset[0],
                VERTICAL_PAN_MARGIN + PAGE_PADDING + point[1] - offset[1],
            ];
            let after = [
                HORIZONTAL_PAN_MARGIN + PAGE_PADDING + point[0] * new.0 as f32 / old.0 as f32
                    - next.x,
                VERTICAL_PAN_MARGIN + PAGE_PADDING + point[1] * new.1 as f32 / old.1 as f32
                    - next.y,
            ];
            assert!((before[0] - after[0]).abs() < 0.001);
            assert!((before[1] - after[1]).abs() < 0.001);
        }
    }

    #[test]
    fn thumbnails_cover_final_visible_rows_without_cache_churn() {
        let bottom: Vec<_> = thumbnail_candidates(3, 7).collect();
        assert_eq!(&bottom[..4], &[3, 4, 5, 6]);
        for count in [0, 1, 7, 30, 100] {
            for focus in 0..count.max(1) {
                let candidates: Vec<_> = thumbnail_candidates(focus, count).collect();
                let expected = thumbnail_window(focus, count);
                assert_eq!(candidates.len(), expected.len());
                assert!(candidates.len() <= MAX_THUMBNAILS);
                for page in expected {
                    assert_eq!(candidates.iter().filter(|&&p| p == page).count(), 1);
                }
            }
        }
    }

    #[test]
    fn drawing_palette_uses_one_column_when_full_height_and_fit_uses_its_width() {
        assert!(drawing_palette_two_columns(832.0, false));
        assert_eq!(drawing_palette_width(832.0, false), 80.0);
        assert!(!drawing_palette_two_columns(1040.0, false));
        assert_eq!(drawing_palette_width(1040.0, false), 44.0);
        let two_col = fit_scale(
            FitMode::Width,
            (600.0, 800.0),
            (1200.0, 832.0),
            true,
            true,
            true,
            false,
        )
        .unwrap();
        let one_col = fit_scale(
            FitMode::Width,
            (600.0, 800.0),
            (1200.0, 1040.0),
            true,
            true,
            true,
            false,
        )
        .unwrap();
        assert!(one_col > two_col);
    }

    #[test]
    fn stale_render_clears_busy_until_debounced_zoom_starts() {
        let bytes = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let model = DocumentModel::new(DocumentId(1), pdf.page_count()).unwrap();
        let stale_ticket = model.view().ticket();
        let mut app = App {
            document: Some(OpenDocument {
                path: "fixture.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            busy: true,
            ..App::default()
        };
        let _ = app.update(Message::Zoom(1.25));
        assert!(app.zoom_timer_active);
        assert!(!app.render_queued);
        let _ = app.update(Message::Rendered(
            0,
            stale_ticket,
            Err("obsolete render".into()),
        ));
        assert!(!app.busy);
        app.zoom_render_due = Some(Instant::now() - Duration::from_millis(1));
        let _ = app.update(Message::ZoomRenderDue);
        assert!(app.busy);
        assert!(app.zoom_render_due.is_none());
    }
}

#[cfg(test)]
mod line_geometry_tests {
    use super::{
        annotation_color, line_bounds, recolored_annotation, resized_annotation, Annotation, Kind,
        Rect,
    };

    #[test]
    fn line_bounds_include_stroke_without_leaving_page() {
        let bounds = line_bounds([0.0, 10.0], [100.0, 10.0], (200.0, 150.0)).unwrap();
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (0.0, 8.0, 102.0, 4.0)
        );
        assert!(line_bounds([20.0, 20.0], [20.0, 20.0], (200.0, 150.0)).is_none());
        assert!(line_bounds([f64::NAN, 0.0], [30.0, 40.0], (200.0, 150.0)).is_none());
        assert!(line_bounds([10.0, 10.0], [201.0, 10.0], (200.0, 150.0)).is_none());
    }

    #[test]
    fn resize_rejects_point_geometry_and_out_of_page_rectangles() {
        let source = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            kind: Kind::Line {
                start: [10.0, 10.0],
                end: [30.0, 30.0],
                color_rgb: [0, 0, 0],
            },
        };
        let target = Rect {
            x: 12.0,
            y: 13.0,
            width: 24.0,
            height: 25.0,
        };
        assert!(resized_annotation(&source, target, (100.0, 100.0)).is_none());
        let shape = Annotation {
            kind: Kind::Ellipse {
                color_rgb: [0, 0, 0],
            },
            ..source
        };
        assert_eq!(
            resized_annotation(&shape, target, (100.0, 100.0))
                .unwrap()
                .bounds,
            target
        );
        assert!(resized_annotation(&shape, Rect { x: 90.0, ..target }, (100.0, 100.0)).is_none());
        assert!(resized_annotation(&shape, shape.bounds, (100.0, 100.0)).is_none());
    }

    #[test]
    fn color_change_preserves_shape_geometry_and_rejects_no_op() {
        let source = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            kind: Kind::Arrow {
                start: [12.0, 12.0],
                end: [28.0, 28.0],
                color_rgb: [255, 0, 0],
            },
        };
        assert!(recolored_annotation(&source, [255, 0, 0]).is_none());
        let changed = recolored_annotation(&source, [0, 102, 204]).unwrap();
        assert_eq!(changed.bounds, source.bounds);
        assert_eq!(annotation_color(&changed.kind), [0, 102, 204]);
        if let Kind::Arrow { start, end, .. } = changed.kind {
            assert_eq!(start, [12.0, 12.0]);
            assert_eq!(end, [28.0, 28.0]);
        } else {
            panic!("arrow kind must remain an arrow");
        }
    }
}

#[cfg(test)]
mod browser_tests {
    use super::{browser_window, list_pdf_directory, visible_browser_indices};

    #[test]
    fn directory_browser_filters_pdfs_and_places_folders_first() {
        let root = std::env::temp_dir().join(format!(
            "opds-browser-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("z Folder")).unwrap();
        std::fs::create_dir(root.join("A Folder")).unwrap();
        std::fs::write(root.join("b.PDF"), b"%PDF-1.7").unwrap();
        std::fs::write(root.join("C.pdf"), b"%PDF-1.7").unwrap();
        std::fs::write(root.join("d.txt"), b"ignore").unwrap();
        let entries = list_pdf_directory(&root).unwrap();
        let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["A Folder", "z Folder", "b.PDF", "C.pdf"]);
        assert!(entries[0].is_dir && entries[1].is_dir);
        assert!(!entries[2].is_dir && !entries[3].is_dir);
        assert_eq!(visible_browser_indices(&entries, "FOLDER"), [0, 1]);
        assert_eq!(visible_browser_indices(&entries, "pdf"), [2, 3]);
        assert_eq!(visible_browser_indices(&entries, "c.PDf"), [3]);
        assert_eq!(browser_window(36.0 * 200.0, 1_000), 197..217);
        assert_eq!(browser_window(36.0 * 999.0, 1_000), 980..1_000);
        assert_eq!(browser_window(0.0, 4), 0..4);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod unsaved_annotation_tests {
    use super::*;

    #[test]
    fn saved_snapshot_remains_dirty_if_text_changes_during_save() {
        let mut model = DocumentModel::new(DocumentId(1), 1).unwrap();
        assert!(!annotations_changed(&model, &[]));
        let original = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 40.0,
            },
            kind: Kind::Text {
                value: "Before".into(),
                color_rgb: [0, 0, 0],
                font_size: 16,
                align: TextAlign::Left,
            },
        };
        model.apply(Edit::Add(original.clone())).unwrap();
        assert!(annotations_changed(&model, &[]));
        let save_started = vec![original.clone()];
        let mut edited = original;
        if let Kind::Text { value, .. } = &mut edited.kind {
            *value = "After".into();
        }
        model.apply(Edit::Replace(edited)).unwrap();
        assert!(annotations_changed(&model, &save_started));
        model.undo().unwrap();
        assert!(!annotations_changed(&model, &save_started));
    }

    #[test]
    fn completed_save_from_previous_document_does_not_mark_current_one_clean() {
        let mut app = App::default();
        app.next_load = 2;
        app.save_busy = true;
        app.message = "Current document".into();
        let _ = app.update(Message::Saved(1, Ok(("old.pdf".into(), Vec::new(), 0))));
        assert!(app.save_busy);
        assert_eq!(app.message, "Current document");
        assert!(app.saved_target.is_none());
    }

    #[test]
    fn close_requires_explicit_discard_for_unsaved_annotation() {
        let bytes = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let mut model = DocumentModel::new(DocumentId(1), pdf.page_count()).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 1,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 20.0,
                    height: 20.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 0, 0],
                },
            }))
            .unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "source.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            ..App::default()
        };
        let _ = app.update(Message::Save);
        assert!(app.save_dialog);
        assert!(app.save_path_input.ends_with("source-annotated.pdf"));
        let _ = app.update(Message::CancelSave);
        let _ = app.update(Message::ShowNew);
        let _ = app.update(Message::NewBlank);
        assert!(matches!(app.confirm_discard, Some(PendingAction::New)));
        assert!(app.document.is_some());
        let _ = app.update(Message::CancelDiscard);
        app.path_input = "next.pdf".into();
        let _ = app.update(Message::Open);
        assert!(matches!(app.confirm_discard, Some(PendingAction::Open)));
        assert!(app.document.is_some());
        let _ = app.update(Message::CancelDiscard);
        let _ = app.update(Message::Close);
        assert!(matches!(app.confirm_discard, Some(PendingAction::Close)));
        assert!(app.document.is_some());
        let _ = app.update(Message::CancelDiscard);
        assert!(app.document.is_some());
        let _ = app.update(Message::CloseWindow);
        assert!(matches!(app.confirm_discard, Some(PendingAction::Exit)));
        assert!(app.document.is_some());
        let _ = app.update(Message::CancelDiscard);
        let _ = app.update(Message::Close);
        let _ = app.update(Message::ConfirmDiscard);
        assert!(app.document.is_none());
    }

    #[test]
    fn clear_annotations_requires_confirmation_and_rejects_stale_document() {
        let bytes = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let mut model = DocumentModel::new(DocumentId(91), pdf.page_count()).unwrap();
        let make_annotation = |id| Annotation {
            id,
            page: 0,
            bounds: Rect {
                x: 10.0 + id as f64 * 20.0,
                y: 10.0,
                width: 12.0,
                height: 12.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 0, 0],
            },
        };
        model.apply(Edit::Add(make_annotation(1))).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "clear-test.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: Some(1),
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            ..App::default()
        };
        let _ = app.update(Message::RequestClearAnnotations(Some(0)));
        assert!(app.pending_clear_annotations.is_some());
        let _ = app.update(Message::CancelClearAnnotations);
        assert!(app.document.as_ref().unwrap().model.get(1).is_some());
        let _ = app.update(Message::RequestClearAnnotations(Some(0)));
        let document = app.document.as_mut().unwrap();
        document
            .model
            .execute(
                document.model.version(),
                DocumentCommand::Edit(Edit::Add(make_annotation(2))),
            )
            .unwrap();
        let _ = app.update(Message::ConfirmClearAnnotations);
        assert!(app.message.contains("StaleDocument"));
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 2);
        let _ = app.update(Message::RequestClearAnnotations(Some(0)));
        let _ = app.update(Message::ConfirmClearAnnotations);
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 0);
        assert!(app.document.as_ref().unwrap().selected_annotation.is_none());
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 2);
        app.document.as_mut().unwrap().selected_annotation = Some(1);
        let _ = app.update(Message::RequestDeleteAnnotation);
        assert!(app.pending_delete_annotation.is_some());
        let _ = app.update(Message::CancelDeleteAnnotation);
        assert!(app.document.as_ref().unwrap().model.get(1).is_some());
        let _ = app.update(Message::RequestDeleteAnnotation);
        let document = app.document.as_mut().unwrap();
        document
            .model
            .execute(
                document.model.version(),
                DocumentCommand::Edit(Edit::Add(make_annotation(3))),
            )
            .unwrap();
        let _ = app.update(Message::ConfirmDeleteAnnotation);
        assert!(app.document.as_ref().unwrap().model.get(1).is_some());
        let _ = app.update(Message::RequestDeleteAnnotation);
        let _ = app.update(Message::ConfirmDeleteAnnotation);
        assert!(app.document.as_ref().unwrap().model.get(1).is_none());
    }

    #[test]
    fn external_delete_requires_exact_live_version_and_replies_after_edit() {
        let bytes = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let mut model = DocumentModel::new(DocumentId(930), pdf.page_count()).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 45,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 20.0,
                    height: 20.0,
                },
                kind: Kind::Ellipse {
                    color_rgb: [255, 0, 0],
                },
            }))
            .unwrap();
        let version = model.version();
        let mut app = App {
            document: Some(OpenDocument {
                path: "external-delete-test.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: Some(45),
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            ..App::default()
        };
        let tool_request = |revision, old, old_epoch, requested| {
            mcp_server::ExternalRequest::new(
                [930, version.generation, revision],
                mcp_server::ExternalAction::SelectTool {
                    expected: old,
                    expected_epoch: old_epoch,
                    requested,
                },
            )
        };
        let (cancelled, response) =
            tool_request(version.annotation_revision, "select", 0, "ellipse");
        drop(response);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert_eq!(app.annotation_tool, AnnotationTool::Select);
        let (stale, response) =
            tool_request(version.annotation_revision + 1, "select", 0, "ellipse");
        let _ = app.update(Message::ExternalRequest(stale));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let (valid, response) = tool_request(version.annotation_revision, "select", 0, "ellipse");
        let _ = app.update(Message::ExternalRequest(valid));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["tool"],
            "ellipse"
        );
        assert_eq!(app.annotation_tool, AnnotationTool::Ellipse);
        assert!(app.annotation_tool_activated);
        let (replay, response) = tool_request(version.annotation_revision, "select", 0, "ellipse");
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale current tool"
        );
        let (restore, response) = tool_request(version.annotation_revision, "ellipse", 1, "select");
        let _ = app.update(Message::ExternalRequest(restore));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["tool"],
            "select"
        );
        assert_eq!(app.document.as_ref().unwrap().model.version(), version);
        let (old_cycle, response) =
            tool_request(version.annotation_revision, "select", 0, "ellipse");
        let _ = app.update(Message::ExternalRequest(old_cycle));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale current tool"
        );
        assert_eq!(app.annotation_tool, AnnotationTool::Select);
        let make_request = |id, revision| {
            mcp_server::ExternalRequest::delete([id, version.generation, revision], 45)
        };
        let (cancelled, result) = make_request(930, version.annotation_revision);
        drop(result);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());

        let (stale_id, result) = make_request(931, version.annotation_revision);
        let _ = app.update(Message::ExternalRequest(stale_id));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["ok"],
            false
        );
        let (stale_revision, result) = make_request(930, version.annotation_revision + 1);
        let _ = app.update(Message::ExternalRequest(stale_revision));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["error"],
            "stale document version"
        );
        assert_eq!(app.document.as_ref().unwrap().model.version(), version);

        let external_selection = |revision, action| {
            mcp_server::ExternalRequest::new([930, version.generation, revision], action)
        };
        let (clear, result) = external_selection(
            version.annotation_revision,
            mcp_server::ExternalAction::ClearSelection,
        );
        let _ = app.update(Message::ExternalRequest(clear));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["selectedId"],
            serde_json::Value::Null
        );
        assert!(app.document.as_ref().unwrap().selected_annotation.is_none());
        let (stale_select, result) = external_selection(
            version.annotation_revision + 1,
            mcp_server::ExternalAction::Select(45),
        );
        let _ = app.update(Message::ExternalRequest(stale_select));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["error"],
            "stale document version"
        );
        let (unknown, result) = external_selection(
            version.annotation_revision,
            mcp_server::ExternalAction::Select(999),
        );
        let _ = app.update(Message::ExternalRequest(unknown));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["ok"],
            false
        );
        let (select, result) = external_selection(
            version.annotation_revision,
            mcp_server::ExternalAction::Select(45),
        );
        let _ = app.update(Message::ExternalRequest(select));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["selectedId"],
            "45"
        );
        assert_eq!(app.document.as_ref().unwrap().selected_annotation, Some(45));
        assert_eq!(app.document.as_ref().unwrap().model.version(), version);

        let (valid, result) = make_request(930, version.annotation_revision);
        let _ = app.update(Message::ExternalRequest(valid));
        let response = iced::futures::executor::block_on(result).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["deletedId"], "45");
        assert!(app.document.as_ref().unwrap().model.get(45).is_none());
        assert!(app.document.as_ref().unwrap().selected_annotation.is_none());
        let next = app.document.as_ref().unwrap().model.version();
        assert_eq!(response["annotationRevision"], next.annotation_revision);
        assert!(next.annotation_revision > version.annotation_revision);

        let (replayed, result) = make_request(930, version.annotation_revision);
        let _ = app.update(Message::ExternalRequest(replayed));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["ok"],
            false
        );
        assert_eq!(app.document.as_ref().unwrap().model.version(), next);

        let external = |revision, action| {
            mcp_server::ExternalRequest::new([930, next.generation, revision], action)
        };
        let (stale_undo, result) = external(
            version.annotation_revision,
            mcp_server::ExternalAction::Undo,
        );
        let _ = app.update(Message::ExternalRequest(stale_undo));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["ok"],
            false
        );
        assert!(app.document.as_ref().unwrap().model.get(45).is_none());
        let (undo, result) = external(next.annotation_revision, mcp_server::ExternalAction::Undo);
        let _ = app.update(Message::ExternalRequest(undo));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["action"],
            "undo"
        );
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());
        let after_undo = app.document.as_ref().unwrap().model.version();
        let (stale_redo, result) =
            external(next.annotation_revision, mcp_server::ExternalAction::Redo);
        let _ = app.update(Message::ExternalRequest(stale_redo));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["ok"],
            false
        );
        let (redo, result) = external(
            after_undo.annotation_revision,
            mcp_server::ExternalAction::Redo,
        );
        let _ = app.update(Message::ExternalRequest(redo));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["action"],
            "redo"
        );
        assert!(app.document.as_ref().unwrap().model.get(45).is_none());
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());
        let switch_version = app.document.as_ref().unwrap().model.version();
        let renderer_request = |revision, old, requested| {
            mcp_server::ExternalRequest::new(
                [930, switch_version.generation, revision],
                mcp_server::ExternalAction::SelectBackend {
                    expected: old,
                    requested,
                },
            )
        };
        let (cancelled, response) = renderer_request(
            switch_version.annotation_revision,
            Backend::Hayro,
            Backend::Dynlex,
        );
        drop(response);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert_eq!(app.backend, Backend::Hayro);
        let (stale, response) = renderer_request(
            switch_version.annotation_revision + 1,
            Backend::Hayro,
            Backend::Dynlex,
        );
        let _ = app.update(Message::ExternalRequest(stale));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        assert_eq!(app.backend, Backend::Hayro);
        let (valid, response) = renderer_request(
            switch_version.annotation_revision,
            Backend::Hayro,
            Backend::Dynlex,
        );
        let _ = app.update(Message::ExternalRequest(valid));
        let response = iced::futures::executor::block_on(response).unwrap();
        assert_eq!(response["selected"], "dynlex");
        assert_eq!(response["renderPending"], true);
        assert_eq!(app.backend, Backend::Dynlex);
        let (replay, response) = renderer_request(
            switch_version.annotation_revision,
            Backend::Hayro,
            Backend::Dynlex,
        );
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale renderer selection"
        );
        assert_eq!(app.backend, Backend::Dynlex);

        let restored = app.document.as_ref().unwrap().model.version();
        let clear_request = |revision, page| {
            mcp_server::ExternalRequest::new(
                [930, restored.generation, revision],
                mcp_server::ExternalAction::ClearAnnotations(page),
            )
        };
        let (cancelled, result) = clear_request(restored.annotation_revision, Some(0));
        drop(result);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());
        let (stale, result) = clear_request(restored.annotation_revision - 1, Some(0));
        let _ = app.update(Message::ExternalRequest(stale));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["error"],
            "stale document version"
        );
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());
        let (invalid_page, result) = clear_request(restored.annotation_revision, Some(1));
        let _ = app.update(Message::ExternalRequest(invalid_page));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["ok"],
            false
        );
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());
        let (clear, result) = clear_request(restored.annotation_revision, Some(0));
        let _ = app.update(Message::ExternalRequest(clear));
        let response = iced::futures::executor::block_on(result).unwrap();
        assert_eq!(response["action"], "clear_annotations");
        assert_eq!(response["page"], 1);
        assert!(app.document.as_ref().unwrap().model.get(45).is_none());
        let empty_version = app.document.as_ref().unwrap().model.version();
        let (empty, result) = clear_request(empty_version.annotation_revision, None);
        let _ = app.update(Message::ExternalRequest(empty));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["error"],
            "no annotations in requested scope"
        );
        assert_eq!(
            app.document.as_ref().unwrap().model.version(),
            empty_version
        );
        let (replay, result) = clear_request(restored.annotation_revision, Some(0));
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(result).unwrap()["error"],
            "stale document version"
        );
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());
        let before_all = app.document.as_ref().unwrap().model.version();
        let (clear_all, result) = clear_request(before_all.annotation_revision, None);
        let _ = app.update(Message::ExternalRequest(clear_all));
        let response = iced::futures::executor::block_on(result).unwrap();
        assert_eq!(response["action"], "clear_annotations");
        assert!(response["page"].is_null());
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 0);
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(45).is_some());

        let before_move = app.document.as_ref().unwrap().model.version();
        let old_bounds = app.document.as_ref().unwrap().model.get(45).unwrap().bounds;
        let move_request = |revision, id, delta| {
            mcp_server::ExternalRequest::new(
                [930, before_move.generation, revision],
                mcp_server::ExternalAction::MoveAnnotation { id, delta },
            )
        };
        let (cancelled, response) = move_request(before_move.annotation_revision, 45, [10.0, 5.0]);
        drop(response);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            old_bounds
        );
        let (stale, response) = move_request(before_move.annotation_revision + 1, 45, [10.0, 5.0]);
        let _ = app.update(Message::ExternalRequest(stale));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let (unknown, response) = move_request(before_move.annotation_revision, 999, [10.0, 5.0]);
        let _ = app.update(Message::ExternalRequest(unknown));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "annotation not found"
        );
        let (no_op, response) = move_request(before_move.annotation_revision, 45, [0.0, 0.0]);
        let _ = app.update(Message::ExternalRequest(no_op));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "move did not change annotation"
        );
        let (valid, response) = move_request(before_move.annotation_revision, 45, [10.0, 5.0]);
        let _ = app.update(Message::ExternalRequest(valid));
        let result = iced::futures::executor::block_on(response).unwrap();
        assert_eq!(result["action"], "move_annotation");
        assert_eq!(result["id"], "45");
        assert_eq!(
            result["annotationRevision"],
            before_move.annotation_revision + 1
        );
        let moved = app.document.as_ref().unwrap().model.get(45).unwrap().bounds;
        assert_eq!(moved.x, old_bounds.x + 10.0);
        assert_eq!(moved.y, old_bounds.y + 5.0);
        let (replay, response) = move_request(before_move.annotation_revision, 45, [10.0, 5.0]);
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            old_bounds
        );
        app.document.as_mut().unwrap().displayed_page = 1;
        let waiting = app.document.as_ref().unwrap().model.version();
        let (hidden, response) = mcp_server::ExternalRequest::new(
            [930, waiting.generation, waiting.annotation_revision],
            mcp_server::ExternalAction::MoveAnnotation {
                id: 45,
                delta: [10.0, 0.0],
            },
        );
        let _ = app.update(Message::ExternalRequest(hidden));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "annotation is not on the current displayed page"
        );
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            old_bounds
        );
        app.document.as_mut().unwrap().displayed_page = 0;

        let before_resize = app.document.as_ref().unwrap().model.version();
        let resized_bounds = Rect {
            x: old_bounds.x + 3.0,
            y: old_bounds.y + 4.0,
            width: old_bounds.width + 8.0,
            height: old_bounds.height + 6.0,
        };
        let resize_request = |revision, bounds| {
            mcp_server::ExternalRequest::new(
                [930, before_resize.generation, revision],
                mcp_server::ExternalAction::ResizeAnnotation { id: 45, bounds },
            )
        };
        let (cancelled, response) =
            resize_request(before_resize.annotation_revision, resized_bounds);
        drop(response);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            old_bounds
        );
        let (stale, response) =
            resize_request(before_resize.annotation_revision + 1, resized_bounds);
        let _ = app.update(Message::ExternalRequest(stale));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let (outside, response) = resize_request(
            before_resize.annotation_revision,
            Rect {
                x: 1_000_000.0,
                ..resized_bounds
            },
        );
        let _ = app.update(Message::ExternalRequest(outside));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["ok"],
            false
        );
        let (unchanged, response) = resize_request(before_resize.annotation_revision, old_bounds);
        let _ = app.update(Message::ExternalRequest(unchanged));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["ok"],
            false
        );
        let (valid, response) = resize_request(before_resize.annotation_revision, resized_bounds);
        let _ = app.update(Message::ExternalRequest(valid));
        let result = iced::futures::executor::block_on(response).unwrap();
        assert_eq!(result["action"], "resize_annotation");
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            resized_bounds
        );
        let (replay, response) = resize_request(before_resize.annotation_revision, resized_bounds);
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            old_bounds
        );
        let _ = app.update(Message::Redo);
        assert_eq!(
            app.document.as_ref().unwrap().model.get(45).unwrap().bounds,
            resized_bounds
        );
        let _ = app.update(Message::SelectAnnotation(Some(45)));
        let before_color = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::SetFormatColor([0, 102, 204]));
        let current = app.document.as_ref().unwrap();
        assert_eq!(
            annotation_color(&current.model.get(45).unwrap().kind),
            [0, 102, 204]
        );
        assert_eq!(
            current.model.version().annotation_revision,
            before_color.annotation_revision + 1
        );
        let _ = app.update(Message::Undo);
        assert_eq!(
            annotation_color(&app.document.as_ref().unwrap().model.get(45).unwrap().kind),
            [255, 0, 0]
        );
        let _ = app.update(Message::Redo);
        assert_eq!(
            annotation_color(&app.document.as_ref().unwrap().model.get(45).unwrap().kind),
            [0, 102, 204]
        );
    }

    /// Run alone because the MCP request stream and snapshot are process-wide:
    /// cargo test -p opds-native-app --all-features live_http_external_delete -- --ignored
    #[test]
    #[ignore]
    fn live_http_external_delete_rejects_stale_version() {
        use iced::futures::StreamExt;
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};

        let bytes = Arc::new(
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let mut model = DocumentModel::new(DocumentId(931), pdf.page_count()).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 46,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 20.0,
                    height: 20.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 0, 0],
                },
            }))
            .unwrap();
        let version = model.version();
        let mut app = App {
            document: Some(OpenDocument {
                path: "http-delete-test.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: Some(46),
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            ..App::default()
        };
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        mcp_server::start(port).unwrap();
        let mut requests = mcp_server::request_stream();
        let call = |tool: &'static str, revision: u64| {
            std::thread::spawn(move || {
                let mut arguments = serde_json::json!({
                    "documentId":"931","generation":version.generation,
                    "annotationRevision":revision});
                if tool == "app_delete_annotation"
                    || tool == "app_select_annotation"
                    || tool == "app_resize_annotation"
                {
                    arguments["id"] = serde_json::json!("46");
                }
                if tool == "app_resize_annotation" {
                    arguments["x"] = serde_json::json!(13.0);
                    arguments["y"] = serde_json::json!(14.0);
                    arguments["width"] = serde_json::json!(28.0);
                    arguments["height"] = serde_json::json!(26.0);
                }
                if tool == "app_clear_annotations" {
                    arguments["scope"] = serde_json::json!("page");
                    arguments["page"] = serde_json::json!(1);
                }
                if tool == "app_select_native_renderer" {
                    arguments["expectedRenderer"] = serde_json::json!("hayro");
                    arguments["renderer"] = serde_json::json!("dynlex");
                }
                let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":tool,"arguments":arguments}})
                .to_string();
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                write!(stream, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                let mut response = Vec::new();
                stream.read_to_end(&mut response).unwrap();
                let split = response
                    .windows(4)
                    .position(|part| part == b"\r\n\r\n")
                    .unwrap()
                    + 4;
                let result: serde_json::Value = serde_json::from_slice(&response[split..]).unwrap();
                serde_json::from_str::<serde_json::Value>(
                    result["result"]["content"][0]["text"].as_str().unwrap(),
                )
                .unwrap()
            })
        };

        let client = call("app_clear_selection", version.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert!(client.join().unwrap()["selectedId"].is_null());
        assert!(app.document.as_ref().unwrap().selected_annotation.is_none());

        let client = call("app_select_annotation", version.annotation_revision + 1);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert!(app.document.as_ref().unwrap().selected_annotation.is_none());

        let client = call("app_select_annotation", version.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["selectedId"], "46");
        assert_eq!(app.document.as_ref().unwrap().selected_annotation, Some(46));

        let client = call("app_delete_annotation", version.annotation_revision + 1);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert!(app.document.as_ref().unwrap().model.get(46).is_some());

        let client = call("app_delete_annotation", version.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["deletedId"], "46");
        assert!(app.document.as_ref().unwrap().model.get(46).is_none());

        let deleted = app.document.as_ref().unwrap().model.version();
        let client = call("app_undo", version.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert!(app.document.as_ref().unwrap().model.get(46).is_none());

        let client = call("app_undo", deleted.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["action"], "undo");
        assert!(app.document.as_ref().unwrap().model.get(46).is_some());
        let undone = app.document.as_ref().unwrap().model.version();

        let client = call("app_redo", deleted.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert!(app.document.as_ref().unwrap().model.get(46).is_some());

        let client = call("app_redo", undone.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["action"], "redo");
        assert!(app.document.as_ref().unwrap().model.get(46).is_none());
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(46).is_some());
        let before_resize = app.document.as_ref().unwrap().model.version();
        let original_bounds = app.document.as_ref().unwrap().model.get(46).unwrap().bounds;
        let client = call(
            "app_resize_annotation",
            before_resize.annotation_revision + 1,
        );
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert_eq!(
            app.document.as_ref().unwrap().model.get(46).unwrap().bounds,
            original_bounds
        );
        let client = call("app_resize_annotation", before_resize.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["action"], "resize_annotation");
        assert_eq!(
            app.document
                .as_ref()
                .unwrap()
                .model
                .get(46)
                .unwrap()
                .bounds
                .width,
            28.0
        );
        let _ = app.update(Message::Undo);
        assert_eq!(
            app.document.as_ref().unwrap().model.get(46).unwrap().bounds,
            original_bounds
        );
        let renderer_version = app.document.as_ref().unwrap().model.version();
        let client = call(
            "app_select_native_renderer",
            renderer_version.annotation_revision + 1,
        );
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert_eq!(app.backend, Backend::Hayro);
        let client = call(
            "app_select_native_renderer",
            renderer_version.annotation_revision,
        );
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["selected"], "dynlex");
        assert_eq!(app.backend, Backend::Dynlex);
        let client = call(
            "app_select_native_renderer",
            renderer_version.annotation_revision,
        );
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale renderer selection");
        let restored = app.document.as_ref().unwrap().model.version();
        let client = call("app_clear_annotations", restored.annotation_revision + 1);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        assert_eq!(client.join().unwrap()["error"], "stale document version");
        assert!(app.document.as_ref().unwrap().model.get(46).is_some());
        let client = call("app_clear_annotations", restored.annotation_revision);
        let request = iced::futures::executor::block_on(requests.next()).unwrap();
        let _ = app.update(Message::ExternalRequest(request));
        let response = client.join().unwrap();
        assert_eq!(response["action"], "clear_annotations");
        assert_eq!(response["page"], 1);
        assert!(app.document.as_ref().unwrap().model.get(46).is_none());
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(46).is_some());
    }
}

struct OpenDocument {
    path: String,
    is_new: bool,
    source_bytes: Arc<Vec<u8>>,
    dynlex_document: Option<Arc<opds_dynlex_backend::DynlexDocument>>,
    pdf: Arc<ParsedPdf>,
    model: DocumentModel,
    last_saved_annotations: Vec<Annotation>,
    bitmap: Option<image::Handle>,
    annotation_overlay_cache: RefCell<Option<AnnotationOverlayCache>>,
    #[cfg(feature = "dynlex-experimental")]
    content_layer_cache: RefCell<Option<ContentLayerCache>>,
    #[cfg(feature = "dynlex-experimental")]
    content_layer_in_flight: Option<RenderTicket>,
    bitmap_size: (u32, u32),
    bitmap_page_points: (f32, f32),
    bitmap_scale: f32,
    bitmap_backend: Backend,
    imported_annotations_dynlex: bool,
    displayed_page: usize,
    selected_annotation: Option<u64>,
    page_cache: VecDeque<CachedPage>,
    thumbnails: VecDeque<(usize, image::Handle)>,
    thumbnail_in_flight: Option<usize>,
    thumbnail_failed: HashSet<usize>,
    thumbnail_focus: usize,
}

struct AnnotationOverlayCache {
    base: image::Handle,
    page: usize,
    annotations: Vec<Annotation>,
    bitmap: Option<image::Handle>,
    selected_bitmap: Option<(u64, image::Handle)>,
}

struct AnnotationGesturePreview {
    selected: u64,
    page: usize,
    base: image::Handle,
    annotations: Vec<Annotation>,
    bitmap: image::Handle,
}

#[cfg(feature = "dynlex-experimental")]
struct ContentLayerCache {
    base: image::Handle,
    page: usize,
    rgba: Arc<Vec<u8>>,
}

struct CachedPage {
    page: usize,
    scale_bits: u32,
    bytes: usize,
    bitmap: image::Handle,
    bitmap_size: (u32, u32),
    page_points: (f32, f32),
    backend: Backend,
    imported_annotations_dynlex: bool,
    #[cfg(feature = "dynlex-experimental")]
    content_layer: Option<Arc<Vec<u8>>>,
}

impl OpenDocument {
    /// Route synchronous UI edits through the same versioned transaction used
    /// by undo/redo and external document commands.
    fn apply_edit(&mut self, edit: Edit) -> Result<(), CommandError> {
        let expected = self.model.version();
        self.model
            .execute(expected, DocumentCommand::Edit(edit))
            .map(|_| ())
    }

    fn is_dirty(&self) -> bool {
        annotations_changed(&self.model, &self.last_saved_annotations)
    }

    fn dynlex_selected_annotation_bitmap(&self, selected: u64) -> Option<image::Handle> {
        self.dynlex_annotation_bitmap()?;
        if let Some((id, bitmap)) = self
            .annotation_overlay_cache
            .borrow()
            .as_ref()
            .and_then(|cache| cache.selected_bitmap.as_ref())
        {
            if *id == selected {
                return Some(bitmap.clone());
            }
        }
        let bounds = self.model.get(selected)?.bounds;
        let bitmap = self.dynlex_selected_annotation_bitmap_at_bounds(selected, bounds)?;
        if let Some(cache) = self.annotation_overlay_cache.borrow_mut().as_mut() {
            cache.selected_bitmap = Some((selected, bitmap.clone()));
        }
        Some(bitmap)
    }

    /// Paint temporary gesture geometry without applying an annotation edit.
    /// The caller owns cache validity for live previews; saved /AP is untouched.
    fn dynlex_selected_annotation_bitmap_at_bounds(
        &self,
        selected: u64,
        bounds: Rect,
    ) -> Option<image::Handle> {
        let static_bitmap = self.dynlex_annotation_bitmap()?;
        let item = self.model.get(selected)?;
        let Kind::Rectangle { .. } = &item.kind else {
            return None;
        };
        if item.page != self.displayed_page {
            return None;
        }
        let image::Handle::Rgba { width, height, .. } = &static_bitmap else {
            return None;
        };
        // The selected viewer preview follows Iced's centered opaque stroke
        // and linear-light Highlight, while the static bitmap and saved /AP
        // retain the PDF-verified composition.
        let mut marks: Vec<_> = self
            .model
            .items()
            .filter(|mark| mark.page == self.displayed_page)
            .cloned()
            .collect();
        marks.iter_mut().find(|mark| mark.id == selected)?.bounds = bounds;
        let base = self.bitmap.as_ref()?;
        let image::Handle::Rgba { pixels: source, .. } = base else {
            return None;
        };
        let selected_source = opds_dynlex_backend::render_native_ui_preview_pixels(
            source.as_ref(),
            *width,
            *height,
            self.bitmap_page_points,
            &marks,
            self.displayed_page,
        )
        .ok()
        .flatten()?
        .rgba;
        let painted = opds_dynlex_backend::render_native_selection_overlay_pixels(
            &selected_source,
            *width,
            *height,
            self.bitmap_page_points,
            bounds,
        )
        .ok()
        .flatten()?;
        Some(image::Handle::from_rgba(
            painted.width,
            painted.height,
            painted.rgba,
        ))
    }

    fn dynlex_annotation_bitmap(&self) -> Option<image::Handle> {
        let base = self.bitmap.as_ref()?;
        let annotations: Vec<_> = self
            .model
            .items()
            .filter(|item| item.page == self.displayed_page)
            .cloned()
            .collect();
        if annotations.is_empty() {
            return None;
        }
        let mut cache = self.annotation_overlay_cache.borrow_mut();
        if let Some(cached) = cache.as_ref() {
            if cached.base == *base
                && cached.page == self.displayed_page
                && cached.annotations == annotations
            {
                return cached.bitmap.clone();
            }
        }
        let bitmap = if let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = base
        {
            let render_shapes = |page_rgba: &[u8], shapes: &[Annotation]| {
                #[cfg(feature = "dynlex-experimental")]
                {
                    let cache = self.content_layer_cache.borrow();
                    if let Some(layer) = cache
                        .as_ref()
                        .filter(|cached| cached.base == *base && cached.page == self.displayed_page)
                    {
                        return opds_dynlex_backend::render_native_annotation_overlay_pixels_with_content(
                            page_rgba,
                            &layer.rgba,
                            *width,
                            *height,
                            self.bitmap_page_points,
                            shapes,
                            self.displayed_page,
                        );
                    }
                }
                opds_dynlex_backend::render_native_annotation_overlay_pixels(
                    page_rgba,
                    *width,
                    *height,
                    self.bitmap_page_points,
                    shapes,
                    self.displayed_page,
                )
            };
            let text: Option<Vec<u8>> = {
                #[cfg(feature = "dynlex-experimental")]
                {
                    if let Some(text_mark) = single_disjoint_text(&annotations) {
                        let shapes: Vec<_> = annotations
                            .iter()
                            .filter(|item| !matches!(item.kind, Kind::Text { .. }))
                            .cloned()
                            .collect();
                        native_text_scene::render_opaque_page_text_box(
                            &self.source_bytes,
                            self.displayed_page,
                            pixels.as_ref(),
                            *width,
                            *height,
                            self.bitmap_page_points,
                            text_mark,
                        )
                        .ok()
                        .flatten()
                        .or_else(|| {
                            if !native_text_scene::eligible_mixed_page(
                                &self.source_bytes,
                                self.displayed_page,
                                *width,
                                *height,
                            ) {
                                return None;
                            }
                            let content_cache = self.content_layer_cache.borrow();
                            let cached = content_cache.as_ref()?;
                            if cached.base != *base || cached.page != self.displayed_page {
                                return None;
                            }
                            native_text_scene::render_mixed_page_text_box(
                                &self.source_bytes,
                                self.displayed_page,
                                pixels.as_ref(),
                                &cached.rgba,
                                *width,
                                *height,
                                self.bitmap_page_points,
                                text_mark,
                            )
                            .ok()
                            .flatten()
                        })
                        .and_then(|text_rgba| {
                            if shapes.is_empty() {
                                return Some(text_rgba);
                            }
                            render_shapes(&text_rgba, &shapes)
                                .ok()
                                .flatten()
                                .map(|result| result.rgba)
                        })
                        .or_else(|| {
                            self.render_ordered_premultiplied_text_annotations(
                                base,
                                pixels.as_ref(),
                                *width,
                                *height,
                                &annotations,
                            )
                        })
                    } else {
                        self.render_ordered_opaque_text_annotations(
                            base,
                            pixels.as_ref(),
                            *width,
                            *height,
                            &annotations,
                        )
                        .or_else(|| {
                            self.render_ordered_premultiplied_text_annotations(
                                base,
                                pixels.as_ref(),
                                *width,
                                *height,
                                &annotations,
                            )
                        })
                    }
                }
                #[cfg(not(feature = "dynlex-experimental"))]
                {
                    None
                }
            };
            if let Some(rgba) = text {
                Some(image::Handle::from_rgba(*width, *height, rgba))
            } else {
                render_shapes(pixels.as_ref(), &annotations)
                    .ok()
                    .flatten()
                    .map(|result| {
                        image::Handle::from_rgba(result.width, result.height, result.rgba)
                    })
            }
        } else {
            None
        };
        *cache = Some(AnnotationOverlayCache {
            base: base.clone(),
            page: self.displayed_page,
            annotations,
            bitmap: bitmap.clone(),
            selected_bitmap: None,
        });
        bitmap
    }

    #[cfg(feature = "dynlex-experimental")]
    fn render_ordered_opaque_text_annotations(
        &self,
        base: &image::Handle,
        base_rgba: &[u8],
        width: u32,
        height: u32,
        annotations: &[Annotation],
    ) -> Option<Vec<u8>> {
        if annotations.len() > 64
            || !annotations
                .iter()
                .any(|item| matches!(item.kind, Kind::Text { .. }))
            || self.bitmap_page_points != (width as f32, height as f32)
            || !native_text_scene::eligible_opaque_page(
                &self.source_bytes,
                self.displayed_page,
                width,
                height,
            )
        {
            return None;
        }
        let paint_shapes = |current: &[u8], shapes: &[Annotation]| -> Option<Vec<u8>> {
            if shapes.is_empty() {
                return Some(current.to_vec());
            }
            let content_cache = self.content_layer_cache.borrow();
            let layer = content_cache
                .as_ref()
                .filter(|cached| cached.base == *base && cached.page == self.displayed_page);
            let painted = if let Some(layer) = layer {
                opds_dynlex_backend::render_native_annotation_overlay_pixels_on_opaque_source(
                    current,
                    base_rgba,
                    &layer.rgba,
                    width,
                    height,
                    self.bitmap_page_points,
                    shapes,
                    self.displayed_page,
                )
            } else {
                opds_dynlex_backend::render_native_annotation_overlay_pixels(
                    current,
                    width,
                    height,
                    self.bitmap_page_points,
                    shapes,
                    self.displayed_page,
                )
            };
            painted.ok().flatten().map(|bitmap| bitmap.rgba)
        };
        let mut current = base_rgba.to_vec();
        let mut at = 0;
        while at < annotations.len() {
            if matches!(&annotations[at].kind, Kind::Text { .. }) {
                let text = &annotations[at];
                let tiles = native_text_scene::stage_native_text_box_tiles(text).ok()?;
                let clip = [
                    text.bounds.x as u32,
                    text.bounds.y as u32,
                    (text.bounds.x + text.bounds.width) as u32,
                    (text.bounds.y + text.bounds.height) as u32,
                ];
                for (scene, origin) in tiles {
                    current = opds_dynlex_backend::render_validated_glyph_scene_over_opaque_at_origin_owned(
                        &scene, current, width, height, clip, origin,
                    )
                    .ok()?
                    .rgba;
                }
                at += 1;
            } else {
                let end = annotations[at..]
                    .iter()
                    .position(|item| matches!(item.kind, Kind::Text { .. }))
                    .map_or(annotations.len(), |offset| at + offset);
                current = paint_shapes(&current, &annotations[at..end])?;
                at = end;
            }
        }
        Some(current)
    }

    #[cfg(feature = "dynlex-experimental")]
    fn render_ordered_premultiplied_text_annotations(
        &self,
        base: &image::Handle,
        base_rgba: &[u8],
        width: u32,
        height: u32,
        annotations: &[Annotation],
    ) -> Option<Vec<u8>> {
        if annotations.len() > 64
            || !annotations
                .iter()
                .any(|item| matches!(item.kind, Kind::Text { .. }))
        {
            return None;
        }
        let content_cache = self.content_layer_cache.borrow();
        let content = content_cache
            .as_ref()
            .filter(|cached| cached.base == *base && cached.page == self.displayed_page)
            .map(|cached| cached.rgba.as_slice());
        let mut layer = native_text_scene::verified_premultiplied_page_layer(
            &self.source_bytes,
            self.displayed_page,
            base_rgba,
            content,
            width,
            height,
            self.bitmap_page_points,
        )?;
        drop(content_cache);
        let paint_shapes = |current: &[u8], shapes: &[Annotation]| -> Option<Vec<u8>> {
            if shapes.is_empty() {
                return Some(current.to_vec());
            }
            opds_dynlex_backend::render_native_annotation_overlay_pixels_over_premultiplied(
                current,
                width,
                height,
                self.bitmap_page_points,
                shapes,
                self.displayed_page,
            )
            .ok()?
            .map(|bitmap| bitmap.rgba)
        };
        let mut at = 0;
        while at < annotations.len() {
            if matches!(&annotations[at].kind, Kind::Text { .. }) {
                let text = &annotations[at];
                let tiles = native_text_scene::stage_native_text_box_tiles(text).ok()?;
                let clip = [
                    text.bounds.x as u32,
                    text.bounds.y as u32,
                    (text.bounds.x + text.bounds.width) as u32,
                    (text.bounds.y + text.bounds.height) as u32,
                ];
                for (scene, origin) in tiles {
                    layer = opds_dynlex_backend::render_validated_glyph_scene_over_premultiplied_at_origin_owned(
                        &scene, layer, width, height, clip, origin,
                    )
                    .ok()?
                    .rgba;
                }
                at += 1;
            } else {
                let end = annotations[at..]
                    .iter()
                    .position(|item| matches!(item.kind, Kind::Text { .. }))
                    .map_or(annotations.len(), |offset| at + offset);
                layer = paint_shapes(&layer, &annotations[at..end])?;
                at = end;
            }
        }
        for pixel in layer.chunks_exact_mut(4) {
            let paper = 255 - pixel[3];
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(paper);
            }
            pixel[3] = 255;
        }
        Some(layer)
    }

    fn cache_bitmap(
        &mut self,
        page: usize,
        scale: f32,
        outcome: RenderOutcome,
        #[cfg(feature = "dynlex-experimental")] content_layer: Option<Raster>,
    ) {
        let mut raster = outcome.raster;
        #[cfg(feature = "dynlex-experimental")]
        {
            self.content_layer_in_flight = None;
            self.content_layer_cache.borrow_mut().take();
        }
        self.bitmap_backend = outcome.used;
        self.imported_annotations_dynlex = outcome.imported_annotations_dynlex;
        self.bitmap_size = (raster.width, raster.height);
        self.bitmap_page_points = (raster.page_width_pt, raster.page_height_pt);
        self.bitmap_scale = scale;
        self.displayed_page = page;
        // Give the current page its own small thumbnail handle. Reusing the
        // full-page image handle in two widgets can lose the thumbnail texture
        // when a modal overlay is removed by Iced's widget-tree diff.
        if !self.thumbnails.iter().any(|(cached, _)| *cached == page)
            && raster.width > 0
            && raster.height > 0
            && (raster.width as usize)
                .checked_mul(raster.height as usize)
                .and_then(|size| size.checked_mul(4))
                == Some(raster.rgba.len())
        {
            let pixels = std::mem::take(&mut raster.rgba);
            let source = ::image::RgbaImage::from_raw(raster.width, raster.height, pixels)
                .expect("validated RGBA dimensions");
            let thumb = ::image::imageops::thumbnail(
                &source,
                THUMBNAIL_WIDTH as u32,
                THUMBNAIL_HEIGHT as u32,
            );
            self.thumbnails.push_back((
                page,
                image::Handle::from_rgba(thumb.width(), thumb.height(), thumb.into_raw()),
            ));
            raster.rgba = source.into_raw();
            while self.thumbnails.len() > MAX_THUMBNAILS {
                let window = thumbnail_window(self.thumbnail_focus, self.pdf.page_count());
                let evict = self
                    .thumbnails
                    .iter()
                    .position(|(cached, _)| !window.contains(cached))
                    .unwrap_or(0);
                self.thumbnails.remove(evict);
            }
        }
        let bitmap_bytes = raster.rgba.len();
        let bitmap = image::Handle::from_rgba(raster.width, raster.height, raster.rgba);
        self.bitmap = Some(bitmap.clone());
        #[cfg(feature = "dynlex-experimental")]
        let content_layer = content_layer.map(|layer| {
            let rgba = Arc::new(layer.rgba);
            *self.content_layer_cache.borrow_mut() = Some(ContentLayerCache {
                base: bitmap.clone(),
                page,
                rgba: Arc::clone(&rgba),
            });
            rgba
        });
        #[cfg(feature = "dynlex-experimental")]
        let bytes = bitmap_bytes + content_layer.as_ref().map_or(0, |rgba| rgba.len());
        #[cfg(not(feature = "dynlex-experimental"))]
        let bytes = bitmap_bytes;
        if bytes > MAX_PAGE_CACHE_BYTES {
            return;
        }
        self.page_cache
            .retain(|item| item.page != page || item.scale_bits != scale.to_bits());
        self.page_cache.push_back(CachedPage {
            page,
            scale_bits: scale.to_bits(),
            bytes,
            bitmap,
            bitmap_size: self.bitmap_size,
            page_points: self.bitmap_page_points,
            backend: outcome.used,
            imported_annotations_dynlex: outcome.imported_annotations_dynlex,
            #[cfg(feature = "dynlex-experimental")]
            content_layer,
        });
        while self.page_cache.iter().map(|item| item.bytes).sum::<usize>() > MAX_PAGE_CACHE_BYTES {
            self.page_cache.pop_front();
        }
    }
}

#[cfg(all(test, feature = "dynlex-experimental"))]
mod dynlex_annotation_overlay_tests {
    use super::*;
    use lopdf::dictionary;

    // Historical mixed-renderer compositor oracle, not the strict UI route.
    fn render_legacy_mixed_oracle(
        pdf: &ParsedPdf,
        source: &[u8],
        page: usize,
        scale: f32,
        backend: Backend,
    ) -> Result<RenderOutcome, String> {
        opds_dynlex_backend::render_with_report(pdf, source, page, scale, backend)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn imported_ap_and_missing_ap_share_one_ordered_dynlex_scene() {
        for rotation in [0, 90, 180, 270] {
            for reverse in [false, true] {
                let mut source = lopdf::Document::load_mem(include_bytes!(
                    "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
                ))
                .unwrap();
                let page_id = *source.get_pages().get(&1).unwrap();
                let contents_id = source
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Contents")
                    .unwrap()
                    .as_reference()
                    .unwrap();
                let page = source.get_dictionary_mut(page_id).unwrap();
                page.set("MediaBox", vec![0.into(), 0.into(), 500.into(), 400.into()]);
                page.set(
                    "CropBox",
                    vec![40.into(), 30.into(), 440.into(), 330.into()],
                );
                page.set("Rotate", rotation);
                source
                    .get_object_mut(contents_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap()
                    .set_plain_content(b"0.2 0.5 0.8 rg 0 0 500 400 re f".to_vec());
                let mut bytes = Vec::new();
                source.save_to(&mut bytes).unwrap();
                let marks = vec![
                    Annotation {
                        id: 1,
                        page: 0,
                        bounds: Rect {
                            x: 50.0,
                            y: 60.0,
                            width: 100.0,
                            height: 80.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [230, 20, 30],
                        },
                    },
                    Annotation {
                        id: 2,
                        page: 0,
                        bounds: Rect {
                            x: 75.0,
                            y: 80.0,
                            width: 90.0,
                            height: 70.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [255, 226, 67],
                        },
                    },
                ];
                let saved = pdf_save::export_annotations(&bytes, &marks).unwrap();
                let mut doc = lopdf::Document::load_mem(&saved).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let ap_ids: Vec<_> = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry.as_reference().unwrap())
                    .collect();
                let rect = doc
                    .get_dictionary(ap_ids[0])
                    .unwrap()
                    .get(b"Rect")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .clone();
                let [x0, y0, x1, y1] = rect.as_slice() else {
                    panic!("AP rect")
                };
                let no_ap_square = doc.add_object(lopdf::dictionary! {
                    "Type" => "Annot", "Subtype" => "Square", "F" => 4,
                    "Rect" => rect.clone(), "C" => vec![0.into(), 1.into(), 0.into()],
                    "BS" => lopdf::dictionary! { "W" => 3 },
                });
                let no_ap_highlight = doc.add_object(lopdf::dictionary! {
                    "Type" => "Annot", "Subtype" => "Highlight", "F" => 4,
                    "Rect" => rect.clone(), "C" => vec![1.into(), 1.into(), 0.into()],
                    "QuadPoints" => vec![x0.clone(), y1.clone(), x1.clone(), y1.clone(),
                        x0.clone(), y0.clone(), x1.clone(), y0.clone()],
                });
                let mut order = vec![no_ap_square, ap_ids[0], no_ap_highlight, ap_ids[1]];
                if reverse {
                    order.reverse();
                }
                doc.get_dictionary_mut(page_id).unwrap().set(
                    "Annots",
                    order
                        .into_iter()
                        .map(lopdf::Object::Reference)
                        .collect::<Vec<_>>(),
                );
                doc.trailer.remove(b"Prev");
                doc.trailer.remove(b"XRefStm");
                bytes.clear();
                doc.save_to(&mut bytes).unwrap();
                if rotation == 90 && !reverse {
                    if let Some(directory) = std::env::var_os("OPDS_MIXED_AP_FIXTURE_DIR") {
                        std::fs::create_dir_all(&directory).unwrap();
                        std::fs::write(
                            std::path::Path::new(&directory).join("mixed-ap-missing-ap.pdf"),
                            &bytes,
                        )
                        .unwrap();
                    }
                }
                let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let selected =
                    render_legacy_mixed_oracle(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                assert_eq!(selected.used, Backend::Hayro);
                assert!(
                    selected.imported_annotations_dynlex,
                    "rotation={rotation}, reverse={reverse}"
                );
                assert_eq!(
                    selected.raster.rgba, expected.rgba,
                    "rotation={rotation}, reverse={reverse}"
                );
                if rotation == 90 && !reverse {
                    let highlight = doc.get_dictionary_mut(no_ap_highlight).unwrap();
                    let mut quad = highlight
                        .get(b"QuadPoints")
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .clone();
                    quad[0] = 999.into();
                    highlight.set("QuadPoints", quad);
                    bytes.clear();
                    doc.save_to(&mut bytes).unwrap();
                    let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                    let expected = pdf.render_page(1, 1.0).unwrap();
                    let selected =
                        render_legacy_mixed_oracle(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(!selected.imported_annotations_dynlex);
                    assert_eq!(selected.raster.rgba, expected.rgba);
                }
            }
        }
    }

    #[test]
    fn imported_highlight_appearance_matches_colored_source_and_square_order() {
        for rotation in [0, 90] {
            for colored in [false, true] {
                for square_first in [false, true] {
                    let mut source = lopdf::Document::load_mem(include_bytes!(
                        "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
                    ))
                    .unwrap();
                    let page_id = *source.get_pages().get(&1).unwrap();
                    let contents_id = source
                        .get_dictionary(page_id)
                        .unwrap()
                        .get(b"Contents")
                        .unwrap()
                        .as_reference()
                        .unwrap();
                    let page = source
                        .get_object_mut(page_id)
                        .unwrap()
                        .as_dict_mut()
                        .unwrap();
                    page.set("MediaBox", vec![0.into(), 0.into(), 500.into(), 400.into()]);
                    page.set(
                        "CropBox",
                        vec![40.into(), 30.into(), 440.into(), 330.into()],
                    );
                    page.set("Rotate", rotation);
                    source
                        .get_object_mut(contents_id)
                        .unwrap()
                        .as_stream_mut()
                        .unwrap()
                        .set_plain_content(if colored {
                            b"0.2 0.5 0.8 rg 0 0 500 400 re f".to_vec()
                        } else {
                            Vec::new()
                        });
                    let mut bytes = Vec::new();
                    source.save_to(&mut bytes).unwrap();
                    let square = Annotation {
                        id: 1,
                        page: 0,
                        bounds: Rect {
                            x: 50.0,
                            y: 60.0,
                            width: 100.0,
                            height: 80.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [230, 20, 30],
                        },
                    };
                    let highlight = Annotation {
                        id: 2,
                        page: 0,
                        bounds: Rect {
                            x: 80.0,
                            y: 85.0,
                            width: 110.0,
                            height: 70.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [255, 226, 67],
                        },
                    };
                    let mut marks = if square_first {
                        vec![square, highlight]
                    } else {
                        vec![highlight, square]
                    };
                    if colored && square_first {
                        marks.push(Annotation {
                            id: 3,
                            page: 0,
                            bounds: Rect {
                                x: 95.0,
                                y: 100.0,
                                width: 85.0,
                                height: 55.0,
                            },
                            kind: Kind::Rectangle {
                                color_rgb: [255, 226, 67],
                            },
                        });
                    }
                    let saved = pdf_save::export_annotations(&bytes, &marks).unwrap();
                    if rotation == 90 && colored && square_first {
                        if let Some(directory) = std::env::var_os("OPDS_IMPORTED_AP_FIXTURE_DIR") {
                            std::fs::create_dir_all(&directory).unwrap();
                            std::fs::write(
                                std::path::Path::new(&directory).join("mixed-imported-ap.pdf"),
                                &saved,
                            )
                            .unwrap();
                        }
                    }
                    let expected = ParsedPdf::open(Arc::new(saved.clone()))
                        .unwrap()
                        .render_page(1, 1.0)
                        .unwrap();
                    let direct =
                        opds_dynlex_backend::render_native_shape_appearances_direct(&saved, 1)
                            .unwrap()
                            .expect("native Square and Highlight appearances");
                    assert_eq!(
                        direct.rgba, expected.rgba,
                        "rotation={rotation}, colored={colored}, square_first={square_first}"
                    );
                    let parsed = ParsedPdf::open(Arc::new(saved.clone())).unwrap();
                    let once = opds_dynlex_backend::render_native_shape_appearances_for_app(
                        &parsed, &saved, 1,
                    )
                    .unwrap()
                    .expect("one content render plus DynLex imported annotations");
                    assert_eq!(
                        once.rgba, expected.rgba,
                        "single render: rotation={rotation}, colored={colored}, square_first={square_first}"
                    );
                    let selected =
                        render_legacy_mixed_oracle(&parsed, &saved, 1, 1.0, Backend::Dynlex)
                            .unwrap();
                    assert_eq!(selected.used, Backend::Hayro);
                    assert!(selected.imported_annotations_dynlex);
                    assert_eq!(selected.raster.rgba, expected.rgba);
                    assert!(
                        opds_dynlex_backend::render_square_appearances_direct(&saved, 1)
                            .unwrap()
                            .is_none()
                    );
                    if rotation == 0 && !colored && square_first {
                        let mut malformed = lopdf::Document::load_mem(&saved).unwrap();
                        let annotation_id = malformed
                            .get_dictionary(page_id)
                            .unwrap()
                            .get(b"Annots")
                            .unwrap()
                            .as_array()
                            .unwrap()[1]
                            .as_reference()
                            .unwrap();
                        malformed
                            .get_object_mut(annotation_id)
                            .unwrap()
                            .as_dict_mut()
                            .unwrap()
                            .set("QuadPoints", Vec::<lopdf::Object>::new());
                        malformed.trailer.remove(b"Prev");
                        malformed.trailer.remove(b"XRefStm");
                        let mut malformed_bytes = Vec::new();
                        malformed.save_to(&mut malformed_bytes).unwrap();
                        assert!(opds_dynlex_backend::render_native_shape_appearances_direct(
                            &malformed_bytes,
                            1
                        )
                        .unwrap()
                        .is_none());
                        let malformed_pdf =
                            ParsedPdf::open(Arc::new(malformed_bytes.clone())).unwrap();
                        let fallback = render_legacy_mixed_oracle(
                            &malformed_pdf,
                            &malformed_bytes,
                            1,
                            1.0,
                            Backend::Dynlex,
                        )
                        .unwrap();
                        assert!(!fallback.imported_annotations_dynlex);
                    }
                }
            }
        }
    }

    #[test]
    fn imported_squares_keep_pdf_order_over_color_and_rotated_crop() {
        for rotation in [0, 90, 180, 270] {
            for reverse in [false, true] {
                let mut source = lopdf::Document::load_mem(include_bytes!(
                    "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
                ))
                .unwrap();
                let page_id = *source.get_pages().get(&1).unwrap();
                let contents_id = source
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Contents")
                    .unwrap()
                    .as_reference()
                    .unwrap();
                let page = source
                    .get_object_mut(page_id)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap();
                page.set("MediaBox", vec![0.into(), 0.into(), 500.into(), 400.into()]);
                page.set(
                    "CropBox",
                    vec![40.into(), 30.into(), 440.into(), 330.into()],
                );
                page.set("Rotate", rotation);
                source
                    .get_object_mut(contents_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap()
                    .set_plain_content(b"0.2 0.5 0.8 rg 0 0 500 400 re f".to_vec());
                let mut bytes = Vec::new();
                source.save_to(&mut bytes).unwrap();
                let mut marks = vec![
                    Annotation {
                        id: 1,
                        page: 0,
                        bounds: Rect {
                            x: 50.0,
                            y: 60.0,
                            width: 120.0,
                            height: 80.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [250, 20, 20],
                        },
                    },
                    Annotation {
                        id: 2,
                        page: 0,
                        bounds: Rect {
                            x: 70.0,
                            y: 80.0,
                            width: 120.0,
                            height: 80.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [20, 200, 40],
                        },
                    },
                    Annotation {
                        id: 3,
                        page: 0,
                        bounds: Rect {
                            x: 200.0,
                            y: 150.0,
                            width: 60.0,
                            height: 50.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [25, 30, 225],
                        },
                    },
                ];
                if reverse {
                    marks.reverse();
                }
                let saved = pdf_save::export_annotations(&bytes, &marks).unwrap();
                let expected = ParsedPdf::open(Arc::new(saved.clone()))
                    .unwrap()
                    .render_page(1, 1.0)
                    .unwrap();
                let direct = opds_dynlex_backend::render_square_appearances_direct(&saved, 1)
                    .unwrap()
                    .expect("three supported Square appearances");
                assert_eq!(
                    direct.rgba, expected.rgba,
                    "rotation={rotation}, reverse={reverse}"
                );
                let parsed = ParsedPdf::open(Arc::new(saved.clone())).unwrap();
                let once = opds_dynlex_backend::render_native_shape_appearances_for_app(
                    &parsed, &saved, 1,
                )
                .unwrap()
                .expect("one content render plus DynLex imported annotations");
                assert_eq!(
                    once.rgba, expected.rgba,
                    "single render: rotation={rotation}, reverse={reverse}"
                );
                let selected =
                    render_legacy_mixed_oracle(&parsed, &saved, 1, 1.0, Backend::Dynlex).unwrap();
                assert_eq!(selected.used, Backend::Hayro);
                assert!(selected.imported_annotations_dynlex);
                assert_eq!(selected.raster.rgba, expected.rgba);
                assert!(
                    opds_dynlex_backend::render_single_square_appearance_direct(&saved, 1)
                        .unwrap()
                        .is_none()
                );
                if rotation == 0 && !reverse {
                    let mut missing_ap = lopdf::Document::load_mem(&saved).unwrap();
                    let annotation_id = missing_ap
                        .get_dictionary(page_id)
                        .unwrap()
                        .get(b"Annots")
                        .unwrap()
                        .as_array()
                        .unwrap()[1]
                        .as_reference()
                        .unwrap();
                    missing_ap
                        .get_object_mut(annotation_id)
                        .unwrap()
                        .as_dict_mut()
                        .unwrap()
                        .remove(b"AP");
                    missing_ap.trailer.remove(b"Prev");
                    missing_ap.trailer.remove(b"XRefStm");
                    let mut missing_bytes = Vec::new();
                    missing_ap.save_to(&mut missing_bytes).unwrap();
                    assert!(opds_dynlex_backend::render_square_appearances_direct(
                        &missing_bytes,
                        1
                    )
                    .unwrap()
                    .is_none());
                }
            }
        }
    }

    #[test]
    fn imported_square_direct_matches_all_quarter_turns_and_rejects_unknown_state() {
        for rotation in [0, 90, 180, 270] {
            let mut source = lopdf::Document::load_mem(include_bytes!(
                "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
            ))
            .unwrap();
            let page_id = *source.get_pages().get(&1).unwrap();
            let contents_id = source
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .as_reference()
                .unwrap();
            let page = source
                .get_object_mut(page_id)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            page.set("MediaBox", vec![0.into(), 0.into(), 500.into(), 400.into()]);
            page.set(
                "CropBox",
                vec![40.into(), 30.into(), 440.into(), 330.into()],
            );
            page.set("Rotate", rotation);
            source
                .get_object_mut(contents_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(Vec::new());
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let imported = Annotation {
                id: 1,
                page: 0,
                bounds: Rect {
                    x: 50.0,
                    y: 60.0,
                    width: 76.0,
                    height: 38.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [250, 20, 20],
                },
            };
            let saved = pdf_save::export_annotations(&bytes, &[imported]).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved.clone()))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let direct = opds_dynlex_backend::render_single_square_appearance_direct(&saved, 1)
                .unwrap()
                .expect("supported Square appearance");
            assert_eq!(direct.rgba, expected.rgba, "rotation={rotation}");

            let mut malformed = lopdf::Document::load_mem(&saved).unwrap();
            malformed
                .get_object_mut(page_id)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", lopdf::Object::Name(b"wrong".to_vec()));
            malformed.trailer.remove(b"Prev");
            malformed.trailer.remove(b"XRefStm");
            let mut malformed_bytes = Vec::new();
            malformed.save_to(&mut malformed_bytes).unwrap();
            assert!(opds_dynlex_backend::render_single_square_appearance_direct(
                &malformed_bytes,
                1
            )
            .unwrap()
            .is_none());

            let mut changed_ap = lopdf::Document::load_mem(&saved).unwrap();
            let annotation_id = changed_ap
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Annots")
                .unwrap()
                .as_array()
                .unwrap()[0]
                .as_reference()
                .unwrap();
            let form_id = changed_ap
                .get_dictionary(annotation_id)
                .unwrap()
                .get(b"AP")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"N")
                .unwrap()
                .as_reference()
                .unwrap();
            changed_ap
                .get_object_mut(form_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(b"q 0 0 0 rg 0 0 10 10 re f Q".to_vec());
            changed_ap.trailer.remove(b"Prev");
            changed_ap.trailer.remove(b"XRefStm");
            let mut changed_bytes = Vec::new();
            changed_ap.save_to(&mut changed_bytes).unwrap();
            assert!(
                opds_dynlex_backend::render_single_square_appearance_direct(&changed_bytes, 1)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn large_page_tiled_text_boxes_match_saved_pdf() {
        for (rotated, content, x, y, a1, (wide, tall)) in [
            (
                false,
                b"0.2 0.5 0.8 rg 0 0 650 900 re f".as_slice(),
                330.0,
                480.0,
                false,
                (false, false),
            ),
            (
                false,
                b"0.2 0.5 0.8 rg 0 0 650 900 re f".as_slice(),
                279.0,
                205.0,
                false,
                (false, false),
            ),
            (
                true,
                b"0.2 0.5 0.8 rg 270 210 260 240 re f".as_slice(),
                500.0,
                330.0,
                false,
                (false, false),
            ),
            (
                true,
                b"0.2 0.5 0.8 rg 270 210 260 240 re f".as_slice(),
                261.0,
                203.0,
                false,
                (false, false),
            ),
            (
                true,
                b"0.2 0.5 0.8 rg 0 0 1684 2384 re f".as_slice(),
                1800.0,
                1200.0,
                true,
                (false, false),
            ),
            (
                false,
                b"0.2 0.5 0.8 rg 0 0 650 900 re f".as_slice(),
                80.0,
                240.0,
                false,
                (true, false),
            ),
            (
                true,
                b"0.2 0.5 0.8 rg 270 210 260 240 re f".as_slice(),
                300.0,
                150.0,
                false,
                (true, false),
            ),
            (
                false,
                b"0.2 0.5 0.8 rg 0 0 650 900 re f".as_slice(),
                100.0,
                100.0,
                false,
                (false, true),
            ),
            (false, b"".as_slice(), 150.0, 220.0, false, (false, false)),
        ] {
            let mut source = lopdf::Document::load_mem(include_bytes!(
                "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
            ))
            .unwrap();
            let page_id = *source.get_pages().get(&1).unwrap();
            let contents_id = source
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .as_reference()
                .unwrap();
            let page = source
                .get_object_mut(page_id)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            if a1 {
                page.set(
                    "MediaBox",
                    vec![0.into(), 0.into(), 1684.into(), 2384.into()],
                );
                page.set(
                    "CropBox",
                    vec![0.into(), 0.into(), 1684.into(), 2384.into()],
                );
            } else if content.is_empty() {
                page.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);
                page.remove(b"CropBox");
            } else {
                page.set("MediaBox", vec![0.into(), 0.into(), 650.into(), 900.into()]);
                page.set(
                    "CropBox",
                    vec![40.into(), 30.into(), 635.into(), 872.into()],
                );
            }
            page.set("Rotate", if rotated { 90 } else { 0 });
            source
                .get_object_mut(contents_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(content.to_vec());
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            // Keep one imported /AP below the new native annotations. The
            // rotated mixed-content case exercises source alpha and order.
            let with_existing_ap = (rotated && wide) || content.is_empty();
            if with_existing_ap {
                let imported = Annotation {
                    id: 91,
                    page: 0,
                    bounds: Rect {
                        x: x + 55.0,
                        y: y + 12.0,
                        width: 76.0,
                        height: 38.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: [250, 20, 20],
                    },
                };
                bytes = pdf_save::export_annotations(&bytes, &[imported]).unwrap();
            }
            let pdf = Arc::new(ParsedPdf::open(Arc::new(bytes.clone())).unwrap());
            let (outcome, layer) = render_page_with_content_layer(&pdf, 1, 1.0).unwrap();
            let (width, height) = (outcome.raster.width, outcome.raster.height);
            if with_existing_ap {
                let direct = opds_dynlex_backend::render_single_square_appearance_direct(&bytes, 1)
                    .unwrap()
                    .expect("imported Square /AP should be painted directly by DynLex");
                assert_eq!(direct.rgba, outcome.raster.rgba);
            }
            if with_existing_ap {
                assert!(native_text_scene::eligible_mixed_page(
                    &bytes, 0, width, height
                ));
                let mut no_appearance = lopdf::Document::load_mem(&bytes).unwrap();
                let page_id = *no_appearance.get_pages().get(&1).unwrap();
                let annotation_id = no_appearance
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                no_appearance
                    .get_object_mut(annotation_id)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap()
                    .remove(b"AP");
                let mut no_ap_bytes = Vec::new();
                no_appearance.save_to(&mut no_ap_bytes).unwrap();
                assert!(!native_text_scene::eligible_mixed_page(
                    &no_ap_bytes,
                    0,
                    width,
                    height
                ));
                let mut wrong_form = lopdf::Document::load_mem(&bytes).unwrap();
                let form_id = wrong_form
                    .get_dictionary(annotation_id)
                    .unwrap()
                    .get(b"AP")
                    .unwrap()
                    .as_dict()
                    .unwrap()
                    .get(b"N")
                    .unwrap()
                    .as_reference()
                    .unwrap();
                wrong_form
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap()
                    .dict
                    .set("Subtype", "Image");
                let mut wrong_form_bytes = Vec::new();
                wrong_form.save_to(&mut wrong_form_bytes).unwrap();
                assert!(!native_text_scene::eligible_mixed_page(
                    &wrong_form_bytes,
                    0,
                    width,
                    height
                ));
            }
            assert_eq!(
                (width, height),
                if a1 {
                    (2384, 1684)
                } else if rotated {
                    (842, 595)
                } else {
                    (595, 842)
                }
            );
            let marks = [
                Annotation {
                    id: 1,
                    page: 0,
                    bounds: Rect {
                        x,
                        y,
                        width: if wide {
                            410.0
                        } else if tall {
                            200.0
                        } else {
                            120.0
                        },
                        height: if tall { 350.0 } else { 60.0 },
                    },
                    kind: Kind::Text {
                        value: if wide {
                            "Ω".repeat(25)
                        } else if tall {
                            "ΩΩ\nΩΩ\nΩΩ\nΩΩ".into()
                        } else {
                            "ΩΩΩ".into()
                        },
                        color_rgb: [0, 80, 240],
                        font_size: if tall { 72 } else { 20 },
                        align: TextAlign::Center,
                    },
                },
                Annotation {
                    id: 2,
                    page: 0,
                    bounds: Rect {
                        x: x + 45.0,
                        y: y + 10.0,
                        width: 45.0,
                        height: 30.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: [255, 226, 67],
                    },
                },
                Annotation {
                    id: 3,
                    page: 0,
                    bounds: Rect {
                        x: x + 24.0,
                        y: y + 4.0,
                        width: 130.0,
                        height: 60.0,
                    },
                    kind: Kind::Text {
                        value: "Hello Ω".into(),
                        color_rgb: [231, 76, 60],
                        font_size: 16,
                        align: TextAlign::Right,
                    },
                },
            ];
            let mut model = DocumentModel::new(DocumentId(102), 1).unwrap();
            for mark in &marks {
                model.apply(Edit::Add(mark.clone())).unwrap();
            }
            if wide || tall {
                assert!(
                    native_text_scene::stage_native_text_box_tiles(&marks[0])
                        .unwrap()
                        .len()
                        > 1
                );
            }
            let mut document = OpenDocument {
                path: "large-tiled-text.pdf".into(),
                is_new: false,
                source_bytes: Arc::new(bytes),
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            };
            document.cache_bitmap(0, 1.0, outcome, Some(layer));
            let combined = document
                .dynlex_annotation_bitmap()
                .expect("large page with movable glyph tile should reach DynLex");
            let saved = pdf_save::export_annotations(&document.source_bytes, &marks).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let image::Handle::Rgba { pixels, .. } = combined else {
                panic!("large DynLex annotation result must be RGBA");
            };
            let mut wrong_count = 0;
            let mut first_wrong = Vec::new();
            for (at, (actual, expected)) in pixels.iter().zip(&expected.rgba).enumerate() {
                if actual != expected {
                    wrong_count += 1;
                    if first_wrong.len() < 16 {
                        first_wrong.push((at, *actual, *expected));
                    }
                }
            }
            assert!(
                wrong_count == 0,
                "rotation={rotated} at ({x},{y}): {} differing channels, first {:?}",
                wrong_count,
                first_wrong
            );
        }
    }

    #[test]
    fn two_overlapping_text_boxes_on_blank_page_follow_id_order() {
        let mut source = lopdf::Document::load_mem(include_bytes!(
            "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
        ))
        .unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        source
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(Vec::new());
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        let pdf = Arc::new(ParsedPdf::open(Arc::new(bytes.clone())).unwrap());
        let raster = pdf.render_page(1, 1.0).unwrap();
        let first = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "ΩΩΩ".into(),
                color_rgb: [0, 80, 240],
                font_size: 20,
                align: TextAlign::Center,
            },
        };
        let second = Annotation {
            id: 2,
            page: 0,
            bounds: Rect {
                x: 50.0,
                y: 32.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "Hello Ω".into(),
                color_rgb: [231, 76, 60],
                font_size: 16,
                align: TextAlign::Right,
            },
        };
        let mut model = DocumentModel::new(DocumentId(101), 1).unwrap();
        model.apply(Edit::Add(first.clone())).unwrap();
        model.apply(Edit::Add(second.clone())).unwrap();
        let mut document = OpenDocument {
            path: "blank-two-text.pdf".into(),
            is_new: false,
            source_bytes: Arc::new(bytes),
            dynlex_document: None,
            pdf,
            model,
            last_saved_annotations: Vec::new(),
            bitmap: None,
            annotation_overlay_cache: RefCell::new(None),
            content_layer_cache: RefCell::new(None),
            content_layer_in_flight: None,
            bitmap_size: (0, 0),
            bitmap_page_points: (0.0, 0.0),
            bitmap_scale: 1.0,
            bitmap_backend: Backend::Hayro,
            imported_annotations_dynlex: false,
            displayed_page: 0,
            selected_annotation: None,
            page_cache: VecDeque::new(),
            thumbnails: VecDeque::new(),
            thumbnail_in_flight: None,
            thumbnail_failed: HashSet::new(),
            thumbnail_focus: 0,
        };
        document.cache_bitmap(
            0,
            1.0,
            RenderOutcome {
                raster,
                used: Backend::Hayro,
                imported_annotations_dynlex: false,
            },
            None,
        );
        let combined = document
            .dynlex_annotation_bitmap()
            .expect("blank page with two Text Boxes should reach DynLex");
        let saved = pdf_save::export_annotations(&document.source_bytes, &[first, second]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("blank multi-text DynLex result must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
    }

    #[test]
    fn colored_highlight_waits_for_opaque_content_proof() {
        let mut source = lopdf::Document::load_mem(include_bytes!(
            "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
        ))
        .unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        source
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0.2 0.4 0.7 rg 0 0 256 192 re f".to_vec());
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        assert!(native_text_scene::eligible_annotation_content_layer(
            &bytes, 0, 256, 192
        ));
        let pdf = Arc::new(ParsedPdf::open(Arc::new(bytes.clone())).unwrap());
        let raster = pdf.render_page(1, 1.0).unwrap();
        let mark = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 21.0,
                y: 31.0,
                width: 41.0,
                height: 29.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let mut model = DocumentModel::new(DocumentId(73), 1).unwrap();
        model.apply(Edit::Add(mark.clone())).unwrap();
        let mut document = OpenDocument {
            path: "opaque-color.pdf".into(),
            is_new: false,
            source_bytes: Arc::new(bytes),
            dynlex_document: None,
            pdf,
            model,
            last_saved_annotations: Vec::new(),
            bitmap: None,
            annotation_overlay_cache: RefCell::new(None),
            content_layer_cache: RefCell::new(None),
            content_layer_in_flight: None,
            bitmap_size: (0, 0),
            bitmap_page_points: (0.0, 0.0),
            bitmap_scale: 1.0,
            bitmap_backend: Backend::Hayro,
            imported_annotations_dynlex: false,
            displayed_page: 0,
            selected_annotation: None,
            page_cache: VecDeque::new(),
            thumbnails: VecDeque::new(),
            thumbnail_in_flight: None,
            thumbnail_failed: HashSet::new(),
            thumbnail_focus: 0,
        };
        document.cache_bitmap(
            0,
            1.0,
            RenderOutcome {
                raster,
                used: Backend::Hayro,
                imported_annotations_dynlex: false,
            },
            None,
        );
        assert!(document.dynlex_annotation_bitmap().is_none());
        let ticket = document.model.view().ticket();
        let base = document.bitmap.as_ref().unwrap().clone();
        let layer = document.pdf.render_page_content_layer(1, 1.0).unwrap();
        let mut app = App {
            backend: Backend::Dynlex,
            next_load: 73,
            document: Some(document),
            ..Default::default()
        };
        let _task = app.render_legacy_content_layer_oracle();
        assert_eq!(
            app.document.as_ref().unwrap().content_layer_in_flight,
            Some(ticket)
        );
        let _ = app.update(Message::ContentLayerRendered(
            73,
            0,
            ticket,
            base,
            Ok(layer),
        ));
        let document = app.document.as_ref().unwrap();
        let combined = document
            .dynlex_annotation_bitmap()
            .expect("verified opaque content");
        let saved = pdf_save::export_annotations(&document.source_bytes, &[mark.clone()]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("DynLex annotation output must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());

        let document = app.document.as_mut().unwrap();
        let square = Annotation {
            id: 2,
            page: 0,
            bounds: Rect {
                x: 41.0,
                y: 43.0,
                width: 48.0,
                height: 36.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        document.model.apply(Edit::Add(square.clone())).unwrap();
        let overlapped = document
            .dynlex_annotation_bitmap()
            .expect("cached source proof also covers overlapping shapes");
        let saved =
            pdf_save::export_annotations(&document.source_bytes, &[mark.clone(), square.clone()])
                .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = overlapped else {
            panic!("DynLex overlap output must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());

        let text = Annotation {
            id: 3,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "ΩΩΩ".into(),
                color_rgb: [0, 80, 240],
                font_size: 20,
                align: TextAlign::Center,
            },
        };
        document.model.apply(Edit::Add(text.clone())).unwrap();
        let ordered = document
            .dynlex_annotation_bitmap()
            .expect("Text Box after overlapping shapes should use ordered DynLex paint");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &[mark.clone(), square.clone(), text.clone()],
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = ordered else {
            panic!("DynLex ordered Text Box output must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());

        let mut text_first = text;
        text_first.id = 1;
        let mut highlight_after = mark;
        highlight_after.id = 2;
        let mut square_after = square;
        square_after.id = 3;
        let mut reordered = DocumentModel::new(DocumentId(74), 1).unwrap();
        for item in [
            text_first.clone(),
            highlight_after.clone(),
            square_after.clone(),
        ] {
            reordered.apply(Edit::Add(item)).unwrap();
        }
        document.model = reordered;
        document.annotation_overlay_cache.borrow_mut().take();
        let ordered = document
            .dynlex_annotation_bitmap()
            .expect("overlapping Text Box before shapes should use ordered DynLex paint");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &[text_first, highlight_after, square_after],
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = ordered else {
            panic!("DynLex reordered Text Box output must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());

        document
            .model
            .apply(Edit::Add(Annotation {
                id: 4,
                page: 0,
                bounds: Rect {
                    x: 60.0,
                    y: 35.0,
                    width: 110.0,
                    height: 58.0,
                },
                kind: Kind::Text {
                    value: "Hello Ω".into(),
                    color_rgb: [231, 76, 60],
                    font_size: 16,
                    align: TextAlign::Right,
                },
            }))
            .unwrap();
        let ordered = document
            .dynlex_annotation_bitmap()
            .expect("two overlapping Text Boxes and shapes should reach DynLex");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &document.model.items().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = ordered else {
            panic!("multi-text DynLex output must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
    }

    #[test]
    fn one_transparent_render_recovers_visible_page_and_text_overlay() {
        let source =
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = Arc::new(ParsedPdf::open(Arc::new(source.clone())).unwrap());
        let reference = pdf.render_page(1, 1.0).unwrap();
        assert!(native_text_scene::eligible_mixed_page(&source, 0, 256, 192));
        let (outcome, layer) = render_page_with_content_layer(&pdf, 1, 1.0).unwrap();
        assert_eq!(outcome.raster.rgba, reference.rgba);
        assert!(layer.rgba.chunks_exact(4).any(|pixel| pixel[3] < 255));
        let text = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 105.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "ΩΩΩ".into(),
                color_rgb: [0, 80, 240],
                font_size: 20,
                align: TextAlign::Center,
            },
        };
        let mut model = DocumentModel::new(DocumentId(99), 1).unwrap();
        model.apply(Edit::Add(text.clone())).unwrap();
        let mut document = OpenDocument {
            path: "fixture.pdf".into(),
            is_new: false,
            source_bytes: Arc::new(source),
            dynlex_document: None,
            pdf,
            model,
            last_saved_annotations: Vec::new(),
            bitmap: None,
            annotation_overlay_cache: RefCell::new(None),
            content_layer_cache: RefCell::new(None),
            content_layer_in_flight: None,
            bitmap_size: (0, 0),
            bitmap_page_points: (0.0, 0.0),
            bitmap_scale: 1.0,
            bitmap_backend: Backend::Hayro,
            imported_annotations_dynlex: false,
            displayed_page: 0,
            selected_annotation: None,
            page_cache: VecDeque::new(),
            thumbnails: VecDeque::new(),
            thumbnail_in_flight: None,
            thumbnail_failed: HashSet::new(),
            thumbnail_focus: 0,
        };
        document.cache_bitmap(0, 1.0, outcome, Some(layer));
        assert!(document.page_cache[0].content_layer.is_some());
        let combined = document.dynlex_annotation_bitmap().unwrap();
        let saved = pdf_save::export_annotations(&document.source_bytes, &[text]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("DynLex result must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        let text = document.model.get(1).unwrap().clone();
        let highlight = Annotation {
            id: 2,
            page: 0,
            bounds: Rect {
                x: 55.0,
                y: 115.0,
                width: 40.0,
                height: 30.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        document.model.apply(Edit::Add(highlight.clone())).unwrap();
        let combined = document
            .dynlex_annotation_bitmap()
            .expect("partial PDF layer must support Highlight after Text Box");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &[text.clone(), highlight.clone()],
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("ordered DynLex result must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        let mut earlier = highlight;
        earlier.id = 1;
        let mut later = text;
        later.id = 2;
        let mut reordered = DocumentModel::new(DocumentId(100), 1).unwrap();
        reordered.apply(Edit::Add(earlier.clone())).unwrap();
        reordered.apply(Edit::Add(later.clone())).unwrap();
        document.model = reordered;
        document.annotation_overlay_cache.borrow_mut().take();
        let combined = document
            .dynlex_annotation_bitmap()
            .expect("partial PDF layer must support Highlight before Text Box");
        let saved =
            pdf_save::export_annotations(&document.source_bytes, &[earlier, later]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("reordered DynLex result must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        document
            .model
            .apply(Edit::Add(Annotation {
                id: 3,
                page: 0,
                bounds: Rect {
                    x: 45.0,
                    y: 107.0,
                    width: 110.0,
                    height: 58.0,
                },
                kind: Kind::Text {
                    value: "Hello Ω".into(),
                    color_rgb: [231, 76, 60],
                    font_size: 16,
                    align: TextAlign::Right,
                },
            }))
            .unwrap();
        let combined = document
            .dynlex_annotation_bitmap()
            .expect("two Text Boxes over a partial PDF layer should reach DynLex");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &document.model.items().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("multi-text premultiplied DynLex result must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        let layer = Arc::clone(&document.content_layer_cache.borrow().as_ref().unwrap().rgba);
        document.content_layer_cache.borrow_mut().take();
        let mut app = App {
            backend: Backend::Dynlex,
            document: Some(document),
            ..Default::default()
        };
        let _ = app.render_current_page();
        assert!(Arc::ptr_eq(
            &layer,
            &app.document
                .as_ref()
                .unwrap()
                .content_layer_cache
                .borrow()
                .as_ref()
                .unwrap()
                .rgba,
        ));
    }

    #[test]
    fn overlay_cache_tracks_edits_and_page_bitmap() {
        let source =
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = Arc::new(ParsedPdf::open(Arc::new(source.clone())).unwrap());
        let raster = pdf.render_page(1, 1.0).unwrap();
        let mut model = DocumentModel::new(DocumentId(48), pdf.page_count()).unwrap();
        let mark = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 12.0,
                y: 16.0,
                width: 40.0,
                height: 28.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        model.apply(Edit::Add(mark.clone())).unwrap();
        let mut document = OpenDocument {
            path: "fixture.pdf".into(),
            is_new: false,
            source_bytes: Arc::new(source),
            dynlex_document: None,
            pdf,
            model,
            last_saved_annotations: Vec::new(),
            bitmap: None,
            annotation_overlay_cache: RefCell::new(None),
            #[cfg(feature = "dynlex-experimental")]
            content_layer_cache: RefCell::new(None),
            #[cfg(feature = "dynlex-experimental")]
            content_layer_in_flight: None,
            bitmap_size: (0, 0),
            bitmap_page_points: (0.0, 0.0),
            bitmap_scale: 1.0,
            bitmap_backend: Backend::Hayro,
            imported_annotations_dynlex: false,
            displayed_page: 0,
            selected_annotation: None,
            page_cache: VecDeque::new(),
            thumbnails: VecDeque::new(),
            thumbnail_in_flight: None,
            thumbnail_failed: HashSet::new(),
            thumbnail_focus: 0,
        };
        document.cache_bitmap(
            0,
            1.0,
            RenderOutcome {
                raster,
                used: Backend::Hayro,
                imported_annotations_dynlex: false,
            },
            None,
        );
        let first = document.dynlex_annotation_bitmap().expect("bounded mark");
        assert!(document.dynlex_annotation_bitmap() == Some(first.clone()));
        let selected = document
            .dynlex_selected_annotation_bitmap(1)
            .expect("selected mark uses DynLex controls");
        assert_ne!(selected, first);
        assert_eq!(
            document.dynlex_selected_annotation_bitmap(1),
            Some(selected.clone())
        );
        let image::Handle::Rgba { pixels, .. } = selected else {
            panic!("selected annotation bitmap must be RGBA");
        };
        assert_eq!(&pixels[((16 * 256 + 12) * 4)..][..4], &[31, 136, 255, 255]);
        // The selected-only centered Square stroke sits under a 75%-covered
        // blue edge, matching Iced's linear-light blend on this red mark.
        assert_eq!(&pixels[((15 * 256 + 17) * 4)..][..4], &[127, 124, 226, 255]);
        let mut moved = mark;
        moved.bounds.x = 60.0;
        document.model.apply(Edit::Replace(moved)).unwrap();
        let second = document.dynlex_annotation_bitmap().expect("moved mark");
        assert!(first != second, "edit must invalidate the derived image");
        let selected_after_move = document
            .dynlex_selected_annotation_bitmap(1)
            .expect("selection follows moved mark");
        let image::Handle::Rgba { pixels, .. } = selected_after_move else {
            panic!("moved annotation bitmap must be RGBA");
        };
        assert_eq!(&pixels[((16 * 256 + 60) * 4)..][..4], &[31, 136, 255, 255]);

        let text = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "ΩΩΩ".into(),
                color_rgb: [0, 80, 240],
                font_size: 20,
                align: TextAlign::Center,
            },
        };
        document.model.apply(Edit::Replace(text)).unwrap();
        document.bitmap = Some(image::Handle::from_rgba(256, 192, vec![255; 256 * 192 * 4]));
        document.bitmap_size = (256, 192);
        document.bitmap_page_points = (256.0, 192.0);
        // A white bitmap alone cannot prove the source page was blank. This
        // fixture has other PDF drawing operations, so text stays on Iced.
        assert!(document.dynlex_annotation_bitmap().is_none());

        let mut actual_text = document.model.get(1).unwrap().clone();
        actual_text.bounds.y = 105.0;
        document
            .model
            .apply(Edit::Replace(actual_text.clone()))
            .unwrap();
        let actual_base = document.pdf.render_page(1, 1.0).unwrap();
        document.cache_bitmap(
            0,
            1.0,
            RenderOutcome {
                raster: actual_base,
                used: Backend::Hayro,
                imported_annotations_dynlex: false,
            },
            None,
        );
        assert!(document.dynlex_annotation_bitmap().is_none());
        let ticket = document.model.view().ticket();
        let base = document.bitmap.as_ref().unwrap().clone();
        let layer = document.pdf.render_page_content_layer(1, 1.0).unwrap();
        let mut app = App {
            backend: Backend::Dynlex,
            next_load: 48,
            document: Some(document),
            ..Default::default()
        };
        let _task = app.render_legacy_content_layer_oracle();
        assert_eq!(
            app.document.as_ref().unwrap().content_layer_in_flight,
            Some(ticket)
        );
        let _ = app.update(Message::ContentLayerRendered(
            48,
            1,
            ticket,
            base.clone(),
            Ok(layer.clone()),
        ));
        assert!(app
            .document
            .as_ref()
            .unwrap()
            .content_layer_cache
            .borrow()
            .is_none());
        let _ = app.update(Message::ContentLayerRendered(
            48,
            0,
            ticket,
            base,
            Ok(layer),
        ));
        let document = app.document.as_mut().unwrap();
        let mixed = document
            .dynlex_annotation_bitmap()
            .expect("small vector PDF and Text Box should reach DynLex");
        assert!(document.content_layer_cache.borrow().is_some());
        assert_eq!(document.dynlex_annotation_bitmap(), Some(mixed.clone()));
        let saved = pdf_save::export_annotations(&document.source_bytes, &[actual_text]).unwrap();
        let hayro = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = mixed else {
            panic!("DynLex annotation output must be RGBA");
        };
        let differences = pixels
            .iter()
            .zip(&hayro.rgba)
            .filter(|(actual, expected)| actual != expected)
            .count();
        assert_eq!(differences, 0, "mixed vector PDF/annotation mismatch");
        let layer_ptr = document
            .content_layer_cache
            .borrow()
            .as_ref()
            .unwrap()
            .rgba
            .as_ptr();
        let mut recolored = document.model.get(1).unwrap().clone();
        if let Kind::Text { color_rgb, .. } = &mut recolored.kind {
            *color_rgb = [255, 0, 0];
        }
        document.model.apply(Edit::Replace(recolored)).unwrap();
        assert!(document.dynlex_annotation_bitmap().is_some());
        assert_eq!(
            document
                .content_layer_cache
                .borrow()
                .as_ref()
                .unwrap()
                .rgba
                .as_ptr(),
            layer_ptr,
            "text edit must reuse unchanged PDF content layer"
        );
        document
            .model
            .apply(Edit::Add(Annotation {
                id: 2,
                page: 0,
                bounds: Rect {
                    x: 170.0,
                    y: 20.0,
                    width: 30.0,
                    height: 20.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [0, 80, 240],
                },
            }))
            .unwrap();
        document
            .model
            .apply(Edit::Add(Annotation {
                id: 3,
                page: 0,
                bounds: Rect {
                    x: 170.0,
                    y: 70.0,
                    width: 30.0,
                    height: 20.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            }))
            .unwrap();
        let combined = document
            .dynlex_annotation_bitmap()
            .expect("disjoint text, square and highlight should reach DynLex");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &document.model.items().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = combined else {
            panic!("combined DynLex annotations must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        let mut second_square = document.model.get(3).unwrap().clone();
        second_square.kind = Kind::Rectangle {
            color_rgb: [231, 76, 60],
        };
        second_square.bounds.x = 185.0;
        second_square.bounds.y = 30.0;
        document.model.apply(Edit::Replace(second_square)).unwrap();
        let opaque_overlap = document
            .dynlex_annotation_bitmap()
            .expect("overlapping opaque squares remain supported beside text");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &document.model.items().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = opaque_overlap else {
            panic!("combined DynLex annotations must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        let mut overlapping = document.model.get(3).unwrap().clone();
        overlapping.bounds.x = 110.0;
        overlapping.bounds.y = 120.0;
        document.model.apply(Edit::Replace(overlapping)).unwrap();
        let ordered_overlap = document
            .dynlex_annotation_bitmap()
            .expect("verified premultiplied text/square overlap should reach DynLex");
        let saved = pdf_save::export_annotations(
            &document.source_bytes,
            &document.model.items().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let image::Handle::Rgba { pixels, .. } = ordered_overlap else {
            panic!("ordered DynLex annotations must be RGBA");
        };
        assert_eq!(pixels.as_ref(), expected.rgba.as_slice());
        let stale_ticket = document.model.view().ticket();
        let stale_base = document.bitmap.as_ref().unwrap().clone();
        let stale_layer = document.pdf.render_page_content_layer(1, 1.0).unwrap();
        document.content_layer_in_flight = Some(stale_ticket);
        document.model.zoom_at(1.25, [0.0, 0.0]).unwrap();
        let _ = app.update(Message::ContentLayerRendered(
            48,
            0,
            stale_ticket,
            stale_base,
            Ok(stale_layer),
        ));
        let document = app.document.as_ref().unwrap();
        assert_eq!(document.content_layer_in_flight, None);
        assert_eq!(
            document
                .content_layer_cache
                .borrow()
                .as_ref()
                .unwrap()
                .rgba
                .as_ptr(),
            layer_ptr,
            "obsolete zoom result must not replace the content layer"
        );
        app.document.as_mut().unwrap().content_layer_in_flight = Some(stale_ticket);
        let _ = app.update(Message::SelectBackend(Backend::Hayro));
        assert_eq!(app.document.as_ref().unwrap().content_layer_in_flight, None);
    }

    #[test]
    fn live_gesture_preview_keeps_model_unchanged_and_rejects_stale_bitmap() {
        let source =
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = Arc::new(ParsedPdf::open(Arc::new(source.clone())).unwrap());
        let raster = pdf.render_page(1, 1.0).unwrap();
        let original = Rect {
            x: 12.0,
            y: 16.0,
            width: 40.0,
            height: 28.0,
        };
        let mut model = DocumentModel::new(DocumentId(91), 1).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 1,
                page: 0,
                bounds: original,
                kind: Kind::Rectangle {
                    color_rgb: [231, 76, 60],
                },
            }))
            .unwrap();
        let mut document = OpenDocument {
            path: "fixture.pdf".into(),
            is_new: false,
            source_bytes: Arc::new(source),
            dynlex_document: None,
            pdf,
            model,
            last_saved_annotations: Vec::new(),
            bitmap: None,
            annotation_overlay_cache: RefCell::new(None),
            content_layer_cache: RefCell::new(None),
            content_layer_in_flight: None,
            bitmap_size: (0, 0),
            bitmap_page_points: (0.0, 0.0),
            bitmap_scale: 1.0,
            bitmap_backend: Backend::Hayro,
            imported_annotations_dynlex: false,
            displayed_page: 0,
            selected_annotation: Some(1),
            page_cache: VecDeque::new(),
            thumbnails: VecDeque::new(),
            thumbnail_in_flight: None,
            thumbnail_failed: HashSet::new(),
            thumbnail_focus: 0,
        };
        document.cache_bitmap(
            0,
            1.0,
            RenderOutcome {
                raster,
                used: Backend::Hayro,
                imported_annotations_dynlex: false,
            },
            None,
        );
        let mut app = App {
            backend: Backend::Dynlex,
            annotation_gesture_active: true,
            document: Some(document),
            ..Default::default()
        };
        let moved = Rect {
            x: 22.0,
            ..original
        };
        let version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AnnotationGesturePreview(version, 1, moved));
        let document = app.document.as_ref().unwrap();
        assert_eq!(document.model.get(1).unwrap().bounds, original);
        let preview = app
            .dynlex_live_gesture_bitmap(document)
            .expect("whole-pixel move must use DynLex");
        let image::Handle::Rgba { pixels, .. } = preview else {
            panic!("gesture preview must be RGBA");
        };
        assert_eq!(&pixels[((16 * 256 + 22) * 4)..][..4], &[31, 136, 255, 255]);

        let _ = app.update(Message::AnnotationGesturePreview(
            version,
            1,
            Rect { x: 22.5, ..moved },
        ));
        assert!(app
            .dynlex_live_gesture_bitmap(app.document.as_ref().unwrap())
            .is_none());
        let _ = app.update(Message::AnnotationGesturePreview(
            version,
            1,
            Rect {
                width: 41.0,
                ..moved
            },
        ));
        assert!(app
            .dynlex_live_gesture_bitmap(app.document.as_ref().unwrap())
            .is_none());
        let _ = app.update(Message::AnnotationGesturePreview(version, 1, moved));
        assert!(app
            .dynlex_live_gesture_bitmap(app.document.as_ref().unwrap())
            .is_some());
        let mut edited = app.document.as_ref().unwrap().model.get(1).unwrap().clone();
        edited.bounds.x += 2.0;
        app.document
            .as_mut()
            .unwrap()
            .model
            .apply(Edit::Replace(edited))
            .unwrap();
        assert!(app
            .dynlex_live_gesture_bitmap(app.document.as_ref().unwrap())
            .is_none());
        let _ = app.update(Message::AnnotationGesturePreview(version, 1, moved));
        assert!(app.annotation_gesture_preview.is_none());
        let current_version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AnnotationGesturePreview(current_version, 1, moved));
        assert!(app
            .dynlex_live_gesture_bitmap(app.document.as_ref().unwrap())
            .is_some());
        app.document.as_mut().unwrap().bitmap =
            Some(image::Handle::from_rgba(256, 192, vec![255; 256 * 192 * 4]));
        assert!(app
            .dynlex_live_gesture_bitmap(app.document.as_ref().unwrap())
            .is_none());
        let _ = app.update(Message::AnnotationGestureEnded);
        assert!(app.annotation_gesture_preview.is_none());
        let current_bounds = app.document.as_ref().unwrap().model.get(1).unwrap().bounds;
        let _ = app.update(Message::MoveAnnotation(version, 1, [10.0, 0.0]));
        assert_eq!(
            app.document.as_ref().unwrap().model.get(1).unwrap().bounds,
            current_bounds,
            "a gesture from an older annotation revision must not edit the page"
        );
        let current_version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::MoveAnnotation(current_version, 1, [10.0, 0.0]));
        assert_eq!(
            app.document
                .as_ref()
                .unwrap()
                .model
                .get(1)
                .unwrap()
                .bounds
                .x,
            current_bounds.x + 10.0
        );
        app.annotation_tool = AnnotationTool::Rectangle;
        app.next_annotation_id = 2;
        let added_bounds = Rect {
            x: 70.0,
            y: 40.0,
            width: 20.0,
            height: 20.0,
        };
        let _ = app.update(Message::AddAnnotation(current_version, added_bounds));
        assert!(app.document.as_ref().unwrap().model.get(2).is_none());
        let current_version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddAnnotation(current_version, added_bounds));
        assert_eq!(
            app.document.as_ref().unwrap().model.get(2).unwrap().bounds,
            added_bounds
        );
        app.annotation_tool = AnnotationTool::Text;
        app.next_annotation_id = 3;
        let current_version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddAnnotation(current_version, added_bounds));
        assert!(app.text_dialog);
        app.text_draft = iced::widget::text_editor::Content::with_text("stale draft");
        let mut changed = app.document.as_ref().unwrap().model.get(2).unwrap().clone();
        changed.bounds.x += 1.0;
        app.document
            .as_mut()
            .unwrap()
            .model
            .apply(Edit::Replace(changed))
            .unwrap();
        let _ = app.update(Message::CommitText);
        assert!(app.document.as_ref().unwrap().model.get(3).is_none());
        assert!(app.message.contains("StaleDocument"));
    }

    #[test]
    fn ellipse_palette_action_adds_an_undoable_ellipse() {
        let bytes = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let model = DocumentModel::new(DocumentId(92), pdf.page_count()).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "ellipse-fixture.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            next_annotation_id: 1,
            ..App::default()
        };
        let _ = app.update(Message::SelectAnnotationTool(AnnotationTool::Ellipse));
        assert_eq!(app.annotation_tool.mcp_name(), "ellipse");
        let bounds = Rect {
            x: 15.0,
            y: 18.0,
            width: 60.0,
            height: 28.0,
        };
        let version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddAnnotation(version, bounds));
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(1).unwrap().kind,
            Kind::Ellipse { .. }
        ));
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(1).is_none());
        let _ = app.update(Message::Redo);
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(1).unwrap().kind,
            Kind::Ellipse { .. }
        ));
        app.document.as_mut().unwrap().bitmap_page_points = (256.0, 192.0);
        let _ = app.update(Message::SelectAnnotationTool(AnnotationTool::Freehand));
        let version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddPolyline(
            version,
            vec![[25.0, 35.0], [40.0, 42.0], [57.0, 49.0]],
        ));
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(2).unwrap().kind,
            Kind::Polyline { .. }
        ));
        let _ = app.update(Message::SelectAnnotationTool(AnnotationTool::Arrow));
        assert_eq!(app.annotation_tool.mcp_name(), "arrow");
        let version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddLine(version, [60.0, 70.0], [140.0, 70.0]));
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(3).unwrap().kind,
            Kind::Arrow { .. }
        ));
        let _ = app.update(Message::SelectAnnotationTool(AnnotationTool::Circle));
        assert_eq!(app.annotation_tool.mcp_name(), "circle");
        let version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddAnnotation(
            version,
            Rect {
                x: 30.0,
                y: 30.0,
                width: 25.0,
                height: 25.0,
            },
        ));
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(4).unwrap().kind,
            Kind::Ellipse { .. }
        ));
        let _ = app.update(Message::SelectAnnotationTool(AnnotationTool::Polygon));
        assert_eq!(app.annotation_tool.mcp_name(), "polygon");
        let version = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::AddAnnotation(
            version,
            Rect {
                x: 45.0,
                y: 45.0,
                width: 60.0,
                height: 40.0,
            },
        ));
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(5).unwrap().kind,
            Kind::Polygon { .. }
        ));
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(5).is_none());
        let _ = app.update(Message::Redo);
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(5).unwrap().kind,
            Kind::Polygon { .. }
        ));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum RibbonTab {
    #[default]
    Home,
    View,
    Drawing,
    Comment,
    Organize,
    Help,
    Format,
}
impl RibbonTab {
    const ALL: [Self; 6] = [
        Self::Home,
        Self::View,
        Self::Drawing,
        Self::Comment,
        Self::Organize,
        Self::Help,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::View => "View",
            Self::Drawing => "Drawing",
            Self::Comment => "Annotation",
            Self::Organize => "Edit & Combine PDFs",
            Self::Help => "Settings",
            Self::Format => "Format",
        }
    }
}

enum ExportEvent {
    Progress(usize),
    Finished(Result<String, String>),
}

#[derive(Debug, Clone)]
enum Message {
    ExternalRequest(mcp_server::ExternalRequest),
    ShowNew,
    CancelNew,
    NewPaperSelected(blank_pdf::Paper),
    NewLandscape(bool),
    NewPagesChanged(String),
    NewCustomWidthChanged(String),
    NewCustomHeightChanged(String),
    NewBlank,
    ShowOpen,
    BrowseDirectory(PathBuf),
    Browsed(u64, Result<Vec<BrowserEntry>, String>),
    SelectPdf(PathBuf),
    RecentSearchChanged(String),
    TogglePinnedRecents,
    ToggleUnpinnedRecents,
    ToggleRecentPinned(String),
    RemoveRecent(String),
    BrowserSearchChanged(String),
    BrowserScrolled(scrollable::Viewport),
    ShowSave,
    Save,
    CancelSave,
    SavePathChanged(String),
    SaveAs,
    Saved(u64, Result<(String, Vec<Annotation>, u64), String>),
    ShowImport,
    ImportPathChanged(String),
    ImportXfdf,
    CancelImport,
    ShowXfdfExport,
    XfdfExportPathChanged(String),
    ExportXfdf,
    CancelXfdfExport,
    ShowExport,
    ExportPathChanged(String),
    SelectExportDpi(image_export::ImageDpi),
    SelectExportFormat(image_export::ImageFormat),
    SelectJpegQuality(u8),
    SetExportAnnotations(bool),
    ExportPageChanged(String),
    SelectExportRange(image_export::ImagePageRange),
    ExportCustomPagesChanged(String),
    ExportPng,
    ExportProgress(usize),
    Exported(Result<String, String>),
    CancelExport,
    ShowPrint,
    CancelPrint,
    PrintPathChanged(String),
    SelectPrintDestination(PrintDestination),
    PrintToPdf,
    Printed(Result<String, String>),
    SelectPrintRange(PrintRange),
    SelectPrintSubset(PrintSubset),
    SetPrintReverse(bool),
    PrintCustomPagesChanged(String),
    SelectPrintPaper(pdf_save::PrintPaper),
    SelectPrintScale(pdf_save::PrintScale),
    PrintZoomDraftChanged(String),
    ApplyPrintZoom,
    PrintPreviewStep(i32),
    PrintPreviewDocumentReady(u64, Result<(Arc<Vec<u8>>, Arc<ParsedPdf>), String>),
    PrintScaledPreviewReady(u64, Result<Arc<ParsedPdf>, String>),
    PrintPreviewReady(u64, usize, Result<Raster, String>),
    CancelOpen,
    ConfirmDiscard,
    CancelDiscard,
    TogglePages,
    ToggleProperties,
    ToggleSymbolPalette,
    ShowProperties,
    TogglePropertySection(usize),
    ToggleAnnotationList,
    ToggleReaderMode,
    ToggleFullscreen,
    ToggleKeystrokes,
    KeystrokePressed(iced::keyboard::Key, iced::keyboard::Modifiers, bool),
    KeystrokesExpired,
    PathChanged(String),
    Open,
    Loaded(
        u64,
        Result<
            (
                String,
                Arc<Vec<u8>>,
                Arc<ParsedPdf>,
                Option<DynlexOpen>,
                usize,
                f32,
                Option<reader_mode::Position>,
                Result<RenderOutcome, String>,
                bool,
            ),
            String,
        >,
    ),
    Zoom(f64),
    FitView(FitMode),
    WindowResized((f32, f32)),
    WindowMaximized(bool),
    KeyboardModifiersChanged(iced::keyboard::Modifiers),
    WheelZoom {
        amount: f32,
        anchor: [f32; 2],
    },
    PanStarted(iced::mouse::Button, [f32; 2]),
    PanMoved([f32; 2]),
    PanEnded,
    ZoomRenderDue,
    PageScrolled(scrollable::Viewport),
    ThumbnailsScrolled(scrollable::Viewport),
    GoToPage(usize),
    Rendered(
        u64,
        RenderTicket,
        Result<(RenderOutcome, Option<Raster>), String>,
    ),
    #[cfg(all(test, feature = "dynlex-experimental"))]
    ContentLayerRendered(
        u64,
        u64,
        RenderTicket,
        image::Handle,
        Result<Raster, String>,
    ),
    ThumbnailRendered(u64, u64, usize, Result<Raster, String>),
    SelectBackend(Backend),
    SelectRibbon(RibbonTab),
    ToggleFileMenu,
    SelectFileOpenPanel,
    CloseFileMenu,
    ShowAbout,
    CloseAbout,
    ShowExtensions,
    ShowExtensionImport,
    CloseExtensions,
    CloseExtensionImport,
    ExtensionIdChanged(String),
    ExtensionNameChanged(String),
    ExtensionOutputChanged(String),
    ExtensionImportPathChanged(String),
    ImportExtension,
    ExtensionHtmlEdited(iced::widget::text_editor::Action),
    ExtensionJavascriptEdited(iced::widget::text_editor::Action),
    BuildExtension,
    EscapeOverlay,
    CaptureUiScreenshot,
    UiScreenshotCaptured(window::Screenshot),
    ToggleRibbonCollapsed,
    DragWindow,
    ResizeWindow,
    MinimizeWindow,
    MaximizeWindow,
    CloseWindow,
    SelectAnnotationTool(AnnotationTool),
    SelectAnnotation(Option<u64>),
    DuplicateSelectedAnnotation,
    MirrorSelectedAnnotation {
        horizontal: bool,
    },
    RotateSelectedAnnotation,
    CopySelectedAnnotation,
    CutSelectedAnnotation,
    PasteAnnotation {
        in_place: bool,
    },
    AnnotationGestureStarted,
    AnnotationGesturePreview(DocumentVersion, u64, Rect),
    AnnotationGestureEnded,
    MoveAnnotation(DocumentVersion, u64, [f64; 2]),
    MoveLineEndpoint(DocumentVersion, u64, bool, [f64; 2]),
    ResizeAnnotation(DocumentVersion, u64, Rect),
    RemoveSelectedAnnotation,
    RequestDeleteAnnotation,
    ConfirmDeleteAnnotation,
    CancelDeleteAnnotation,
    RequestClearAnnotations(Option<usize>),
    ConfirmClearAnnotations,
    CancelClearAnnotations,
    AddAnnotation(DocumentVersion, Rect),
    AddLine(DocumentVersion, [f64; 2], [f64; 2]),
    AddPolyline(DocumentVersion, Vec<[f64; 2]>),
    TextEdited(iced::widget::text_editor::Action),
    TextPointer(iced::advanced::text::editor::Cursor, bool),
    SetFormatColor([u8; 3]),
    SetTextFontSize(u16),
    SetTextAlign(TextAlign),
    CommitText,
    CancelText,
    EditSelectedText,
    Undo,
    Redo,
    Close,
}

#[cfg(test)]
mod view_panel_tests {
    use super::*;

    #[test]
    fn recent_files_are_deduplicated_searchable_and_open_through_the_normal_route() {
        let mut recent = VecDeque::new();
        for index in 0..25 {
            remember_recent(&mut recent, &format!("/tmp/document-{index}.pdf"));
        }
        assert_eq!(recent.len(), recent_files::MAX_ENTRIES);
        assert_eq!(recent.front().unwrap().path, "/tmp/document-24.pdf");
        remember_recent(&mut recent, "/tmp/document-12.pdf");
        assert_eq!(recent.front().unwrap().path, "/tmp/document-12.pdf");
        assert_eq!(
            recent
                .iter()
                .filter(|entry| entry.path == "/tmp/document-12.pdf")
                .count(),
            1
        );
        let oldest = recent.back_mut().unwrap();
        oldest.pinned = true;
        let oldest_path = oldest.path.clone();
        remember_recent(&mut recent, "/tmp/new.pdf");
        assert_eq!(recent.len(), recent_files::MAX_ENTRIES);
        assert!(recent
            .iter()
            .any(|entry| entry.path == oldest_path && entry.pinned));
        assert!(recent_file_matches("/tmp/Tekst.PDF", " tekst "));

        let mut app = App {
            file_menu_open: true,
            ..App::default()
        };
        let _ = app.update(Message::SelectPdf(PathBuf::from("/tmp/document-12.pdf")));
        assert!(!app.file_menu_open);
        assert_eq!(app.path_input, "/tmp/document-12.pdf");
        assert!(app.opening_document);
    }

    #[test]
    fn app_recent_file_record_survives_a_new_app_instance() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("opds-app-recents-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("recent.json");
        let mut app = App {
            recent_store_path: Some(path.clone()),
            ..App::default()
        };
        app.record_recent_file("/tmp/one.pdf");
        app.record_recent_file("/tmp/two.pdf");
        let restored = recent_files::read(&path);
        assert_eq!(restored, app.recent_files);
        assert_eq!(restored.front().unwrap().path, "/tmp/two.pdf");
        let _ = app.update(Message::ToggleRecentPinned("/tmp/one.pdf".into()));
        assert!(
            recent_files::read(&path)
                .iter()
                .find(|entry| entry.path == "/tmp/one.pdf")
                .unwrap()
                .pinned
        );
        app.record_recent_file("/tmp/one.pdf");
        assert!(app.recent_files.front().unwrap().pinned);
        let _ = app.update(Message::RemoveRecent("/tmp/two.pdf".into()));
        assert!(!recent_files::read(&path)
            .iter()
            .any(|entry| entry.path == "/tmp/two.pdf"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recent_groups_toggle_without_changing_files_or_persisted_state() {
        let mut app = App::default();
        app.recent_files.push_back(RecentFile {
            path: "/tmp/one.pdf".into(),
            pinned: true,
        });
        let before = app.recent_files.clone();
        let _ = app.update(Message::TogglePinnedRecents);
        let _ = app.update(Message::ToggleUnpinnedRecents);
        assert!(app.pinned_recents_collapsed);
        assert!(app.unpinned_recents_collapsed);
        assert_eq!(app.recent_files, before);
        let _ = app.update(Message::TogglePinnedRecents);
        let _ = app.update(Message::ToggleUnpinnedRecents);
        assert!(!app.pinned_recents_collapsed);
        assert!(!app.unpinned_recents_collapsed);
    }

    #[test]
    fn annotation_list_and_properties_are_exclusive() {
        let mut app = App::default();
        assert!(!app.hide_properties);
        let _ = app.update(Message::ToggleAnnotationList);
        assert!(app.show_annotation_list);
        assert!(!app.hide_properties);
        let _ = app.update(Message::ToggleProperties);
        assert!(!app.show_annotation_list);
        assert!(!app.hide_properties);
        let _ = app.update(Message::ToggleAnnotationList);
        let _ = app.update(Message::ToggleAnnotationList);
        assert!(!app.show_annotation_list);
        assert!(app.hide_properties);
    }

    #[test]
    fn symbol_palette_toggle_reclaims_document_width() {
        let mut app = App::default();
        let shown = fit_scale(
            FitMode::Width,
            (600.0, 800.0),
            (1200.0, 832.0),
            true,
            true,
            true,
            false,
        )
        .unwrap();
        let _ = app.update(Message::ToggleSymbolPalette);
        assert!(app.hide_symbol_palette);
        let hidden = fit_scale(
            FitMode::Width,
            (600.0, 800.0),
            (1200.0, 832.0),
            true,
            true,
            !app.hide_symbol_palette,
            false,
        )
        .unwrap();
        assert!(hidden > shown);
        let _ = app.update(Message::ToggleSymbolPalette);
        assert!(!app.hide_symbol_palette);
    }

    #[test]
    fn duplicate_offsets_every_point_and_stays_on_page() {
        let source = Annotation {
            id: 7,
            page: 0,
            bounds: Rect {
                x: 170.0,
                y: 20.0,
                width: 25.0,
                height: 20.0,
            },
            kind: Kind::Polyline {
                points: vec![[170.0, 20.0], [195.0, 40.0]],
                color_rgb: [231, 76, 60],
            },
        };
        let copy = duplicate_annotation(&source, 8, 1.0, (200.0, 100.0)).unwrap();
        assert_eq!(copy.id, 8);
        assert_eq!(
            copy.bounds.x, 150.0,
            "right edge requires a leftward offset"
        );
        assert_eq!(copy.bounds.y, 40.0);
        let Kind::Polyline { points, .. } = copy.kind else {
            panic!("a duplicate must retain the annotation kind");
        };
        assert_eq!(points, vec![[150.0, 40.0], [175.0, 60.0]]);
        assert!(duplicate_annotation(&source, 8, 0.0, (200.0, 100.0)).is_none());
        assert!(duplicate_annotation(&source, 8, 1.0, (25.0, 100.0)).is_none());
        let line = Annotation {
            kind: Kind::Arrow {
                start: [170.0, 20.0],
                end: [195.0, 40.0],
                color_rgb: [231, 76, 60],
            },
            ..source.clone()
        };
        let shifted = duplicate_annotation(&line, 9, 2.0, (200.0, 100.0)).unwrap();
        assert_eq!(shifted.bounds.x, 160.0);
        assert_eq!(shifted.bounds.y, 30.0);
        assert!(matches!(
            shifted.kind,
            Kind::Arrow {
                start: [160.0, 30.0],
                end: [185.0, 50.0],
                ..
            }
        ));
        assert_eq!(source.id, 7);
    }

    #[test]
    fn moved_point_geometry_uses_clamped_actual_delta() {
        let source = Annotation {
            id: 7,
            page: 0,
            bounds: Rect {
                x: 170.0,
                y: 20.0,
                width: 25.0,
                height: 20.0,
            },
            kind: Kind::Polyline {
                points: vec![[170.0, 20.0], [195.0, 40.0]],
                color_rgb: [231, 76, 60],
            },
        };
        let moved = translated_annotation(&source, [100.0, -5.0], (200.0, 100.0)).unwrap();
        assert_eq!((moved.bounds.x, moved.bounds.y), (175.0, 15.0));
        let Kind::Polyline { points, .. } = moved.kind else {
            panic!("expected polyline");
        };
        assert_eq!(points, vec![[175.0, 15.0], [200.0, 35.0]]);
        assert_eq!(source.bounds.x, 170.0);
        let line = Annotation {
            kind: Kind::Line {
                start: [170.0, 20.0],
                end: [195.0, 40.0],
                color_rgb: [231, 76, 60],
            },
            ..source
        };
        let shifted = translated_annotation(&line, [-10.0, 10.0], (200.0, 100.0)).unwrap();
        assert!(matches!(
            shifted.kind,
            Kind::Line {
                start: [160.0, 30.0],
                end: [185.0, 50.0],
                ..
            }
        ));
        assert!(translated_annotation(&line, [f64::NAN, 0.0], (200.0, 100.0)).is_none());
        assert!(translated_annotation(&line, [0.0, 0.0], (200.0, 100.0)).is_none());
    }

    #[test]
    fn mirror_point_geometry_recomputes_bounds_and_undo_restores_source() {
        let page_size = (200.0, 100.0);
        let start = [20.0, 30.0];
        let end = [60.0, 50.0];
        let source = Annotation {
            id: 1,
            page: 0,
            bounds: arrow_bounds(start, end, page_size).unwrap(),
            kind: Kind::Arrow {
                start,
                end,
                color_rgb: [231, 76, 60],
            },
        };
        let mirrored = mirror_annotation(&source, true, page_size).unwrap();
        assert!(matches!(
            mirrored.kind,
            Kind::Arrow {
                start: [60.0, 30.0],
                end: [20.0, 50.0],
                ..
            }
        ));
        let mut model = DocumentModel::new(DocumentId(96), 1).unwrap();
        model.apply(Edit::Add(source.clone())).unwrap();
        let version = model.version();
        assert!(model
            .execute(version, DocumentCommand::Edit(Edit::Replace(mirrored)))
            .unwrap());
        assert_eq!(model.get(1).unwrap().id, 1);
        assert!(model
            .execute(model.version(), DocumentCommand::Undo)
            .unwrap());
        assert_eq!(model.get(1), Some(&source));

        let polyline = Annotation {
            id: 2,
            page: 0,
            bounds: polyline_bounds(&[[10.0, 10.0], [30.0, 40.0], [50.0, 20.0]], page_size)
                .unwrap(),
            kind: Kind::Polyline {
                points: vec![[10.0, 10.0], [30.0, 40.0], [50.0, 20.0]],
                color_rgb: [0, 0, 255],
            },
        };
        let vertical = mirror_annotation(&polyline, false, page_size).unwrap();
        assert!(matches!(
            vertical.kind,
            Kind::Polyline { ref points, .. }
                if points == &[[10.0, 40.0], [30.0, 10.0], [50.0, 30.0]]
        ));
        assert_eq!(
            mirror_annotation(&vertical, false, page_size).unwrap().kind,
            polyline.kind
        );
        let rotated = rotate_annotation_clockwise_90(&polyline, page_size).unwrap();
        assert!(matches!(
            rotated.kind,
            Kind::Polyline { ref points, .. }
                if points == &[[45.0, 5.0], [15.0, 25.0], [35.0, 45.0]]
        ));
        assert!(rotate_annotation_clockwise_90(&polyline, (30.0, 30.0)).is_none());
        let rectangle = Annotation {
            kind: Kind::Rectangle {
                color_rgb: [0, 0, 0],
            },
            ..source
        };
        assert!(mirror_annotation(&rectangle, true, page_size).is_none());
    }

    #[test]
    fn drawing_mirror_button_uses_versioned_document_command() {
        let bytes = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let page_size = pdf.page_dimensions(1).unwrap();
        let source = Annotation {
            id: 1,
            page: 0,
            bounds: arrow_bounds([20.0, 30.0], [60.0, 50.0], page_size).unwrap(),
            kind: Kind::Arrow {
                start: [20.0, 30.0],
                end: [60.0, 50.0],
                color_rgb: [231, 76, 60],
            },
        };
        let mut model = DocumentModel::new(DocumentId(97), pdf.page_count()).unwrap();
        model.apply(Edit::Add(source.clone())).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "mirror-fixture.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: vec![source.clone()],
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: Some(1),
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            ..App::default()
        };
        let _ = app.update(Message::MirrorSelectedAnnotation { horizontal: true });
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(1).unwrap().kind,
            Kind::Arrow {
                start: [60.0, 30.0],
                end: [20.0, 50.0],
                ..
            }
        ));
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.as_ref().unwrap().model.get(1), Some(&source));
        let _ = app.update(Message::Redo);
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(1).unwrap().kind,
            Kind::Arrow {
                start: [60.0, 30.0],
                end: [20.0, 50.0],
                ..
            }
        ));
        let _ = app.update(Message::RotateSelectedAnnotation);
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(1).unwrap().kind,
            Kind::Arrow {
                start: [50.0, 60.0],
                end: [30.0, 20.0],
                ..
            }
        ));
        let _ = app.update(Message::Undo);
        assert!(matches!(
            app.document.as_ref().unwrap().model.get(1).unwrap().kind,
            Kind::Arrow {
                start: [60.0, 30.0],
                end: [20.0, 50.0],
                ..
            }
        ));
    }

    #[test]
    fn drawing_copy_duplicates_selection_in_one_undoable_transaction() {
        let bytes = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let source = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 40.0,
                height: 25.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        let mut model = DocumentModel::new(DocumentId(94), pdf.page_count()).unwrap();
        model.apply(Edit::Add(source.clone())).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "copy-fixture.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: vec![source.clone()],
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: Some(1),
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            next_annotation_id: 2,
            ..App::default()
        };
        let _ = app.update(Message::DuplicateSelectedAnnotation);
        let document = app.document.as_ref().unwrap();
        assert_eq!(document.selected_annotation, Some(2));
        assert_eq!(document.model.get(1), Some(&source));
        assert_eq!(document.model.get(2).unwrap().bounds.x, 40.0);
        assert_eq!(document.model.get(2).unwrap().bounds.y, 50.0);
        assert_eq!(app.next_annotation_id, 3);
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(2).is_none());
        assert_eq!(app.document.as_ref().unwrap().model.get(1), Some(&source));
        let _ = app.update(Message::Redo);
        assert!(app.document.as_ref().unwrap().model.get(2).is_some());
    }

    #[test]
    fn clipboard_copy_paste_and_paste_in_place_are_separate_undoable_edits() {
        let bytes = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let source = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 40.0,
                height: 25.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        let mut model = DocumentModel::new(DocumentId(95), pdf.page_count()).unwrap();
        model.apply(Edit::Add(source.clone())).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "clipboard-fixture.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: vec![source.clone()],
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: Some(1),
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            next_annotation_id: 2,
            ..App::default()
        };
        let _ = app.update(Message::CopySelectedAnnotation);
        assert_eq!(app.clipboard_annotation, Some(source.clone()));
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 1);
        let _ = app.update(Message::PasteAnnotation { in_place: false });
        let _ = app.update(Message::PasteAnnotation { in_place: false });
        let _ = app.update(Message::PasteAnnotation { in_place: true });
        let document = app.document.as_ref().unwrap();
        assert_eq!(document.model.get(2).unwrap().bounds.x, 40.0);
        assert_eq!(document.model.get(3).unwrap().bounds.x, 60.0);
        assert_eq!(document.model.get(4).unwrap().bounds, source.bounds);
        assert_eq!(document.selected_annotation, Some(4));
        assert_eq!(app.clipboard_paste_count, 2);
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(4).is_none());
        assert!(app.document.as_ref().unwrap().model.get(3).is_some());
        let _ = app.update(Message::Redo);
        assert!(app.document.as_ref().unwrap().model.get(4).is_some());
        app.document.as_mut().unwrap().selected_annotation = Some(4);
        let _ = app.update(Message::CutSelectedAnnotation);
        assert!(app.document.as_ref().unwrap().model.get(4).is_none());
        assert_eq!(
            app.clipboard_annotation.as_ref().unwrap().bounds,
            source.bounds
        );
        assert_eq!(app.document.as_ref().unwrap().selected_annotation, None);
        let _ = app.update(Message::Undo);
        assert!(app.document.as_ref().unwrap().model.get(4).is_some());
        let _ = app.update(Message::PasteAnnotation { in_place: true });
        assert_eq!(
            app.document.as_ref().unwrap().model.get(5).unwrap().bounds,
            source.bounds
        );
    }

    #[test]
    fn file_document_properties_always_opens_properties() {
        let mut app = App::default();
        app.hide_properties = true;
        app.show_annotation_list = true;
        app.file_menu_open = true;
        let _ = app.update(Message::ShowProperties);
        assert!(!app.file_menu_open);
        assert!(!app.show_annotation_list);
        assert!(!app.hide_properties);
        let _ = app.update(Message::ShowProperties);
        assert!(!app.hide_properties);
    }

    #[test]
    fn file_menu_opens_without_opening_a_pdf_dialog() {
        let mut app = App::default();
        let _ = app.update(Message::ToggleFileMenu);
        assert!(app.file_menu_open);
        assert!(!app.open_dialog);
        let _ = app.update(Message::SelectFileOpenPanel);
        assert!(app.file_menu_open);
        assert!(!app.open_dialog);
        let _ = app.update(Message::CloseFileMenu);
        assert!(!app.file_menu_open);
        let _ = app.update(Message::ToggleFileMenu);
        let _ = app.update(Message::ShowOpen);
        assert!(!app.file_menu_open);
        assert!(app.open_dialog);
    }

    #[test]
    fn cancel_export_signals_worker_and_waits_for_its_completion() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut app = App {
            export_busy: true,
            export_dialog: true,
            export_cancel: Some(Arc::clone(&cancel)),
            export_total: 3,
            ..App::default()
        };
        let _ = app.update(Message::ExportProgress(1));
        assert_eq!(app.export_completed, 1);
        assert_eq!(app.message, "Exporting 1/3 pages…");
        let _ = app.update(Message::ExportProgress(4));
        assert_eq!(app.export_completed, 1);
        let _ = app.update(Message::CancelExport);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(app.export_busy);
        assert!(app.export_dialog);
        let _ = app.update(Message::ExportProgress(2));
        assert_eq!(app.export_completed, 2);
        assert_eq!(
            app.message,
            "Cancelling image export after the current page…"
        );
        let _ = app.update(Message::Exported(Err("Export cancelled".into())));
        assert!(!app.export_busy);
        assert!(!app.export_dialog);
        assert!(app.export_cancel.is_none());
        assert_eq!(app.message, "Export cancelled");
    }

    #[test]
    fn external_page_navigation_rejects_stale_and_replay_and_uses_live_ui_action() {
        let bytes = Arc::new(blank_pdf::create(100.0, 100.0, 2).unwrap());
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let model = DocumentModel::new(DocumentId(122), 2).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "/tmp/two-pages-navigation.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            ..App::default()
        };
        let version = app.document.as_ref().unwrap().model.version();
        let request = |generation, expected_page, page| {
            mcp_server::ExternalRequest::new(
                [122, generation, version.annotation_revision],
                mcp_server::ExternalAction::GoToPage {
                    expected_page,
                    page,
                },
            )
        };
        let (cancelled, response) = request(version.generation, 0, 1);
        drop(response);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert_eq!(app.document.as_ref().unwrap().model.view().page(), 0);
        let (invalid, response) = request(version.generation, 0, 2);
        let _ = app.update(Message::ExternalRequest(invalid));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["ok"],
            false
        );
        let (valid, response) = request(version.generation, 0, 1);
        let _ = app.update(Message::ExternalRequest(valid));
        let result = iced::futures::executor::block_on(response).unwrap();
        assert_eq!(result["page"], 2);
        assert_eq!(result["renderPending"], true);
        assert_eq!(app.document.as_ref().unwrap().model.view().page(), 1);
        assert_eq!(app.document.as_ref().unwrap().thumbnail_focus, 1);
        let (replay, response) = request(version.generation, 0, 1);
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        assert_eq!(app.document.as_ref().unwrap().model.view().page(), 1);
    }

    #[test]
    fn external_fit_view_uses_native_geometry_and_rejects_stale_zoom() {
        let bytes = Arc::new(blank_pdf::create(100.0, 100.0, 1).unwrap());
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let model = DocumentModel::new(DocumentId(123), 1).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "/tmp/fit-view-test.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 0,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 0,
            }),
            window_size: (1200.0, 350.0),
            ..App::default()
        };
        let version = app.document.as_ref().unwrap().model.version();
        let request = |generation, expected_zoom, mode| {
            mcp_server::ExternalRequest::new(
                [123, generation, version.annotation_revision],
                mcp_server::ExternalAction::FitView {
                    expected_zoom,
                    mode,
                },
            )
        };
        let (cancelled, response) = request(version.generation, 1.0, "width");
        drop(response);
        let _ = app.update(Message::ExternalRequest(cancelled));
        assert_eq!(
            app.document
                .as_ref()
                .unwrap()
                .model
                .view()
                .viewport()
                .scale(),
            1.0
        );
        let (stale, response) = request(version.generation + 1, 1.0, "width");
        let _ = app.update(Message::ExternalRequest(stale));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let (width, response) = request(version.generation, 1.0, "width");
        let _ = app.update(Message::ExternalRequest(width));
        let result = iced::futures::executor::block_on(response).unwrap();
        let width_zoom = result["zoom"].as_f64().unwrap();
        assert!(width_zoom > 1.0);
        assert_eq!(result["renderPending"], true);
        let (replay, response) = request(version.generation, 1.0, "width");
        let _ = app.update(Message::ExternalRequest(replay));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale document version"
        );
        let width_generation = app.document.as_ref().unwrap().model.version().generation;
        let (page, response) = request(width_generation, width_zoom, "page");
        let _ = app.update(Message::ExternalRequest(page));
        let result = iced::futures::executor::block_on(response).unwrap();
        assert_eq!(result["mode"], "page");
        assert!(result["zoom"].as_f64().unwrap() < width_zoom);
        let zoom = app
            .document
            .as_ref()
            .unwrap()
            .model
            .view()
            .viewport()
            .scale();
        let page_generation = app.document.as_ref().unwrap().model.version().generation;
        let (stale_zoom, response) = request(page_generation, width_zoom, "width");
        let _ = app.update(Message::ExternalRequest(stale_zoom));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "stale current zoom"
        );
        let (no_op, response) = request(page_generation, zoom, "page");
        let _ = app.update(Message::ExternalRequest(no_op));
        assert_eq!(
            iced::futures::executor::block_on(response).unwrap()["error"],
            "fit did not change zoom"
        );
    }

    #[test]
    fn export_page_input_tracks_selection_and_rejects_invalid_pages() {
        let bytes = Arc::new(blank_pdf::create(100.0, 100.0, 2).unwrap());
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let mut model = DocumentModel::new(DocumentId(121), 2).unwrap();
        model.go_to_page(1).unwrap();
        let mut app = App {
            document: Some(OpenDocument {
                path: "/tmp/two-pages.pdf".into(),
                is_new: false,
                source_bytes: bytes,
                dynlex_document: None,
                pdf,
                model,
                last_saved_annotations: Vec::new(),
                bitmap: None,
                annotation_overlay_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_cache: RefCell::new(None),
                #[cfg(feature = "dynlex-experimental")]
                content_layer_in_flight: None,
                bitmap_size: (0, 0),
                bitmap_page_points: (0.0, 0.0),
                bitmap_scale: 1.0,
                bitmap_backend: Backend::Hayro,
                imported_annotations_dynlex: false,
                displayed_page: 1,
                selected_annotation: None,
                page_cache: VecDeque::new(),
                thumbnails: VecDeque::new(),
                thumbnail_in_flight: None,
                thumbnail_failed: HashSet::new(),
                thumbnail_focus: 1,
            }),
            ..App::default()
        };
        let _ = app.update(Message::ShowExport);
        assert_eq!(app.export_page_input, "2");
        assert!(app.export_include_annotations);
        assert_eq!(app.export_jpeg_quality.value(), 92);
        let _ = app.update(Message::SetExportAnnotations(false));
        assert!(!app.export_include_annotations);
        let _ = app.update(Message::SelectJpegQuality(10));
        assert_eq!(app.export_jpeg_quality.value(), 10);
        let _ = app.update(Message::SelectJpegQuality(101));
        assert_eq!(app.export_jpeg_quality.value(), 10);
        assert_eq!(app.export_path_input, "/tmp/two-pages-page-2.png");
        let _ = app.update(Message::SelectExportFormat(image_export::ImageFormat::Jpeg));
        assert_eq!(app.export_path_input, "/tmp/two-pages-page-2.jpg");
        let _ = app.update(Message::SelectExportFormat(image_export::ImageFormat::Tiff));
        assert_eq!(app.export_path_input, "/tmp/two-pages-page-2.tiff");
        let _ = app.update(Message::SelectExportFormat(image_export::ImageFormat::Png));
        assert_eq!(app.export_path_input, "/tmp/two-pages-page-2.png");
        let _ = app.update(Message::ExportPageChanged("1".into()));
        assert_eq!(app.export_page, 0);
        assert_eq!(app.export_path_input, "/tmp/two-pages-page-1.png");
        let _ = app.update(Message::ExportPathChanged("/tmp/custom.png".into()));
        let _ = app.update(Message::ExportPageChanged("2".into()));
        assert_eq!(app.export_page, 1);
        assert_eq!(app.export_path_input, "/tmp/custom.png");
        let _ = app.update(Message::ExportPageChanged("3".into()));
        let _ = app.update(Message::ExportPng);
        assert!(!app.export_busy);
        assert!(app.message.contains("Choose a page from 1 to 2"));
        let _ = app.update(Message::ShowExport);
        let _ = app.update(Message::SelectExportRange(
            image_export::ImagePageRange::All,
        ));
        assert_eq!(app.export_path_input, "/tmp/two-pages-images");
        let _ = app.update(Message::SelectExportFormat(image_export::ImageFormat::Jpeg));
        assert_eq!(app.export_path_input, "/tmp/two-pages-jpeg-images");
        let _ = app.update(Message::SelectExportFormat(image_export::ImageFormat::Tiff));
        assert_eq!(app.export_path_input, "/tmp/two-pages-tiff-images");
        let _ = app.update(Message::SelectExportFormat(image_export::ImageFormat::Png));
        assert_eq!(app.export_path_input, "/tmp/two-pages-images");
        let _ = app.update(Message::SelectExportRange(
            image_export::ImagePageRange::Custom,
        ));
        let _ = app.update(Message::ExportCustomPagesChanged("1-3".into()));
        let _ = app.update(Message::ExportPng);
        assert!(!app.export_busy);
        assert!(app.message.contains("Choose pages from 1 to 2"));
        let _ = app.update(Message::SelectExportRange(
            image_export::ImagePageRange::Current,
        ));
        assert_eq!(app.export_path_input, "/tmp/two-pages-page-2.png");
    }

    #[test]
    fn new_blank_uses_an_unsaved_source_without_recent_or_reader_position() {
        let mut app = App {
            backend: Backend::Dynlex,
            window_size: (1200.0, 832.0),
            ..App::default()
        };
        let _ = app.update(Message::ShowNew);
        let _ = app.update(Message::NewBlank);
        assert!(app.opening_document);
        assert_eq!(app.next_load, 1);
        let bytes = Arc::new(blank_pdf::a4_portrait().unwrap());
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let dynlex = Arc::new(opds_dynlex_backend::DynlexDocument::new(Arc::clone(&bytes)));
        let (scale, outcome) = render_initial_dynlex_page(&pdf, &dynlex, 1, 1.0);
        assert!(outcome.is_ok());
        let _ = app.update(Message::Loaded(
            1,
            Ok((
                "Untitled.pdf".into(),
                bytes,
                pdf,
                Some(DynlexOpen(dynlex)),
                1,
                scale,
                None,
                outcome,
                true,
            )),
        ));
        assert!(!app.opening_document);
        assert!(app.document.as_ref().unwrap().is_new);
        assert!(app.recent_files.is_empty());
        let _ = app.update(Message::ToggleReaderMode);
        assert!(app.reader_position().is_none());
        let _ = app.update(Message::ShowSave);
        assert_eq!(app.save_path_input, "Untitled.pdf");
        assert!(app.save_dialog);
        let _ = app.update(Message::ToggleReaderMode);
        let destination = "/tmp/opds-new-saved.pdf".to_owned();
        let _ = app.update(Message::Saved(1, Ok((destination.clone(), Vec::new(), 42))));
        assert_eq!(app.document.as_ref().unwrap().path, destination);
        assert!(!app.document.as_ref().unwrap().is_new);
        assert_eq!(app.recent_files.front().unwrap().path, destination);
    }

    #[test]
    fn file_xfdf_import_adds_supported_shapes_as_one_undoable_edit() {
        let mut app = App {
            backend: Backend::Dynlex,
            window_size: (1200.0, 832.0),
            ..App::default()
        };
        let _ = app.update(Message::ShowNew);
        let _ = app.update(Message::NewBlank);
        let bytes = Arc::new(blank_pdf::a4_portrait().unwrap());
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&bytes)).unwrap());
        let dynlex = Arc::new(opds_dynlex_backend::DynlexDocument::new(Arc::clone(&bytes)));
        let (scale, outcome) = render_initial_dynlex_page(&pdf, &dynlex, 1, 1.0);
        let _ = app.update(Message::Loaded(
            1,
            Ok((
                "Untitled.pdf".into(),
                bytes,
                pdf,
                Some(DynlexOpen(dynlex)),
                1,
                scale,
                None,
                outcome,
                true,
            )),
        ));
        let path =
            std::env::temp_dir().join(format!("opds-xfdf-import-{}.xfdf", std::process::id()));
        std::fs::write(
            &path,
            r##"<xfdf xmlns="http://ns.adobe.com/xfdf/"><annots>
            <square page="0" rect="10,10,50,50" color="#ff9800"/>
            <circle page="0" rect="60,10,100,50" color="#102030"/>
            </annots></xfdf>"##,
        )
        .unwrap();
        let _ = app.update(Message::ShowImport);
        assert!(app.import_dialog);
        let _ = app.update(Message::ImportPathChanged(path.display().to_string()));
        let _ = app.update(Message::ImportXfdf);
        assert!(!app.import_dialog, "{}", app.message);
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 2);
        assert_eq!(app.next_annotation_id, 2);
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 0);
        assert!(!app.document.as_ref().unwrap().model.can_undo());
        let _ = app.update(Message::Redo);
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 2);
        let export_path =
            std::env::temp_dir().join(format!("opds-xfdf-export-{}.xfdf", std::process::id()));
        let _ = std::fs::remove_file(&export_path);
        let _ = app.update(Message::ShowXfdfExport);
        assert!(app.xfdf_export_dialog);
        let _ = app.update(Message::XfdfExportPathChanged(
            export_path.display().to_string(),
        ));
        let _ = app.update(Message::ExportXfdf);
        assert!(!app.xfdf_export_dialog, "{}", app.message);
        let exported = std::fs::read_to_string(&export_path).unwrap();
        let roundtrip = xfdf_import::parse(&exported, 1, 10).unwrap();
        assert_eq!(roundtrip.len(), 2);
        assert!(matches!(roundtrip[0].kind, Kind::Rectangle { .. }));
        assert!(matches!(roundtrip[1].kind, Kind::Ellipse { .. }));
        let _ = app.update(Message::ShowXfdfExport);
        let _ = app.update(Message::XfdfExportPathChanged(
            export_path.display().to_string(),
        ));
        let _ = app.update(Message::ExportXfdf);
        assert!(app.xfdf_export_dialog);
        assert!(app.message.contains("File exists") || app.message.contains("exists"));
        assert_eq!(std::fs::read_to_string(&export_path).unwrap(), exported);
        std::fs::remove_file(export_path).unwrap();
        let _ = app.update(Message::CancelXfdfExport);
        std::fs::write(
            &path,
            r##"<xfdf><annots><square page="0" rect="10,10,50,50" color="#ff9800"/>
            <stamp page="0" rect="60,10,100,50"/></annots></xfdf>"##,
        )
        .unwrap();
        let before = app.document.as_ref().unwrap().model.version();
        let _ = app.update(Message::ShowImport);
        let _ = app.update(Message::ImportPathChanged(path.display().to_string()));
        let _ = app.update(Message::ImportXfdf);
        assert!(app.import_dialog);
        assert!(app.message.contains("Unsupported XFDF annotation"));
        assert_eq!(app.document.as_ref().unwrap().model.version(), before);
        assert_eq!(app.document.as_ref().unwrap().model.items().count(), 2);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn new_dialog_checks_pages_and_custom_size_before_replacing_document() {
        let mut app = App::default();
        let _ = app.update(Message::ShowNew);
        assert!(app.new_dialog);
        assert_eq!(app.new_paper, blank_pdf::Paper::A4);
        let _ = app.update(Message::NewPagesChanged("0".into()));
        let _ = app.update(Message::NewBlank);
        assert!(app.new_dialog);
        assert_eq!(app.next_load, 0);
        let _ = app.update(Message::NewPagesChanged("3".into()));
        let _ = app.update(Message::NewPaperSelected(blank_pdf::Paper::Custom));
        let _ = app.update(Message::NewCustomWidthChanged("9".into()));
        let _ = app.update(Message::NewBlank);
        assert!(app.new_dialog);
        assert_eq!(app.next_load, 0);
        let _ = app.update(Message::NewCustomWidthChanged("210".into()));
        let _ = app.update(Message::NewBlank);
        assert!(!app.new_dialog);
        assert!(app.opening_document);
        assert_eq!(app.next_load, 1);
    }

    #[test]
    fn file_about_opens_and_closes_without_changing_document_state() {
        let mut app = App::default();
        app.file_menu_open = true;
        let _ = app.update(Message::ShowAbout);
        assert!(app.about_dialog);
        assert!(!app.file_menu_open);
        assert!(app.document.is_none());
        let _ = app.update(Message::EscapeOverlay);
        assert!(!app.about_dialog);
        let _ = app.update(Message::ShowAbout);
        let _ = app.update(Message::CloseAbout);
        assert!(!app.about_dialog);
    }

    #[test]
    fn extension_builder_opens_from_file_menu_and_writes_package() {
        let mut app = App::default();
        app.file_menu_open = true;
        let _ = app.update(Message::ShowExtensions);
        assert!(app.extensions_dialog);
        assert!(!app.file_menu_open);
        assert!(app.extension_html.text().contains("<html"));
        assert!(app.extension_javascript.text().contains("activate"));
        let html_before = app.extension_html.text();
        let javascript_before = app.extension_javascript.text();
        let _ = app.update(Message::ExtensionHtmlEdited(
            iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Insert('X')),
        ));
        let _ = app.update(Message::ExtensionJavascriptEdited(
            iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Insert('Y')),
        ));
        assert_ne!(app.extension_html.text(), html_before);
        assert_ne!(app.extension_javascript.text(), javascript_before);
        let path = std::env::temp_dir().join(format!(
            "opds-extension-ui-test-{}.oppx",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let _ = app.update(Message::ExtensionIdChanged("ui_test".into()));
        let _ = app.update(Message::ExtensionNameChanged("UI Test".into()));
        let _ = app.update(Message::ExtensionOutputChanged(path.display().to_string()));
        let _ = app.update(Message::BuildExtension);
        assert!(app.extension_error.is_empty(), "{}", app.extension_error);
        assert!(path.is_file());
        let _ = app.update(Message::ExtensionIdChanged("changed".into()));
        let _ = app.update(Message::ShowExtensionImport);
        assert!(app.extension_import_dialog);
        assert!(!app.extensions_dialog);
        let _ = app.update(Message::ExtensionImportPathChanged(
            "/definitely-not-an-extension.oppx".into(),
        ));
        let _ = app.update(Message::ImportExtension);
        assert!(app.extension_import_dialog);
        assert!(!app.extension_error.is_empty());
        let _ = app.update(Message::ExtensionImportPathChanged(
            path.display().to_string(),
        ));
        let _ = app.update(Message::ImportExtension);
        assert!(app.extension_error.is_empty(), "{}", app.extension_error);
        assert!(!app.extension_import_dialog);
        assert!(app.extensions_dialog);
        assert_eq!(app.extension_id, "ui_test");
        assert!(app.extension_html.text().contains('X'));
        assert!(app.extension_javascript.text().contains('Y'));
        assert!(app.extension_output.is_empty());
        std::fs::remove_file(path).unwrap();
        let _ = app.update(Message::EscapeOverlay);
        assert!(!app.extensions_dialog);
    }
}

#[cfg(test)]
mod reader_shutdown_tests {
    use super::*;

    #[test]
    fn normal_shutdown_writes_latest_opt_in_position() {
        let pdf =
            std::env::temp_dir().join(format!("opds-reader-shutdown-{}.pdf", std::process::id()));
        let position = reader_mode::Position {
            page: 3,
            scale: 1.25,
            scroll_top: 100.0,
            scroll_height: 500.0,
            single_page: true,
        };
        {
            let mut guard = ReaderShutdown::default();
            guard.position = Some((pdf.clone(), position));
        }
        assert_eq!(reader_mode::read(&pdf), Some(position));
        reader_mode::clear(&pdf).unwrap();
    }
}

impl App {
    fn reader_position(&self) -> Option<(&Path, reader_mode::Position)> {
        if !self.reader_mode_active {
            return None;
        }
        let document = self.document.as_ref()?;
        if document.is_new {
            return None;
        }
        let preview_size = zoom_preview_size(
            document.bitmap_page_points,
            document.bitmap_size,
            document.model.view().viewport().scale(),
        );
        let scroll_height = if preview_size.1 == 0 {
            0.0
        } else {
            preview_size.1 as f32 + 48.0
        };
        Some((
            Path::new(&document.path),
            reader_mode::Position {
                page: document.model.view().page() + 1,
                scale: document.model.view().viewport().scale(),
                scroll_top: (self.page_scroll_offset.y - VERTICAL_PAN_MARGIN).max(0.0),
                scroll_height,
                single_page: true,
            },
        ))
    }

    fn persist_reader_position(&self) -> std::io::Result<()> {
        if let Some((path, position)) = self.reader_position() {
            reader_mode::write(path, position)?;
        }
        Ok(())
    }

    fn dynlex_visible_annotation_bitmap(&self, document: &OpenDocument) -> Option<image::Handle> {
        if self.backend != Backend::Dynlex || self.editing_text_id.is_some() {
            return None;
        }
        if self.annotation_gesture_active {
            return self.dynlex_live_gesture_bitmap(document);
        }
        match document.selected_annotation {
            Some(id) => document.dynlex_selected_annotation_bitmap(id),
            None => document.dynlex_annotation_bitmap(),
        }
    }

    fn dynlex_live_gesture_bitmap(&self, document: &OpenDocument) -> Option<image::Handle> {
        if !self.annotation_gesture_active
            || self.backend != Backend::Dynlex
            || self.editing_text_id.is_some()
        {
            return None;
        }
        let preview = self.annotation_gesture_preview.as_ref()?;
        if preview.page != document.displayed_page
            || document.selected_annotation != Some(preview.selected)
            || document.bitmap.as_ref() != Some(&preview.base)
            || !document
                .model
                .items()
                .filter(|item| item.page == preview.page)
                .eq(preview.annotations.iter())
        {
            return None;
        }
        Some(preview.bitmap.clone())
    }

    fn has_unsaved_work(&self) -> bool {
        self.text_dialog || self.document.as_ref().is_some_and(OpenDocument::is_dirty)
    }

    fn record_recent_file(&mut self, path: &str) {
        remember_recent(&mut self.recent_files, path);
        self.persist_recent_files();
    }

    fn persist_recent_files(&self) {
        if let Some(store) = &self.recent_store_path {
            if let Err(error) = recent_files::write(store, &self.recent_files) {
                eprintln!("Could not persist recent PDFs: {error}");
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let (task, external_response) = match message {
            Message::ExternalRequest(request) => {
                let (task, result) = self.apply_external_command(&request);
                (task, Some((request, result)))
            }
            message => (self.update_inner(message), None),
        };
        mcp_server::publish_tool(self.annotation_tool.mcp_name(), self.annotation_tool_epoch);
        mcp_server::publish_backend(self.backend);
        if let Some(document) = self.document.as_ref() {
            mcp_server::publish_model(&document.model);
            mcp_server::publish_tab_state(
                &document.model,
                &document.path,
                document.is_new,
                document.is_dirty(),
                document.selected_annotation,
            );
            mcp_server::publish_page_geometry(
                &document.model,
                document
                    .pdf
                    .page_dimensions(document.model.view().page() + 1)
                    .ok(),
            );
        } else {
            mcp_server::publish_document(None);
        }
        // Publish the resulting document version before returning the MCP
        // reply, so a client can immediately re-read a coherent snapshot.
        if let Some((request, result)) = external_response {
            request.respond(result);
        }
        self.reader_shutdown.position = self
            .reader_position()
            .map(|(path, position)| (path.to_path_buf(), position));
        task
    }

    fn apply_external_command(
        &mut self,
        request: &mcp_server::ExternalRequest,
    ) -> (Task<Message>, serde_json::Value) {
        if request.was_cancelled() {
            return (
                Task::none(),
                serde_json::json!({"ok":false,"error":"request cancelled"}),
            );
        }
        let Some(document) = self.document.as_ref() else {
            return (
                Task::none(),
                serde_json::json!({"ok":false,"error":"no active document"}),
            );
        };
        let expected = DocumentVersion {
            id: DocumentId(request.expected_document_id),
            generation: request.expected_generation,
            annotation_revision: request.expected_annotation_revision,
        };
        if document.model.version() != expected {
            return (
                Task::none(),
                serde_json::json!({"ok":false,"error":"stale document version"}),
            );
        }
        if let mcp_server::ExternalAction::Select(id) = request.action {
            if !document
                .model
                .get(id)
                .is_some_and(|item| item.page == document.displayed_page)
            {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"annotation is not on the displayed page"}),
                );
            }
            let task = self.update_inner(Message::SelectAnnotation(Some(id)));
            return (
                task,
                serde_json::json!({"ok":true,"selectedId":id.to_string(),
                "documentId":expected.id.0.to_string(),"generation":expected.generation,
                "annotationRevision":expected.annotation_revision}),
            );
        }
        if let mcp_server::ExternalAction::ClearSelection = request.action {
            let task = self.update_inner(Message::SelectAnnotation(None));
            return (
                task,
                serde_json::json!({"ok":true,"selectedId":null,
                "documentId":expected.id.0.to_string(),"generation":expected.generation,
                "annotationRevision":expected.annotation_revision}),
            );
        }
        if let mcp_server::ExternalAction::SelectBackend {
            expected: expected_backend,
            requested: backend,
        } = request.action
        {
            if self.backend != expected_backend {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"stale renderer selection"}),
                );
            }
            if self.opening_document || self.backend == backend {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"renderer selection did not change"}),
                );
            }
            let available = match backend {
                Backend::Hayro => true,
                Backend::Dynlex => cfg!(feature = "dynlex-experimental"),
                Backend::Pdfium => opds_dynlex_backend::pdfium_available(),
                Backend::OwnRust => opds_dynlex_backend::own_rust_available(),
                Backend::PdfCraft => opds_dynlex_backend::pdfcraft_available(),
            };
            if !available {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"renderer is unavailable"}),
                );
            }
            let task = self.update_inner(Message::SelectBackend(backend));
            return (
                task,
                serde_json::json!({"ok":true,"selected":mcp_server::backend_id(backend),
                    "renderPending":true,"documentId":expected.id.0.to_string(),
                    "generation":expected.generation,
                    "annotationRevision":expected.annotation_revision}),
            );
        }
        if let mcp_server::ExternalAction::GoToPage {
            expected_page,
            page,
        } = request.action
        {
            if self.opening_document {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"document is opening"}),
                );
            }
            if document.model.view().page() != expected_page {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"stale current page"}),
                );
            }
            if page >= document.model.view().page_count() || page == expected_page {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"page selection did not change"}),
                );
            }
            let task = self.update_inner(Message::GoToPage(page));
            let version = self.document.as_ref().unwrap().model.version();
            return (
                task,
                serde_json::json!({"ok":true,"page":page+1,"renderPending":true,
                    "documentId":version.id.0.to_string(),"generation":version.generation,
                    "annotationRevision":version.annotation_revision}),
            );
        }
        if let mcp_server::ExternalAction::FitView {
            expected_zoom,
            mode,
        } = request.action
        {
            if self.opening_document {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"document is opening"}),
                );
            }
            if document.model.view().viewport().scale().to_bits() != expected_zoom.to_bits() {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"stale current zoom"}),
                );
            }
            let fit_mode = if mode == "width" {
                FitMode::Width
            } else {
                FitMode::Page
            };
            let page = document.model.view().page() + 1;
            let scale = document.pdf.page_dimensions(page).ok().and_then(|size| {
                fit_scale(
                    fit_mode,
                    size,
                    self.window_size,
                    !self.hide_pages,
                    !self.hide_properties,
                    !self.hide_symbol_palette,
                    self.ribbon_collapsed,
                )
            });
            let Some(scale) = scale else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"page cannot be fitted"}),
                );
            };
            if scale.to_bits() == expected_zoom.to_bits() {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"fit did not change zoom"}),
                );
            }
            if request.was_cancelled() {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"request cancelled"}),
                );
            }
            let task = self.update_inner(Message::FitView(fit_mode));
            let version = self.document.as_ref().unwrap().model.version();
            let zoom = self
                .document
                .as_ref()
                .unwrap()
                .model
                .view()
                .viewport()
                .scale();
            return (
                task,
                serde_json::json!({"ok":true,"mode":mode,"zoom":zoom,"renderPending":true,
                    "documentId":version.id.0.to_string(),"generation":version.generation,
                    "annotationRevision":version.annotation_revision}),
            );
        }
        if let mcp_server::ExternalAction::SelectTool {
            expected: expected_tool,
            expected_epoch,
            requested,
        } = request.action
        {
            if self.opening_document
                || self.new_dialog
                || self.open_dialog
                || self.file_menu_open
                || self.about_dialog
                || self.save_dialog
                || self.import_dialog
                || self.xfdf_export_dialog
                || self.export_dialog
                || self.print_dialog
                || self.text_dialog
                || self.confirm_discard.is_some()
                || self.pending_clear_annotations.is_some()
                || self.pending_delete_annotation.is_some()
            {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"tool selection is unavailable during modal work"}),
                );
            }
            if self.annotation_tool.mcp_name() != expected_tool
                || self.annotation_tool_epoch != expected_epoch
            {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"stale current tool"}),
                );
            }
            let Some(tool) = AnnotationTool::from_mcp_name(requested) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"tool is not implemented"}),
                );
            };
            if self.annotation_tool == tool {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"tool selection did not change"}),
                );
            }
            let task = self.update_inner(Message::SelectAnnotationTool(tool));
            return (
                task,
                serde_json::json!({"ok":true,"tool":self.annotation_tool.mcp_name(),
                    "toolEpoch":self.annotation_tool_epoch,
                    "documentId":expected.id.0.to_string(),"generation":expected.generation,
                    "annotationRevision":expected.annotation_revision}),
            );
        }
        if let mcp_server::ExternalAction::MoveAnnotation { id, delta } = request.action {
            let Some(item) = document.model.get(id) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"annotation not found"}),
                );
            };
            if self.opening_document
                || item.page != document.displayed_page
                || item.page != document.model.view().page()
            {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"annotation is not on the current displayed page"}),
                );
            }
            let Ok(page_size) = document.pdf.page_dimensions(item.page + 1) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"page dimensions unavailable"}),
                );
            };
            let Some(moved) = translated_annotation(item, delta, page_size) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"move did not change annotation"}),
                );
            };
            if request.was_cancelled() {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"request cancelled"}),
                );
            }
            let bounds = moved.bounds;
            let document = self.document.as_mut().expect("document checked above");
            return match document
                .model
                .execute(expected, DocumentCommand::Edit(Edit::Replace(moved)))
            {
                Ok(true) => {
                    self.annotation_gesture_active = false;
                    self.annotation_gesture_preview = None;
                    self.message = "Annotation moved".into();
                    let version = document.model.version();
                    (
                        self.render_content_layer_if_needed(),
                        serde_json::json!({"ok":true,"action":"move_annotation",
                            "id":id.to_string(),"bounds":{"x":bounds.x,"y":bounds.y,
                                "width":bounds.width,"height":bounds.height},
                            "documentId":version.id.0.to_string(),"generation":version.generation,
                            "annotationRevision":version.annotation_revision}),
                    )
                }
                Ok(false) => (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"move did not change annotation"}),
                ),
                Err(CommandError::StaleDocument) => (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"stale document version"}),
                ),
                Err(CommandError::Edit(error)) => (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":format!("annotation edit failed: {error:?}")}),
                ),
            };
        }
        if let mcp_server::ExternalAction::ResizeAnnotation { id, bounds } = request.action {
            let Some(item) = document.model.get(id) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"annotation not found"}),
                );
            };
            if self.opening_document
                || item.page != document.displayed_page
                || item.page != document.model.view().page()
            {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"annotation is not on the current displayed page"}),
                );
            }
            let Ok(page_size) = document.pdf.page_dimensions(item.page + 1) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"page dimensions unavailable"}),
                );
            };
            let Some(resized) = resized_annotation(item, bounds, page_size) else {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"resize is unsupported, outside page or unchanged"}),
                );
            };
            if request.was_cancelled() {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"request cancelled"}),
                );
            }
            let document = self.document.as_mut().expect("document checked above");
            return match document
                .model
                .execute(expected, DocumentCommand::Edit(Edit::Replace(resized)))
            {
                Ok(true) => {
                    self.annotation_gesture_active = false;
                    self.annotation_gesture_preview = None;
                    self.message = "Annotation resized".into();
                    let version = document.model.version();
                    (
                        self.render_content_layer_if_needed(),
                        serde_json::json!({"ok":true,"action":"resize_annotation",
                            "id":id.to_string(),"bounds":{"x":bounds.x,"y":bounds.y,
                                "width":bounds.width,"height":bounds.height},
                            "documentId":version.id.0.to_string(),"generation":version.generation,
                            "annotationRevision":version.annotation_revision}),
                    )
                }
                Ok(false) => (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"resize did not change annotation"}),
                ),
                Err(CommandError::StaleDocument) => (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"stale document version"}),
                ),
                Err(CommandError::Edit(error)) => (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":format!("annotation edit failed: {error:?}")}),
                ),
            };
        }
        if let mcp_server::ExternalAction::ClearAnnotations(page) = request.action {
            if page.is_some_and(|page| page >= document.model.view().page_count()) {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"page is outside the document"}),
                );
            }
            let affected = document
                .model
                .items()
                .filter(|item| page.is_none_or(|page| item.page == page))
                .count();
            if affected == 0 {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"no annotations in requested scope"}),
                );
            }
            if affected > 4096 {
                return (
                    Task::none(),
                    serde_json::json!({"ok":false,"error":"too many annotations for one external transaction"}),
                );
            }
        }
        let document = self.document.as_mut().expect("document checked above");
        let command = match request.action {
            mcp_server::ExternalAction::Delete(id) => DocumentCommand::Edit(Edit::Remove(id)),
            mcp_server::ExternalAction::MoveAnnotation { .. } => {
                unreachable!("annotation move handled above")
            }
            mcp_server::ExternalAction::ResizeAnnotation { .. } => {
                unreachable!("annotation resize handled above")
            }
            mcp_server::ExternalAction::ClearAnnotations(page) => {
                DocumentCommand::Edit(Edit::Clear { page })
            }
            mcp_server::ExternalAction::Undo => DocumentCommand::Undo,
            mcp_server::ExternalAction::Redo => DocumentCommand::Redo,
            mcp_server::ExternalAction::Select(_) | mcp_server::ExternalAction::ClearSelection => {
                unreachable!("selection handled above")
            }
            mcp_server::ExternalAction::SelectBackend { .. } => {
                unreachable!("renderer selection handled above")
            }
            mcp_server::ExternalAction::GoToPage { .. } => {
                unreachable!("page selection handled above")
            }
            mcp_server::ExternalAction::FitView { .. } => {
                unreachable!("fit view handled above")
            }
            mcp_server::ExternalAction::SelectTool { .. } => {
                unreachable!("tool selection handled above")
            }
        };
        match document.model.execute(expected, command) {
            Ok(true) => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                if document
                    .selected_annotation
                    .is_some_and(|id| document.model.get(id).is_none())
                {
                    document.selected_annotation = None;
                    if self.active_ribbon == RibbonTab::Format {
                        self.active_ribbon = RibbonTab::Comment;
                    }
                }
                let (action, detail) = match request.action {
                    mcp_server::ExternalAction::Delete(id) => {
                        self.message = format!("Annotation {id} deleted");
                        ("delete", serde_json::json!({"deletedId":id.to_string()}))
                    }
                    mcp_server::ExternalAction::MoveAnnotation { .. } => {
                        unreachable!("annotation move handled above")
                    }
                    mcp_server::ExternalAction::ResizeAnnotation { .. } => {
                        unreachable!("annotation resize handled above")
                    }
                    mcp_server::ExternalAction::ClearAnnotations(page) => {
                        self.message = if page.is_some() {
                            "Page annotations cleared".into()
                        } else {
                            "All annotations cleared".into()
                        };
                        (
                            "clear_annotations",
                            serde_json::json!({"page":page.map(|page| page+1)}),
                        )
                    }
                    mcp_server::ExternalAction::Undo => {
                        self.message = "Undo".into();
                        ("undo", serde_json::json!({}))
                    }
                    mcp_server::ExternalAction::Redo => {
                        self.message = "Redo".into();
                        ("redo", serde_json::json!({}))
                    }
                    mcp_server::ExternalAction::Select(_)
                    | mcp_server::ExternalAction::ClearSelection => {
                        unreachable!("selection handled above")
                    }
                    mcp_server::ExternalAction::SelectBackend { .. } => {
                        unreachable!("renderer selection handled above")
                    }
                    mcp_server::ExternalAction::GoToPage { .. } => {
                        unreachable!("page selection handled above")
                    }
                    mcp_server::ExternalAction::FitView { .. } => {
                        unreachable!("fit view handled above")
                    }
                    mcp_server::ExternalAction::SelectTool { .. } => {
                        unreachable!("tool selection handled above")
                    }
                };
                let version = document.model.version();
                let mut payload = serde_json::json!({"ok":true,"action":action,
                    "documentId":version.id.0.to_string(),"generation":version.generation,
                    "annotationRevision":version.annotation_revision});
                if let (Some(payload), Some(detail)) = (payload.as_object_mut(), detail.as_object())
                {
                    payload.extend(detail.clone());
                }
                (self.render_content_layer_if_needed(), payload)
            }
            Ok(false) => (
                Task::none(),
                serde_json::json!({"ok":false,"error":"no change"}),
            ),
            Err(CommandError::StaleDocument) => (
                Task::none(),
                serde_json::json!({"ok":false,"error":"stale document version"}),
            ),
            Err(CommandError::Edit(error)) => (
                Task::none(),
                serde_json::json!({"ok":false,"error":format!("annotation edit failed: {error:?}")}),
            ),
        }
    }

    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ExternalRequest(_) => return Task::none(),
            Message::ToggleFileMenu => self.file_menu_open = !self.file_menu_open,
            Message::SelectFileOpenPanel => self.file_menu_open = true,
            Message::CloseFileMenu => self.file_menu_open = false,
            Message::ShowAbout => {
                self.file_menu_open = false;
                self.about_dialog = true;
            }
            Message::CloseAbout => self.about_dialog = false,
            Message::ShowExtensions => {
                self.file_menu_open = false;
                self.extension_import_dialog = false;
                self.extensions_dialog = true;
                if self.extension_html.text().is_empty() {
                    self.extension_html = iced::widget::text_editor::Content::with_text(
                        extension_builder::default_html(),
                    );
                }
                if self.extension_javascript.text().is_empty() {
                    self.extension_javascript = iced::widget::text_editor::Content::with_text(
                        extension_builder::default_javascript(),
                    );
                }
            }
            Message::ShowExtensionImport => {
                self.file_menu_open = false;
                self.extensions_dialog = false;
                self.extension_import_dialog = true;
                self.extension_error.clear();
            }
            Message::CloseExtensions => self.extensions_dialog = false,
            Message::CloseExtensionImport => self.extension_import_dialog = false,
            Message::ExtensionIdChanged(value) => self.extension_id = value,
            Message::ExtensionNameChanged(value) => self.extension_name = value,
            Message::ExtensionOutputChanged(value) => self.extension_output = value,
            Message::ExtensionImportPathChanged(value) => self.extension_import_path = value,
            Message::ImportExtension => {
                match extension_builder::import(Path::new(self.extension_import_path.trim())) {
                    Ok(draft) => {
                        self.extension_id = draft.id;
                        self.extension_name = draft.name;
                        self.extension_html =
                            iced::widget::text_editor::Content::with_text(&draft.html);
                        self.extension_javascript =
                            iced::widget::text_editor::Content::with_text(&draft.javascript);
                        self.extension_output.clear();
                        self.extension_error.clear();
                        self.extension_import_dialog = false;
                        self.extensions_dialog = true;
                    }
                    Err(error) => self.extension_error = error,
                }
            }
            Message::ExtensionHtmlEdited(action) => self.extension_html.perform(action),
            Message::ExtensionJavascriptEdited(action) => self.extension_javascript.perform(action),
            Message::BuildExtension => {
                let output = PathBuf::from(self.extension_output.trim());
                match extension_builder::build(
                    &output,
                    self.extension_id.trim(),
                    &self.extension_name,
                    &self.extension_html.text(),
                    &self.extension_javascript.text(),
                ) {
                    Ok(()) => {
                        self.extension_error.clear();
                        self.message = format!("Extension package created: {}", output.display());
                    }
                    Err(error) => self.extension_error = error,
                }
            }
            Message::EscapeOverlay => {
                if self.new_dialog {
                    self.new_dialog = false;
                } else if self.pending_delete_annotation.is_some() {
                    self.pending_delete_annotation = None;
                } else if self.pending_clear_annotations.is_some() {
                    self.pending_clear_annotations = None;
                } else if self.extension_import_dialog {
                    self.extension_import_dialog = false;
                } else if self.extensions_dialog {
                    self.extensions_dialog = false;
                } else if self.about_dialog {
                    self.about_dialog = false;
                } else if self.import_dialog {
                    self.import_dialog = false;
                } else if self.xfdf_export_dialog {
                    self.xfdf_export_dialog = false;
                } else if self.file_menu_open {
                    self.file_menu_open = false;
                }
            }
            Message::CaptureUiScreenshot => {
                return window::oldest()
                    .and_then(window::screenshot)
                    .map(Message::UiScreenshotCaptured);
            }
            Message::UiScreenshotCaptured(screenshot) => {
                if let Some(path) = std::env::var_os("OPDS_UI_SCREENSHOT_RGBA") {
                    let path = PathBuf::from(path);
                    let _ = std::fs::write(&path, screenshot.rgba.as_ref());
                    let _ = std::fs::write(
                        path.with_extension("size"),
                        format!("{} {}\n", screenshot.size.width, screenshot.size.height),
                    );
                }
                return window::oldest().and_then(window::close);
            }
            Message::ShowOpen => {
                self.file_menu_open = false;
                self.open_dialog = true;
                let initial = self
                    .document
                    .as_ref()
                    .and_then(|document| {
                        Path::new(&document.path)
                            .parent()
                            .filter(|parent| !parent.as_os_str().is_empty())
                            .and_then(|parent| parent.canonicalize().ok())
                    })
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
                return self.update(Message::BrowseDirectory(initial));
            }
            Message::BrowseDirectory(path) => {
                self.next_browse = self.next_browse.wrapping_add(1);
                let token = self.next_browse;
                self.browser_dir = path.clone();
                self.browser_entries.clear();
                self.browser_query.clear();
                self.browser_scroll_y = 0.0;
                self.browser_busy = true;
                self.browser_error.clear();
                return Task::perform(async move { list_pdf_directory(&path) }, move |result| {
                    Message::Browsed(token, result)
                });
            }
            Message::Browsed(token, result) => {
                if token != self.next_browse || !self.open_dialog {
                    return Task::none();
                }
                self.browser_busy = false;
                match result {
                    Ok(entries) => {
                        self.browser_entries = entries;
                    }
                    Err(error) => self.browser_error = error,
                }
                return scroll_browser_to(0.0);
            }
            Message::SelectPdf(path) => {
                self.file_menu_open = false;
                self.path_input = path.to_string_lossy().into_owned();
                return self.update(Message::Open);
            }
            Message::RecentSearchChanged(query) => self.recent_query = query,
            Message::TogglePinnedRecents => {
                self.pinned_recents_collapsed = !self.pinned_recents_collapsed;
            }
            Message::ToggleUnpinnedRecents => {
                self.unpinned_recents_collapsed = !self.unpinned_recents_collapsed;
            }
            Message::ToggleRecentPinned(path) => {
                if let Some(entry) = self
                    .recent_files
                    .iter_mut()
                    .find(|entry| entry.path == path)
                {
                    entry.pinned = !entry.pinned;
                    self.persist_recent_files();
                }
            }
            Message::RemoveRecent(path) => {
                let before = self.recent_files.len();
                self.recent_files.retain(|entry| entry.path != path);
                if self.recent_files.len() != before {
                    self.persist_recent_files();
                }
            }
            Message::BrowserSearchChanged(query) => {
                self.browser_query = query;
                self.browser_scroll_y = 0.0;
                return scroll_browser_to(0.0);
            }
            Message::BrowserScrolled(viewport) => {
                self.browser_scroll_y = viewport.absolute_offset().y;
            }
            Message::ShowSave => {
                self.file_menu_open = false;
                if let Some(document) = &self.document {
                    let path = std::path::Path::new(&document.path);
                    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                    self.save_path_input = if document.is_new {
                        "Untitled.pdf".into()
                    } else {
                        path.with_file_name(format!("{stem}-annotated.pdf"))
                            .to_string_lossy()
                            .into_owned()
                    };
                    self.save_dialog = true;
                    self.save_scroll_offset = Some(self.page_scroll_offset);
                }
            }
            Message::Save => {
                if self.save_busy {
                    return Task::none();
                }
                let Some((path, expected_digest)) = self.saved_target.clone() else {
                    return self.update(Message::ShowSave);
                };
                let Some(document) = &self.document else {
                    return Task::none();
                };
                let source = Arc::clone(&document.source_bytes);
                let annotations: Vec<_> = document.model.items().cloned().collect();
                let saved_annotations = annotations.clone();
                let token = self.next_load;
                self.file_menu_open = false;
                self.save_busy = true;
                self.message = "Saving PDF…".into();
                return Task::perform(
                    async move {
                        let bytes = pdf_save::export_annotations(&source, &annotations)?;
                        pdf_save::replace_unchanged(
                            std::path::Path::new(&path),
                            expected_digest,
                            &bytes,
                        )?;
                        let digest = pdf_save::saved_bytes_digest(&bytes);
                        Ok((path, saved_annotations, digest))
                    },
                    move |result| Message::Saved(token, result),
                );
            }
            Message::CancelSave => {
                self.save_dialog = false;
                if let Some(offset) = self.save_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return scroll_page_to(offset);
                }
            }
            Message::SavePathChanged(value) => self.save_path_input = value,
            Message::SaveAs => {
                if self.save_busy {
                    return Task::none();
                }
                let Some(document) = &self.document else {
                    return Task::none();
                };
                let path = self.save_path_input.trim().to_owned();
                if path.is_empty() {
                    self.message = "Enter a destination PDF path".into();
                    return Task::none();
                }
                if !document.is_new
                    && std::path::Path::new(&path) == std::path::Path::new(&document.path)
                {
                    self.message = "Choose a different path for Save As".into();
                    return Task::none();
                }
                let source = Arc::clone(&document.source_bytes);
                let annotations: Vec<_> = document.model.items().cloned().collect();
                let saved_annotations = annotations.clone();
                let token = self.next_load;
                self.save_dialog = false;
                self.save_busy = true;
                self.message = "Saving PDF…".into();
                let save = Task::perform(
                    async move {
                        let bytes = pdf_save::export_annotations(&source, &annotations)?;
                        pdf_save::save_new(std::path::Path::new(&path), &bytes)?;
                        let digest = pdf_save::saved_bytes_digest(&bytes);
                        Ok((path, saved_annotations, digest))
                    },
                    move |result| Message::Saved(token, result),
                );
                if let Some(offset) = self.save_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return Task::batch([save, scroll_page_to(offset)]);
                }
                return save;
            }
            Message::Saved(token, result) => {
                if token != self.next_load {
                    return Task::none();
                }
                self.save_busy = false;
                match result {
                    Ok((path, annotations, digest)) => {
                        let mut record_new_path = false;
                        if let Some(document) = &mut self.document {
                            document.last_saved_annotations = annotations;
                            if document.is_new {
                                document.path = path.clone();
                                document.is_new = false;
                                record_new_path = true;
                            }
                            self.saved_target = Some((path.clone(), digest));
                            self.message = format!("Saved PDF: {path}");
                        }
                        if record_new_path {
                            self.record_recent_file(&path);
                        }
                    }
                    Err(error) => self.message = format!("Save As failed: {error}"),
                }
            }
            Message::ShowImport => {
                if self.opening_document || self.save_busy {
                    return Task::none();
                }
                self.file_menu_open = false;
                if let Some(document) = &self.document {
                    self.import_path_input = Path::new(&document.path)
                        .with_extension("xfdf")
                        .to_string_lossy()
                        .into_owned();
                    self.import_dialog = true;
                }
            }
            Message::ImportPathChanged(value) => self.import_path_input = value,
            Message::CancelImport => self.import_dialog = false,
            Message::ImportXfdf => {
                if self.opening_document || self.save_busy {
                    return Task::none();
                }
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                let path = self.import_path_input.trim();
                if path.is_empty() {
                    self.message = "Choose an XFDF file".into();
                    return Task::none();
                }
                let expected = document.model.version();
                let items = match xfdf_import::read(
                    Path::new(path),
                    document.model.view().page_count(),
                    self.next_annotation_id,
                ) {
                    Ok(items) => items,
                    Err(error) => {
                        self.message = format!("XFDF import failed: {error}");
                        return Task::none();
                    }
                };
                let Some(next_id) = self.next_annotation_id.checked_add(items.len() as u64) else {
                    self.message = "XFDF annotation IDs exhausted".into();
                    return Task::none();
                };
                if document.model.items().count().saturating_add(items.len()) > 4096 {
                    self.message = "XFDF would exceed the 4096-annotation native limit".into();
                    return Task::none();
                }
                let count = items.len();
                match document
                    .model
                    .execute(expected, DocumentCommand::Edit(Edit::AddMany(items)))
                {
                    Ok(true) => {
                        self.next_annotation_id = next_id;
                        self.import_dialog = false;
                        self.message = format!("Imported {count} XFDF annotations");
                        return self.render_content_layer_if_needed();
                    }
                    Ok(false) => {}
                    Err(error) => self.message = format!("XFDF import failed: {error:?}"),
                }
            }
            Message::ShowXfdfExport => {
                if self.opening_document || self.save_busy {
                    return Task::none();
                }
                self.file_menu_open = false;
                if let Some(document) = &self.document {
                    self.xfdf_export_path_input = Path::new(&document.path)
                        .with_extension("xfdf")
                        .to_string_lossy()
                        .into_owned();
                    self.xfdf_export_dialog = true;
                }
            }
            Message::XfdfExportPathChanged(value) => self.xfdf_export_path_input = value,
            Message::CancelXfdfExport => self.xfdf_export_dialog = false,
            Message::ExportXfdf => {
                if self.opening_document || self.save_busy {
                    return Task::none();
                }
                let Some(document) = &self.document else {
                    return Task::none();
                };
                let path = self.xfdf_export_path_input.trim();
                if path.is_empty() {
                    self.message = "Choose an XFDF destination".into();
                    return Task::none();
                }
                let items: Vec<_> = document.model.items().cloned().collect();
                let name = Path::new(&document.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("document.pdf");
                match xfdf_export::serialize(&items, name)
                    .and_then(|bytes| pdf_save::save_new(Path::new(path), &bytes))
                {
                    Ok(()) => {
                        self.xfdf_export_dialog = false;
                        self.message = format!("Exported {} XFDF annotations", items.len());
                    }
                    Err(error) => self.message = format!("XFDF export failed: {error}"),
                }
            }
            Message::ShowExport => {
                if self.export_busy {
                    return Task::none();
                }
                self.file_menu_open = false;
                if let Some(document) = &self.document {
                    self.export_include_annotations = true;
                    self.export_page = document.model.view().page();
                    self.export_page_input = (self.export_page + 1).to_string();
                    self.export_range = image_export::ImagePageRange::Current;
                    self.export_custom_pages = self.export_page_input.clone();
                    self.export_path_input =
                        export_page_path(&document.path, self.export_page + 1, self.export_format);
                    self.export_dialog = true;
                    self.export_scroll_offset = Some(self.page_scroll_offset);
                }
            }
            Message::ExportPathChanged(value) => self.export_path_input = value,
            Message::SelectExportDpi(dpi) => self.export_dpi = dpi,
            Message::SelectJpegQuality(value) => {
                if let Ok(quality) = image_export::JpegQuality::new(value) {
                    self.export_jpeg_quality = quality;
                }
            }
            Message::SetExportAnnotations(value) => self.export_include_annotations = value,
            Message::SelectExportFormat(format) => {
                if let Some(document) = &self.document {
                    let old_default = match self.export_range {
                        image_export::ImagePageRange::Current => export_page_path(
                            &document.path,
                            self.export_page + 1,
                            self.export_format,
                        ),
                        _ => export_pages_folder(&document.path, self.export_format),
                    };
                    if self.export_path_input == old_default {
                        self.export_path_input = match self.export_range {
                            image_export::ImagePageRange::Current => {
                                export_page_path(&document.path, self.export_page + 1, format)
                            }
                            _ => export_pages_folder(&document.path, format),
                        };
                    }
                }
                self.export_format = format;
            }
            Message::SelectExportRange(range) => {
                if let Some(document) = &self.document {
                    let old_default = match self.export_range {
                        image_export::ImagePageRange::Current => export_page_path(
                            &document.path,
                            self.export_page + 1,
                            self.export_format,
                        ),
                        _ => export_pages_folder(&document.path, self.export_format),
                    };
                    if self.export_path_input == old_default {
                        self.export_path_input = match range {
                            image_export::ImagePageRange::Current => export_page_path(
                                &document.path,
                                self.export_page + 1,
                                self.export_format,
                            ),
                            _ => export_pages_folder(&document.path, self.export_format),
                        };
                    }
                    self.export_range = range;
                }
            }
            Message::ExportCustomPagesChanged(value) => self.export_custom_pages = value,
            Message::ExportPageChanged(value) => {
                self.export_page_input = value;
                if let Some(document) = &self.document {
                    if let Ok(page_number) = self.export_page_input.parse::<usize>() {
                        if (1..=document.pdf.page_count()).contains(&page_number) {
                            let old_default = export_page_path(
                                &document.path,
                                self.export_page + 1,
                                self.export_format,
                            );
                            if self.export_range == image_export::ImagePageRange::Current
                                && self.export_path_input == old_default
                            {
                                self.export_path_input = export_page_path(
                                    &document.path,
                                    page_number,
                                    self.export_format,
                                );
                            }
                            self.export_page = page_number - 1;
                        }
                    }
                }
            }
            Message::ExportPng => {
                if self.export_busy {
                    return Task::none();
                }
                let Some(document) = &self.document else {
                    return Task::none();
                };
                let pages = match self.export_range {
                    image_export::ImagePageRange::Current => {
                        if self.export_page_input.parse::<usize>().ok()
                            != Some(self.export_page + 1)
                            || self.export_page >= document.pdf.page_count()
                        {
                            self.message =
                                format!("Choose a page from 1 to {}", document.pdf.page_count());
                            return Task::none();
                        }
                        vec![self.export_page]
                    }
                    image_export::ImagePageRange::All => (0..document.pdf.page_count()).collect(),
                    image_export::ImagePageRange::Custom => {
                        match image_export::parse_page_range(
                            &self.export_custom_pages,
                            document.pdf.page_count(),
                        ) {
                            Ok(pages) => pages,
                            Err(error) => {
                                self.message = error;
                                return Task::none();
                            }
                        }
                    }
                };
                let path = self.export_path_input.trim().to_owned();
                if path.is_empty() {
                    self.message = "Enter a destination path".into();
                    return Task::none();
                }
                let source = Arc::clone(&document.source_bytes);
                let annotations: Vec<_> = if self.export_include_annotations {
                    document.model.items().cloned().collect()
                } else {
                    Vec::new()
                };
                let backend = self.backend;
                let dpi = self.export_dpi;
                let format = self.export_format;
                let quality = self.export_jpeg_quality;
                let range = self.export_range;
                let source_name = document.path.clone();
                let progress_interval = (pages.len() / 100).max(1);
                let progress_total = pages.len();
                let cancel = Arc::new(AtomicBool::new(false));
                self.export_cancel = Some(Arc::clone(&cancel));
                self.export_busy = true;
                self.export_completed = 0;
                self.export_total = pages.len();
                self.message = format!("Exporting {} page(s) as {format}…", pages.len());
                let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
                std::thread::spawn(move || {
                    let result = (|| {
                        if range == image_export::ImagePageRange::Current {
                            image_export::current_page_image_cancellable(
                                source,
                                &annotations,
                                pages[0],
                                backend,
                                dpi,
                                format,
                                quality,
                                Path::new(&path),
                                &cancel,
                            )?;
                            let _ = sender.unbounded_send(ExportEvent::Progress(1));
                            Ok(format!("Exported page {format}: {path}"))
                        } else {
                            let count = image_export::selected_pages_image_with_progress(
                                source,
                                &annotations,
                                &pages,
                                backend,
                                dpi,
                                format,
                                quality,
                                Path::new(&path),
                                &source_name,
                                &cancel,
                                |completed| {
                                    if (completed % progress_interval == 0
                                        || completed == progress_total)
                                        && sender
                                            .unbounded_send(ExportEvent::Progress(completed))
                                            .is_err()
                                    {
                                        cancel.store(true, Ordering::Relaxed);
                                    }
                                },
                            )?;
                            Ok(format!("Exported {count} {format} images to {path}"))
                        }
                    })();
                    let _ = sender.unbounded_send(ExportEvent::Finished(result));
                });
                return Task::run(receiver, |event| match event {
                    ExportEvent::Progress(completed) => Message::ExportProgress(completed),
                    ExportEvent::Finished(result) => Message::Exported(result),
                });
            }
            Message::ExportProgress(completed) => {
                if self.export_busy && completed <= self.export_total {
                    self.export_completed = completed;
                    if !self
                        .export_cancel
                        .as_ref()
                        .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
                    {
                        self.message =
                            format!("Exporting {completed}/{} pages…", self.export_total);
                    }
                }
            }
            Message::Exported(result) => {
                self.export_busy = false;
                self.export_cancel = None;
                match result {
                    Ok(message) => {
                        self.export_dialog = false;
                        self.message = message;
                        if let Some(offset) = self.export_scroll_offset.take() {
                            self.page_scroll_offset = offset;
                            return scroll_page_to(offset);
                        }
                    }
                    Err(error) if error == "Export cancelled" => {
                        self.export_dialog = false;
                        self.message = error;
                        if let Some(offset) = self.export_scroll_offset.take() {
                            self.page_scroll_offset = offset;
                            return scroll_page_to(offset);
                        }
                    }
                    Err(error) => self.message = format!("Image export failed: {error}"),
                }
            }
            Message::CancelExport => {
                if self.export_busy {
                    if let Some(cancel) = &self.export_cancel {
                        cancel.store(true, Ordering::Relaxed);
                        self.message = "Cancelling image export after the current page…".into();
                    }
                    return Task::none();
                }
                self.export_dialog = false;
                if let Some(offset) = self.export_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return scroll_page_to(offset);
                }
            }
            Message::ShowPrint => {
                self.file_menu_open = false;
                if let Some(document) = &self.document {
                    let path = std::path::Path::new(&document.path);
                    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                    self.print_path_input = path
                        .with_file_name(format!("{stem}-print.pdf"))
                        .to_string_lossy()
                        .into_owned();
                    self.print_dialog = true;
                    self.print_scroll_offset = Some(self.page_scroll_offset);
                    self.print_destination = PrintDestination::PdfFile;
                    self.print_range = PrintRange::All;
                    self.print_subset = PrintSubset::All;
                    self.print_reverse = false;
                    self.print_custom_pages.clear();
                    self.print_paper = pdf_save::PrintPaper::Original;
                    self.print_scale = pdf_save::PrintScale::Fit;
                    self.print_zoom_percent = 100;
                    self.print_zoom_draft = "100".into();
                    self.print_preview_page = 0;
                    self.print_preview_index = 0;
                    self.print_preview = None;
                    self.print_preview_pdf = None;
                    self.print_preview_base_bytes = None;
                    self.print_preview_base_pdf = None;
                    self.print_preview_error.clear();
                    self.print_preview_document_revision =
                        self.print_preview_document_revision.wrapping_add(1);
                    let revision = self.print_preview_document_revision;
                    let source = Arc::clone(&document.source_bytes);
                    let annotations: Vec<_> = document.model.items().cloned().collect();
                    if annotations.is_empty() {
                        self.print_preview_base_bytes = Some(source);
                        self.print_preview_base_pdf = Some(Arc::clone(&document.pdf));
                        self.print_preview_pdf = Some(Arc::clone(&document.pdf));
                        return self.refresh_print_preview();
                    }
                    return Task::perform(
                        async move {
                            let bytes = pdf_save::export_annotations(&source, &annotations)?;
                            let bytes = Arc::new(bytes);
                            let pdf = ParsedPdf::open(Arc::clone(&bytes))
                                .map(Arc::new)
                                .map_err(|e| e.to_string())?;
                            Ok((bytes, pdf))
                        },
                        move |result| Message::PrintPreviewDocumentReady(revision, result),
                    );
                }
            }
            Message::CancelPrint => {
                self.print_dialog = false;
                self.print_preview_pdf = None;
                self.print_preview_base_bytes = None;
                self.print_preview_base_pdf = None;
                self.print_preview = None;
                if let Some(offset) = self.print_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return scroll_page_to(offset);
                }
            }
            Message::PrintPathChanged(value) => self.print_path_input = value,
            Message::SelectPrintDestination(destination) => self.print_destination = destination,
            Message::SelectPrintRange(range) => {
                self.print_range = range;
                self.print_preview_index = 0;
                return self.refresh_print_preview();
            }
            Message::SelectPrintSubset(subset) => {
                self.print_subset = subset;
                self.print_preview_index = 0;
                return self.refresh_print_preview();
            }
            Message::SetPrintReverse(value) => {
                self.print_reverse = value;
                self.print_preview_index = 0;
                return self.refresh_print_preview();
            }
            Message::PrintCustomPagesChanged(value) => {
                self.print_custom_pages = value;
                if self.print_range == PrintRange::Custom {
                    self.print_preview_index = 0;
                    return self.refresh_print_preview();
                }
            }
            Message::PrintZoomDraftChanged(value) => self.print_zoom_draft = value,
            Message::SelectPrintPaper(paper) => {
                self.print_paper = paper;
                return self.refresh_print_placement_preview();
            }
            Message::SelectPrintScale(scale) => {
                self.print_scale = scale;
                return self.refresh_print_placement_preview();
            }
            Message::ApplyPrintZoom => {
                if self.print_preview_base_bytes.is_none() {
                    return Task::none();
                }
                let Ok(percent) = self.print_zoom_draft.trim().parse::<u16>() else {
                    self.print_preview_error = "Page Zoom must be 10–400%".into();
                    return Task::none();
                };
                if !(10..=400).contains(&percent) {
                    self.print_preview_error = "Page Zoom must be 10–400%".into();
                    return Task::none();
                }
                self.print_zoom_percent = percent;
                self.print_scale = pdf_save::PrintScale::Custom;
                return self.refresh_print_placement_preview();
            }
            Message::PrintPreviewStep(step) => {
                if let Some(document) = &self.document {
                    if let Ok(pages) = print_pages(
                        self.print_range,
                        self.print_subset,
                        self.print_reverse,
                        document.model.view().page(),
                        &self.print_custom_pages,
                        document.pdf.page_count(),
                    ) {
                        self.print_preview_index = self
                            .print_preview_index
                            .saturating_add_signed(step as isize)
                            .min(pages.len().saturating_sub(1));
                        return self.refresh_print_preview();
                    }
                }
            }
            Message::PrintPreviewDocumentReady(revision, result) => {
                if self.print_dialog && revision == self.print_preview_document_revision {
                    match result {
                        Ok((bytes, pdf)) => {
                            self.print_preview_base_bytes = Some(bytes);
                            self.print_preview_base_pdf = Some(Arc::clone(&pdf));
                            self.print_preview_pdf = Some(pdf);
                            return self.refresh_print_placement_preview();
                        }
                        Err(error) => self.print_preview_error = error,
                    }
                }
            }
            Message::PrintScaledPreviewReady(revision, result) => {
                if self.print_dialog && revision == self.print_preview_document_revision {
                    match result {
                        Ok(pdf) => {
                            self.print_preview_pdf = Some(pdf);
                            return self.refresh_print_preview();
                        }
                        Err(error) => self.print_preview_error = error,
                    }
                }
            }
            Message::PrintPreviewReady(revision, page, result) => {
                if self.print_dialog && revision == self.print_preview_revision {
                    match result {
                        Ok(raster) => {
                            self.print_preview = Some(image::Handle::from_rgba(
                                raster.width,
                                raster.height,
                                raster.rgba,
                            ));
                            self.print_preview_page = page;
                            self.print_preview_error.clear();
                        }
                        Err(error) => {
                            self.print_preview = None;
                            self.print_preview_error = error;
                        }
                    }
                }
            }
            Message::PrintToPdf => {
                if self.print_busy {
                    return Task::none();
                }
                if self.print_preview_pdf.is_none() {
                    self.print_preview_error = "Wait for a valid print preview".into();
                    return Task::none();
                }
                let Some(document) = &self.document else {
                    return Task::none();
                };
                let path = self.print_path_input.trim().to_owned();
                if path.is_empty() {
                    self.message = "Enter a print PDF destination path".into();
                    return Task::none();
                }
                if std::path::Path::new(&path) == std::path::Path::new(&document.path) {
                    self.message = "Choose a different path for the print PDF".into();
                    return Task::none();
                }
                let source = Arc::clone(&document.source_bytes);
                let destination = self.print_destination;
                let paper = self.print_paper;
                let scale = self.print_scale;
                let zoom = self.print_zoom_percent;
                let annotations: Vec<_> = document.model.items().cloned().collect();
                let pages = match print_pages(
                    self.print_range,
                    self.print_subset,
                    self.print_reverse,
                    document.model.view().page(),
                    &self.print_custom_pages,
                    document.pdf.page_count(),
                ) {
                    Ok(pages) => pages,
                    Err(error) => {
                        self.message = error;
                        return Task::none();
                    }
                };
                self.print_dialog = false;
                self.print_preview_pdf = None;
                self.print_preview_base_bytes = None;
                self.print_preview_base_pdf = None;
                self.print_preview = None;
                self.print_busy = true;
                self.message = "Creating print PDF…".into();
                let print = Task::perform(
                    async move {
                        let bytes = pdf_save::export_print_pages(&source, &annotations, &pages)?;
                        let bytes = pdf_save::apply_print_placement(&bytes, paper, scale, zoom)?;
                        let bytes = if destination == PrintDestination::VirtualIpp {
                            virtual_ipp::print_roundtrip(&bytes)?
                        } else {
                            bytes
                        };
                        pdf_save::save_new(std::path::Path::new(&path), &bytes)?;
                        Ok(if destination == PrintDestination::VirtualIpp {
                            format!("Virtual IPP captured PDF: {path}")
                        } else {
                            format!("Print PDF created: {path}")
                        })
                    },
                    Message::Printed,
                );
                if let Some(offset) = self.print_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return Task::batch([print, scroll_page_to(offset)]);
                }
                return print;
            }
            Message::Printed(result) => {
                self.print_busy = false;
                match result {
                    Ok(message) => self.message = message,
                    Err(error) => self.message = format!("Print to PDF failed: {error}"),
                }
            }
            Message::CancelOpen => {
                self.open_dialog = false;
                self.next_browse = self.next_browse.wrapping_add(1);
            }
            Message::CancelDiscard => self.confirm_discard = None,
            Message::ConfirmDiscard => {
                let Some(action) = self.confirm_discard.take() else {
                    return Task::none();
                };
                return match action {
                    PendingAction::New => {
                        self.allow_discard_once = true;
                        self.update(Message::NewBlank)
                    }
                    PendingAction::Open => {
                        self.allow_discard_once = true;
                        self.update(Message::Open)
                    }
                    PendingAction::Close => {
                        self.allow_discard_once = true;
                        self.update(Message::Close)
                    }
                    PendingAction::Exit => {
                        let _ = self.persist_reader_position();
                        window::oldest().and_then(window::close)
                    }
                };
            }
            Message::TogglePages => {
                self.hide_pages = !self.hide_pages;
                return self.render_nearby_thumbnail();
            }
            Message::ToggleProperties => {
                self.file_menu_open = false;
                if self.show_annotation_list {
                    self.show_annotation_list = false;
                    self.hide_properties = false;
                } else {
                    self.hide_properties = !self.hide_properties;
                }
            }
            Message::ToggleSymbolPalette => {
                self.hide_symbol_palette = !self.hide_symbol_palette;
            }
            Message::ShowProperties => {
                self.file_menu_open = false;
                self.show_annotation_list = false;
                self.hide_properties = false;
            }
            Message::TogglePropertySection(index) => {
                if let Some(collapsed) = self.properties_collapsed.get_mut(index) {
                    *collapsed = !*collapsed;
                }
            }
            Message::ToggleAnnotationList => {
                self.show_annotation_list = !self.show_annotation_list;
                self.hide_properties = !self.show_annotation_list;
            }
            Message::ToggleReaderMode => {
                let Some(document) = &self.document else {
                    return Task::none();
                };
                if self.reader_mode_active {
                    match reader_mode::clear(Path::new(&document.path)) {
                        Ok(()) => {
                            self.reader_mode_active = false;
                            self.message = "Reader Mode disabled".into();
                        }
                        Err(error) => {
                            self.message = format!("Reader Mode could not be disabled: {error}")
                        }
                    }
                } else {
                    self.reader_mode_active = true;
                    match self.persist_reader_position() {
                        Ok(()) => self.message = "Reader Mode enabled".into(),
                        Err(error) => {
                            self.reader_mode_active = false;
                            self.message = format!("Reader Mode could not save: {error}");
                        }
                    }
                }
            }
            Message::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                let mode = if self.fullscreen {
                    window::Mode::Fullscreen
                } else {
                    window::Mode::Windowed
                };
                return window::oldest().and_then(move |id| window::set_mode(id, mode));
            }
            Message::ToggleKeystrokes => self.keystrokes.toggle(),
            Message::KeystrokePressed(key, modifiers, captured) => {
                if self.file_menu_open
                    && matches!(
                        key,
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    )
                {
                    self.file_menu_open = false;
                    return Task::none();
                }
                let modal_open = self.new_dialog
                    || self.open_dialog
                    || self.file_menu_open
                    || self.about_dialog
                    || self.save_dialog
                    || self.import_dialog
                    || self.xfdf_export_dialog
                    || self.export_dialog
                    || self.print_dialog
                    || self.text_dialog
                    || self.confirm_discard.is_some();
                let modal_open = modal_open
                    || self.pending_clear_annotations.is_some()
                    || self.pending_delete_annotation.is_some();
                if !captured
                    && !modal_open
                    && !modifiers.control()
                    && matches!(
                        key,
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete)
                    )
                    && self
                        .document
                        .as_ref()
                        .and_then(|document| document.selected_annotation)
                        .is_some()
                {
                    return self.update(Message::RequestDeleteAnnotation);
                }
                // Match the Tauri Drawing workflow: keyboard undo/redo must
                // reach the same validated DocumentCommand path as the toolbar.
                // Captured keys belong to an active text field or other widget.
                if !captured && !modal_open && modifiers.control() && self.document.is_some() {
                    if let iced::keyboard::Key::Character(character) = &key {
                        if character.eq_ignore_ascii_case("s") {
                            return self.update(if modifiers.shift() {
                                Message::ShowSave
                            } else {
                                Message::Save
                            });
                        }
                        if character.eq_ignore_ascii_case("z") {
                            return self.update(if modifiers.shift() {
                                Message::Redo
                            } else {
                                Message::Undo
                            });
                        }
                        if character.eq_ignore_ascii_case("y") {
                            return self.update(Message::Redo);
                        }
                        if character.eq_ignore_ascii_case("c") && !modifiers.shift() {
                            return self.update(Message::CopySelectedAnnotation);
                        }
                        if character.eq_ignore_ascii_case("x") && !modifiers.shift() {
                            return self.update(Message::CutSelectedAnnotation);
                        }
                        if character.eq_ignore_ascii_case("v") {
                            return self.update(Message::PasteAnnotation {
                                in_place: modifiers.shift(),
                            });
                        }
                    }
                }
                if !captured && !modal_open && modifiers.control() {
                    if let iced::keyboard::Key::Character(character) = &key {
                        if character.eq_ignore_ascii_case("n") {
                            return self.update(Message::ShowNew);
                        }
                    }
                }
                if let Some(label) = keystrokes::label(&key, modifiers, captured, modal_open) {
                    if let Some(delay) = self.keystrokes.record(label, Instant::now()) {
                        return keystroke_timeout(delay);
                    }
                }
            }
            Message::KeystrokesExpired => {
                if let Some(delay) = self.keystrokes.expire(Instant::now()) {
                    return keystroke_timeout(delay);
                }
            }
            Message::SelectBackend(backend) => {
                if self.opening_document
                    || self.backend == backend
                    || (backend == Backend::Dynlex && !cfg!(feature = "dynlex-experimental"))
                    || (backend == Backend::Pdfium && !opds_dynlex_backend::pdfium_available())
                    || (backend == Backend::OwnRust && !opds_dynlex_backend::own_rust_available())
                    || (backend == Backend::PdfCraft && !opds_dynlex_backend::pdfcraft_available())
                {
                    return Task::none();
                }
                let previous_backend = self.backend;
                self.backend = backend;
                self.render_queued = false;
                self.zoom_render_due = None;
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                self.backend_generation = self.backend_generation.wrapping_add(1);
                self.dynlex_switch_retry = if backend == Backend::Dynlex {
                    self.document
                        .as_ref()
                        .map(|document| (self.backend_generation, document.model.view().ticket()))
                } else {
                    None
                };
                if let Some(document) = &mut self.document {
                    // The temporary empty bitmap lets Iced clamp the horizontal
                    // scroll offset. Restore the previous viewport only after
                    // the replacement bitmap has been laid out.
                    self.pending_zoom_offset = Some(self.page_scroll_offset);
                    document.page_cache.clear();
                    document.bitmap = None;
                    document.imported_annotations_dynlex = false;
                    #[cfg(feature = "dynlex-experimental")]
                    {
                        document.content_layer_in_flight = None;
                    }
                    if backend == Backend::Dynlex || previous_backend == Backend::Dynlex {
                        document.thumbnails.clear();
                        document.thumbnail_failed.clear();
                    }
                    document.thumbnail_in_flight = None;
                    return Task::batch([
                        self.render_current_page(),
                        self.render_nearby_thumbnail(),
                    ]);
                }
            }
            Message::PageScrolled(viewport) => {
                self.page_scroll_offset = viewport.absolute_offset();
            }
            Message::PanStarted(button, pointer) => {
                if self.document.is_some() && pointer.iter().all(|n| n.is_finite()) {
                    self.pan_drag = Some(PanDrag {
                        button,
                        start: pointer,
                        scroll_start: self.page_scroll_offset,
                    });
                }
            }
            Message::PanMoved(pointer) => {
                if let Some(drag) = self.pan_drag {
                    if pointer.iter().all(|n| n.is_finite()) {
                        let offset = dragged_scroll_offset(drag, pointer);
                        if offset != self.page_scroll_offset {
                            self.page_scroll_offset = offset;
                            return scroll_page_to(offset);
                        }
                    }
                }
            }
            Message::PanEnded => self.pan_drag = None,
            Message::ThumbnailsScrolled(viewport) => {
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                document.thumbnail_focus = thumbnail_page_at_scroll(
                    viewport.absolute_offset().y,
                    document.pdf.page_count(),
                );
                return self.render_nearby_thumbnail();
            }
            Message::PathChanged(value) => self.path_input = value,
            Message::SelectRibbon(tab) => self.active_ribbon = tab,
            Message::ToggleRibbonCollapsed => self.ribbon_collapsed = !self.ribbon_collapsed,
            Message::DragWindow => return window::oldest().and_then(window::drag),
            Message::ResizeWindow => {
                if !self.window_maximized {
                    return window::oldest()
                        .and_then(|id| window::drag_resize(id, window::Direction::SouthEast));
                }
            }
            Message::MinimizeWindow => {
                return window::oldest().and_then(|id| window::minimize(id, true));
            }
            Message::MaximizeWindow => {
                return window::oldest().and_then(|id| {
                    window::toggle_maximize::<Message>(id)
                        .chain(window::is_maximized(id).map(Message::WindowMaximized))
                });
            }
            Message::CloseWindow => {
                self.file_menu_open = false;
                if self.save_busy {
                    self.message = "Wait for Save As to finish".into();
                    return Task::none();
                }
                if self.has_unsaved_work() {
                    self.confirm_discard = Some(PendingAction::Exit);
                    return Task::none();
                }
                let _ = self.persist_reader_position();
                return window::oldest().and_then(window::close);
            }
            Message::SelectAnnotationTool(tool) => {
                self.pan_drag = None;
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                self.annotation_tool = tool;
                self.annotation_tool_epoch = self.annotation_tool_epoch.wrapping_add(1);
                self.annotation_tool_activated = true;
                if tool == AnnotationTool::Polyline {
                    self.message = "Polyline: click vertices; double-click, Enter or right-click to finish; Esc cancels".into();
                } else if tool == AnnotationTool::Freehand {
                    self.message = "Freehand: hold the mouse button and draw".into();
                } else if tool == AnnotationTool::Hand {
                    self.message =
                        "Hand: drag to pan; middle-button drag works with any tool".into();
                }
            }
            Message::TextEdited(action) => self.text_draft.perform(action),
            Message::TextPointer(cursor, focus) => {
                self.text_draft.move_to(cursor);
                if focus {
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        "inline-text-editor",
                    ));
                }
            }
            Message::SetFormatColor(color) => {
                if self.text_dialog {
                    self.text_color = color;
                } else if let Some(document) = &mut self.document {
                    if let Some(source) = document
                        .selected_annotation
                        .and_then(|id| document.model.get(id))
                        .filter(|item| item.page == document.displayed_page)
                    {
                        if let Some(item) = recolored_annotation(source, color) {
                            let is_text = matches!(item.kind, Kind::Text { .. });
                            match document.apply_edit(Edit::Replace(item)) {
                                Ok(()) => {
                                    if is_text {
                                        self.text_color = color;
                                    }
                                    self.message = "Annotation color updated".into();
                                    return self.render_content_layer_if_needed();
                                }
                                Err(error) => self.message = format!("Color failed: {error:?}"),
                            }
                        }
                    }
                }
            }
            Message::SetTextFontSize(size) => {
                if !TEXT_FONT_SIZES.contains(&size) {
                    return Task::none();
                }
                self.text_font_size = size;
                if !self.text_dialog {
                    if let Some(document) = &mut self.document {
                        if let Some(id) = document.selected_annotation {
                            if let Some(mut item) = document.model.get(id).cloned() {
                                if let Kind::Text { font_size, .. } = &mut item.kind {
                                    if *font_size != size {
                                        *font_size = size;
                                        if document.apply_edit(Edit::Replace(item)).is_ok() {
                                            self.message = "Text size updated".into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Message::SetTextAlign(value) => {
                self.text_align = value;
                if !self.text_dialog {
                    if let Some(document) = &mut self.document {
                        if let Some(id) = document.selected_annotation {
                            if let Some(mut item) = document.model.get(id).cloned() {
                                if let Kind::Text { align, .. } = &mut item.kind {
                                    if *align != value {
                                        *align = value;
                                        if document.apply_edit(Edit::Replace(item)).is_ok() {
                                            self.message = "Text alignment updated".into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Message::CancelText => {
                self.text_dialog = false;
                self.active_ribbon = if self.editing_text_id.is_some() {
                    RibbonTab::Format
                } else {
                    RibbonTab::Comment
                };
                self.pending_text_bounds = None;
                self.editing_text_id = None;
                self.text_dialog_version = None;
                self.text_draft = iced::widget::text_editor::Content::new();
                if let Some(offset) = self.text_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return Task::batch([
                        scroll_page_to(offset),
                        self.render_content_layer_if_needed(),
                    ]);
                }
                return self.render_content_layer_if_needed();
            }
            Message::EditSelectedText => {
                if let Some(document) = &self.document {
                    if let Some(id) = document.selected_annotation {
                        if let Some(Annotation {
                            kind:
                                Kind::Text {
                                    value,
                                    color_rgb,
                                    font_size,
                                    align,
                                },
                            ..
                        }) = document.model.get(id)
                        {
                            self.text_draft = iced::widget::text_editor::Content::with_text(value);
                            self.text_color = *color_rgb;
                            self.text_font_size = *font_size;
                            self.text_align = *align;
                            self.editing_text_id = Some(id);
                            self.pending_text_bounds = None;
                            self.text_dialog_version = Some(document.model.version());
                            self.text_dialog = true;
                            self.active_ribbon = RibbonTab::Format;
                            self.text_scroll_offset = Some(self.page_scroll_offset);
                        }
                    }
                }
            }
            Message::CommitText => {
                let value = self
                    .text_draft
                    .text()
                    .replace("\r\n", "\n")
                    .replace('\r', "\n");
                if value.trim().is_empty() {
                    self.message = "Enter annotation text".into();
                    return Task::none();
                }
                if let Some(document) = &mut self.document {
                    let expected = self.text_dialog_version;
                    if let (Some(expected), Some(id)) = (expected, self.editing_text_id) {
                        if let Some(mut item) = document.model.get(id).cloned() {
                            if matches!(item.kind, Kind::Text { .. }) {
                                item.kind = Kind::Text {
                                    value,
                                    color_rgb: self.text_color,
                                    font_size: self.text_font_size,
                                    align: self.text_align,
                                };
                                match document
                                    .model
                                    .execute(expected, DocumentCommand::Edit(Edit::Replace(item)))
                                {
                                    Ok(true) => self.message = "Text annotation updated".into(),
                                    Ok(false) => {}
                                    Err(error) => {
                                        self.message = format!("Text edit failed: {error:?}")
                                    }
                                }
                            }
                        }
                    } else if let (Some(expected), Some(bounds)) =
                        (expected, self.pending_text_bounds)
                    {
                        let item = Annotation {
                            id: self.next_annotation_id,
                            page: document.displayed_page,
                            bounds,
                            kind: Kind::Text {
                                value,
                                color_rgb: self.text_color,
                                font_size: self.text_font_size,
                                align: self.text_align,
                            },
                        };
                        match document
                            .model
                            .execute(expected, DocumentCommand::Edit(Edit::Add(item)))
                        {
                            Ok(true) => {
                                document.selected_annotation = Some(self.next_annotation_id);
                                self.next_annotation_id += 1;
                                self.message = "Text annotation added".into();
                            }
                            Ok(false) => {}
                            Err(error) => {
                                self.message = format!("Text annotation failed: {error:?}")
                            }
                        }
                    }
                }
                self.text_dialog = false;
                self.active_ribbon = RibbonTab::Format;
                self.pending_text_bounds = None;
                self.editing_text_id = None;
                self.text_dialog_version = None;
                self.text_draft = iced::widget::text_editor::Content::new();
                if let Some(offset) = self.text_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return Task::batch([
                        scroll_page_to(offset),
                        self.render_content_layer_if_needed(),
                    ]);
                }
                return self.render_content_layer_if_needed();
            }
            Message::SelectAnnotation(id) => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                if let Some(document) = &mut self.document {
                    document.selected_annotation = id.filter(|id| {
                        document
                            .model
                            .get(*id)
                            .is_some_and(|item| item.page == document.displayed_page)
                    });
                    self.annotation_gesture_active = document.selected_annotation.is_some();
                    if let Some(selected) = document
                        .selected_annotation
                        .and_then(|id| document.model.get(id))
                    {
                        if let Kind::Text {
                            color_rgb,
                            font_size,
                            align,
                            ..
                        } = &selected.kind
                        {
                            self.text_color = *color_rgb;
                            self.text_font_size = *font_size;
                            self.text_align = *align;
                            self.active_ribbon = RibbonTab::Format;
                        }
                    } else if self.active_ribbon == RibbonTab::Format && !self.text_dialog {
                        self.active_ribbon = RibbonTab::Comment;
                    }
                }
                return self.render_content_layer_if_needed();
            }
            Message::DuplicateSelectedAnnotation => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                if let Some(document) = &mut self.document {
                    let Some(selected) = document
                        .selected_annotation
                        .and_then(|id| document.model.get(id))
                        .filter(|item| item.page == document.displayed_page)
                    else {
                        return Task::none();
                    };
                    let Ok(page_size) = document.pdf.page_dimensions(document.displayed_page + 1)
                    else {
                        self.message = "Could not read the page size".into();
                        return Task::none();
                    };
                    let Some(next_id) = self.next_annotation_id.checked_add(1) else {
                        self.message = "Annotation ID limit reached".into();
                        return Task::none();
                    };
                    let scale = document.model.view().viewport().scale();
                    let Some(copy) =
                        duplicate_annotation(selected, self.next_annotation_id, scale, page_size)
                    else {
                        self.message = "No room to duplicate the annotation on this page".into();
                        return Task::none();
                    };
                    let expected = document.model.version();
                    match document
                        .model
                        .execute(expected, DocumentCommand::Edit(Edit::Add(copy)))
                    {
                        Ok(true) => {
                            document.selected_annotation = Some(self.next_annotation_id);
                            self.next_annotation_id = next_id;
                            self.message = "Annotation duplicated".into();
                        }
                        Ok(false) => {}
                        Err(error) => self.message = format!("Duplicate failed: {error:?}"),
                    }
                }
                return self.render_content_layer_if_needed();
            }
            Message::MirrorSelectedAnnotation { horizontal } => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                let Some(document) = self.document.as_mut() else {
                    return Task::none();
                };
                let Some(source) = document
                    .selected_annotation
                    .and_then(|id| document.model.get(id))
                    .filter(|item| item.page == document.displayed_page)
                else {
                    return Task::none();
                };
                let Ok(page_size) = document.pdf.page_dimensions(document.displayed_page + 1)
                else {
                    self.message = "Could not read the page size".into();
                    return Task::none();
                };
                let Some(mirrored) = mirror_annotation(source, horizontal, page_size) else {
                    return Task::none();
                };
                let expected = document.model.version();
                match document
                    .model
                    .execute(expected, DocumentCommand::Edit(Edit::Replace(mirrored)))
                {
                    Ok(true) => self.message = "Annotation mirrored".into(),
                    Ok(false) => {}
                    Err(error) => self.message = format!("Mirror failed: {error:?}"),
                }
                return self.render_content_layer_if_needed();
            }
            Message::RotateSelectedAnnotation => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                let Some(document) = self.document.as_mut() else {
                    return Task::none();
                };
                let Some(source) = document
                    .selected_annotation
                    .and_then(|id| document.model.get(id))
                    .filter(|item| item.page == document.displayed_page)
                else {
                    return Task::none();
                };
                let Ok(page_size) = document.pdf.page_dimensions(document.displayed_page + 1)
                else {
                    self.message = "Could not read the page size".into();
                    return Task::none();
                };
                let Some(rotated) = rotate_annotation_clockwise_90(source, page_size) else {
                    self.message = "No room to rotate the annotation on this page".into();
                    return Task::none();
                };
                let expected = document.model.version();
                match document
                    .model
                    .execute(expected, DocumentCommand::Edit(Edit::Replace(rotated)))
                {
                    Ok(true) => self.message = "Annotation rotated 90°".into(),
                    Ok(false) => {}
                    Err(error) => self.message = format!("Rotate failed: {error:?}"),
                }
                return self.render_content_layer_if_needed();
            }
            Message::CopySelectedAnnotation => {
                let selected = self.document.as_ref().and_then(|document| {
                    document
                        .selected_annotation
                        .and_then(|id| document.model.get(id))
                        .filter(|item| item.page == document.displayed_page)
                });
                if let Some(annotation) = selected {
                    self.clipboard_annotation = Some(annotation.clone());
                    self.clipboard_paste_count = 0;
                    self.message = "Annotation copied".into();
                }
            }
            Message::CutSelectedAnnotation => {
                if self.text_dialog
                    || self.save_busy
                    || self.pending_delete_annotation.is_some()
                    || self.pending_clear_annotations.is_some()
                {
                    return Task::none();
                }
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                let Some(document) = self.document.as_mut() else {
                    return Task::none();
                };
                let Some(source) = document
                    .selected_annotation
                    .and_then(|id| document.model.get(id))
                    .filter(|item| item.page == document.displayed_page)
                    .cloned()
                else {
                    return Task::none();
                };
                let expected = document.model.version();
                match document
                    .model
                    .execute(expected, DocumentCommand::Edit(Edit::Remove(source.id)))
                {
                    Ok(true) => {
                        document.selected_annotation = None;
                        self.clipboard_annotation = Some(source);
                        self.clipboard_paste_count = 0;
                        if self.active_ribbon == RibbonTab::Format {
                            self.active_ribbon = RibbonTab::Comment;
                        }
                        self.message = "Annotation cut".into();
                    }
                    Ok(false) => {}
                    Err(error) => self.message = format!("Cut failed: {error:?}"),
                }
                return self.render_content_layer_if_needed();
            }
            Message::PasteAnnotation { in_place } => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                let Some(source) = self.clipboard_annotation.as_ref() else {
                    return Task::none();
                };
                let Some(document) = self.document.as_mut() else {
                    return Task::none();
                };
                let Ok(page_size) = document.pdf.page_dimensions(document.displayed_page + 1)
                else {
                    self.message = "Could not read the page size".into();
                    return Task::none();
                };
                let Some(next_id) = self.next_annotation_id.checked_add(1) else {
                    self.message = "Annotation ID limit reached".into();
                    return Task::none();
                };
                let sequence = if in_place {
                    self.clipboard_paste_count
                } else {
                    let Some(sequence) = self.clipboard_paste_count.checked_add(1) else {
                        self.message = "Paste limit reached".into();
                        return Task::none();
                    };
                    sequence
                };
                let offset_px = if in_place {
                    0.0
                } else {
                    20.0 * f64::from(sequence)
                };
                let scale = document.model.view().viewport().scale();
                let Some(mut copy) = copy_annotation_with_offset(
                    source,
                    self.next_annotation_id,
                    scale,
                    page_size,
                    offset_px,
                ) else {
                    self.message = "No room to paste the annotation on this page".into();
                    return Task::none();
                };
                copy.page = document.displayed_page;
                let expected = document.model.version();
                match document
                    .model
                    .execute(expected, DocumentCommand::Edit(Edit::Add(copy)))
                {
                    Ok(true) => {
                        document.selected_annotation = Some(self.next_annotation_id);
                        self.next_annotation_id = next_id;
                        self.clipboard_paste_count = sequence;
                        self.message = "Annotation pasted".into();
                    }
                    Ok(false) => {}
                    Err(error) => self.message = format!("Paste failed: {error:?}"),
                }
                return self.render_content_layer_if_needed();
            }
            Message::AnnotationGestureStarted => {
                self.annotation_gesture_active = true;
                self.annotation_gesture_preview = None;
            }
            Message::AnnotationGesturePreview(expected, id, bounds) => {
                self.annotation_gesture_preview = None;
                if self.annotation_gesture_active && self.backend == Backend::Dynlex {
                    if let Some(document) = self.document.as_ref() {
                        if document.model.version() == expected
                            && document.selected_annotation == Some(id)
                        {
                            let annotations: Vec<_> = document
                                .model
                                .items()
                                .filter(|item| item.page == document.displayed_page)
                                .cloned()
                                .collect();
                            if let (Some(base), Some(bitmap)) = (
                                document.bitmap.clone(),
                                document.dynlex_selected_annotation_bitmap_at_bounds(id, bounds),
                            ) {
                                self.annotation_gesture_preview = Some(AnnotationGesturePreview {
                                    selected: id,
                                    page: document.displayed_page,
                                    base,
                                    annotations,
                                    bitmap,
                                });
                            }
                        }
                    }
                }
            }
            Message::AnnotationGestureEnded => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
            }
            Message::MoveAnnotation(expected, id, delta) => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                if let Some(document) = &mut self.document {
                    if document.model.version() == expected
                        && document.selected_annotation == Some(id)
                        && delta.iter().all(|value| value.is_finite())
                    {
                        if let Some(item) = document.model.get(id).and_then(|item| {
                            translated_annotation(item, delta, document.bitmap_page_points)
                        }) {
                            match document
                                .model
                                .execute(expected, DocumentCommand::Edit(Edit::Replace(item)))
                            {
                                Ok(true) => self.message = "Annotation moved".into(),
                                Ok(false) => {}
                                Err(error) => self.message = format!("Move failed: {error:?}"),
                            }
                        }
                    }
                }
            }
            Message::MoveLineEndpoint(expected, id, move_start, target) => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                if let Some(document) = &mut self.document {
                    if document.model.version() == expected
                        && document.selected_annotation == Some(id)
                    {
                        if let Some(mut item) = document.model.get(id).cloned() {
                            if item.page == document.displayed_page {
                                let is_arrow = matches!(item.kind, Kind::Arrow { .. });
                                if let Kind::Line { start, end, .. }
                                | Kind::Arrow { start, end, .. } = &mut item.kind
                                {
                                    let original = if move_start { *start } else { *end };
                                    if move_start {
                                        *start = target;
                                    } else {
                                        *end = target;
                                    }
                                    if target != original {
                                        if let Some(bounds) = if is_arrow {
                                            arrow_bounds(*start, *end, document.bitmap_page_points)
                                        } else {
                                            line_bounds(*start, *end, document.bitmap_page_points)
                                        } {
                                            item.bounds = bounds;
                                            match document.model.execute(
                                                expected,
                                                DocumentCommand::Edit(Edit::Replace(item)),
                                            ) {
                                                Ok(true) => {
                                                    self.message = "Line endpoint moved".into()
                                                }
                                                Ok(false) => {}
                                                Err(error) => {
                                                    self.message =
                                                        format!("Endpoint failed: {error:?}")
                                                }
                                            }
                                        } else {
                                            self.message =
                                                "Line endpoint outside page or too close".into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Message::ResizeAnnotation(expected, id, bounds) => {
                self.annotation_gesture_active = false;
                self.annotation_gesture_preview = None;
                if let Some(document) = &mut self.document {
                    if document.model.version() == expected
                        && document.selected_annotation == Some(id)
                    {
                        if let Some(item) = document.model.get(id).and_then(|item| {
                            (item.page == document.displayed_page)
                                .then(|| {
                                    resized_annotation(item, bounds, document.bitmap_page_points)
                                })
                                .flatten()
                        }) {
                            match document
                                .model
                                .execute(expected, DocumentCommand::Edit(Edit::Replace(item)))
                            {
                                Ok(true) => self.message = "Annotation resized".into(),
                                Ok(false) => {}
                                Err(error) => self.message = format!("Resize failed: {error:?}"),
                            }
                        } else if bounds.x < 0.0
                            || bounds.y < 0.0
                            || bounds.x + bounds.width
                                > f64::from(document.bitmap_page_points.0) + 1e-6
                            || bounds.y + bounds.height
                                > f64::from(document.bitmap_page_points.1) + 1e-6
                        {
                            self.message = "Resize outside page".into();
                        }
                    }
                }
            }
            Message::RemoveSelectedAnnotation => {
                self.annotation_gesture_active = false;
                if let Some(document) = &mut self.document {
                    if let Some(id) = document.selected_annotation {
                        let expected = document.model.version();
                        match document
                            .model
                            .execute(expected, DocumentCommand::Edit(Edit::Remove(id)))
                        {
                            Ok(true) => {
                                document.selected_annotation = None;
                                if self.active_ribbon == RibbonTab::Format {
                                    self.active_ribbon = RibbonTab::Comment;
                                }
                                self.message = "Annotation removed".into();
                            }
                            Ok(false) => {}
                            Err(error) => self.message = format!("Delete failed: {error:?}"),
                        }
                    }
                }
            }
            Message::RequestDeleteAnnotation => {
                if self.text_dialog
                    || self.save_busy
                    || self.pending_delete_annotation.is_some()
                    || self.pending_clear_annotations.is_some()
                {
                    return Task::none();
                }
                if let Some(document) = &self.document {
                    if let Some(id) = document.selected_annotation {
                        self.pending_delete_annotation = Some((document.model.version(), id));
                    }
                }
            }
            Message::CancelDeleteAnnotation => self.pending_delete_annotation = None,
            Message::ConfirmDeleteAnnotation => {
                let Some((expected, id)) = self.pending_delete_annotation.take() else {
                    return Task::none();
                };
                let Some(document) = &self.document else {
                    return Task::none();
                };
                if document.model.version() != expected || document.selected_annotation != Some(id)
                {
                    self.message = "Delete cancelled: document or selection changed".into();
                    return Task::none();
                }
                return self.update(Message::RemoveSelectedAnnotation);
            }
            Message::RequestClearAnnotations(page) => {
                if self.text_dialog || self.save_busy || self.pending_clear_annotations.is_some() {
                    return Task::none();
                }
                let Some(document) = &self.document else {
                    return Task::none();
                };
                if page.is_some_and(|page| page != document.model.view().page()) {
                    return Task::none();
                }
                if !document
                    .model
                    .items()
                    .any(|item| page.is_none_or(|page| item.page == page))
                {
                    return Task::none();
                }
                self.pending_clear_annotations = Some((document.model.version(), page));
            }
            Message::CancelClearAnnotations => self.pending_clear_annotations = None,
            Message::ConfirmClearAnnotations => {
                let Some((expected, page)) = self.pending_clear_annotations.take() else {
                    return Task::none();
                };
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                match document
                    .model
                    .execute(expected, DocumentCommand::Edit(Edit::Clear { page }))
                {
                    Ok(true) => {
                        document.selected_annotation = None;
                        self.annotation_gesture_active = false;
                        self.annotation_gesture_preview = None;
                        self.message = if page.is_some() {
                            "Page annotations cleared".into()
                        } else {
                            "All annotations cleared".into()
                        };
                    }
                    Ok(false) => {}
                    Err(error) => self.message = format!("Clear failed: {error:?}"),
                }
            }
            Message::AddAnnotation(expected, bounds) => {
                if self
                    .document
                    .as_ref()
                    .map(|document| document.model.version())
                    != Some(expected)
                {
                    return Task::none();
                }
                if self.annotation_tool == AnnotationTool::Text {
                    self.pending_text_bounds = Some(bounds);
                    self.editing_text_id = None;
                    self.text_dialog_version = Some(expected);
                    self.text_draft = iced::widget::text_editor::Content::new();
                    self.text_color = [0, 0, 0];
                    self.text_font_size = DEFAULT_TEXT_FONT_SIZE;
                    self.text_align = TextAlign::Left;
                    self.text_dialog = true;
                    self.active_ribbon = RibbonTab::Format;
                    self.text_scroll_offset = Some(self.page_scroll_offset);
                    return Task::none();
                }
                if let Some(document) = &mut self.document {
                    let color_rgb = match self.annotation_tool {
                        AnnotationTool::Rectangle
                        | AnnotationTool::Circle
                        | AnnotationTool::Ellipse
                        | AnnotationTool::Polygon => [231, 76, 60],
                        AnnotationTool::Line | AnnotationTool::Arrow => return Task::none(),
                        AnnotationTool::Polyline | AnnotationTool::Freehand => return Task::none(),
                        AnnotationTool::Highlight => [255, 226, 67],
                        AnnotationTool::Select => return Task::none(),
                        AnnotationTool::Hand => return Task::none(),
                        AnnotationTool::Text => unreachable!("handled above"),
                    };
                    let item = Annotation {
                        id: self.next_annotation_id,
                        page: document.displayed_page,
                        bounds,
                        kind: if self.annotation_tool == AnnotationTool::Polygon {
                            Kind::Polygon { color_rgb }
                        } else if matches!(
                            self.annotation_tool,
                            AnnotationTool::Circle | AnnotationTool::Ellipse
                        ) {
                            Kind::Ellipse { color_rgb }
                        } else {
                            Kind::Rectangle { color_rgb }
                        },
                    };
                    match document
                        .model
                        .execute(expected, DocumentCommand::Edit(Edit::Add(item)))
                    {
                        Ok(true) => {
                            document.selected_annotation = Some(self.next_annotation_id);
                            self.next_annotation_id += 1;
                            self.message = "Annotation added".into();
                        }
                        Ok(false) => {}
                        Err(error) => self.message = format!("Annotation failed: {error:?}"),
                    }
                }
            }
            Message::AddLine(expected, start, end) => {
                if !matches!(
                    self.annotation_tool,
                    AnnotationTool::Line | AnnotationTool::Arrow
                ) {
                    return Task::none();
                }
                if let Some(document) = &mut self.document {
                    if document.model.version() != expected {
                        return Task::none();
                    }
                    let Some(bounds) = (if self.annotation_tool == AnnotationTool::Arrow {
                        arrow_bounds(start, end, document.bitmap_page_points)
                    } else {
                        line_bounds(start, end, document.bitmap_page_points)
                    }) else {
                        return Task::none();
                    };
                    let item = Annotation {
                        id: self.next_annotation_id,
                        page: document.displayed_page,
                        bounds,
                        kind: if self.annotation_tool == AnnotationTool::Arrow {
                            Kind::Arrow {
                                start,
                                end,
                                color_rgb: [231, 76, 60],
                            }
                        } else {
                            Kind::Line {
                                start,
                                end,
                                color_rgb: [231, 76, 60],
                            }
                        },
                    };
                    match document
                        .model
                        .execute(expected, DocumentCommand::Edit(Edit::Add(item)))
                    {
                        Ok(true) => {
                            document.selected_annotation = Some(self.next_annotation_id);
                            self.next_annotation_id += 1;
                            self.message = "Line added".into();
                        }
                        Ok(false) => {}
                        Err(error) => self.message = format!("Line failed: {error:?}"),
                    }
                }
            }
            Message::AddPolyline(expected, points) => {
                if !matches!(
                    self.annotation_tool,
                    AnnotationTool::Polyline | AnnotationTool::Freehand
                ) {
                    return Task::none();
                }
                if let Some(document) = &mut self.document {
                    if document.model.version() != expected {
                        return Task::none();
                    }
                    let Some(bounds) = polyline_bounds(&points, document.bitmap_page_points) else {
                        self.message = "Invalid polyline geometry".into();
                        return Task::none();
                    };
                    let item = Annotation {
                        id: self.next_annotation_id,
                        page: document.displayed_page,
                        bounds,
                        kind: Kind::Polyline {
                            points,
                            color_rgb: [231, 76, 60],
                        },
                    };
                    match document
                        .model
                        .execute(expected, DocumentCommand::Edit(Edit::Add(item)))
                    {
                        Ok(true) => {
                            document.selected_annotation = Some(self.next_annotation_id);
                            self.next_annotation_id += 1;
                            self.message = if self.annotation_tool == AnnotationTool::Freehand {
                                "Freehand stroke added".into()
                            } else {
                                "Polyline added".into()
                            };
                        }
                        Ok(false) => {}
                        Err(error) => self.message = format!("Polyline failed: {error:?}"),
                    }
                }
            }
            Message::Undo => {
                self.annotation_gesture_active = false;
                if let Some(document) = &mut self.document {
                    let expected = document.model.version();
                    if let Err(error) = document.model.execute(expected, DocumentCommand::Undo) {
                        self.message = format!("Undo failed: {error:?}");
                    }
                    if document
                        .selected_annotation
                        .is_some_and(|id| document.model.get(id).is_none())
                    {
                        document.selected_annotation = None;
                        if self.active_ribbon == RibbonTab::Format {
                            self.active_ribbon = RibbonTab::Comment;
                        }
                    }
                }
                return self.render_content_layer_if_needed();
            }
            Message::Redo => {
                self.annotation_gesture_active = false;
                if let Some(document) = &mut self.document {
                    let expected = document.model.version();
                    if let Err(error) = document.model.execute(expected, DocumentCommand::Redo) {
                        self.message = format!("Redo failed: {error:?}");
                    }
                }
                return self.render_content_layer_if_needed();
            }
            Message::ShowNew => {
                if self.save_busy || self.opening_document {
                    return Task::none();
                }
                self.file_menu_open = false;
                self.new_dialog = true;
                self.new_paper = blank_pdf::Paper::A4;
                self.new_landscape = false;
                self.new_pages_input = "1".into();
                self.new_custom_width_mm = "210".into();
                self.new_custom_height_mm = "297".into();
                self.new_error.clear();
            }
            Message::CancelNew => self.new_dialog = false,
            Message::NewPaperSelected(paper) => {
                self.new_paper = paper;
                self.new_error.clear();
            }
            Message::NewLandscape(landscape) => self.new_landscape = landscape,
            Message::NewPagesChanged(value) => self.new_pages_input = value,
            Message::NewCustomWidthChanged(value) => self.new_custom_width_mm = value,
            Message::NewCustomHeightChanged(value) => self.new_custom_height_mm = value,
            Message::NewBlank => {
                if self.save_busy {
                    self.message = "Wait for Save As to finish".into();
                    return Task::none();
                }
                let page_count = match self.new_pages_input.trim().parse::<u16>() {
                    Ok(count) if (1..=999).contains(&count) => count,
                    _ => {
                        self.new_error = "Page count must be 1–999".into();
                        return Task::none();
                    }
                };
                let custom_mm = if self.new_paper == blank_pdf::Paper::Custom {
                    let width = self.new_custom_width_mm.trim().parse::<f64>();
                    let height = self.new_custom_height_mm.trim().parse::<f64>();
                    match (width, height) {
                        (Ok(width), Ok(height)) => (width, height),
                        _ => {
                            self.new_error = "Enter numeric custom dimensions in mm".into();
                            return Task::none();
                        }
                    }
                } else {
                    (0.0, 0.0)
                };
                let size =
                    match blank_pdf::dimensions(self.new_paper, self.new_landscape, custom_mm) {
                        Ok(size) => size,
                        Err(error) => {
                            self.new_error = error;
                            return Task::none();
                        }
                    };
                self.new_dialog = false;
                if !self.allow_discard_once && self.has_unsaved_work() {
                    self.confirm_discard = Some(PendingAction::New);
                    return Task::none();
                }
                self.allow_discard_once = false;
                self.file_menu_open = false;
                self.next_load += 1;
                let token = self.next_load;
                let backend = self.backend;
                let window_size = self.window_size;
                let pages_visible = !self.hide_pages;
                let properties_visible = !self.hide_properties;
                let symbols_visible = !self.hide_symbol_palette;
                let ribbon_collapsed = self.ribbon_collapsed;
                self.busy = true;
                self.opening_document = true;
                self.render_queued = false;
                self.zoom_render_due = None;
                self.message = "Creating blank PDF…".into();
                return Task::perform(
                    async move {
                        let bytes = Arc::new(blank_pdf::create(size.0, size.1, page_count)?);
                        let pdf = Arc::new(
                            ParsedPdf::open(Arc::clone(&bytes))
                                .map_err(|error| error.to_string())?,
                        );
                        let size = pdf.page_dimensions(1).map_err(|error| error.to_string())?;
                        let scale = fit_scale(
                            FitMode::Page,
                            size,
                            window_size,
                            pages_visible,
                            properties_visible,
                            symbols_visible,
                            ribbon_collapsed,
                        )
                        .unwrap_or(1.0) as f32;
                        let dynlex_document = (backend == Backend::Dynlex).then(|| {
                            DynlexOpen(Arc::new(opds_dynlex_backend::DynlexDocument::new(
                                Arc::clone(&bytes),
                            )))
                        });
                        let (scale, outcome) = if let Some(document) = &dynlex_document {
                            render_initial_dynlex_page(&pdf, &document.0, 1, scale)
                        } else {
                            render_initial_selected_page(&pdf, &bytes, 1, scale, backend)
                        };
                        Ok((
                            "Untitled.pdf".to_owned(),
                            bytes,
                            pdf,
                            dynlex_document,
                            1,
                            scale,
                            None,
                            outcome,
                            true,
                        ))
                    },
                    move |result| Message::Loaded(token, result),
                );
            }
            Message::Open => {
                if self.save_busy {
                    self.message = "Wait for Save As to finish".into();
                    return Task::none();
                }
                let path = self.path_input.trim().to_owned();
                if path.is_empty() {
                    self.message = "Enter a PDF path".into();
                    return Task::none();
                }
                if !self.allow_discard_once && self.has_unsaved_work() {
                    self.confirm_discard = Some(PendingAction::Open);
                    return Task::none();
                }
                self.allow_discard_once = false;
                self.open_dialog = false;
                self.next_load += 1;
                let token = self.next_load;
                let backend = self.backend;
                let window_size = self.window_size;
                let pages_visible = !self.hide_pages;
                let properties_visible = !self.hide_properties;
                let symbol_palette_visible = !self.hide_symbol_palette;
                let ribbon_collapsed = self.ribbon_collapsed;
                self.busy = true;
                self.opening_document = true;
                self.render_queued = false;
                self.zoom_render_due = None;
                self.message = "Opening PDF…".into();
                return Task::perform(
                    async move {
                        let bytes =
                            Arc::new(std::fs::read(&path).map_err(|error| error.to_string())?);
                        let pdf = Arc::new(
                            ParsedPdf::open(Arc::clone(&bytes))
                                .map_err(|error| error.to_string())?,
                        );
                        let saved = reader_mode::read(Path::new(&path));
                        let initial_page = saved
                            .map(|position| position.page.min(pdf.page_count()))
                            .unwrap_or(1);
                        let page_size = pdf
                            .page_dimensions(initial_page)
                            .map_err(|error| error.to_string())?;
                        // The original viewer opens a page fitted to the available
                        // canvas. Use the same dock-aware fit calculation as the
                        // visible Fit Page button, rather than a fixed 550 px
                        // target that leaves small PDFs tiny on a large screen.
                        let fit = fit_scale(
                            FitMode::Page,
                            page_size,
                            window_size,
                            pages_visible,
                            properties_visible,
                            symbol_palette_visible,
                            ribbon_collapsed,
                        )
                        .unwrap_or_else(|| (550.0 / page_size.0).clamp(0.1, 1.0) as f64)
                            as f32;
                        let dynlex_document = (backend == Backend::Dynlex).then(|| {
                            DynlexOpen(Arc::new(opds_dynlex_backend::DynlexDocument::new(
                                Arc::clone(&bytes),
                            )))
                        });
                        let (initial_scale, outcome) = if let Some(document) = &dynlex_document {
                            render_initial_dynlex_page(&pdf, &document.0, initial_page, fit)
                        } else {
                            let scale = saved.map_or(fit, |position| position.scale as f32);
                            render_initial_selected_page(&pdf, &bytes, initial_page, scale, backend)
                        };
                        Ok((
                            path,
                            bytes,
                            pdf,
                            dynlex_document,
                            initial_page,
                            initial_scale,
                            saved,
                            outcome,
                            false,
                        ))
                    },
                    move |result| Message::Loaded(token, result),
                );
            }
            Message::Loaded(token, result) => {
                if token != self.next_load {
                    return Task::none();
                }
                self.annotation_gesture_active = false;
                self.busy = false;
                self.opening_document = false;
                match result {
                    Ok((
                        path,
                        source_bytes,
                        pdf,
                        dynlex_document,
                        initial_page,
                        initial_scale,
                        saved,
                        outcome,
                        is_new,
                    )) => {
                        let _ = self.persist_reader_position();
                        let restored_scroll_height = outcome
                            .as_ref()
                            .map_or(48.0, |outcome| outcome.raster.height as f32 + 48.0);
                        let Ok(mut model) = DocumentModel::new(DocumentId(token), pdf.page_count())
                        else {
                            return Task::none();
                        };
                        if model.zoom_at(f64::from(initial_scale), [0.0, 0.0]).is_err() {
                            return Task::none();
                        }
                        if model.go_to_page(initial_page - 1).is_err() {
                            return Task::none();
                        }
                        if !is_new {
                            self.record_recent_file(&path);
                        }
                        self.message = match &outcome {
                            Ok(outcome) => format!(
                                "{} pages • {}×{} px",
                                pdf.page_count(),
                                outcome.raster.width,
                                outcome.raster.height
                            ),
                            Err(error) => error.clone(),
                        };
                        let mut document = OpenDocument {
                            path,
                            is_new,
                            source_bytes,
                            dynlex_document: dynlex_document.map(|document| document.0),
                            pdf,
                            model,
                            last_saved_annotations: Vec::new(),
                            bitmap: None,
                            annotation_overlay_cache: RefCell::new(None),
                            #[cfg(feature = "dynlex-experimental")]
                            content_layer_cache: RefCell::new(None),
                            #[cfg(feature = "dynlex-experimental")]
                            content_layer_in_flight: None,
                            bitmap_size: (0, 0),
                            bitmap_page_points: (0.0, 0.0),
                            bitmap_scale: initial_scale,
                            bitmap_backend: self.backend,
                            imported_annotations_dynlex: false,
                            displayed_page: initial_page - 1,
                            selected_annotation: None,
                            page_cache: VecDeque::new(),
                            thumbnails: VecDeque::new(),
                            thumbnail_in_flight: None,
                            thumbnail_failed: HashSet::new(),
                            thumbnail_focus: initial_page - 1,
                        };
                        if let Ok(outcome) = outcome {
                            document.cache_bitmap(
                                initial_page - 1,
                                initial_scale,
                                outcome,
                                #[cfg(feature = "dynlex-experimental")]
                                None,
                            );
                        }
                        self.reader_mode_active = saved.is_some();
                        self.page_scroll_offset = if saved.is_some() || document.bitmap.is_none() {
                            scrollable::AbsoluteOffset {
                                x: HORIZONTAL_PAN_MARGIN,
                                y: VERTICAL_PAN_MARGIN
                                    + saved.map_or(0.0, |position| {
                                        position.restored_scroll_top(restored_scroll_height)
                                    }),
                            }
                        } else {
                            centered_page_offset(
                                self.window_size,
                                document.bitmap_size,
                                !self.hide_pages,
                                !self.hide_properties,
                                !self.hide_symbol_palette,
                                self.ribbon_collapsed,
                            )
                        };
                        self.saved_target = None;
                        self.document = Some(document);
                        self.pending_zoom_offset = None;
                        return Task::batch([
                            scroll_page_to(self.page_scroll_offset),
                            scroll_thumbnail_to(initial_page - 1),
                            self.render_nearby_thumbnail(),
                        ]);
                    }
                    Err(error) => self.message = format!("PDF open/render failed: {error}"),
                }
            }
            Message::Zoom(scale) => {
                self.annotation_gesture_active = false;
                self.pending_zoom_offset = None;
                if let Some(document) = &mut self.document {
                    if document.model.zoom_at(scale, [0.0, 0.0]).is_err() {
                        return Task::none();
                    }
                } else {
                    return Task::none();
                }
                return self.schedule_zoom_render();
            }
            Message::FitView(mode) => {
                let Some(document) = &self.document else {
                    return Task::none();
                };
                let page = document.model.view().page() + 1;
                let Ok(size) = document.pdf.page_dimensions(page) else {
                    return Task::none();
                };
                let Some(scale) = fit_scale(
                    mode,
                    size,
                    self.window_size,
                    !self.hide_pages,
                    !self.hide_properties,
                    !self.hide_symbol_palette,
                    self.ribbon_collapsed,
                ) else {
                    return Task::none();
                };
                return self.update(Message::Zoom(scale));
            }
            Message::WindowResized(size) => {
                self.window_size = size;
                return window::oldest()
                    .and_then(|id| window::is_maximized(id).map(Message::WindowMaximized));
            }
            Message::WindowMaximized(maximized) => self.window_maximized = maximized,
            Message::KeyboardModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers;
            }
            Message::ThumbnailRendered(token, generation, page, result) => {
                if token != self.next_load || generation != self.backend_generation {
                    return Task::none();
                }
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                if document.thumbnail_in_flight != Some(page) {
                    return Task::none();
                }
                document.thumbnail_in_flight = None;
                match result {
                    Ok(raster) => {
                        document.thumbnails.retain(|(cached, _)| *cached != page);
                        document.thumbnails.push_back((
                            page,
                            image::Handle::from_rgba(raster.width, raster.height, raster.rgba),
                        ));
                        while document.thumbnails.len() > MAX_THUMBNAILS {
                            let window = thumbnail_window(
                                document.thumbnail_focus,
                                document.pdf.page_count(),
                            );
                            let evict = document
                                .thumbnails
                                .iter()
                                .position(|(page, _)| !window.contains(page))
                                .unwrap_or(0);
                            document.thumbnails.remove(evict);
                        }
                    }
                    Err(_) => {
                        document.thumbnail_failed.insert(page);
                    }
                }
                return self.render_nearby_thumbnail();
            }
            Message::WheelZoom { amount, anchor } => {
                if !amount.is_finite() || !anchor.iter().all(|n| n.is_finite()) {
                    return Task::none();
                }
                if let Some(document) = &mut self.document {
                    let scale = document.model.view().viewport().scale();
                    let next = (scale * 1.15_f64.powf(f64::from(amount))).clamp(0.1, 8.0);
                    if (next - scale).abs() < f64::EPSILON {
                        return Task::none();
                    }
                    if document
                        .model
                        .zoom_at(next, [f64::from(anchor[0]), f64::from(anchor[1])])
                        .is_err()
                    {
                        return Task::none();
                    }
                    if document.bitmap.is_none() {
                        self.pending_zoom_offset = Some(scrollable::AbsoluteOffset {
                            x: HORIZONTAL_PAN_MARGIN,
                            y: VERTICAL_PAN_MARGIN,
                        });
                    } else {
                        let anchor = [
                            self.page_scroll_offset.x + anchor[0]
                                - HORIZONTAL_PAN_MARGIN
                                - PAGE_PADDING,
                            self.page_scroll_offset.y + anchor[1]
                                - VERTICAL_PAN_MARGIN
                                - PAGE_PADDING,
                        ];
                        let old_size = zoom_preview_size(
                            document.bitmap_page_points,
                            document.bitmap_size,
                            scale,
                        );
                        let new_size = zoom_preview_size(
                            document.bitmap_page_points,
                            document.bitmap_size,
                            next,
                        );
                        self.pending_zoom_offset = Some(zoom_scroll_offset(
                            self.page_scroll_offset,
                            anchor,
                            old_size,
                            new_size,
                        ));
                    }
                } else {
                    return Task::none();
                }
                let render = self.schedule_zoom_render();
                if let Some(offset) = self.pending_zoom_offset {
                    self.page_scroll_offset = offset;
                }
                return Task::batch([
                    render,
                    self.pending_zoom_offset
                        .take()
                        .map_or_else(Task::none, scroll_page_to),
                ]);
            }
            Message::ZoomRenderDue => {
                self.zoom_timer_active = false;
                let Some(due) = self.zoom_render_due else {
                    return Task::none();
                };
                if let Some(remaining) = due.checked_duration_since(Instant::now()) {
                    self.zoom_timer_active = true;
                    return zoom_timeout(remaining);
                }
                self.zoom_render_due = None;
                if self.document.is_none() {
                    return Task::none();
                }
                if self.busy {
                    self.render_queued = true;
                    return Task::none();
                }
                return self.render_current_page();
            }
            Message::GoToPage(page) => {
                self.annotation_gesture_active = false;
                self.pending_zoom_offset = None;
                self.zoom_render_due = None;
                if let Some(document) = &mut self.document {
                    if document.model.go_to_page(page).is_err() {
                        return Task::none();
                    }
                    document.selected_annotation = None;
                    document.thumbnail_focus = page;
                    if self.active_ribbon == RibbonTab::Format {
                        self.active_ribbon = RibbonTab::Comment;
                    }
                }
                return Task::batch([
                    self.render_current_page(),
                    self.render_nearby_thumbnail(),
                    scroll_thumbnail_to(page),
                ]);
            }
            #[cfg(all(test, feature = "dynlex-experimental"))]
            Message::ContentLayerRendered(token, generation, ticket, base, result) => {
                if token != self.next_load || generation != self.backend_generation {
                    return Task::none();
                }
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                if document.content_layer_in_flight != Some(ticket) {
                    return Task::none();
                }
                document.content_layer_in_flight = None;
                if !document.model.view().accepts(ticket) || document.bitmap.as_ref() != Some(&base)
                {
                    return Task::none();
                }
                if let Ok(layer) = result {
                    if (layer.width, layer.height) == document.bitmap_size {
                        *document.content_layer_cache.borrow_mut() = Some(ContentLayerCache {
                            base,
                            page: document.displayed_page,
                            rgba: Arc::new(layer.rgba),
                        });
                        if let Some(cached) = document.page_cache.iter_mut().find(|cached| {
                            cached.page == document.displayed_page
                                && cached.scale_bits == document.bitmap_scale.to_bits()
                        }) {
                            let layer = Arc::clone(
                                &document.content_layer_cache.borrow().as_ref().unwrap().rgba,
                            );
                            cached.bytes += layer.len();
                            cached.content_layer = Some(layer);
                        }
                        while document
                            .page_cache
                            .iter()
                            .map(|item| item.bytes)
                            .sum::<usize>()
                            > MAX_PAGE_CACHE_BYTES
                        {
                            document.page_cache.pop_front();
                        }
                        document.annotation_overlay_cache.borrow_mut().take();
                    }
                }
                return Task::none();
            }
            Message::Rendered(generation, ticket, result) => {
                if generation != self.backend_generation {
                    return Task::none();
                }
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                if !document.model.view().accepts(ticket) {
                    if self.dynlex_switch_retry == Some((generation, ticket)) {
                        self.dynlex_switch_retry = None;
                    }
                    self.busy = false;
                    if self.render_queued {
                        self.render_queued = false;
                        return self.render_current_page();
                    }
                    return Task::none();
                }
                self.busy = false;
                self.render_queued = false;
                let retry_at_100 = self.dynlex_switch_retry == Some((generation, ticket));
                if retry_at_100 {
                    self.dynlex_switch_retry = None;
                }
                match result {
                    Ok((outcome, _content_layer)) => {
                        let raster = &outcome.raster;
                        let (width, height) = (raster.width, raster.height);
                        let view = document.model.view();
                        let page = view.page();
                        let scale = view.viewport().scale() as f32;
                        // An empty error workspace has no meaningful pixel
                        // anchor. Restore the page at its top-left after recovery.
                        if document.bitmap.is_none() {
                            self.pending_zoom_offset = Some(scrollable::AbsoluteOffset {
                                x: HORIZONTAL_PAN_MARGIN,
                                y: VERTICAL_PAN_MARGIN,
                            });
                        }
                        document.cache_bitmap(
                            page,
                            scale,
                            outcome,
                            #[cfg(feature = "dynlex-experimental")]
                            _content_layer,
                        );
                        self.message = format!("{}×{} px", width, height);
                        if let Some(offset) = self.pending_zoom_offset.take() {
                            self.page_scroll_offset = offset;
                            return Task::batch([
                                scroll_page_to(offset),
                                self.render_content_layer_if_needed(),
                                self.render_nearby_thumbnail(),
                            ]);
                        }
                        return Task::batch([
                            self.render_content_layer_if_needed(),
                            self.render_nearby_thumbnail(),
                        ]);
                    }
                    Err(error) => {
                        if retry_at_100
                            && self.backend == Backend::Dynlex
                            && document.model.view().viewport().scale() != 1.0
                            && document.model.zoom_at(1.0, [0.0, 0.0]).is_ok()
                        {
                            // A renderer switch may inherit an unproven fit
                            // scale. Retry once at 100% with the *same* strict
                            // DynLex kernel; never show the old renderer's pixels.
                            document.bitmap = None;
                            document.annotation_overlay_cache.borrow_mut().take();
                            self.pending_zoom_offset = Some(scrollable::AbsoluteOffset {
                                x: HORIZONTAL_PAN_MARGIN,
                                y: VERTICAL_PAN_MARGIN,
                            });
                            self.message = "DynLex: trying this page at 100%…".into();
                            return self.render_current_page();
                        }
                        self.pending_zoom_offset = None;
                        // A temporary zoom preview is valid only while sharpening
                        // is pending. Strict DynLex failure must not leave an old
                        // bitmap displayed as the unsupported requested zoom.
                        if self.backend == Backend::Dynlex
                            || document.displayed_page != document.model.view().page()
                            || document.bitmap_backend != self.backend
                        {
                            document.bitmap = None;
                            document.annotation_overlay_cache.borrow_mut().take();
                            if self.backend == Backend::Dynlex {
                                let page = document.model.view().page();
                                document.thumbnails.retain(|(cached, _)| *cached != page);
                            }
                        }
                        self.message = format!("Render failed: {error}");
                        return self.render_nearby_thumbnail();
                    }
                }
            }
            Message::Close => {
                if self.save_busy {
                    self.message = "Wait for Save As to finish".into();
                    return Task::none();
                }
                if !self.allow_discard_once && self.has_unsaved_work() {
                    self.confirm_discard = Some(PendingAction::Close);
                    return Task::none();
                }
                self.allow_discard_once = false;
                let _ = self.persist_reader_position();
                self.next_load += 1;
                self.document = None;
                self.saved_target = None;
                self.reader_mode_active = false;
                self.active_ribbon = RibbonTab::Home;
                self.page_scroll_offset = scrollable::AbsoluteOffset::default();
                self.pending_zoom_offset = None;
                self.busy = false;
                self.opening_document = false;
                self.render_queued = false;
                self.zoom_render_due = None;
                self.message.clear();
            }
        }
        Task::none()
    }

    fn render_nearby_thumbnail(&mut self) -> Task<Message> {
        // Foreground sharpening takes precedence over speculative page previews.
        if self.busy || self.zoom_render_due.is_some() {
            return Task::none();
        }
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if self.hide_pages || document.thumbnail_in_flight.is_some() {
            return Task::none();
        }
        let candidate = next_thumbnail(document);
        let Some(page) = candidate else {
            return Task::none();
        };
        document.thumbnail_in_flight = Some(page);
        let pdf = Arc::clone(&document.pdf);
        let source = Arc::clone(&document.source_bytes);
        let token = self.next_load;
        let generation = self.backend_generation;
        let selected_backend = self.backend;
        let dynlex_document = if selected_backend == Backend::Dynlex {
            Some(Arc::clone(document.dynlex_document.get_or_insert_with(
                || {
                    Arc::new(opds_dynlex_backend::DynlexDocument::new(Arc::clone(
                        &source,
                    )))
                },
            )))
        } else {
            None
        };
        Task::perform(
            async move {
                let (width, height) = pdf.page_dimensions(page + 1).map_err(|e| e.to_string())?;
                let scale = thumbnail_scale(width, height)?;
                if let Some(document) = dynlex_document {
                    // Share prepared pages and JPEG decoding with the main view.
                    if let Ok(outcome) = document.render_strict(&pdf, page + 1, scale) {
                        return Ok(outcome.raster);
                    }
                    // Legacy vector scenes may still require aligned pixel edges.
                    return document
                        .render_strict(&pdf, page + 1, 1.0)
                        .map(|outcome| outcome.raster);
                }
                // PDFium is substantially faster for complex vector-page
                // previews. The actual document page still uses the selected
                // renderer; if PDFium is unavailable this falls back to Hayro.
                let preview_backend = if opds_dynlex_backend::pdfium_available() {
                    Backend::Pdfium
                } else {
                    Backend::Hayro
                };
                render_page_selected(&pdf, &source, page + 1, scale, preview_backend)
                    .map(|outcome| outcome.raster)
            },
            move |result| Message::ThumbnailRendered(token, generation, page, result),
        )
    }

    fn render_content_layer_if_needed(&mut self) -> Task<Message> {
        // Strict DynLex never asks Hayro for a transparent page body.
        Task::none()
    }

    #[cfg(all(test, feature = "dynlex-experimental"))]
    fn render_legacy_content_layer_oracle(&mut self) -> Task<Message> {
        #[cfg(all(test, feature = "dynlex-experimental"))]
        {
            if self.backend != Backend::Dynlex {
                return Task::none();
            }
            let Some(document) = &mut self.document else {
                return Task::none();
            };
            let view = document.model.view();
            let page = view.page();
            let ticket = view.ticket();
            let Some(base) = document.bitmap.as_ref() else {
                return Task::none();
            };
            let image::Handle::Rgba { width, height, .. } = base else {
                return Task::none();
            };
            let text_count = document
                .model
                .items()
                .filter(|item| item.page == page && matches!(item.kind, Kind::Text { .. }))
                .take(65)
                .count();
            let wants_text = (1..=64).contains(&text_count);
            let wants_highlight = document.model.items().any(|item| {
                item.page == page
                    && matches!(
                        item.kind,
                        Kind::Rectangle {
                            color_rgb: [255, 226, 67]
                        }
                    )
            });
            if page != document.displayed_page
                || document.bitmap_scale != 1.0
                || !(wants_text || wants_highlight)
                || !((wants_text
                    && native_text_scene::eligible_mixed_page(
                        &document.source_bytes,
                        page,
                        *width,
                        *height,
                    ))
                    || (wants_highlight
                        && native_text_scene::eligible_annotation_content_layer(
                            &document.source_bytes,
                            page,
                            *width,
                            *height,
                        )))
                || document.content_layer_in_flight == Some(ticket)
                || document
                    .content_layer_cache
                    .borrow()
                    .as_ref()
                    .is_some_and(|cached| cached.base == *base && cached.page == page)
            {
                return Task::none();
            }
            document.content_layer_in_flight = Some(ticket);
            let pdf = Arc::clone(&document.pdf);
            let base = base.clone();
            let token = self.next_load;
            let generation = self.backend_generation;
            return Task::perform(
                async move {
                    pdf.render_page_content_layer(page + 1, 1.0)
                        .map_err(|error| error.to_string())
                },
                move |result| {
                    Message::ContentLayerRendered(token, generation, ticket, base.clone(), result)
                },
            );
        }
        #[cfg(not(all(test, feature = "dynlex-experimental")))]
        Task::none()
    }

    fn schedule_zoom_render(&mut self) -> Task<Message> {
        self.render_queued = false;
        if self.document.as_ref().is_some_and(|document| {
            let view = document.model.view();
            let scale_bits = (view.viewport().scale() as f32).to_bits();
            document
                .page_cache
                .iter()
                .any(|item| item.page == view.page() && item.scale_bits == scale_bits)
        }) {
            self.zoom_render_due = None;
            return self.render_current_page();
        }
        // A single zoom step should start sharpening promptly. Once another
        // step arrives, keep the longer quiet period to avoid rendering every
        // intermediate wheel position.
        let delay = zoom_render_delay(self.zoom_timer_active);
        self.zoom_render_due = Some(Instant::now() + delay);
        if let Some(document) = &self.document {
            self.message = format!(
                "Zoom {:.0}% • sharpening…",
                document.model.view().viewport().scale() * 100.0
            );
        }
        if self.zoom_timer_active {
            Task::none()
        } else {
            self.zoom_timer_active = true;
            zoom_timeout(delay)
        }
    }

    fn render_current_page(&mut self) -> Task<Message> {
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        let view = document.model.view();
        let ticket = view.ticket();
        let page = view.page() + 1;
        let scale = view.viewport().scale() as f32;
        if let Some(cached) = document
            .page_cache
            .iter()
            .find(|item| item.page == page - 1 && item.scale_bits == scale.to_bits())
        {
            let restoring_empty = document.bitmap.is_none();
            document.bitmap = Some(cached.bitmap.clone());
            document.bitmap_size = cached.bitmap_size;
            document.bitmap_page_points = cached.page_points;
            document.bitmap_scale = scale;
            document.bitmap_backend = cached.backend;
            document.imported_annotations_dynlex = cached.imported_annotations_dynlex;
            document.displayed_page = cached.page;
            #[cfg(feature = "dynlex-experimental")]
            {
                document.content_layer_in_flight = None;
                *document.content_layer_cache.borrow_mut() =
                    cached.content_layer.as_ref().map(|rgba| ContentLayerCache {
                        base: cached.bitmap.clone(),
                        page: cached.page,
                        rgba: Arc::clone(rgba),
                    });
            }
            self.busy = false;
            self.message = format!("Page {page} • {:.0}% (cached)", scale * 100.0);
            let restore_scroll = if restoring_empty {
                self.page_scroll_offset = scrollable::AbsoluteOffset {
                    x: HORIZONTAL_PAN_MARGIN,
                    y: VERTICAL_PAN_MARGIN,
                };
                scroll_page_to(self.page_scroll_offset)
            } else {
                Task::none()
            };
            return Task::batch([
                restore_scroll,
                self.render_content_layer_if_needed(),
                self.render_nearby_thumbnail(),
            ]);
        }
        let pdf = Arc::clone(&document.pdf);
        let source = Arc::clone(&document.source_bytes);
        let dynlex_document = if self.backend == Backend::Dynlex {
            Some(Arc::clone(document.dynlex_document.get_or_insert_with(
                || {
                    Arc::new(opds_dynlex_backend::DynlexDocument::new(Arc::clone(
                        &source,
                    )))
                },
            )))
        } else {
            None
        };
        let generation = self.backend_generation;
        let backend = self.backend;
        self.busy = true;
        self.message = format!("Rendering page {page} at {:.0}%…", scale * 100.0);
        Task::perform(
            async move {
                let rendered = if let Some(document) = dynlex_document {
                    document
                        .render_strict(&pdf, page, scale)
                        .map_err(|error| error.to_string())
                } else {
                    render_page_selected(&pdf, &source, page, scale, backend)
                };
                rendered.map(|outcome| (outcome, None))
            },
            move |result| Message::Rendered(generation, ticket, result),
        )
    }

    fn refresh_print_placement_preview(&mut self) -> Task<Message> {
        let Some(bytes) = self.print_preview_base_bytes.clone() else {
            return Task::none();
        };
        self.print_preview_document_revision = self.print_preview_document_revision.wrapping_add(1);
        self.print_preview_revision = self.print_preview_revision.wrapping_add(1);
        let revision = self.print_preview_document_revision;
        self.print_preview_pdf = None;
        self.print_preview = None;
        self.print_preview_error.clear();
        let paper = self.print_paper;
        let scale = self.print_scale;
        let zoom = self.print_zoom_percent;
        if paper == pdf_save::PrintPaper::Original
            && (scale != pdf_save::PrintScale::Custom || zoom == 100)
        {
            self.print_preview_pdf = self.print_preview_base_pdf.clone();
            return self.refresh_print_preview();
        }
        Task::perform(
            async move {
                let placed = pdf_save::apply_print_placement(&bytes, paper, scale, zoom)?;
                ParsedPdf::open(Arc::new(placed))
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
            },
            move |result| Message::PrintScaledPreviewReady(revision, result),
        )
    }

    fn refresh_print_preview(&mut self) -> Task<Message> {
        self.print_preview_revision = self.print_preview_revision.wrapping_add(1);
        let revision = self.print_preview_revision;
        self.print_preview = None;
        if self.backend == Backend::Dynlex {
            self.print_preview_error =
                "Print preview is not supported by DynLex yet. Select another renderer manually."
                    .into();
            return Task::none();
        }
        let Some(document) = &self.document else {
            return Task::none();
        };
        let pages = match print_pages(
            self.print_range,
            self.print_subset,
            self.print_reverse,
            document.model.view().page(),
            &self.print_custom_pages,
            document.pdf.page_count(),
        ) {
            Ok(pages) => pages,
            Err(error) => {
                self.print_preview_error = error;
                return Task::none();
            }
        };
        self.print_preview_error.clear();
        self.print_preview_index = self.print_preview_index.min(pages.len() - 1);
        let page = pages[self.print_preview_index];
        let Some(pdf) = &self.print_preview_pdf else {
            return Task::none();
        };
        let pdf = Arc::clone(pdf);
        Task::perform(
            async move { pdf.render_page(page + 1, 0.4).map_err(|e| e.to_string()) },
            move |result| Message::PrintPreviewReady(revision, page, result),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        hakan_ui::view(self)
    }
}

#[cfg(all(test, feature = "own-rust-experimental"))]
mod strict_own_rust_start_tests {
    use super::*;

    #[test]
    fn unsupported_fit_uses_same_kernel_at_a_supported_scale() {
        let source = include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf");
        let pdf = ParsedPdf::open(Arc::new(source.to_vec())).unwrap();
        let (scale, outcome) =
            render_initial_selected_page(&pdf, source, 1, 1.36, Backend::OwnRust);
        let outcome = outcome.unwrap();
        assert_eq!(scale, 1.0);
        assert_eq!(outcome.used, Backend::OwnRust);
        assert_eq!(outcome.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
    }
}

#[cfg(all(test, feature = "dynlex-experimental"))]
mod strict_dynlex_tests {
    use super::*;

    #[test]
    fn opening_at_unproven_fit_keeps_a_supported_page_visible_on_dynlex() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = ParsedPdf::open(Arc::clone(&source)).unwrap();
        let document = opds_dynlex_backend::DynlexDocument::new(source);
        let (scale, outcome) = render_initial_dynlex_page(&pdf, &document, 1, 1.36);
        let outcome = outcome.unwrap();
        assert_eq!(scale, 1.0);
        assert_eq!(outcome.used, Backend::Dynlex);
        assert_eq!(outcome.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
    }

    #[test]
    fn loaded_page_keeps_its_prepared_dynlex_document_for_later_zooms() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&source)).unwrap());
        let prepared = Arc::new(opds_dynlex_backend::DynlexDocument::new(Arc::clone(
            &source,
        )));
        let (scale, outcome) = render_initial_dynlex_page(&pdf, &prepared, 1, 1.36);
        let mut app = App {
            backend: Backend::Dynlex,
            next_load: 9,
            ..Default::default()
        };
        let _ = app.update(Message::Loaded(
            9,
            Ok((
                "test.pdf".into(),
                source,
                pdf,
                Some(DynlexOpen(Arc::clone(&prepared))),
                1,
                scale,
                None,
                outcome,
                false,
            )),
        ));
        let opened = app.document.as_ref().unwrap();
        assert_eq!(opened.bitmap_scale, 1.0);
        assert!(Arc::ptr_eq(
            opened.dynlex_document.as_ref().unwrap(),
            &prepared
        ));
    }

    #[test]
    fn switching_to_dynlex_retries_unproven_fit_at_100_with_only_dynlex_pixels() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(Arc::clone(&source)).unwrap());
        let hayro = render_page_selected(&pdf, &source, 1, 1.55, Backend::Hayro).unwrap();
        let mut app = App {
            next_load: 91,
            ..Default::default()
        };
        let _ = app.update(Message::Loaded(
            91,
            Ok((
                "test.pdf".into(),
                Arc::clone(&source),
                Arc::clone(&pdf),
                None,
                1,
                1.55,
                None,
                Ok(hayro),
                false,
            )),
        ));
        assert_eq!(
            app.document.as_ref().unwrap().bitmap_backend,
            Backend::Hayro
        );
        let _ = app.update(Message::SelectBackend(Backend::Dynlex));
        assert!(app.document.as_ref().unwrap().bitmap.is_none());
        let first = app.document.as_ref().unwrap().model.view().ticket();
        let _ = app.update(Message::Rendered(
            app.backend_generation,
            first,
            Err("unsupported fit zoom".into()),
        ));
        assert_eq!(app.backend, Backend::Dynlex);
        assert_eq!(
            app.document
                .as_ref()
                .unwrap()
                .model
                .view()
                .viewport()
                .scale(),
            1.0
        );
        assert!(app.document.as_ref().unwrap().bitmap.is_none());
        assert!(app.dynlex_switch_retry.is_none());
        let second = app.document.as_ref().unwrap().model.view().ticket();
        assert_ne!(first, second);
        let rendered = app
            .document
            .as_ref()
            .unwrap()
            .dynlex_document
            .as_ref()
            .unwrap()
            .render_strict(&pdf, 1, 1.0)
            .unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(rendered.raster.rgba, expected.rgba);
        let _ = app.update(Message::Rendered(
            app.backend_generation,
            second,
            Ok((rendered, None)),
        ));
        let document = app.document.as_ref().unwrap();
        assert_eq!(document.bitmap_backend, Backend::Dynlex);
        assert!(document.bitmap.is_some());
        assert_eq!(document.bitmap_scale, 1.0);
        app.document
            .as_mut()
            .unwrap()
            .model
            .zoom_at(1.25, [0.0, 0.0])
            .unwrap();
        let unsupported_zoom = app.document.as_ref().unwrap().model.view().ticket();
        let _ = app.update(Message::Rendered(
            app.backend_generation,
            unsupported_zoom,
            Err("unsupported zoom".into()),
        ));
        assert_eq!(
            app.document
                .as_ref()
                .unwrap()
                .model
                .view()
                .viewport()
                .scale(),
            1.25
        );
        assert!(app.document.as_ref().unwrap().bitmap.is_none());
    }

    #[test]
    fn selected_dynlex_is_strict_for_supported_text_and_zoom() {
        let bytes =
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let outcome = render_page_selected(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(outcome.used, Backend::Dynlex);
        assert!(!outcome.imported_annotations_dynlex);
        assert_eq!(outcome.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
        assert!(render_page_selected(&pdf, &bytes, 1, 1.25, Backend::Dynlex).is_err());
        let bytes =
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        assert!(render_page_selected(&pdf, &bytes, 1, 1.0, Backend::Dynlex).is_err());
        assert!(render_page_selected(&pdf, &bytes, 1, 1.0, Backend::Hayro).is_ok());
        let cache = opds_dynlex_backend::DynlexDocument::new(Arc::new(bytes));
        assert!(cache.render_strict(&pdf, 1, 1.0).is_err());
    }

    #[test]
    fn unsupported_open_stays_navigable_and_switch_drops_foreign_pixels() {
        let source = Arc::new(
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec(),
        );
        let pdf = Arc::new(ParsedPdf::open(source.clone()).unwrap());
        let mut app = App {
            backend: Backend::Dynlex,
            next_load: 7,
            ..Default::default()
        };
        let _ = app.update(Message::Loaded(
            7,
            Ok((
                "test.pdf".into(),
                source.clone(),
                pdf.clone(),
                None,
                1,
                1.0,
                None,
                Err("Unsupported DynLex page".into()),
                false,
            )),
        ));
        assert!(app.document.as_ref().unwrap().bitmap.is_none());
        assert_eq!(app.message, "Unsupported DynLex page");
        let _ = app.update(Message::SelectBackend(Backend::Hayro));
        let outcome = render_page_selected(&pdf, &source, 1, 1.0, Backend::Hayro).unwrap();
        let doc = app.document.as_mut().unwrap();
        doc.cache_bitmap(0, 1.0, outcome, None);
        doc.thumbnails.push_back((0, doc.bitmap.clone().unwrap()));
        let old_generation = app.backend_generation;
        let _ = app.update(Message::SelectBackend(Backend::Dynlex));
        let doc = app.document.as_ref().unwrap();
        assert!(doc.bitmap.is_none());
        assert!(doc.thumbnails.is_empty());
        assert!(doc.page_cache.is_empty());
        let _ = app.update(Message::ThumbnailRendered(
            7,
            old_generation,
            0,
            Ok(pdf.render_page(1, 1.0).unwrap()),
        ));
        assert!(app.document.as_ref().unwrap().thumbnails.is_empty());
        // A failed strict zoom clears even the same-page DynLex preview.
        let outcome = render_page_selected(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
        let doc = app.document.as_mut().unwrap();
        doc.cache_bitmap(0, 1.0, outcome, None);
        let ticket = doc.model.view().ticket();
        let _ = app.update(Message::Rendered(
            app.backend_generation,
            ticket,
            Err("unsupported zoom".into()),
        ));
        assert!(app.document.as_ref().unwrap().bitmap.is_none());
        assert!(app
            .document
            .as_ref()
            .unwrap()
            .content_layer_in_flight
            .is_none());
        app.document.as_mut().unwrap().thumbnail_in_flight = None;
        assert_eq!(next_thumbnail(app.document.as_ref().unwrap()), Some(0));
        app.busy = true;
        let _ = app.render_nearby_thumbnail();
        assert!(app.document.as_ref().unwrap().thumbnail_in_flight.is_none());
        app.busy = false;
        app.zoom_render_due = Some(Instant::now());
        let _ = app.render_nearby_thumbnail();
        assert!(app.document.as_ref().unwrap().thumbnail_in_flight.is_none());
        app.zoom_render_due = None;
        let _ = app.render_nearby_thumbnail();
        assert_eq!(app.document.as_ref().unwrap().thumbnail_in_flight, Some(0));
        // The empty/error workspace must remain zoomable to recover without
        // changing renderer or showing a substitute bitmap.
        let before = app
            .document
            .as_ref()
            .unwrap()
            .model
            .view()
            .viewport()
            .scale();
        let _ = app.update(Message::WheelZoom {
            amount: -1.0,
            anchor: [200., 200.],
        });
        assert!(
            app.document
                .as_ref()
                .unwrap()
                .model
                .view()
                .viewport()
                .scale()
                < before
        );
        assert!(app.zoom_render_due.is_some());
        assert!(app.document.as_ref().unwrap().bitmap.is_none());
    }
}
