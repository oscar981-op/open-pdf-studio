//! First native window; UI structure follows the current Open PDF Studio app.
#![forbid(unsafe_code)]
use iced::widget::{button, column, container, image, row, scrollable, text, text_input};
use iced::{color, Element, Fill, Length, Task, Theme};
use opds_engine_probe::{ParsedPdf, Raster};
use opds_native_core::annotations::DocumentModel;
use opds_native_core::{DocumentId, RenderTicket};
use std::collections::VecDeque;
use std::sync::Arc;

const BG: iced::Color = color!(0x36363E);
const SURFACE: iced::Color = color!(0x44444C);
const ACCENT: iced::Color = color!(0xD97706);
const MAX_PAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;

fn theme(_: &App) -> Theme {
    Theme::Dark
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
            Message::PathChanged(value) => self.path_input = value,
            Message::SelectRibbon(tab) => self.active_ribbon = tab,
            Message::Open => {
                let path = self.path_input.trim().to_owned();
                if path.is_empty() {
                    self.message = "Enter a PDF path".into();
                    return Task::none();
                }
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
        let title = row![
            text("Open PDF Studio").size(17).color(iced::Color::WHITE),
            text("  Rust").color(ACCENT)
        ]
        .spacing(8)
        .padding(10);
        let title = container(title).width(Fill).style(|_| container::Style {
            background: Some(SURFACE.into()),
            ..Default::default()
        });

        let mut tabs = row![].spacing(4);
        for tab in RibbonTab::ALL {
            let label = if tab == self.active_ribbon {
                format!("● {}", tab.label())
            } else {
                tab.label().into()
            };
            tabs = tabs.push(button(text(label)).on_press(Message::SelectRibbon(tab)));
        }
        let controls = row![
            text_input("PDF path", &self.path_input)
                .on_input(Message::PathChanged)
                .on_submit(Message::Open)
                .width(Length::Fixed(420.0)),
            button("Open").on_press(Message::Open),
            button("50%").on_press(Message::Zoom(0.5)),
            button("100%").on_press(Message::Zoom(1.0)),
            button("200%").on_press(Message::Zoom(2.0)),
            button("◀").on_press(Message::GoToPage(
                self.document
                    .as_ref()
                    .map_or(0, |d| d.model.view().page().saturating_sub(1))
            )),
            text(self.document.as_ref().map_or_else(
                || "0 / 0".to_owned(),
                |d| format!(
                    "{} / {}",
                    d.model.view().page() + 1,
                    d.model.view().page_count()
                )
            )),
            button("▶").on_press(Message::GoToPage(
                self.document
                    .as_ref()
                    .map_or(0, |d| d.model.view().page() + 1)
            )),
        ]
        .spacing(8)
        .padding(8);
        let ribbon = container(column![tabs.padding(6), controls])
            .width(Fill)
            .style(|_| container::Style {
                background: Some(BG.into()),
                ..Default::default()
            });

        let name = self
            .document
            .as_ref()
            .and_then(|d| std::path::Path::new(&d.path).file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No document".into());
        let doc_tab = row![
            text(name).color(ACCENT),
            button("×").on_press(Message::Close)
        ]
        .spacing(12)
        .padding(8);
        let doc_tab = container(doc_tab).width(Fill).style(|_| container::Style {
            background: Some(SURFACE.into()),
            ..Default::default()
        });

        let left = container(
            column![
                text("Pages").size(18),
                text("Bookmarks"),
                text("Annotations"),
                text("Layers")
            ]
            .spacing(15)
            .padding(12),
        )
        .width(Length::Fixed(210.0))
        .height(Fill)
        .style(|_| container::Style {
            background: Some(SURFACE.into()),
            ..Default::default()
        });
        let center: Element<'_, Message> =
            if let Some(handle) = self.document.as_ref().and_then(|d| d.bitmap.clone()) {
                scrollable(
                    container(image(handle).width(Length::Shrink))
                        .center_x(Fill)
                        .padding(24),
                )
                .into()
            } else {
                container(text(if self.busy {
                    "Rendering…"
                } else {
                    "Open a PDF to begin"
                }))
                .center(Fill)
                .into()
            };
        let right = container(
            column![text("Properties").size(18), text("No selection")]
                .spacing(12)
                .padding(12),
        )
        .width(Length::Fixed(210.0))
        .height(Fill)
        .style(|_| container::Style {
            background: Some(SURFACE.into()),
            ..Default::default()
        });
        let workspace = row![left, container(center).width(Fill).height(Fill), right].height(Fill);
        let status = container(text(&self.message).size(13))
            .width(Fill)
            .padding(8)
            .style(|_| container::Style {
                background: Some(BG.into()),
                ..Default::default()
            });
        column![title, ribbon, doc_tab, workspace, status]
            .width(Fill)
            .height(Fill)
            .into()
    }
}
