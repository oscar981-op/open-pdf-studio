//! Opt-in local MCP transport. External edits are dispatched to the UI's
//! document transaction entry point; the HTTP worker never owns a document.
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use iced::futures::channel::{mpsc, oneshot};
use iced::futures::{SinkExt, StreamExt};
use opds_dynlex_backend::Backend;
use opds_native_core::annotations::{DocumentModel, Kind, Rect, TextAlign};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

const MAX_PDF_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CORPUS_ENTRIES: usize = 256;
const MAX_ANNOTATION_SNAPSHOT: usize = 4096;
const MAX_ANNOTATION_DETAILS_BYTES: usize = 4 * 1024 * 1024;
static ACTIVE_DOCUMENT: OnceLock<RwLock<Option<ActiveDocument>>> = OnceLock::new();
static ACTIVE_TOOL: OnceLock<RwLock<(&'static str, u64)>> = OnceLock::new();
static ACTIVE_BACKEND: OnceLock<RwLock<Backend>> = OnceLock::new();
static REQUEST_QUEUE: OnceLock<Mutex<RequestQueue>> = OnceLock::new();

struct RequestQueue {
    sender: mpsc::Sender<ExternalRequest>,
    receiver: Option<mpsc::Receiver<ExternalRequest>>,
}

#[derive(Clone, Copy, Debug)]
pub enum ExternalAction {
    Delete(u64),
    MoveAnnotation {
        id: u64,
        delta: [f64; 2],
    },
    ResizeAnnotation {
        id: u64,
        bounds: Rect,
    },
    ClearAnnotations(Option<usize>),
    Undo,
    Redo,
    Select(u64),
    ClearSelection,
    SelectBackend {
        expected: Backend,
        requested: Backend,
    },
    GoToPage {
        expected_page: usize,
        page: usize,
    },
    FitView {
        expected_zoom: f64,
        mode: &'static str,
    },
    SelectTool {
        expected: &'static str,
        expected_epoch: u64,
        requested: &'static str,
    },
}

#[derive(Clone, Debug)]
pub struct ExternalRequest {
    pub expected_document_id: u64,
    pub expected_generation: u64,
    pub expected_annotation_revision: u64,
    pub action: ExternalAction,
    response: Arc<Mutex<Option<oneshot::Sender<Value>>>>,
}

impl ExternalRequest {
    pub fn new(expected: [u64; 3], action: ExternalAction) -> (Self, oneshot::Receiver<Value>) {
        let (response_sender, response_receiver) = oneshot::channel();
        (
            Self {
                expected_document_id: expected[0],
                expected_generation: expected[1],
                expected_annotation_revision: expected[2],
                action,
                response: Arc::new(Mutex::new(Some(response_sender))),
            },
            response_receiver,
        )
    }

    #[cfg(test)]
    pub fn delete(expected: [u64; 3], annotation_id: u64) -> (Self, oneshot::Receiver<Value>) {
        Self::new(expected, ExternalAction::Delete(annotation_id))
    }

    pub fn respond(&self, value: Value) {
        if let Ok(mut response) = self.response.lock() {
            if let Some(sender) = response.take() {
                let _ = sender.send(value);
            }
        }
    }

    pub fn was_cancelled(&self) -> bool {
        self.response.lock().map_or(true, |response| {
            response.as_ref().is_none_or(oneshot::Sender::is_canceled)
        })
    }
}

fn request_queue() -> &'static Mutex<RequestQueue> {
    REQUEST_QUEUE.get_or_init(|| {
        let (sender, receiver) = mpsc::channel(32);
        Mutex::new(RequestQueue {
            sender,
            receiver: Some(receiver),
        })
    })
}

pub fn request_stream() -> iced::futures::stream::BoxStream<'static, ExternalRequest> {
    let receiver = request_queue()
        .lock()
        .ok()
        .and_then(|mut queue| queue.receiver.take());
    match receiver {
        Some(receiver) => receiver.boxed(),
        None => iced::futures::stream::pending().boxed(),
    }
}

async fn dispatch_command(expected: [u64; 3], action: ExternalAction) -> Result<Value, String> {
    let (request, response_receiver) = ExternalRequest::new(expected, action);
    let mut sender = request_queue()
        .lock()
        .map_err(|_| "MCP request queue is unavailable")?
        .sender
        .clone();
    tokio::time::timeout(Duration::from_secs(5), sender.send(request))
        .await
        .map_err(|_| "MCP request queue is busy; re-read document state")?
        .map_err(|_| "Rust UI request stream is unavailable")?;
    tokio::time::timeout(Duration::from_secs(10), response_receiver)
        .await
        .map_err(|_| "MCP operation timed out; re-read document state before retrying".to_owned())?
        .map_err(|_| "Rust UI did not return an operation result".to_owned())
}

#[derive(Clone)]
pub struct ActiveDocument {
    pub id: u64,
    pub generation: u64,
    pub annotation_revision: u64,
    pub page_count: usize,
    pub current_page: usize,
    pub scale: f64,
    pub offset: [f64; 2],
    pub file_path: Option<String>,
    pub is_untitled: bool,
    pub modified: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub annotation_count: usize,
    page_size_points: Option<[f32; 2]>,
    selected_annotation: Option<u64>,
    annotations: Arc<Vec<Value>>,
    annotation_details: Arc<Vec<Option<Value>>>,
    annotations_overflow: bool,
}

impl ActiveDocument {
    pub fn from_model(model: &DocumentModel) -> Self {
        let view = model.view();
        let mut annotations = Vec::new();
        let mut annotation_details = Vec::new();
        let mut detail_bytes = 0usize;
        let mut overflow = false;
        for annotation in model.items() {
            if annotations.len() == MAX_ANNOTATION_SNAPSHOT {
                overflow = true;
                break;
            }
            let bounds = annotation.bounds;
            let (kind, color, extra) = match &annotation.kind {
                Kind::Rectangle { color_rgb } => ("box", color_rgb, json!({})),
                Kind::Ellipse { color_rgb } => ("ellipse", color_rgb, json!({})),
                Kind::Polygon { color_rgb } => ("polygon", color_rgb, json!({"sides":6})),
                Kind::Line {
                    start,
                    end,
                    color_rgb,
                } => (
                    "line",
                    color_rgb,
                    json!({"startX":start[0],"startY":start[1],"endX":end[0],"endY":end[1]}),
                ),
                Kind::Arrow {
                    start,
                    end,
                    color_rgb,
                } => (
                    "arrow",
                    color_rgb,
                    json!({"startX":start[0],"startY":start[1],"endX":end[0],"endY":end[1]}),
                ),
                Kind::Polyline { points, color_rgb } => {
                    ("polyline", color_rgb, json!({"pointCount":points.len()}))
                }
                Kind::Text {
                    value, color_rgb, ..
                } => (
                    "textbox",
                    color_rgb,
                    json!({"text":value.chars().take(200).collect::<String>()}),
                ),
            };
            let mut item = json!({"id":annotation.id.to_string(),"type":kind,
                "page":annotation.page+1,"x":bounds.x,"y":bounds.y,
                "width":bounds.width,"height":bounds.height,
                "color":format!("#{:02x}{:02x}{:02x}",color[0],color[1],color[2])});
            if let (Some(fields), Some(extra)) = (item.as_object_mut(), extra.as_object()) {
                fields.extend(extra.clone());
            }
            // Reject oversized text before cloning or JSON-encoding it for IPC.
            let detail_budget = MAX_ANNOTATION_DETAILS_BYTES.saturating_sub(detail_bytes);
            // A JSON string can expand to six bytes per source byte (control
            // character escapes), so leave that worst-case headroom.
            if matches!(&annotation.kind, Kind::Text { value, .. } if value.len() > detail_budget / 6)
            {
                annotation_details.push(None);
                annotations.push(item);
                continue;
            }
            let extra_detail = match &annotation.kind {
                Kind::Rectangle { .. } => json!({}),
                Kind::Ellipse { .. } => json!({}),
                Kind::Polygon { .. } => json!({"sides":6}),
                Kind::Line { .. } | Kind::Arrow { .. } => extra,
                Kind::Polyline { points, .. } => json!({"points":points.iter()
                    .map(|point| json!({"x":point[0],"y":point[1]})).collect::<Vec<_>>()}),
                Kind::Text {
                    value,
                    font_size,
                    align,
                    ..
                } => json!({
                    "text":value,"fontSize":font_size,
                    "align":match align { TextAlign::Left=>"left",TextAlign::Center=>"center",
                        TextAlign::Right=>"right" }
                }),
            };
            let mut detail = item.clone();
            if let (Some(fields), Some(extra)) = (detail.as_object_mut(), extra_detail.as_object())
            {
                fields.extend(extra.clone());
            }
            let encoded_bytes = serde_json::to_vec(&detail).map_or(usize::MAX, |bytes| bytes.len());
            if encoded_bytes <= detail_budget {
                detail_bytes += encoded_bytes;
                annotation_details.push(Some(detail));
            } else {
                annotation_details.push(None);
            }
            annotations.push(item);
        }
        Self {
            id: view.document_id().0,
            generation: view.generation(),
            annotation_revision: model.annotation_revision(),
            page_count: view.page_count(),
            current_page: view.page() + 1,
            scale: view.viewport().scale(),
            offset: view.viewport().offset(),
            file_path: None,
            is_untitled: false,
            modified: false,
            can_undo: model.can_undo(),
            can_redo: model.can_redo(),
            annotation_count: model.items().count(),
            page_size_points: None,
            selected_annotation: None,
            annotations: Arc::new(annotations),
            annotation_details: Arc::new(annotation_details),
            annotations_overflow: overflow,
        }
    }

