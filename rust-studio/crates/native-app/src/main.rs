//! First native window; UI structure follows the current Open PDF Studio app.
#![forbid(unsafe_code)]
mod annotation_layer;
mod hakan_ui;
mod inline_text;
mod pdf_save;
mod text_layout;
mod virtual_ipp;
use iced::widget::{image, scrollable};
use iced::{color, window, Element, Task, Theme};
use opds_engine_probe::{ParsedPdf, Raster};
use opds_native_core::annotations::{
    Annotation, DocumentModel, Edit, Kind, Rect, TextAlign, DEFAULT_TEXT_FONT_SIZE, TEXT_FONT_SIZES,
};
use opds_native_core::{DocumentId, RenderTicket};
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_PAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_THUMBNAILS: usize = 12;
const THUMBNAIL_ROW_STEP: f32 = 213.0;
const THUMBNAIL_CARD_HEIGHT: f32 = 206.0;
const THUMBNAIL_HEADER_HEIGHT: f32 = 30.0;
const THUMBNAIL_WINDOW_SIZE: usize = 12;
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
        .font(include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf").as_slice())
        .window(window::Settings {
            size: iced::Size::new(1200.0, 832.0),
            min_size: Some(iced::Size::new(760.0, 500.0)),
            decorations: false,
            ..Default::default()
        })
        .run()
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

#[derive(Default)]
struct App {
    path_input: String,
    browser_dir: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    browser_busy: bool,
    browser_error: String,
    next_browse: u64,
    active_ribbon: RibbonTab,
    document: Option<OpenDocument>,
    next_load: u64,
    busy: bool,
    message: String,
    open_dialog: bool,
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
    use super::list_pdf_directory;

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
        std::fs::remove_dir_all(root).unwrap();
    }
}

struct OpenDocument {
    path: String,
    source_bytes: Arc<Vec<u8>>,
    pdf: Arc<ParsedPdf>,
    model: DocumentModel,
    bitmap: Option<image::Handle>,
    bitmap_size: (u32, u32),
    bitmap_page_points: (f32, f32),
    bitmap_scale: f32,
    displayed_page: usize,
    selected_annotation: Option<u64>,
    page_cache: VecDeque<CachedPage>,
    thumbnails: VecDeque<(usize, image::Handle)>,
    thumbnail_in_flight: Option<usize>,
    thumbnail_failed: HashSet<usize>,
    thumbnail_focus: usize,
}

struct CachedPage {
    page: usize,
    scale_bits: u32,
    bytes: usize,
    bitmap: image::Handle,
    bitmap_size: (u32, u32),
    page_points: (f32, f32),
}

