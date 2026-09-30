//! First native window; UI structure follows the current Open PDF Studio app.
#![forbid(unsafe_code)]
mod hakan_ui;
use iced::widget::image;
use iced::{color, Element, Task, Theme};
use opds_engine_probe::{ParsedPdf, Raster};
use opds_native_core::annotations::DocumentModel;
use opds_native_core::{DocumentId, RenderTicket};
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_PAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;

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
        .run()
}

#[derive(Default)]
struct App {
    path_input: String,
    active_ribbon: RibbonTab,
    document: Option<OpenDocument>,
    next_load: u64,
    busy: bool,
    message: String,
    open_dialog: bool,
    hide_pages: bool,
    hide_properties: bool,
}

struct OpenDocument {
    path: String,
    pdf: Arc<ParsedPdf>,
    model: DocumentModel,
    bitmap: Option<image::Handle>,
    page_cache: VecDeque<CachedPage>,
}

struct CachedPage {
    page: usize,
    scale_bits: u32,
    bytes: usize,
    bitmap: image::Handle,
}

impl OpenDocument {
    fn cache_bitmap(&mut self, page: usize, scale: f32, raster: Raster) {
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
            Self::Comment => "Comment",
            Self::Organize => "Organize",
            Self::Help => "Help",
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ShowOpen,
    CancelOpen,
    TogglePages,
    ToggleProperties,
    PathChanged(String),
    Open,
    Loaded(u64, Result<(String, Arc<ParsedPdf>, Raster), String>),
    Zoom(f64),
    GoToPage(usize),
    Rendered(RenderTicket, Result<Raster, String>),
    SelectRibbon(RibbonTab),
    Close,
}

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ShowOpen => self.open_dialog = true,
            Message::CancelOpen => self.open_dialog = false,
            Message::TogglePages => self.hide_pages = !self.hide_pages,
            Message::ToggleProperties => self.hide_properties = !self.hide_properties,
            Message::PathChanged(value) => self.path_input = value,
            Message::SelectRibbon(tab) => self.active_ribbon = tab,
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
                        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                        let pdf = Arc::new(
                            ParsedPdf::open(Arc::new(bytes)).map_err(|error| error.to_string())?,
                        );
                        let raster = pdf.render_page(1, 1.0).map_err(|error| error.to_string())?;
                        Ok((path, pdf, raster))
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
                    Ok((path, pdf, raster)) => {
                        let Ok(model) = DocumentModel::new(DocumentId(token), raster.total_pages)
                        else {
                            return Task::none();
                        };
                        self.message = format!(
                            "{} pages • {}×{} px",
                            raster.total_pages, raster.width, raster.height
                        );
                        let mut document = OpenDocument {
                            path,
                            pdf,
                            model,
                            bitmap: None,
                            page_cache: VecDeque::new(),
                        };
                        document.cache_bitmap(0, 1.0, raster);
                        self.document = Some(document);
                    }
                    Err(error) => self.message = format!("PDF open/render failed: {error}"),
                }
            }
            Message::Zoom(scale) => {
                if let Some(document) = &mut self.document {
                    if document.model.zoom_at(scale, [0.0, 0.0]).is_err() {
                        return Task::none();
                    }
                }
                return self.render_current_page();
            }
            Message::GoToPage(page) => {
                if let Some(document) = &mut self.document {
                    if document.model.go_to_page(page).is_err() {
                        return Task::none();
                    }
                }
                return self.render_current_page();
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
                    }
                    Err(error) => self.message = format!("Render failed: {error}"),
                }
            }
            Message::Close => {
                self.next_load += 1;
                self.document = None;
                self.busy = false;
                self.message.clear();
            }
        }
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

    fn view(&self) -> Element<'_, Message> {
        hakan_ui::view(self)
    }
}