    fn update_tab_state(
        &mut self,
        model: &DocumentModel,
        path: &str,
        is_untitled: bool,
        modified: bool,
        selected_annotation: Option<u64>,
    ) {
        let view = model.view();
        if self.id == view.document_id().0
            && self.generation == view.generation()
            && self.annotation_revision == model.annotation_revision()
        {
            self.file_path = (!is_untitled && path.len() <= 4096).then(|| path.to_owned());
            self.is_untitled = is_untitled;
            self.modified = modified;
            self.selected_annotation = selected_annotation.filter(|id| {
                model
                    .get(*id)
                    .is_some_and(|annotation| annotation.page == view.page())
            });
        }
    }

    fn update_page_geometry(&mut self, model: &DocumentModel, size: Option<(f32, f32)>) {
        let view = model.view();
        if self.id != view.document_id().0
            || self.generation != view.generation()
            || self.annotation_revision != model.annotation_revision()
        {
            return;
        }
        self.page_size_points = size
            .filter(|(width, height)| {
                width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0
            })
            .map(|(width, height)| [width, height]);
    }
}

fn accepts_new_snapshot(previous: &ActiveDocument, next: &ActiveDocument) -> bool {
    previous.id != next.id
        || (next.generation >= previous.generation
            && next.annotation_revision >= previous.annotation_revision)
}

pub fn publish_document(document: Option<ActiveDocument>) {
    let lock = ACTIVE_DOCUMENT.get_or_init(|| RwLock::new(None));
    if let Ok(mut active) = lock.write() {
        if let (Some(previous), Some(next)) = (active.as_ref(), document.as_ref()) {
            if !accepts_new_snapshot(previous, next) {
                return;
            }
        }
        *active = document;
    }
}

pub fn publish_model(model: &DocumentModel) {
    let view = model.view();
    if ACTIVE_DOCUMENT
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|current| {
            current.as_ref().map(|current| {
                current.id == view.document_id().0
                    && current.generation >= view.generation()
                    && current.annotation_revision >= model.annotation_revision()
            })
        })
        .unwrap_or(false)
    {
        return;
    }
    publish_document(Some(ActiveDocument::from_model(model)));
}

pub fn publish_tab_state(
    model: &DocumentModel,
    path: &str,
    is_untitled: bool,
    modified: bool,
    selected_annotation: Option<u64>,
) {
    let Some(lock) = ACTIVE_DOCUMENT.get() else {
        return;
    };
    if let Ok(mut active) = lock.write() {
        let Some(current) = active.as_mut() else {
            return;
        };
        current.update_tab_state(model, path, is_untitled, modified, selected_annotation);
    }
}

pub fn publish_page_geometry(model: &DocumentModel, size: Option<(f32, f32)>) {
    let Some(lock) = ACTIVE_DOCUMENT.get() else {
        return;
    };
    if let Ok(mut active) = lock.write() {
        let Some(current) = active.as_mut() else {
            return;
        };
        current.update_page_geometry(model, size);
    }
}

fn active_document() -> Option<ActiveDocument> {
    ACTIVE_DOCUMENT.get()?.read().ok()?.as_ref().cloned()
}

pub fn publish_tool(tool: &'static str, epoch: u64) {
    let lock = ACTIVE_TOOL.get_or_init(|| RwLock::new(("select", 0)));
    if let Ok(mut current) = lock.write() {
        *current = (tool, epoch);
    }
}

fn active_tool() -> (&'static str, u64) {
    ACTIVE_TOOL
        .get()
        .and_then(|lock| lock.read().ok().map(|tool| *tool))
        .unwrap_or(("select", 0))
}

pub fn publish_backend(backend: Backend) {
    let lock = ACTIVE_BACKEND.get_or_init(|| RwLock::new(Backend::Hayro));
    if let Ok(mut current) = lock.write() {
        *current = backend;
    }
}

fn active_backend() -> Backend {
    ACTIVE_BACKEND
        .get()
        .and_then(|lock| lock.read().ok().map(|backend| *backend))
        .unwrap_or(Backend::Hayro)
}

fn annotation_response(document: ActiveDocument, page: Option<usize>) -> Result<Value, String> {
    if document.annotations_overflow {
        return Err("annotation snapshot exceeds 4096 items".into());
    }
    let annotations: Vec<_> = document
        .annotations
        .iter()
        .filter(|annotation| page.is_none_or(|page| annotation["page"] == page))
        .cloned()
        .collect();
    Ok(json!({"ok":true,"count":annotations.len(),"annotations":annotations}))
}

fn annotation_detail(document: ActiveDocument, id: &str) -> Result<Value, String> {
    if document.annotations_overflow {
        return Err("annotation snapshot exceeds 4096 items".into());
    }
    match document
        .annotations
        .iter()
        .position(|annotation| annotation["id"] == id)
    {
        None => Ok(json!({"ok":false,"error":format!("annotation not found: {id}")})),
        Some(index) => match document.annotation_details[index].as_ref() {
            Some(annotation) => Ok(json!({"ok":true,"annotation":annotation})),
            None => Err("annotation details exceed the 4 MiB snapshot limit".into()),
        },
    }
}

#[derive(Clone)]
struct ServerState {
    corpus: Arc<PathBuf>,
    port: u16,
}

pub fn document_argument() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--mcp-port" {
            args.next();
        } else if arg != "--mcp-server" && !arg.starts_with("--mcp-port=") {
            return Some(arg);
        }
    }
    None
}

pub fn requested_port() -> Option<u16> {
    let mut args = std::env::args().skip(1);
    let mut enabled = false;
    let mut port = 9223;
    while let Some(arg) = args.next() {
        if arg == "--mcp-server" {
            enabled = true;
        } else if arg == "--mcp-port" || arg.starts_with("--mcp-port=") {
            let value = if arg == "--mcp-port" {
                args.next().unwrap_or_default()
            } else {
                arg[11..].to_owned()
            };
            port = value
                .parse()
                .ok()
                .filter(|port: &u16| *port > 0)
                .unwrap_or_else(|| {
                    eprintln!("Invalid --mcp-port");
                    std::process::exit(2)
                });
        }
    }
    enabled.then_some(port)
}

pub fn start(port: u16) -> Result<(), String> {
    if !cfg!(debug_assertions) && std::env::var("OPS_ENABLE_MCP").as_deref() != Ok("1") {
        return Err("release MCP requires OPS_ENABLE_MCP=1".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| format!("could not bind 127.0.0.1:{port}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("could not configure MCP listener: {error}"))?;
    let corpus = std::env::var_os("OPS_TEST_PDFS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../test pdf-bestanden/Originele bestanden")
        });
    let corpus = corpus
        .canonicalize()
        .map_err(|error| format!("could not open MCP corpus: {error}"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start MCP runtime: {error}"))?;
    std::thread::spawn(move || {
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("MCP listener");
            let state = ServerState {
                corpus: Arc::new(corpus),
                port,
            };
            let app = Router::new()
                .route("/mcp", post(handle))
                .layer(DefaultBodyLimit::max(64 * 1024))
                .with_state(state);
            eprintln!("MCP server listening on http://127.0.0.1:{port}/mcp");
            if let Err(error) = axum::serve(listener, app).await {
                eprintln!("MCP server stopped: {error}");
            }
        });
    });
    Ok(())
}

fn local_request(headers: &HeaderMap, port: u16) -> bool {
    let expected = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host_ok = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            expected
                .iter()
                .any(|expected| host.eq_ignore_ascii_case(expected))
        });
    let origin_ok = headers.get("origin").is_none_or(|origin| {
        origin.to_str().ok().is_some_and(|value| {
            let authority = value
                .split_once("://")
                .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
                .unwrap_or_default();
            expected
                .iter()
                .any(|expected| authority.eq_ignore_ascii_case(expected))
        })
    });
    host_ok && origin_ok
}

