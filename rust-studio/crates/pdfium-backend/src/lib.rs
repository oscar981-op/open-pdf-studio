//! Optional PDFium renderer called through PDFium's public C ABI from Rust.
//! No PDFium binary is bundled. Set `OPDS_PDFIUM_LIBRARY` to a trusted
//! `libpdfium.so`, or install one on the system library search path.
use libloading::Library;
use opds_engine_probe::Raster;
use std::{
    ffi::c_void,
    ptr,
    sync::{Mutex, OnceLock},
    time::Instant,
};

type Handle = *mut c_void;
type Close = unsafe extern "C" fn(Handle);

struct OwnedHandle {
    raw: Handle,
    close: Close,
}

impl OwnedHandle {
    fn new(raw: Handle, close: Close) -> Result<Self, String> {
        (!raw.is_null())
            .then_some(Self { raw, close })
            .ok_or_else(|| "PDFium returned a null handle".into())
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: Every handle is returned by PDFium and closed exactly once
        // using the matching function while the library is still loaded.
        unsafe { (self.close)(self.raw) }
    }
}

struct Api {
    _library: Library,
    load_document: unsafe extern "C" fn(*const c_void, i32, *const i8) -> Handle,
    close_document: Close,
    page_count: unsafe extern "C" fn(Handle) -> i32,
    load_page: unsafe extern "C" fn(Handle, i32) -> Handle,
    close_page: Close,
    page_width: unsafe extern "C" fn(Handle) -> f32,
    page_height: unsafe extern "C" fn(Handle) -> f32,
    bitmap_create: unsafe extern "C" fn(i32, i32, i32, *mut c_void, i32) -> Handle,
    bitmap_destroy: Close,
    bitmap_fill: unsafe extern "C" fn(Handle, i32, i32, i32, i32, u32) -> i32,
    bitmap_width: unsafe extern "C" fn(Handle) -> i32,
    bitmap_height: unsafe extern "C" fn(Handle) -> i32,
    bitmap_stride: unsafe extern "C" fn(Handle) -> i32,
    bitmap_buffer: unsafe extern "C" fn(Handle) -> *mut c_void,
    render_page: unsafe extern "C" fn(Handle, Handle, i32, i32, i32, i32, i32, i32),
}

impl Api {
    fn load() -> Result<Self, String> {
        let path = std::env::var_os("OPDS_PDFIUM_LIBRARY").unwrap_or_else(|| "libpdfium.so".into());
        // SAFETY: The library path is supplied by the local installation.
        // Symbol types below follow PDFium's public fpdfview.h ABI.
        let library = unsafe { Library::new(&path) }.map_err(|e| e.to_string())?;
        unsafe {
            macro_rules! symbol {
                ($name:literal, $type:ty) => {
                    *library
                        .get::<$type>(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
                };
            }
            let init = symbol!("FPDF_InitLibrary", unsafe extern "C" fn());
            let api = Self {
                load_document: symbol!(
                    "FPDF_LoadMemDocument",
                    unsafe extern "C" fn(*const c_void, i32, *const i8) -> Handle
                ),
                close_document: symbol!("FPDF_CloseDocument", Close),
                page_count: symbol!("FPDF_GetPageCount", unsafe extern "C" fn(Handle) -> i32),
                load_page: symbol!("FPDF_LoadPage", unsafe extern "C" fn(Handle, i32) -> Handle),
                close_page: symbol!("FPDF_ClosePage", Close),
                page_width: symbol!("FPDF_GetPageWidthF", unsafe extern "C" fn(Handle) -> f32),
                page_height: symbol!("FPDF_GetPageHeightF", unsafe extern "C" fn(Handle) -> f32),
                bitmap_create: symbol!(
                    "FPDFBitmap_CreateEx",
                    unsafe extern "C" fn(i32, i32, i32, *mut c_void, i32) -> Handle
                ),
                bitmap_destroy: symbol!("FPDFBitmap_Destroy", Close),
                bitmap_fill: symbol!(
                    "FPDFBitmap_FillRect",
                    unsafe extern "C" fn(Handle, i32, i32, i32, i32, u32) -> i32
                ),
                bitmap_width: symbol!("FPDFBitmap_GetWidth", unsafe extern "C" fn(Handle) -> i32),
                bitmap_height: symbol!("FPDFBitmap_GetHeight", unsafe extern "C" fn(Handle) -> i32),
                bitmap_stride: symbol!("FPDFBitmap_GetStride", unsafe extern "C" fn(Handle) -> i32),
                bitmap_buffer: symbol!(
                    "FPDFBitmap_GetBuffer",
                    unsafe extern "C" fn(Handle) -> *mut c_void
                ),
                render_page: symbol!(
                    "FPDF_RenderPageBitmap",
                    unsafe extern "C" fn(Handle, Handle, i32, i32, i32, i32, i32, i32)
                ),
                _library: library,
            };
            init();
            Ok(api)
        }
    }
}

static API: OnceLock<Result<Mutex<Api>, String>> = OnceLock::new();

fn api() -> Result<&'static Mutex<Api>, String> {
    API.get_or_init(|| Api::load().map(Mutex::new))
        .as_ref()
        .map_err(Clone::clone)
}

