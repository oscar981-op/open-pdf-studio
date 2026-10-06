//! First native window; UI structure follows the current Open PDF Studio app.
#![forbid(unsafe_code)]
mod annotation_layer;
mod hakan_ui;
mod inline_text;
#[cfg(feature = "dynlex-experimental")]
mod native_text_scene;
mod pdf_save;
mod text_layout;
mod virtual_ipp;
use iced::widget::{image, scrollable};
use iced::{color, window, Element, Task, Theme};
use opds_dynlex_backend::{Backend, RenderOutcome};
use opds_engine_probe::{ParsedPdf, Raster};
use opds_native_core::annotations::{
    Annotation, DocumentModel, Edit, Kind, Rect, TextAlign, DEFAULT_TEXT_FONT_SIZE, TEXT_FONT_SIZES,
};
use opds_native_core::{DocumentId, RenderTicket};
use std::cell::RefCell;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn render_page_selected(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
) -> Result<RenderOutcome, String> {
    opds_dynlex_backend::render_with_report(pdf, source, page, scale, backend)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "dynlex-experimental")]
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
        },
        layer,
    ))
}

const MAX_PAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_THUMBNAILS: usize = 12;
const THUMBNAIL_ROW_STEP: f32 = 213.0;
const THUMBNAIL_CARD_HEIGHT: f32 = 206.0;
const THUMBNAIL_HEADER_HEIGHT: f32 = 30.0;
const THUMBNAIL_WINDOW_SIZE: usize = 12;
const BROWSER_ROW_STEP: f32 = 36.0;
const BROWSER_WINDOW_SIZE: usize = 20;
const HORIZONTAL_PAN_MARGIN: f32 = 512.0;

fn thumbnail_scale(width: f32, height: f32) -> Result<f32, String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("Invalid PDF page dimensions".into());
    }
    let scale = (128.0 / width).min(170.0 / height).min(1.0);
    if scale < 0.01 {
        return Err("Page is too large for a thumbnail".into());
    }
    Ok(scale)
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

fn boot() -> (App, Task<Message>) {
    let mut app = App::default();
    if cfg!(feature = "dynlex-experimental")
        && std::env::var("OPDS_RENDER_BACKEND").ok().as_deref() == Some("dynlex")
    {
        app.backend = Backend::Dynlex;
    }
    app.text_font_size = DEFAULT_TEXT_FONT_SIZE;
    if let Some(path) = std::env::args().nth(1) {
        app.path_input = path;
        let task = app.update(Message::Open);
        (app, task)
    } else {
        (app, Task::none())
    }
}