fn result(id: Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}

fn error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}

fn text_result(payload: Value) -> Value {
    json!({"content":[{"type":"text","text":payload.to_string()}],"isError":false})
}

fn tab_response(document: Option<ActiveDocument>) -> Value {
    let Some(document) = document else {
        return json!({"ok":true,"activeIndex":-1,"tabs":[]});
    };
    let file_name = if document.is_untitled {
        Some("Untitled.pdf".to_owned())
    } else {
        document.file_path.as_deref().and_then(|path| {
            Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    };
    json!({"ok":true,"activeIndex":0,"tabs":[{
        "index":0,"fileName":file_name,"filePath":document.file_path,
        "modified":document.modified,"active":true,
        "pageCount":document.page_count,"isUntitled":document.is_untitled,
        "currentPage":document.current_page,
        "annotationCount":document.annotation_count
    }]})
}

fn native_view_response(document: Option<ActiveDocument>) -> Value {
    let Some(document) = document else {
        return json!({"ok":false,"error":"no active document"});
    };
    json!({"ok":true,"documentId":document.id.to_string(),
        "generation":document.generation,
        "annotationRevision":document.annotation_revision,
        "pageCount":document.page_count,"currentPage":document.current_page,
        "pageWidthPoints":document.page_size_points.map(|size| size[0]),
        "pageHeightPoints":document.page_size_points.map(|size| size[1]),
        "scale":document.scale,"offsetX":document.offset[0],
        "offsetY":document.offset[1]})
}

fn native_selection_response(document: Option<ActiveDocument>) -> Value {
    let Some(document) = document else {
        return json!({"ok":false,"error":"no active document"});
    };
    json!({"ok":true,"documentId":document.id.to_string(),
        "generation":document.generation,
        "annotationRevision":document.annotation_revision,
        "selectedId":document.selected_annotation.map(|id| id.to_string())})
}

fn native_edit_response(document: Option<ActiveDocument>) -> Value {
    let Some(document) = document else {
        return json!({"ok":false,"error":"no active document"});
    };
    json!({"ok":true,"documentId":document.id.to_string(),
        "generation":document.generation,
        "annotationRevision":document.annotation_revision,
        "modified":document.modified,
        "canUndo":document.can_undo,"canRedo":document.can_redo})
}

fn native_renderers_response(selected: Backend) -> Value {
    let renderers = [
        ("hayro", Backend::Hayro, true),
        (
            "dynlex",
            Backend::Dynlex,
            cfg!(feature = "dynlex-experimental"),
        ),
        (
            "pdfium",
            Backend::Pdfium,
            opds_dynlex_backend::pdfium_available(),
        ),
        (
            "own-rust",
            Backend::OwnRust,
            opds_dynlex_backend::own_rust_available(),
        ),
        (
            "pdfcraft",
            Backend::PdfCraft,
            opds_dynlex_backend::pdfcraft_available(),
        ),
    ];
    let selected_id = renderers
        .iter()
        .find(|(_, backend, _)| *backend == selected)
        .map_or("hayro", |(id, _, _)| *id);
    json!({"ok":true,"selected":selected_id,"renderers":renderers
        .iter().map(|(id, _, available)| json!({"id":id,"available":available})).collect::<Vec<_>>()})
}

fn version_fields(fields: &serde_json::Map<String, Value>) -> Option<[u64; 3]> {
    let id = fields.get("documentId")?.as_str()?.parse::<u64>().ok()?;
    let generation = fields.get("generation")?.as_u64()?;
    let revision = fields.get("annotationRevision")?.as_u64()?;
    if id == 0 {
        return None;
    }
    Some([id, generation, revision])
}

fn version_arguments(args: &Value) -> Option<[u64; 3]> {
    let fields = args.as_object()?;
    (fields.len() == 3)
        .then(|| version_fields(fields))
        .flatten()
}

fn delete_arguments(args: &Value) -> Option<([u64; 3], u64)> {
    let fields = args.as_object()?;
    if fields.len() != 4 {
        return None;
    }
    let expected = version_fields(fields)?;
    let annotation_id = fields.get("id")?.as_str()?.parse::<u64>().ok()?;
    Some((expected, annotation_id))
}

fn move_annotation_arguments(args: &Value) -> Option<([u64; 3], u64, [f64; 2])> {
    let fields = args.as_object()?;
    if fields.len() != 6 {
        return None;
    }
    let expected = version_fields(fields)?;
    let id = fields.get("id")?.as_str()?.parse::<u64>().ok()?;
    let dx = fields.get("dx")?.as_f64()?;
    let dy = fields.get("dy")?.as_f64()?;
    if ![dx, dy]
        .iter()
        .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
    {
        return None;
    }
    Some((expected, id, [dx, dy]))
}

fn resize_annotation_arguments(args: &Value) -> Option<([u64; 3], u64, Rect)> {
    let fields = args.as_object()?;
    if fields.len() != 8 {
        return None;
    }
    let expected = version_fields(fields)?;
    let id = fields.get("id")?.as_str()?.parse::<u64>().ok()?;
    let bounds = Rect {
        x: fields.get("x")?.as_f64()?,
        y: fields.get("y")?.as_f64()?,
        width: fields.get("width")?.as_f64()?,
        height: fields.get("height")?.as_f64()?,
    };
    if ![bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.width <= 0.0
        || bounds.height <= 0.0
    {
        return None;
    }
    Some((expected, id, bounds))
}

fn select_arguments(args: &Value) -> Option<([u64; 3], u64)> {
    // Selection uses the same exact version and native ID syntax as deletion.
    delete_arguments(args)
}

fn clear_annotations_arguments(args: &Value) -> Option<([u64; 3], Option<usize>)> {
    let fields = args.as_object()?;
    let expected = version_fields(fields)?;
    match fields.get("scope")?.as_str()? {
        "all" if fields.len() == 4 => Some((expected, None)),
        "page" if fields.len() == 5 => {
            let page = fields.get("page")?.as_u64()?;
            let page = usize::try_from(page).ok()?.checked_sub(1)?;
            Some((expected, Some(page)))
        }
        _ => None,
    }
}

pub fn backend_id(backend: Backend) -> &'static str {
    match backend {
        Backend::Hayro => "hayro",
        Backend::Dynlex => "dynlex",
        Backend::Pdfium => "pdfium",
        Backend::OwnRust => "own-rust",
        Backend::PdfCraft => "pdfcraft",
    }
}

fn parse_backend(value: &Value) -> Option<Backend> {
    match value.as_str()? {
        "hayro" => Some(Backend::Hayro),
        "dynlex" => Some(Backend::Dynlex),
        "pdfium" => Some(Backend::Pdfium),
        "own-rust" => Some(Backend::OwnRust),
        "pdfcraft" => Some(Backend::PdfCraft),
        _ => None,
    }
}

fn select_backend_arguments(args: &Value) -> Option<([u64; 3], Backend, Backend)> {
    let fields = args.as_object()?;
    if fields.len() != 5 {
        return None;
    }
    Some((
        version_fields(fields)?,
        parse_backend(fields.get("expectedRenderer")?)?,
        parse_backend(fields.get("renderer")?)?,
    ))
}

fn go_to_page_arguments(args: &Value) -> Option<([u64; 3], usize, usize)> {
    let fields = args.as_object()?;
    if fields.len() != 5 {
        return None;
    }
    let expected_page = usize::try_from(fields.get("expectedPage")?.as_u64()?)
        .ok()?
        .checked_sub(1)?;
    let page = usize::try_from(fields.get("page")?.as_u64()?)
        .ok()?
        .checked_sub(1)?;
    Some((version_fields(fields)?, expected_page, page))
}

fn fit_view_arguments(args: &Value) -> Option<([u64; 3], f64, &'static str)> {
    let fields = args.as_object()?;
    if fields.len() != 5 {
        return None;
    }
    let zoom = fields.get("expectedZoom")?.as_f64()?;
    if !zoom.is_finite() || !(0.05..=32.0).contains(&zoom) {
        return None;
    }
    let mode = match fields.get("mode")?.as_str()? {
        "page" => "page",
        "width" => "width",
        _ => return None,
    };
    Some((version_fields(fields)?, zoom, mode))
}

fn parse_tool(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "select" => Some("select"),
        "hand" => Some("hand"),
        "box" => Some("box"),
        "circle" => Some("circle"),
        "ellipse" => Some("ellipse"),
        "polygon" => Some("polygon"),
        "freehand" => Some("freehand"),
        "line" => Some("line"),
        "arrow" => Some("arrow"),
        "polyline" => Some("polyline"),
        "highlight" => Some("highlight"),
        "textbox" => Some("textbox"),
        _ => None,
    }
}

fn select_tool_arguments(args: &Value) -> Option<([u64; 3], &'static str, u64, &'static str)> {
    let fields = args.as_object()?;
    if fields.len() != 6 {
        return None;
    }
    Some((
        version_fields(fields)?,
        parse_tool(fields.get("expectedTool")?)?,
        fields.get("expectedToolEpoch")?.as_u64()?,
        parse_tool(fields.get("tool")?)?,
    ))
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"list_test_pdfs","description":"List PDFs in the configured local test corpus.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"get_pdf_metadata","description":"Read PDF version, producer, creator and page geometry.",
         "inputSchema":{"type":"object","properties":{"path":{"type":"string"}},
                        "required":["path"],"additionalProperties":false}},
        {"name":"app_get_page_count","description":"Return the active Rust document's page count and current page.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_go_to_page","description":"Navigate the live Rust document to a 1-based page via the same UI action as its page controls. Requires the exact document version and current page; success schedules a render and does not imply the chosen renderer supports that page.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"expectedPage":{"type":"integer","minimum":1},"page":{"type":"integer","minimum":1}},"required":["documentId","generation","annotationRevision","expectedPage","page"],"additionalProperties":false}},
        {"name":"app_fit_view","description":"Fit the live Rust document to the page or available width using the current native window geometry. Supply the exact document version and zoom read from app_get_native_view_state; stale zooms and no-op requests are rejected. Success schedules a render, not a bitmap guarantee.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"expectedZoom":{"type":"number","minimum":0.05,"maximum":32},"mode":{"type":"string","enum":["page","width"]}},"required":["documentId","generation","annotationRevision","expectedZoom","mode"],"additionalProperties":false}},
        {"name":"app_get_current_tool","description":"Return the active native annotation tool.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_set_tool","description":"Select one of the twelve implemented native annotation tools through the same Rust UI action as the ribbon or left palette. Requires the exact document version plus current tool and epoch from app_get_current_tool; stale requests and replay are rejected.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"expectedTool":{"type":"string","enum":["select","hand","box","circle","ellipse","polygon","freehand","line","arrow","polyline","highlight","textbox"]},"expectedToolEpoch":{"type":"integer","minimum":0},"tool":{"type":"string","enum":["select","hand","box","circle","ellipse","polygon","freehand","line","arrow","polyline","highlight","textbox"]}},"required":["documentId","generation","annotationRevision","expectedTool","expectedToolEpoch","tool"],"additionalProperties":false}},
        {"name":"app_get_native_renderers","description":"List the five Rust UI renderer choices, their current availability and the selected choice. Availability can change if an optional external library or CLI disappears.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_list_tabs","description":"List the single active native document tab and its live page and modified state.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_get_native_view_state","description":"Read the native DocumentView page, zoom and page-to-core-viewport offset. These are model coordinates, not Tauri screen or canvas coordinates.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_get_native_selection","description":"Read the selected native annotation id from the active Rust document, or null when none is selected. Use app_get_annotation for its fields.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_get_native_edit_state","description":"Read the active Rust document's modified and undo/redo availability state with its document version. This is not a mutation tool.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"app_list_annotations","description":"List up to 4096 annotations from the active native document model, optionally filtered by 1-based page. Markup-layer filtering is not yet supported.",
         "inputSchema":{"type":"object","properties":{"page":{"type":"integer","minimum":1}},"additionalProperties":false}},
        {"name":"app_get_annotation","description":"Get native fields for one annotation by id. Only Rust-supported annotation kinds and properties are returned; large details may be rejected.",
         "inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}},
        {"name":"app_delete_annotation","description":"Delete one native annotation through the live Rust document transaction. Supply the exact document id, generation and annotation revision previously read from app_get_native_edit_state; stale requests have no effect. On timeout, re-read state before retrying.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"id":{"type":"string"}},"required":["documentId","generation","annotationRevision","id"],"additionalProperties":false}},
        {"name":"app_move_annotation","description":"Move one native annotation on the displayed page by dx/dy PDF points, clamped to page bounds, through the same versioned undoable Rust transaction as GUI dragging. Supply the exact document version and id; stale, unknown and no-op moves have no effect. On timeout, re-read state before retrying.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"id":{"type":"string"},"dx":{"type":"number","minimum":-1000000,"maximum":1000000},"dy":{"type":"number","minimum":-1000000,"maximum":1000000}},"required":["documentId","generation","annotationRevision","id","dx","dy"],"additionalProperties":false}},
        {"name":"app_resize_annotation","description":"Resize one native rectangle, ellipse, polygon or text annotation on the displayed page using PDF-point bounds. Uses the same versioned undoable Rust transaction as GUI resize handles. Point-based lines and polylines are rejected. Supply the exact document version; stale, unknown, out-of-page and no-op requests have no effect. On timeout, re-read state before retrying.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"id":{"type":"string"},"x":{"type":"number","minimum":0,"maximum":1000000},"y":{"type":"number","minimum":0,"maximum":1000000},"width":{"type":"number","exclusiveMinimum":0,"maximum":1000000},"height":{"type":"number","exclusiveMinimum":0,"maximum":1000000}},"required":["documentId","generation","annotationRevision","id","x","y","width","height"],"additionalProperties":false}},
        {"name":"app_clear_annotations","description":"Clear native annotations on one 1-based page or all pages as one undoable, versioned document transaction. An explicit scope is required. On timeout, re-read state before retrying.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"scope":{"type":"string","enum":["page","all"]},"page":{"type":"integer","minimum":1}},"required":["documentId","generation","annotationRevision","scope"],"additionalProperties":false}},
        {"name":"app_undo","description":"Undo one live native annotation edit through the versioned Rust document transaction. Requires the exact document id, generation and annotation revision from app_get_native_edit_state; a stale request has no effect.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0}},"required":["documentId","generation","annotationRevision"],"additionalProperties":false}},
        {"name":"app_redo","description":"Redo one live native annotation edit through the versioned Rust document transaction. Requires the exact document id, generation and annotation revision from app_get_native_edit_state; a stale request has no effect.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0}},"required":["documentId","generation","annotationRevision"],"additionalProperties":false}},
        {"name":"app_select_annotation","description":"Select one annotation on the currently displayed native page. Requires its exact id and the current document version; does not edit PDF content.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"id":{"type":"string"}},"required":["documentId","generation","annotationRevision","id"],"additionalProperties":false}},
        {"name":"app_clear_selection","description":"Clear the current native annotation selection. Requires the exact current document version; does not edit PDF content.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0}},"required":["documentId","generation","annotationRevision"],"additionalProperties":false}},
        {"name":"app_select_native_renderer","description":"Select an available Rust UI renderer through the same action as the bottom-left picker. Requires the exact document version and currently selected renderer. Success means a new page render was scheduled, not that the page is supported or finished; strict DynLex never falls back.",
         "inputSchema":{"type":"object","properties":{"documentId":{"type":"string"},"generation":{"type":"integer","minimum":0},"annotationRevision":{"type":"integer","minimum":0},"expectedRenderer":{"type":"string","enum":["hayro","dynlex","pdfium","own-rust","pdfcraft"]},"renderer":{"type":"string","enum":["hayro","dynlex","pdfium","own-rust","pdfcraft"]}},"required":["documentId","generation","annotationRevision","expectedRenderer","renderer"],"additionalProperties":false}}
    ]})
}

async fn handle(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    if !local_request(&headers, state.port) {
        return (
            StatusCode::FORBIDDEN,
            Json(error(id, -32600, "local clients only")),
        );
    }
    if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return (
            StatusCode::OK,
            Json(error(id, -32600, "invalid JSON-RPC version")),
        );
    }
    let response = match body.get("method").and_then(Value::as_str) {
        Some("initialize") => result(
            id,
            json!({"protocolVersion":"2025-03-26","serverInfo":{"name":"open-pdf-studio-rust",
                   "version":env!("CARGO_PKG_VERSION")},"capabilities":{"tools":{"listChanged":false}}}),
        ),
        Some("tools/list") => result(id, tools()),
        Some("notifications/initialized" | "initialized") => result(id, json!({})),
        Some("tools/call") => {
            let params = body.get("params").unwrap_or(&Value::Null);
            let args = params.get("arguments").unwrap_or(&Value::Null);
            match params.get("name").and_then(Value::as_str) {
                Some("list_test_pdfs")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    match tokio::task::spawn_blocking({
                        let corpus = Arc::clone(&state.corpus);
                        move || list_test_pdfs(&corpus)
                    })
                    .await
                    {
                        Ok(Ok(value)) => result(id, text_result(value)),
                        Ok(Err(message)) => error(id, -32603, message),
                        Err(_) => error(id, -32603, "corpus task failed"),
                    }
                }
                Some("get_pdf_metadata") => {
                    if let Some(path) = args
                        .get("path")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty() && s.len() <= 4096)
                    {
                        let path = PathBuf::from(path);
                        match tokio::task::spawn_blocking(move || get_pdf_metadata(&path)).await {
                            Ok(Ok(value)) => result(id, text_result(value)),
                            Ok(Err(message)) => error(id, -32603, message),
                            Err(_) => error(id, -32603, "metadata task failed"),
                        }
                    } else {
                        error(id, -32602, "missing or invalid 'path'")
                    }
                }
                Some("app_get_page_count")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    let payload = match active_document() {
                        Some(document) => json!({"ok":true,"pageCount":document.page_count,
                            "currentPage":document.current_page}),
                        None => json!({"ok":false,"error":"no active document"}),
                    };
                    result(id, text_result(payload))
                }
                Some("app_get_page_count") => error(id, -32602, "unexpected arguments"),
                Some("app_get_current_tool")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    let (tool, epoch) = active_tool();
                    result(
                        id,
                        text_result(json!({"ok":true,"tool":tool,"toolEpoch":epoch})),
                    )
                }
                Some("app_get_current_tool") => error(id, -32602, "unexpected arguments"),
                Some("app_get_native_renderers")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    result(id, text_result(native_renderers_response(active_backend())))
                }
                Some("app_get_native_renderers") => error(id, -32602, "unexpected arguments"),
                Some("app_list_tabs")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    result(id, text_result(tab_response(active_document())))
                }
                Some("app_list_tabs") => error(id, -32602, "unexpected arguments"),
                Some("app_get_native_view_state")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    result(id, text_result(native_view_response(active_document())))
                }
                Some("app_get_native_view_state") => error(id, -32602, "unexpected arguments"),
                Some("app_get_native_selection")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    result(
                        id,
                        text_result(native_selection_response(active_document())),
                    )
                }
                Some("app_get_native_selection") => error(id, -32602, "unexpected arguments"),
                Some("app_get_native_edit_state")
                    if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) =>
                {
                    result(id, text_result(native_edit_response(active_document())))
                }
                Some("app_get_native_edit_state") => error(id, -32602, "unexpected arguments"),
                Some("app_list_annotations") => {
                    let page = if args.is_null() || args.as_object().is_some_and(|o| o.is_empty()) {
                        Ok(None)
                    } else if args
                        .as_object()
                        .is_some_and(|o| o.len() == 1 && o.contains_key("page"))
                    {
                        args.get("page")
                            .and_then(Value::as_u64)
                            .filter(|page| *page > 0)
                            .and_then(|page| usize::try_from(page).ok())
                            .map(Some)
                            .ok_or("invalid page")
                    } else {
                        Err("unexpected arguments")
                    };
                    match page {
                        Err(message) => error(id, -32602, message),
                        Ok(page) => match active_document() {
                            None => result(
                                id,
                                text_result(json!({"ok":false,"error":"no active document"})),
                            ),
                            Some(document) => match annotation_response(document, page) {
                                Ok(payload) => result(id, text_result(payload)),
                                Err(message) => error(id, -32603, message),
                            },
                        },
                    }
                }
                Some("app_get_annotation") => {
                    let annotation_id = args
                        .as_object()
                        .filter(|args| args.len() == 1)
                        .and_then(|args| args.get("id"))
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty() && id.len() <= 64);
                    match annotation_id {
                        None => error(id, -32602, "missing or invalid 'id'"),
                        Some(annotation_id) => match active_document() {
                            None => result(
                                id,
                                text_result(json!({"ok":false,"error":"no active document"})),
                            ),
                            Some(document) => match annotation_detail(document, annotation_id) {
                                Ok(payload) => result(id, text_result(payload)),
                                Err(message) => error(id, -32603, message),
                            },
                        },
                    }
                }
                Some("app_delete_annotation") => match delete_arguments(args) {
                    Some((expected, annotation_id)) => {
                        match dispatch_command(expected, ExternalAction::Delete(annotation_id))
                            .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version or annotation id",
                    ),
                },
                Some("app_move_annotation") => match move_annotation_arguments(args) {
                    Some((expected, id_to_move, delta)) => {
                        match dispatch_command(
                            expected,
                            ExternalAction::MoveAnnotation {
                                id: id_to_move,
                                delta,
                            },
                        )
                        .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version, id or delta",
                    ),
                },
                Some("app_resize_annotation") => match resize_annotation_arguments(args) {
                    Some((expected, id_to_resize, bounds)) => {
                        match dispatch_command(
                            expected,
                            ExternalAction::ResizeAnnotation {
                                id: id_to_resize,
                                bounds,
                            },
                        )
                        .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version, id or bounds",
                    ),
                },
                Some("app_clear_annotations") => match clear_annotations_arguments(args) {
                    Some((expected, page)) => {
                        match dispatch_command(expected, ExternalAction::ClearAnnotations(page))
                            .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version, scope or page",
                    ),
                },
                Some("app_undo" | "app_redo") => match version_arguments(args) {
                    Some(expected) => {
                        let action = if params["name"] == "app_undo" {
                            ExternalAction::Undo
                        } else {
                            ExternalAction::Redo
                        };
                        match dispatch_command(expected, action).await {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(id, -32602, "missing or invalid document version"),
                },
                Some("app_select_annotation") => match select_arguments(args) {
                    Some((expected, annotation_id)) => {
                        match dispatch_command(expected, ExternalAction::Select(annotation_id))
                            .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version or annotation id",
                    ),
                },
                Some("app_clear_selection") => match version_arguments(args) {
                    Some(expected) => {
                        match dispatch_command(expected, ExternalAction::ClearSelection).await {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(id, -32602, "missing or invalid document version"),
                },
                Some("app_select_native_renderer") => match select_backend_arguments(args) {
                    Some((expected, old, requested)) => {
                        match dispatch_command(
                            expected,
                            ExternalAction::SelectBackend {
                                expected: old,
                                requested,
                            },
                        )
                        .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version or renderer",
                    ),
                },
                Some("app_go_to_page") => match go_to_page_arguments(args) {
                    Some((expected, expected_page, page)) => {
                        match dispatch_command(
                            expected,
                            ExternalAction::GoToPage {
                                expected_page,
                                page,
                            },
                        )
                        .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(id, -32602, "missing or invalid document version or page"),
                },
                Some("app_fit_view") => match fit_view_arguments(args) {
                    Some((expected, expected_zoom, mode)) => {
                        match dispatch_command(
                            expected,
                            ExternalAction::FitView {
                                expected_zoom,
                                mode,
                            },
                        )
                        .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(
                        id,
                        -32602,
                        "missing or invalid document version, zoom or mode",
                    ),
                },
                Some("app_set_tool") => match select_tool_arguments(args) {
                    Some((expected, old, old_epoch, requested)) => {
                        match dispatch_command(
                            expected,
                            ExternalAction::SelectTool {
                                expected: old,
                                expected_epoch: old_epoch,
                                requested,
                            },
                        )
                        .await
                        {
                            Ok(payload) => result(id, text_result(payload)),
                            Err(message) => error(id, -32603, message),
                        }
                    }
                    None => error(id, -32602, "missing or invalid document version or tool"),
                },
                Some("list_test_pdfs") => error(id, -32602, "unexpected arguments"),
                _ => error(id, -32602, "tool is not available in the Rust app"),
            }
        }
        Some(method) => error(id, -32601, format!("method not found: {method}")),
        None => error(id, -32600, "missing method"),
    };
    (StatusCode::OK, Json(response))
}

fn read_pdf(path: &Path) -> Result<lopdf::Document, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_PDF_BYTES {
        return Err("PDF is not a regular file within the 64 MiB limit".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PDF_BYTES {
        return Err("PDF exceeds the 64 MiB limit".into());
    }
    lopdf::Document::load_mem(&bytes).map_err(|error| format!("parse: {error}"))
}

fn list_test_pdfs(corpus: &Path) -> Result<Value, String> {
    let mut pdfs = Vec::new();
    let entries = std::fs::read_dir(corpus).map_err(|error| error.to_string())?;
    for entry in entries.take(MAX_CORPUS_ENTRIES + 1) {
        if pdfs.len() == MAX_CORPUS_ENTRIES {
            return Err("test corpus has too many entries".into());
        }
        let path = entry.map_err(|error| error.to_string())?.path();
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        {
            continue;
        }
        let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() > MAX_PDF_BYTES {
            continue;
        }
        let page_count = read_pdf(&path)
            .map(|pdf| pdf.get_pages().len())
            .unwrap_or(0);
        pdfs.push(
            json!({"path":path.to_string_lossy(),"page_count":page_count,
                         "file_size":metadata.len()}),
        );
    }
    pdfs.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({"pdfs":pdfs}))
}

fn get_pdf_metadata(path: &Path) -> Result<Value, String> {
    let pdf = read_pdf(path)?;
    let info = pdf
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|obj| pdf.dereference(obj).ok())
        .and_then(|(_, obj)| obj.as_dict().ok());
    let field = |key: &[u8]| -> String {
        info.and_then(|dict| dict.get(key).ok())
            .and_then(|obj| obj.as_str().ok())
            .filter(|bytes| bytes.len() <= 4096)
            .and_then(|bytes| {
                lopdf::decode_text_string(&lopdf::Object::String(
                    bytes.to_vec(),
                    lopdf::StringFormat::Literal,
                ))
                .ok()
            })
            .unwrap_or_default()
    };
    let mut pages = Vec::new();
    for (index, (_, id)) in pdf.get_pages().iter().enumerate() {
        let dict = pdf.get_object(*id).ok().and_then(|obj| obj.as_dict().ok());
        let mediabox: Vec<f32> = dict
            .and_then(|dict| dict.get(b"MediaBox").ok())
            .and_then(|obj| obj.as_array().ok())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| match value {
                        lopdf::Object::Integer(n) => Some(*n as f32),
                        lopdf::Object::Real(n) => Some(*n),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let rotation = dict
            .and_then(|dict| dict.get(b"Rotate").ok())
            .and_then(|obj| obj.as_i64().ok())
            .unwrap_or(0);
        pages.push(json!({"index":index,"mediabox":mediabox,"rotation":rotation}));
    }
    Ok(json!({"pdf_version":pdf.version,"page_count":pages.len(),
              "producer":field(b"Producer"),"creator":field(b"Creator"),"pages":pages}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn only_local_host_and_origin_are_accepted() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("localhost:9223"));
        assert!(local_request(&headers, 9223));
        headers.insert("origin", HeaderValue::from_static("https://example.com"));
        assert!(!local_request(&headers, 9223));
        headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:9223"));
        assert!(local_request(&headers, 9223));
        headers.insert("host", HeaderValue::from_static("example.com:9223"));
        assert!(!local_request(&headers, 9223));
    }

    #[test]
    fn only_implemented_tools_are_advertised() {
        let value = tools();
        let names: Vec<_> = value["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            [
                "list_test_pdfs",
                "get_pdf_metadata",
                "app_get_page_count",
                "app_go_to_page",
                "app_fit_view",
                "app_get_current_tool",
                "app_set_tool",
                "app_get_native_renderers",
                "app_list_tabs",
                "app_get_native_view_state",
                "app_get_native_selection",
                "app_get_native_edit_state",
                "app_list_annotations",
                "app_get_annotation",
                "app_delete_annotation",
                "app_move_annotation",
                "app_resize_annotation",
                "app_clear_annotations",
                "app_undo",
                "app_redo",
                "app_select_annotation",
                "app_clear_selection",
                "app_select_native_renderer"
            ]
        );
    }

    #[test]
    fn go_to_page_requires_exact_version_and_two_positive_pages() {
        let valid = json!({"documentId":"7","generation":3,"annotationRevision":2,
            "expectedPage":1,"page":2});
        assert_eq!(go_to_page_arguments(&valid), Some(([7, 3, 2], 0, 1)));
        for bad in [
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedPage":0,"page":2}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedPage":1,"page":0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedPage":1,"page":2,"extra":true}),
        ] {
            assert_eq!(go_to_page_arguments(&bad), None);
        }
    }

    #[test]
    fn fit_view_requires_exact_version_zoom_and_mode() {
        let valid = json!({"documentId":"7","generation":3,"annotationRevision":2,
            "expectedZoom":1.25,"mode":"width"});
        assert_eq!(fit_view_arguments(&valid), Some(([7, 3, 2], 1.25, "width")));
        for bad in [
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedZoom":0.0,"mode":"width"}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedZoom":1.0,"mode":"unknown"}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedZoom":1.0,"mode":"page","extra":true}),
        ] {
            assert_eq!(fit_view_arguments(&bad), None);
        }
    }

    #[test]
    fn move_annotation_accepts_only_bounded_page_point_deltas() {
        let valid = json!({"documentId":"7","generation":3,"annotationRevision":2,
            "id":"0","dx":10.5,"dy":-2.0});
        assert_eq!(
            move_annotation_arguments(&valid),
            Some(([7, 3, 2], 0, [10.5, -2.0]))
        );
        for bad in [
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","dx":1000001.0,"dy":0.0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","dx":"1","dy":0.0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","dx":1.0,"dy":0.0,"extra":true}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"-1","dx":1.0,"dy":0.0}),
        ] {
            assert_eq!(move_annotation_arguments(&bad), None);
        }
    }

    #[test]
    fn resize_annotation_accepts_only_bounded_positive_page_point_rectangles() {
        let valid = json!({"documentId":"7","generation":3,"annotationRevision":2,
            "id":"0","x":10.5,"y":2.0,"width":40.0,"height":30.0});
        assert_eq!(
            resize_annotation_arguments(&valid),
            Some((
                [7, 3, 2],
                0,
                Rect {
                    x: 10.5,
                    y: 2.0,
                    width: 40.0,
                    height: 30.0
                }
            ))
        );
        for bad in [
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","x":-1.0,"y":2.0,"width":40.0,"height":30.0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","x":1.0,"y":2.0,"width":0.0,"height":30.0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","x":1.0,"y":2.0,"width":40.0,"height":1000001.0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","x":"1","y":2.0,"width":40.0,"height":30.0}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"id":"0","x":1.0,"y":2.0,"width":40.0,"height":30.0,"extra":true}),
        ] {
            assert_eq!(resize_annotation_arguments(&bad), None);
        }
    }

    #[test]
    fn set_tool_accepts_only_implemented_names_and_exact_version() {
        let valid = json!({"documentId":"7","generation":3,"annotationRevision":2,
            "expectedTool":"select","expectedToolEpoch":4,"tool":"ellipse"});
        assert_eq!(
            select_tool_arguments(&valid),
            Some(([7, 3, 2], "select", 4, "ellipse"))
        );
        for bad in [
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedTool":"select","expectedToolEpoch":4,"tool":"stamp"}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedTool":"select","expectedToolEpoch":4,"tool":"ellipse","extra":true}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"tool":"ellipse"}),
            json!({"documentId":"7","generation":3,"annotationRevision":2,"expectedTool":"select","tool":"ellipse"}),
        ] {
            assert_eq!(select_tool_arguments(&bad), None);
        }
    }

    #[test]
    fn delete_arguments_require_an_exact_version_and_id() {
        let valid = json!({"documentId":"12","generation":7,
            "annotationRevision":3,"id":"99"});
        assert_eq!(delete_arguments(&valid), Some(([12, 7, 3], 99)));
        assert_eq!(
            delete_arguments(&json!({"documentId":"12","generation":7,
            "annotationRevision":3,"id":"0"})),
            Some(([12, 7, 3], 0))
        );
        assert_eq!(
            delete_arguments(&json!({"documentId":"12","id":"99"})),
            None
        );
        assert_eq!(
            delete_arguments(&json!({"documentId":"0","generation":7,
            "annotationRevision":3,"id":"99"})),
            None
        );
        assert_eq!(
            delete_arguments(&json!({"documentId":"12","generation":7,
                "annotationRevision":3,"id":"99","extra":true})),
            None
        );
        assert_eq!(
            version_arguments(&json!({"documentId":"12","generation":7,
            "annotationRevision":3})),
            Some([12, 7, 3])
        );
        assert_eq!(version_arguments(&valid), None);
        assert_eq!(select_arguments(&valid), Some(([12, 7, 3], 99)));
        assert_eq!(select_arguments(&json!({"id":"99"})), None);
        let clear_page = json!({"documentId":"12","generation":7,
            "annotationRevision":3,"scope":"page","page":2});
        assert_eq!(
            clear_annotations_arguments(&clear_page),
            Some(([12, 7, 3], Some(1)))
        );
        assert_eq!(
            clear_annotations_arguments(&json!({"documentId":"12","generation":7,
            "annotationRevision":3,"scope":"all"})),
            Some(([12, 7, 3], None))
        );
        for invalid in [
            json!({"documentId":"12","generation":7,"annotationRevision":3,"scope":"page"}),
            json!({"documentId":"12","generation":7,"annotationRevision":3,"scope":"page","page":0}),
            json!({"documentId":"12","generation":7,"annotationRevision":3,"scope":"all","page":1}),
            json!({"documentId":"12","generation":7,"annotationRevision":3,"scope":"all","extra":true}),
        ] {
            assert_eq!(clear_annotations_arguments(&invalid), None);
        }
        let renderer = json!({"documentId":"12","generation":7,"annotationRevision":3,
            "expectedRenderer":"hayro","renderer":"dynlex"});
        assert_eq!(
            select_backend_arguments(&renderer),
            Some(([12, 7, 3], Backend::Hayro, Backend::Dynlex))
        );
        assert_eq!(
            select_backend_arguments(&json!({"renderer":"dynlex"})),
            None
        );
        assert_eq!(
            select_backend_arguments(&json!({"documentId":"12","generation":7,
            "annotationRevision":3,"expectedRenderer":"hayro","renderer":"unknown"})),
            None
        );
        assert_eq!(backend_id(Backend::PdfCraft), "pdfcraft");
    }

    #[test]
    fn native_renderer_list_reports_selection_and_real_feature_availability() {
        let response = native_renderers_response(Backend::Dynlex);
        assert_eq!(response["selected"], "dynlex");
        let renderers = response["renderers"].as_array().unwrap();
        assert_eq!(renderers.len(), 5);
        assert_eq!(renderers[0]["id"], "hayro");
        assert_eq!(renderers[0]["available"], true);
        assert_eq!(
            renderers[1]["available"],
            cfg!(feature = "dynlex-experimental")
        );
        assert_eq!(
            renderers[2]["available"],
            opds_dynlex_backend::pdfium_available()
        );
        assert_eq!(
            renderers[3]["available"],
            opds_dynlex_backend::own_rust_available()
        );
        assert_eq!(
            renderers[4]["available"],
            opds_dynlex_backend::pdfcraft_available()
        );
    }

    #[test]
    fn tab_snapshot_reports_one_document_and_ignores_stale_view_state() {
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(31), 2).unwrap();
        let mut snapshot = ActiveDocument::from_model(&model);
        snapshot.update_tab_state(&model, "/tmp/one.pdf", false, true, None);
        let tabs = tab_response(Some(snapshot.clone()));
        assert_eq!(tabs["activeIndex"], 0);
        assert_eq!(tabs["tabs"][0]["fileName"], "one.pdf");
        assert_eq!(tabs["tabs"][0]["modified"], true);
        assert_eq!(tabs["tabs"][0]["pageCount"], 2);
        assert_eq!(tabs["tabs"][0]["annotationCount"], 0);
        model.go_to_page(1).unwrap();
        snapshot.update_tab_state(&model, "/tmp/stale.pdf", false, false, None);
        assert_eq!(snapshot.file_path.as_deref(), Some("/tmp/one.pdf"));
        assert!(snapshot.modified);
        let mut current = ActiveDocument::from_model(&model);
        current.update_tab_state(&model, "/tmp/two.pdf", false, false, None);
        assert_eq!(tab_response(Some(current))["tabs"][0]["currentPage"], 2);
        assert_eq!(tab_response(None)["tabs"], json!([]));
    }

    #[test]
    fn in_memory_new_pdf_is_untitled_until_saved() {
        use opds_native_core::DocumentId;
        let model = DocumentModel::new(DocumentId(90), 1).unwrap();
        let mut snapshot = ActiveDocument::from_model(&model);
        snapshot.update_tab_state(&model, "Untitled.pdf", true, false, None);
        let response = tab_response(Some(snapshot.clone()));
        let tab = &response["tabs"][0];
        assert_eq!(tab["fileName"], "Untitled.pdf");
        assert!(tab["filePath"].is_null());
        assert_eq!(tab["isUntitled"], true);
        snapshot.update_tab_state(&model, "/tmp/saved.pdf", false, false, None);
        let response = tab_response(Some(snapshot));
        let tab = &response["tabs"][0];
        assert_eq!(tab["filePath"], "/tmp/saved.pdf");
        assert_eq!(tab["isUntitled"], false);
    }

    #[test]
    fn native_view_snapshot_follows_zoom_and_rejects_stale_generation() {
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(32), 2).unwrap();
        let stale = ActiveDocument::from_model(&model);
        assert_eq!(native_view_response(None)["ok"], false);
        assert_eq!(native_view_response(Some(stale.clone()))["scale"], 1.0);
        model.zoom_at(2.0, [10.0, 20.0]).unwrap();
        let current = ActiveDocument::from_model(&model);
        let response = native_view_response(Some(current.clone()));
        assert_eq!(response["documentId"], "32");
        assert_eq!(response["scale"], 2.0);
        assert_eq!(response["offsetX"], -10.0);
        assert_eq!(response["offsetY"], -20.0);
        assert!(response["pageWidthPoints"].is_null());
        let mut sized = current.clone();
        sized.update_page_geometry(&model, Some((595.0, 842.0)));
        let response = native_view_response(Some(sized.clone()));
        assert_eq!(response["pageWidthPoints"], 595.0);
        assert_eq!(response["pageHeightPoints"], 842.0);
        model.go_to_page(1).unwrap();
        sized.update_page_geometry(&model, Some((100.0, 200.0)));
        assert_eq!(sized.page_size_points, Some([595.0, 842.0]));
        let mut next = ActiveDocument::from_model(&model);
        next.update_page_geometry(&model, Some((f32::NAN, 200.0)));
        assert!(next.page_size_points.is_none());
        assert!(!accepts_new_snapshot(&current, &stale));
    }

    #[test]
    fn native_selection_tracks_live_choice_and_rejects_stale_or_unknown_ids() {
        use opds_native_core::annotations::{Annotation, Edit, Rect};
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(33), 1).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 87,
                page: 0,
                bounds: Rect {
                    x: 10.,
                    y: 20.,
                    width: 30.,
                    height: 40.,
                },
                kind: Kind::Rectangle {
                    color_rgb: [1, 2, 3],
                },
            }))
            .unwrap();
        let mut snapshot = ActiveDocument::from_model(&model);
        snapshot.update_tab_state(&model, "/tmp/selected.pdf", false, false, Some(87));
        assert_eq!(
            native_selection_response(Some(snapshot.clone()))["selectedId"],
            "87"
        );
        snapshot.update_tab_state(&model, "/tmp/selected.pdf", false, false, Some(999));
        assert!(native_selection_response(Some(snapshot.clone()))["selectedId"].is_null());
        snapshot.update_tab_state(&model, "/tmp/selected.pdf", false, false, Some(87));
        model.zoom_at(2.0, [0., 0.]).unwrap();
        snapshot.update_tab_state(&model, "/tmp/stale.pdf", false, false, None);
        assert_eq!(
            native_selection_response(Some(snapshot))["selectedId"],
            "87"
        );
        assert_eq!(native_selection_response(None)["ok"], false);
    }

    #[test]
    fn native_edit_state_tracks_undo_redo_and_document_version() {
        use opds_native_core::annotations::{Annotation, DocumentCommand, Edit, Rect};
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(34), 1).unwrap();
        assert_eq!(native_edit_response(None)["ok"], false);
        let original = ActiveDocument::from_model(&model);
        assert_eq!(
            native_edit_response(Some(original.clone()))["canUndo"],
            false
        );
        let expected = model.version();
        model
            .execute(
                expected,
                DocumentCommand::Edit(Edit::Add(Annotation {
                    id: 1,
                    page: 0,
                    bounds: Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 30.0,
                        height: 40.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: [1, 2, 3],
                    },
                })),
            )
            .unwrap();
        let mut edited = ActiveDocument::from_model(&model);
        edited.update_tab_state(&model, "/tmp/edited.pdf", false, true, Some(1));
        let response = native_edit_response(Some(edited.clone()));
        assert_eq!(response["documentId"], "34");
        assert_eq!(response["modified"], true);
        assert_eq!(response["canUndo"], true);
        assert_eq!(response["canRedo"], false);
        assert!(!accepts_new_snapshot(&edited, &original));
        model
            .execute(model.version(), DocumentCommand::Undo)
            .unwrap();
        let undone = ActiveDocument::from_model(&model);
        let response = native_edit_response(Some(undone));
        assert_eq!(response["canUndo"], false);
        assert_eq!(response["canRedo"], true);
    }

    #[test]
    fn annotation_snapshot_tracks_model_and_page_filter() {
        use opds_native_core::annotations::{Annotation, Edit, Rect};
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(91), 2).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 7,
                page: 1,
                bounds: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 30.0,
                    height: 40.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [0xab, 0xcd, 0xef],
                },
            }))
            .unwrap();
        let snapshot = ActiveDocument::from_model(&model);
        assert_eq!(snapshot.id, 91);
        assert_eq!(snapshot.generation, model.view().generation());
        assert_eq!(snapshot.annotation_revision, 1);
        assert_eq!(
            annotation_response(snapshot.clone(), Some(1)).unwrap()["count"],
            0
        );
        let page_two = annotation_response(snapshot, Some(2)).unwrap();
        assert_eq!(page_two["count"], 1);
        assert_eq!(page_two["annotations"][0]["id"], "7");
        assert_eq!(page_two["annotations"][0]["type"], "box");
        assert_eq!(page_two["annotations"][0]["color"], "#abcdef");
        model.undo().unwrap();
        assert_eq!(model.annotation_revision(), 2);
        assert_eq!(
            annotation_response(ActiveDocument::from_model(&model), None).unwrap()["count"],
            0
        );
    }

    #[test]
    fn annotation_detail_keeps_full_native_text_and_points() {
        use opds_native_core::annotations::{Annotation, Edit, Rect};
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(92), 1).unwrap();
        let long_text = "é".repeat(250);
        model
            .apply(Edit::Add(Annotation {
                id: 8,
                page: 0,
                bounds: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 30.0,
                    height: 40.0,
                },
                kind: Kind::Text {
                    value: long_text.clone(),
                    color_rgb: [1, 2, 3],
                    font_size: 16,
                    align: TextAlign::Right,
                },
            }))
            .unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 9,
                page: 0,
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                kind: Kind::Polyline {
                    points: vec![[0.0, 0.0], [5.0, 7.0]],
                    color_rgb: [4, 5, 6],
                },
            }))
            .unwrap();
        let snapshot = ActiveDocument::from_model(&model);
        assert_eq!(
            annotation_response(snapshot.clone(), None).unwrap()["annotations"][0]["text"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            200
        );
        let text = annotation_detail(snapshot.clone(), "8").unwrap();
        assert_eq!(text["annotation"]["text"], long_text);
        assert_eq!(text["annotation"]["fontSize"], 16);
        assert_eq!(text["annotation"]["align"], "right");
        assert_eq!(
            annotation_detail(snapshot.clone(), "9").unwrap()["annotation"]["points"][1],
            json!({"x":5.0,"y":7.0})
        );
        assert_eq!(annotation_detail(snapshot, "missing").unwrap()["ok"], false);
    }

    #[test]
    fn oversized_annotation_detail_is_rejected_without_losing_the_list() {
        use opds_native_core::annotations::{Annotation, Edit, Rect};
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(93), 1).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 10,
                page: 0,
                bounds: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 30.0,
                    height: 40.0,
                },
                kind: Kind::Text {
                    value: "\n".repeat(700_000),
                    color_rgb: [0, 0, 0],
                    font_size: 16,
                    align: TextAlign::Left,
                },
            }))
            .unwrap();
        let snapshot = ActiveDocument::from_model(&model);
        assert_eq!(
            annotation_response(snapshot.clone(), None).unwrap()["count"],
            1
        );
        assert!(annotation_detail(snapshot, "10")
            .unwrap_err()
            .contains("4 MiB"));
    }

    #[test]
    fn stale_snapshot_cannot_replace_same_document_generation() {
        use opds_native_core::annotations::{Annotation, Edit, Rect};
        use opds_native_core::DocumentId;
        let mut model = DocumentModel::new(DocumentId(23), 3).unwrap();
        let stale = ActiveDocument::from_model(&model);
        model
            .apply(Edit::Add(Annotation {
                id: 1,
                page: 0,
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [0, 0, 0],
                },
            }))
            .unwrap();
        let edited = ActiveDocument::from_model(&model);
        assert_eq!(edited.generation, stale.generation);
        assert!(!accepts_new_snapshot(&edited, &stale));
        assert!(accepts_new_snapshot(&stale, &edited));
        model.go_to_page(1).unwrap();
        let current = ActiveDocument::from_model(&model);
        assert!(!accepts_new_snapshot(&current, &stale));
        assert!(accepts_new_snapshot(&stale, &current));
        let other = ActiveDocument::from_model(&DocumentModel::new(DocumentId(24), 1).unwrap());
        assert!(accepts_new_snapshot(&current, &other));
    }

    #[test]
    fn metadata_decodes_pdf_text_strings() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../test pdf-bestanden/Originele bestanden/Tekst.pdf");
        let value = get_pdf_metadata(&path).unwrap();
        assert_eq!(value["page_count"], 5);
        assert_eq!(value["pdf_version"], "1.7");
        assert_eq!(value["creator"], "Writer");
        assert!(value["producer"]
            .as_str()
            .unwrap()
            .starts_with("LibreOffice"));
    }

    /// Run alone with `cargo test -p opds-native-app --all-features
    /// mcp_server::tests::live_http_annotation_read -- --ignored` because
    /// other App tests may publish to the process-wide document snapshot.
    #[test]
    #[ignore]
    fn live_http_annotation_read() {
        use opds_native_core::annotations::{Annotation, Edit, Rect};
        use opds_native_core::DocumentId;
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let mut model = DocumentModel::new(DocumentId(9_001), 2).unwrap();
        model
            .apply(Edit::Add(Annotation {
                id: 87,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [1, 2, 3],
                },
            }))
            .unwrap();
        publish_model(&model);
        publish_tab_state(&model, "/tmp/native-live.pdf", false, true, Some(87));
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        start(port).unwrap();
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"app_list_annotations","arguments":{"page":1}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["annotations"][0]["id"], "87");
        assert_eq!(payload["annotations"][0]["color"], "#010203");

        let body = json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"app_get_annotation","arguments":{"id":"87"}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["annotation"]["id"], "87");
        assert_eq!(payload["annotation"]["type"], "box");

        publish_tool("polyline", 7);
        let body = json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"app_get_current_tool","arguments":{}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload, json!({"ok":true,"tool":"polyline","toolEpoch":7}));
        let body = json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"app_list_tabs","arguments":{}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["tabs"][0]["fileName"], "native-live.pdf");
        assert_eq!(payload["tabs"][0]["modified"], true);
        assert_eq!(payload["tabs"][0]["annotationCount"], 1);
        let body = json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
            "params":{"name":"app_get_native_view_state","arguments":{}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["currentPage"], 1);
        assert_eq!(payload["scale"], 1.0);
        assert_eq!(payload["offsetX"], 0.0);
        let body = json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":"app_get_native_selection","arguments":{}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["selectedId"], "87");
        let body = json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"app_get_native_edit_state","arguments":{}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["documentId"], "9001");
        assert_eq!(payload["modified"], true);
        assert_eq!(payload["canUndo"], true);
        assert_eq!(payload["canRedo"], false);
        publish_backend(Backend::Hayro);
        let body = json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
            "params":{"name":"app_get_native_renderers","arguments":{}}})
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
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let result: Value = serde_json::from_slice(&response[split..]).unwrap();
        let payload: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["selected"], "hayro");
        assert_eq!(payload["renderers"].as_array().unwrap().len(), 5);
        publish_tool("select", 0);
    }
}