/// Whether a usable PDFium shared library was found at first use.
pub fn available() -> bool {
    api().is_ok()
}

/// Render a PDF page using PDFium. All PDFium calls are serialized because
/// its library state is process-wide. The input bytes outlive the document.
pub fn render(source: &[u8], page_number: usize, scale: f32) -> Result<Raster, String> {
    if source.is_empty() || source.len() > 128_000_000 || source.len() > i32::MAX as usize {
        return Err("PDFium input outside size limit".into());
    }
    if page_number == 0
        || page_number > i32::MAX as usize
        || !scale.is_finite()
        || !(0.01..=8.0).contains(&scale)
    {
        return Err("PDFium page or scale outside limit".into());
    }
    let api = api()?.lock().map_err(|_| "PDFium mutex poisoned")?;
    let started = Instant::now();
    // SAFETY: Every raw handle is checked and guarded; the source slice stays
    // alive until the document guard closes. PDFium is serialized by the mutex.
    unsafe {
        let document = OwnedHandle::new(
            (api.load_document)(source.as_ptr().cast(), source.len() as i32, ptr::null()),
            api.close_document,
        )?;
        let count = (api.page_count)(document.raw);
        if count <= 0 || page_number > count as usize {
            return Err("PDFium page not found".into());
        }
        let page = OwnedHandle::new(
            (api.load_page)(document.raw, page_number as i32 - 1),
            api.close_page,
        )?;
        let (page_width_pt, page_height_pt) =
            ((api.page_width)(page.raw), (api.page_height)(page.raw));
        let (w, h) = (
            page_width_pt as f64 * scale as f64,
            page_height_pt as f64 * scale as f64,
        );
        if !w.is_finite()
            || !h.is_finite()
            || w <= 0.0
            || h <= 0.0
            || w > 4096.0
            || h > 4096.0
            || w * h > 16_000_000.0
        {
            return Err("PDFium page exceeds bitmap limit".into());
        }
        let (width, height) = (w.ceil() as i32, h.ceil() as i32);
        // 4 is PDFium's BGRA format. PDFium owns this bitmap buffer.
        let bitmap = OwnedHandle::new(
            (api.bitmap_create)(width, height, 4, ptr::null_mut(), 0),
            api.bitmap_destroy,
        )?;
        if (api.bitmap_fill)(bitmap.raw, 0, 0, width, height, 0xffff_ffff) == 0 {
            return Err("PDFium bitmap fill failed".into());
        }
        // FPDF_ANNOT draws non-interactive annotation appearances.
        (api.render_page)(bitmap.raw, page.raw, 0, 0, width, height, 0, 0x01);
        let stride = (api.bitmap_stride)(bitmap.raw);
        let buffer = (api.bitmap_buffer)(bitmap.raw);
        if buffer.is_null()
            || (api.bitmap_width)(bitmap.raw) != width
            || (api.bitmap_height)(bitmap.raw) != height
            || stride < width * 4
            || stride > width * 4 + 256
        {
            return Err("PDFium returned an invalid bitmap layout".into());
        }
        let raw =
            std::slice::from_raw_parts(buffer.cast::<u8>(), stride as usize * height as usize);
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in raw.chunks_exact(stride as usize) {
            for bgra in row[..width as usize * 4].chunks_exact(4) {
                rgba.extend_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
            }
        }
        Ok(Raster {
            width: width as u32,
            height: height as u32,
            rgba,
            page_width_pt,
            page_height_pt,
            total_pages: count as usize,
            parse_ms: 0,
            render_ms: started.elapsed().as_millis(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opds_engine_probe::ParsedPdf;
    use std::sync::Arc;

    const PDF: &[u8] = include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf");

    #[test]
    fn rejects_invalid_requests_before_loading_pdfium() {
        assert!(render(PDF, 0, 1.0).is_err());
        assert!(render(PDF, 1, f32::NAN).is_err());
        assert!(render(&[], 1, 1.0).is_err());
    }

    #[test]
    fn renders_real_page_when_pdfium_is_installed() {
        if !available() {
            return;
        }
        let raster = render(PDF, 1, 1.0).unwrap();
        assert_eq!(raster.total_pages, 1);
        assert_eq!(
            raster.rgba.len(),
            raster.width as usize * raster.height as usize * 4
        );
        assert!(raster.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(raster
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [255; 3]));
        let hayro = ParsedPdf::open(Arc::new(PDF.to_vec()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert_eq!((raster.width, raster.height), (hayro.width, hayro.height));
        let differing_channels = raster
            .rgba
            .iter()
            .zip(&hayro.rgba)
            .filter(|(a, b)| a != b)
            .count();
        eprintln!("PDFium/Hayro fixture: {differing_channels} differing RGBA channels");
        assert!(render(PDF, 2, 1.0).is_err());
    }
}