fn main() -> iced::Result {
    iced::application(boot, App::update, App::view)
        .title("Open PDF Studio Rust")
        .theme(theme)
        .subscription(|_| iced::event::listen_with(window_event_to_message))
        .exit_on_close_request(false)
        .font(include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf").as_slice())
        .window(window::Settings {
            size: iced::Size::new(1200.0, 832.0),
            min_size: Some(iced::Size::new(760.0, 500.0)),
            decorations: false,
            ..Default::default()
        })
        .run()
}

fn window_event_to_message(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    matches!(event, iced::Event::Window(window::Event::CloseRequested))
        .then_some(Message::CloseWindow)
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

#[derive(Default)]
struct App {
    backend: Backend,
    backend_generation: u64,
    path_input: String,
    browser_dir: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    browser_busy: bool,
    browser_error: String,
    browser_query: String,
    browser_scroll_y: f32,
    next_browse: u64,
    active_ribbon: RibbonTab,
    document: Option<OpenDocument>,
    next_load: u64,
    busy: bool,
    message: String,
    open_dialog: bool,
    confirm_discard: Option<PendingAction>,
    allow_discard_once: bool,
    save_dialog: bool,
    save_path_input: String,
    save_busy: bool,
    save_scroll_offset: Option<scrollable::AbsoluteOffset>,
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
    text_scroll_offset: Option<scrollable::AbsoluteOffset>,
    hide_pages: bool,
    hide_properties: bool,
    ribbon_collapsed: bool,
    annotation_tool: AnnotationTool,
    annotation_tool_activated: bool,
    next_annotation_id: u64,
    page_scroll_offset: scrollable::AbsoluteOffset,
    pending_zoom_offset: Option<scrollable::AbsoluteOffset>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum AnnotationTool {
    #[default]
    Select,
    Rectangle,
    Highlight,
    Text,
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
    use super::{thumbnail_page_at_scroll, thumbnail_scale, thumbnail_window};

    #[test]
    fn fits_large_landscape_and_portrait_pages_without_upscaling() {
        assert!((thumbnail_scale(3000.0, 2000.0).unwrap() - 128.0 / 3000.0).abs() < 1e-6);
        assert!((thumbnail_scale(600.0, 1200.0).unwrap() - 170.0 / 1200.0).abs() < 1e-6);
        assert_eq!(thumbnail_scale(50.0, 50.0).unwrap(), 1.0);
        assert!(thumbnail_scale(20000.0, 200.0).is_err());
        assert!(thumbnail_scale(f32::NAN, 100.0).is_err());
    }

    #[test]
    fn scroll_focus_tracks_visible_thumbnail_card() {
        assert_eq!(thumbnail_page_at_scroll(0.0, 100), 0);
        assert_eq!(thumbnail_page_at_scroll(30.0 + 213.0 * 9.0, 100), 9);
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
        let _ = app.update(Message::Saved(1, Ok(("old.pdf".into(), Vec::new()))));
        assert!(!app.save_busy);
        assert_eq!(app.message, "Current document");
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
                source_bytes: bytes,
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
        let _ = app.update(Message::Close);
        let _ = app.update(Message::ConfirmDiscard);
        assert!(app.document.is_none());
    }
}

struct OpenDocument {
    path: String,
    source_bytes: Arc<Vec<u8>>,
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
    #[cfg(feature = "dynlex-experimental")]
    content_layer: Option<Arc<Vec<u8>>>,
}

impl OpenDocument {
    fn is_dirty(&self) -> bool {
        annotations_changed(&self.model, &self.last_saved_annotations)
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
        let (text_index, text) = single_text_index(annotations)?;
        if self.bitmap_page_points != (width as f32, height as f32)
            || !native_text_scene::eligible_opaque_page(
                &self.source_bytes,
                self.displayed_page,
                width,
                height,
            )
        {
            return None;
        }
        let scene = native_text_scene::stage_native_text_box_scene(text).ok()?;
        let clip = [
            text.bounds.x as u32,
            text.bounds.y as u32,
            (text.bounds.x + text.bounds.width) as u32,
            (text.bounds.y + text.bounds.height) as u32,
        ];
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
        let (before, text_and_after) = annotations.split_at(text_index);
        let current = paint_shapes(base_rgba, before)?;
        let current = opds_dynlex_backend::render_validated_glyph_scene_over_opaque(
            &scene, &current, width, height, clip,
        )
        .ok()?
        .rgba;
        paint_shapes(&current, &text_and_after[1..])
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
        let (text_index, text) = single_text_index(annotations)?;
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
        let scene = native_text_scene::stage_native_text_box_scene(text).ok()?;
        let clip = [
            text.bounds.x as u32,
            text.bounds.y as u32,
            (text.bounds.x + text.bounds.width) as u32,
            (text.bounds.y + text.bounds.height) as u32,
        ];
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
        let (before, text_and_after) = annotations.split_at(text_index);
        layer = paint_shapes(&layer, before)?;
        layer = opds_dynlex_backend::render_validated_glyph_scene_over_premultiplied(
            &scene, &layer, width, height, clip,
        )
        .ok()?
        .rgba;
        layer = paint_shapes(&layer, &text_and_after[1..])?;
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
        let raster = outcome.raster;
        #[cfg(feature = "dynlex-experimental")]
        {
            self.content_layer_in_flight = None;
            self.content_layer_cache.borrow_mut().take();
        }
        self.bitmap_backend = outcome.used;
        self.bitmap_size = (raster.width, raster.height);
        self.bitmap_page_points = (raster.page_width_pt, raster.page_height_pt);
        self.bitmap_scale = scale;
        self.displayed_page = page;
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
            source_bytes: Arc::new(bytes),
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
        let _task = app.render_content_layer_if_needed();
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
            source_bytes: Arc::new(source),
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
            source_bytes: Arc::new(source),
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
            },
            None,
        );
        let first = document.dynlex_annotation_bitmap().expect("bounded mark");
        assert!(document.dynlex_annotation_bitmap() == Some(first.clone()));
        let mut moved = mark;
        moved.bounds.x = 60.0;
        document.model.apply(Edit::Replace(moved)).unwrap();
        let second = document.dynlex_annotation_bitmap().expect("moved mark");
        assert!(first != second, "edit must invalidate the derived image");

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
        let _task = app.render_content_layer_if_needed();
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

#[derive(Debug, Clone)]
enum Message {
    ShowOpen,
    BrowseDirectory(PathBuf),
    Browsed(u64, Result<Vec<BrowserEntry>, String>),
    SelectPdf(PathBuf),
    BrowserSearchChanged(String),
    BrowserScrolled(scrollable::Viewport),
    ShowSave,
    CancelSave,
    SavePathChanged(String),
    SaveAs,
    Saved(u64, Result<(String, Vec<Annotation>), String>),
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
    PathChanged(String),
    Open,
    Loaded(
        u64,
        Result<(String, Arc<Vec<u8>>, Arc<ParsedPdf>, f32, RenderOutcome), String>,
    ),
    Zoom(f64),
    WheelZoom {
        amount: f32,
        anchor: [f32; 2],
    },
    PageScrolled(scrollable::Viewport),
    ThumbnailsScrolled(scrollable::Viewport),
    GoToPage(usize),
    Rendered(
        u64,
        RenderTicket,
        Result<(RenderOutcome, Option<Raster>), String>,
    ),
    #[cfg(feature = "dynlex-experimental")]
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
    ToggleRibbonCollapsed,
    DragWindow,
    MinimizeWindow,
    MaximizeWindow,
    CloseWindow,
    SelectAnnotationTool(AnnotationTool),
    SelectAnnotation(Option<u64>),
    MoveAnnotation(u64, [f64; 2]),
    ResizeAnnotation(u64, Rect),
    RemoveSelectedAnnotation,
    AddAnnotation(Rect),
    TextEdited(iced::widget::text_editor::Action),
    TextPointer(iced::advanced::text::editor::Cursor, bool),
    SetTextColor([u8; 3]),
    SetTextFontSize(u16),
    SetTextAlign(TextAlign),
    CommitText,
    CancelText,
    EditSelectedText,
    Undo,
    Redo,
    Close,
}

impl App {
    fn has_unsaved_work(&self) -> bool {
        self.text_dialog || self.document.as_ref().is_some_and(OpenDocument::is_dirty)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ShowOpen => {
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
                self.path_input = path.to_string_lossy().into_owned();
                return self.update(Message::Open);
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
                if let Some(document) = &self.document {
                    let path = std::path::Path::new(&document.path);
                    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                    self.save_path_input = path
                        .with_file_name(format!("{stem}-annotated.pdf"))
                        .to_string_lossy()
                        .into_owned();
                    self.save_dialog = true;
                    self.save_scroll_offset = Some(self.page_scroll_offset);
                }
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
                if std::path::Path::new(&path) == std::path::Path::new(&document.path) {
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
                        Ok((path, saved_annotations))
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
                self.save_busy = false;
                if token != self.next_load {
                    return Task::none();
                }
                match result {
                    Ok((path, annotations)) => {
                        if let Some(document) = &mut self.document {
                            document.last_saved_annotations = annotations;
                        }
                        self.message = format!("Saved PDF: {path}");
                    }
                    Err(error) => self.message = format!("Save As failed: {error}"),
                }
            }
            Message::ShowPrint => {
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
                    PendingAction::Open => {
                        self.allow_discard_once = true;
                        self.update(Message::Open)
                    }
                    PendingAction::Close => {
                        self.allow_discard_once = true;
                        self.update(Message::Close)
                    }
                    PendingAction::Exit => window::oldest().and_then(window::close),
                };
            }
            Message::TogglePages => {
                self.hide_pages = !self.hide_pages;
                return self.render_nearby_thumbnail();
            }
            Message::ToggleProperties => self.hide_properties = !self.hide_properties,
            Message::SelectBackend(backend) => {
                if self.busy
                    || self.backend == backend
                    || (backend == Backend::Dynlex && !cfg!(feature = "dynlex-experimental"))
                {
                    return Task::none();
                }
                self.backend = backend;
                self.backend_generation = self.backend_generation.wrapping_add(1);
                if let Some(document) = &mut self.document {
                    document.page_cache.clear();
                    document.bitmap = None;
                    #[cfg(feature = "dynlex-experimental")]
                    {
                        document.content_layer_in_flight = None;
                    }
                    document.thumbnails.clear();
                    document.thumbnail_failed.clear();
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
            Message::MinimizeWindow => {
                return window::oldest().and_then(|id| window::minimize(id, true));
            }
            Message::MaximizeWindow => return window::oldest().and_then(window::toggle_maximize),
            Message::CloseWindow => {
                if self.save_busy {
                    self.message = "Wait for Save As to finish".into();
                    return Task::none();
                }
                if self.has_unsaved_work() {
                    self.confirm_discard = Some(PendingAction::Exit);
                    return Task::none();
                }
                return window::oldest().and_then(window::close);
            }
            Message::SelectAnnotationTool(tool) => {
                self.annotation_tool = tool;
                self.annotation_tool_activated = true;
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
            Message::SetTextColor(color) => {
                self.text_color = color;
                if !self.text_dialog {
                    if let Some(document) = &mut self.document {
                        if let Some(id) = document.selected_annotation {
                            if let Some(mut item) = document.model.get(id).cloned() {
                                if let Kind::Text { color_rgb, .. } = &mut item.kind {
                                    if *color_rgb != color {
                                        *color_rgb = color;
                                        if document.model.apply(Edit::Replace(item)).is_ok() {
                                            self.message = "Text color updated".into();
                                        }
                                    }
                                }
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
                                        if document.model.apply(Edit::Replace(item)).is_ok() {
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
                                        if document.model.apply(Edit::Replace(item)).is_ok() {
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
                    if let Some(id) = self.editing_text_id {
                        if let Some(mut item) = document.model.get(id).cloned() {
                            if matches!(item.kind, Kind::Text { .. }) {
                                item.kind = Kind::Text {
                                    value,
                                    color_rgb: self.text_color,
                                    font_size: self.text_font_size,
                                    align: self.text_align,
                                };
                                match document.model.apply(Edit::Replace(item)) {
                                    Ok(()) => self.message = "Text annotation updated".into(),
                                    Err(error) => {
                                        self.message = format!("Text edit failed: {error:?}")
                                    }
                                }
                            }
                        }
                    } else if let Some(bounds) = self.pending_text_bounds {
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
                        match document.model.apply(Edit::Add(item)) {
                            Ok(()) => {
                                document.selected_annotation = Some(self.next_annotation_id);
                                self.next_annotation_id += 1;
                                self.message = "Text annotation added".into();
                            }
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
                if let Some(document) = &mut self.document {
                    document.selected_annotation = id.filter(|id| {
                        document
                            .model
                            .get(*id)
                            .is_some_and(|item| item.page == document.displayed_page)
                    });
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
                        } else if self.active_ribbon == RibbonTab::Format {
                            self.active_ribbon = RibbonTab::Comment;
                        }
                    } else if self.active_ribbon == RibbonTab::Format && !self.text_dialog {
                        self.active_ribbon = RibbonTab::Comment;
                    }
                }
                return self.render_content_layer_if_needed();
            }
            Message::MoveAnnotation(id, delta) => {
                if let Some(document) = &mut self.document {
                    if document.selected_annotation == Some(id)
                        && delta.iter().all(|value| value.is_finite())
                    {
                        if let Some(mut item) = document.model.get(id).cloned() {
                            let previous = item.bounds;
                            let max_x = (f64::from(document.bitmap_page_points.0)
                                - item.bounds.width)
                                .max(0.0);
                            let max_y = (f64::from(document.bitmap_page_points.1)
                                - item.bounds.height)
                                .max(0.0);
                            item.bounds.x = (item.bounds.x + delta[0]).clamp(0.0, max_x);
                            item.bounds.y = (item.bounds.y + delta[1]).clamp(0.0, max_y);
                            if item.bounds != previous {
                                match document.model.apply(Edit::Replace(item)) {
                                    Ok(()) => self.message = "Annotation moved".into(),
                                    Err(error) => self.message = format!("Move failed: {error:?}"),
                                }
                            }
                        }
                    }
                }
            }
            Message::ResizeAnnotation(id, bounds) => {
                if let Some(document) = &mut self.document {
                    if document.selected_annotation == Some(id) {
                        let page_width = f64::from(document.bitmap_page_points.0);
                        let page_height = f64::from(document.bitmap_page_points.1);
                        if bounds.x < 0.0
                            || bounds.y < 0.0
                            || bounds.x + bounds.width > page_width + 1e-6
                            || bounds.y + bounds.height > page_height + 1e-6
                        {
                            self.message = "Resize outside page".into();
                        } else if let Some(mut item) = document.model.get(id).cloned() {
                            if item.page == document.displayed_page && item.bounds != bounds {
                                item.bounds = bounds;
                                match document.model.apply(Edit::Replace(item)) {
                                    Ok(()) => self.message = "Annotation resized".into(),
                                    Err(error) => {
                                        self.message = format!("Resize failed: {error:?}")
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Message::RemoveSelectedAnnotation => {
                if let Some(document) = &mut self.document {
                    if let Some(id) = document.selected_annotation {
                        match document.model.apply(Edit::Remove(id)) {
                            Ok(()) => {
                                document.selected_annotation = None;
                                if self.active_ribbon == RibbonTab::Format {
                                    self.active_ribbon = RibbonTab::Comment;
                                }
                                self.message = "Annotation removed".into();
                            }
                            Err(error) => self.message = format!("Delete failed: {error:?}"),
                        }
                    }
                }
            }
            Message::AddAnnotation(bounds) => {
                if self.annotation_tool == AnnotationTool::Text {
                    self.pending_text_bounds = Some(bounds);
                    self.editing_text_id = None;
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
                        AnnotationTool::Rectangle => [231, 76, 60],
                        AnnotationTool::Highlight => [255, 226, 67],
                        AnnotationTool::Select => return Task::none(),
                        AnnotationTool::Text => unreachable!("handled above"),
                    };
                    let item = Annotation {
                        id: self.next_annotation_id,
                        page: document.displayed_page,
                        bounds,
                        kind: Kind::Rectangle { color_rgb },
                    };
                    match document.model.apply(Edit::Add(item)) {
                        Ok(()) => {
                            document.selected_annotation = Some(self.next_annotation_id);
                            self.next_annotation_id += 1;
                            self.message = "Annotation added".into();
                        }
                        Err(error) => self.message = format!("Annotation failed: {error:?}"),
                    }
                }
            }
            Message::Undo => {
                if let Some(document) = &mut self.document {
                    if let Err(error) = document.model.undo() {
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
                if let Some(document) = &mut self.document {
                    if let Err(error) = document.model.redo() {
                        self.message = format!("Redo failed: {error:?}");
                    }
                }
                return self.render_content_layer_if_needed();
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
                self.busy = true;
                self.message = "Opening PDF…".into();
                return Task::perform(
                    async move {
                        let bytes =
                            Arc::new(std::fs::read(&path).map_err(|error| error.to_string())?);
                        let pdf = Arc::new(
                            ParsedPdf::open(Arc::clone(&bytes))
                                .map_err(|error| error.to_string())?,
                        );
                        let (page_width, _) =
                            pdf.page_dimensions(1).map_err(|error| error.to_string())?;
                        let initial_scale = (550.0 / page_width).clamp(0.1, 1.0);
                        let outcome =
                            render_page_selected(&pdf, &bytes, 1, initial_scale, backend)?;
                        Ok((path, bytes, pdf, initial_scale, outcome))
                    },
                    move |result| Message::Loaded(token, result),
                );
            }
            Message::Loaded(token, result) => {
                if token != self.next_load {
                    return Task::none();
                }
                self.busy = false;
                match result {
                    Ok((path, source_bytes, pdf, initial_scale, outcome)) => {
                        let raster = &outcome.raster;
                        let Ok(mut model) =
                            DocumentModel::new(DocumentId(token), raster.total_pages)
                        else {
                            return Task::none();
                        };
                        if model.zoom_at(f64::from(initial_scale), [0.0, 0.0]).is_err() {
                            return Task::none();
                        }
                        self.message = format!(
                            "{} pages • {}×{} px",
                            raster.total_pages, raster.width, raster.height
                        );
                        let mut document = OpenDocument {
                            path,
                            source_bytes,
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
                            bitmap_backend: outcome.used,
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
                            initial_scale,
                            outcome,
                            #[cfg(feature = "dynlex-experimental")]
                            None,
                        );
                        self.document = Some(document);
                        self.page_scroll_offset = scrollable::AbsoluteOffset {
                            x: HORIZONTAL_PAN_MARGIN,
                            y: 0.0,
                        };
                        self.pending_zoom_offset = None;
                        return Task::batch([
                            scroll_page_to(self.page_scroll_offset),
                            scroll_thumbnail_to(0),
                            self.render_nearby_thumbnail(),
                        ]);
                    }
                    Err(error) => self.message = format!("PDF open/render failed: {error}"),
                }
            }
            Message::Zoom(scale) => {
                self.pending_zoom_offset = None;
                if let Some(document) = &mut self.document {
                    if document.model.zoom_at(scale, [0.0, 0.0]).is_err() {
                        return Task::none();
                    }
                }
                return self.render_current_page();
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
                            document.thumbnails.pop_front();
                        }
                    }
                    Err(_) => {
                        document.thumbnail_failed.insert(page);
                    }
                }
                return self.render_nearby_thumbnail();
            }
            Message::WheelZoom { amount, anchor } => {
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
                    let current = self.page_scroll_offset;
                    let factor = (next / f64::from(document.bitmap_scale)) as f32;
                    self.pending_zoom_offset = Some(scrollable::AbsoluteOffset {
                        x: (current.x + anchor[0] * (factor - 1.0)).max(0.0),
                        y: (current.y + anchor[1] * (factor - 1.0)).max(0.0),
                    });
                }
                let render = self.render_current_page();
                if self.busy {
                    return render;
                }
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
            Message::GoToPage(page) => {
                self.pending_zoom_offset = None;
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
            #[cfg(feature = "dynlex-experimental")]
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
                    return Task::none();
                }
                self.busy = false;
                match result {
                    Ok((outcome, _content_layer)) => {
                        let raster = &outcome.raster;
                        let (width, height) = (raster.width, raster.height);
                        let view = document.model.view();
                        let page = view.page();
                        let scale = view.viewport().scale() as f32;
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
                            ]);
                        }
                        return self.render_content_layer_if_needed();
                    }
                    Err(error) => {
                        self.pending_zoom_offset = None;
                        self.message = format!("Render failed: {error}");
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
                self.next_load += 1;
                self.document = None;
                self.active_ribbon = RibbonTab::Home;
                self.page_scroll_offset = scrollable::AbsoluteOffset::default();
                self.pending_zoom_offset = None;
                self.busy = false;
                self.message.clear();
            }
        }
        Task::none()
    }

    fn render_nearby_thumbnail(&mut self) -> Task<Message> {
        let Some(document) = &mut self.document else {
            return Task::none();
        };
        if self.hide_pages || document.thumbnail_in_flight.is_some() {
            return Task::none();
        }
        let current = document.model.view().page();
        let focus = document.thumbnail_focus;
        let count = document.pdf.page_count();
        let candidate = [
            Some(focus),
            focus.checked_add(1),
            focus.checked_sub(1),
            focus.checked_add(2),
            focus.checked_sub(2),
            current.checked_add(1),
            current.checked_sub(1),
            current.checked_add(2),
            current.checked_sub(2),
        ]
        .into_iter()
        .flatten()
        .find(|&page| {
            page < count
                && !(page == document.displayed_page && document.bitmap.is_some())
                && !document
                    .thumbnails
                    .iter()
                    .any(|(cached, _)| *cached == page)
                && !document.thumbnail_failed.contains(&page)
        });
        let Some(page) = candidate else {
            return Task::none();
        };
        document.thumbnail_in_flight = Some(page);
        let pdf = Arc::clone(&document.pdf);
        let source = Arc::clone(&document.source_bytes);
        let token = self.next_load;
        let generation = self.backend_generation;
        let backend = self.backend;
        Task::perform(
            async move {
                let (width, height) = pdf.page_dimensions(page + 1).map_err(|e| e.to_string())?;
                let scale = thumbnail_scale(width, height)?;
                render_page_selected(&pdf, &source, page + 1, scale, backend)
                    .map(|outcome| outcome.raster)
            },
            move |result| Message::ThumbnailRendered(token, generation, page, result),
        )
    }

    fn render_content_layer_if_needed(&mut self) -> Task<Message> {
        #[cfg(feature = "dynlex-experimental")]
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
            let wants_text = document
                .model
                .items()
                .filter(|item| item.page == page && matches!(item.kind, Kind::Text { .. }))
                .count()
                == 1;
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
        #[cfg(not(feature = "dynlex-experimental"))]
        Task::none()
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
            document.bitmap = Some(cached.bitmap.clone());
            document.bitmap_size = cached.bitmap_size;
            document.bitmap_page_points = cached.page_points;
            document.bitmap_scale = scale;
            document.bitmap_backend = cached.backend;
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
            return self.render_content_layer_if_needed();
        }
        let pdf = Arc::clone(&document.pdf);
        let source = Arc::clone(&document.source_bytes);
        let generation = self.backend_generation;
        let backend = self.backend;
        #[cfg(feature = "dynlex-experimental")]
        let wants_content_layer = backend == Backend::Dynlex
            && scale == 1.0
            && single_text_index(
                &document
                    .model
                    .items()
                    .filter(|item| item.page + 1 == page)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .is_some();
        self.busy = true;
        self.message = format!("Rendering page {page} at {:.0}%…", scale * 100.0);
        Task::perform(
            async move {
                #[cfg(feature = "dynlex-experimental")]
                if wants_content_layer {
                    if let Ok((width, height)) = pdf.page_dimensions(page) {
                        if width.fract() == 0.0
                            && height.fract() == 0.0
                            && width > 0.0
                            && width <= 256.0
                            && height > 0.0
                            && height <= 4096.0
                            && native_text_scene::eligible_mixed_page(
                                &source,
                                page - 1,
                                width as u32,
                                height as u32,
                            )
                        {
                            let (outcome, layer) =
                                render_page_with_content_layer(&pdf, page, scale)?;
                            return Ok((outcome, Some(layer)));
                        }
                    }
                }
                render_page_selected(&pdf, &source, page, scale, backend)
                    .map(|outcome| (outcome, None))
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