impl OpenDocument {
    fn cache_bitmap(&mut self, page: usize, scale: f32, raster: Raster) {
        self.bitmap_size = (raster.width, raster.height);
        self.bitmap_page_points = (raster.page_width_pt, raster.page_height_pt);
        self.bitmap_scale = scale;
        self.displayed_page = page;
        let bytes = raster.rgba.len();
        let bitmap = image::Handle::from_rgba(raster.width, raster.height, raster.rgba);
        self.bitmap = Some(bitmap.clone());
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
        });
        while self.page_cache.iter().map(|item| item.bytes).sum::<usize>() > MAX_PAGE_CACHE_BYTES {
            self.page_cache.pop_front();
        }
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
    ShowSave,
    CancelSave,
    SavePathChanged(String),
    SaveAs,
    Saved(Result<String, String>),
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
    TogglePages,
    ToggleProperties,
    PathChanged(String),
    Open,
    Loaded(
        u64,
        Result<(String, Arc<Vec<u8>>, Arc<ParsedPdf>, f32, Raster), String>,
    ),
    Zoom(f64),
    WheelZoom {
        amount: f32,
        anchor: [f32; 2],
    },
    PageScrolled(scrollable::Viewport),
    ThumbnailsScrolled(scrollable::Viewport),
    GoToPage(usize),
    Rendered(RenderTicket, Result<Raster, String>),
    ThumbnailRendered(u64, usize, Result<Raster, String>),
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
            }
            Message::SelectPdf(path) => {
                self.path_input = path.to_string_lossy().into_owned();
                return self.update(Message::Open);
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
                self.save_dialog = false;
                self.save_busy = true;
                self.message = "Saving PDF…".into();
                let save = Task::perform(
                    async move {
                        let bytes = pdf_save::export_annotations(&source, &annotations)?;
                        pdf_save::save_new(std::path::Path::new(&path), &bytes)?;
                        Ok(path)
                    },
                    Message::Saved,
                );
                if let Some(offset) = self.save_scroll_offset.take() {
                    self.page_scroll_offset = offset;
                    return Task::batch([save, scroll_page_to(offset)]);
                }
                return save;
            }
            Message::Saved(result) => {
                self.save_busy = false;
                match result {
                    Ok(path) => self.message = format!("Saved PDF: {path}"),
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
            Message::TogglePages => {
                self.hide_pages = !self.hide_pages;
                return self.render_nearby_thumbnail();
            }
            Message::ToggleProperties => self.hide_properties = !self.hide_properties,
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
            Message::CloseWindow => return window::oldest().and_then(window::close),
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
                    return scroll_page_to(offset);
                }
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
                    return scroll_page_to(offset);
                }
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
            }
            Message::Redo => {
                if let Some(document) = &mut self.document {
                    if let Err(error) = document.model.redo() {
                        self.message = format!("Redo failed: {error:?}");
                    }
                }
            }
            Message::Open => {
                let path = self.path_input.trim().to_owned();
                if path.is_empty() {
                    self.message = "Enter a PDF path".into();
                    return Task::none();
                }
                self.open_dialog = false;
                self.next_load += 1;
                let token = self.next_load;
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
                        let raster = pdf
                            .render_page(1, initial_scale)
                            .map_err(|error| error.to_string())?;
                        Ok((path, bytes, pdf, initial_scale, raster))
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
                    Ok((path, source_bytes, pdf, initial_scale, raster)) => {
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
                            bitmap: None,
                            bitmap_size: (0, 0),
                            bitmap_page_points: (0.0, 0.0),
                            bitmap_scale: initial_scale,
                            displayed_page: 0,
                            selected_annotation: None,
                            page_cache: VecDeque::new(),
                            thumbnails: VecDeque::new(),
                            thumbnail_in_flight: None,
                            thumbnail_failed: HashSet::new(),
                            thumbnail_focus: 0,
                        };
                        document.cache_bitmap(0, initial_scale, raster);
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
            Message::ThumbnailRendered(token, page, result) => {
                if token != self.next_load {
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
            Message::Rendered(ticket, result) => {
                let Some(document) = &mut self.document else {
                    return Task::none();
                };
                if !document.model.view().accepts(ticket) {
                    return Task::none();
                }
                self.busy = false;
                match result {
                    Ok(raster) => {
                        let (width, height) = (raster.width, raster.height);
                        let view = document.model.view();
                        let page = view.page();
                        let scale = view.viewport().scale() as f32;
                        document.cache_bitmap(page, scale, raster);
                        self.message = format!("{}×{} px", width, height);
                        if let Some(offset) = self.pending_zoom_offset.take() {
                            self.page_scroll_offset = offset;
                            return scroll_page_to(offset);
                        }
                    }
                    Err(error) => {
                        self.pending_zoom_offset = None;
                        self.message = format!("Render failed: {error}");
                    }
                }
            }
            Message::Close => {
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
        let token = self.next_load;
        Task::perform(
            async move {
                let (width, height) = pdf.page_dimensions(page + 1).map_err(|e| e.to_string())?;
                let scale = thumbnail_scale(width, height)?;
                pdf.render_page(page + 1, scale).map_err(|e| e.to_string())
            },
            move |result| Message::ThumbnailRendered(token, page, result),
        )
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
            document.displayed_page = cached.page;
            self.busy = false;
            self.message = format!("Page {page} • {:.0}% (cached)", scale * 100.0);
            return Task::none();
        }
        let pdf = Arc::clone(&document.pdf);
        self.busy = true;
        self.message = format!("Rendering page {page} at {:.0}%…", scale * 100.0);
        Task::perform(
            async move {
                pdf.render_page(page, scale)
                    .map_err(|error| error.to_string())
            },
            move |result| Message::Rendered(ticket, result),
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
