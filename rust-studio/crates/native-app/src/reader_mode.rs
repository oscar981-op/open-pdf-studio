//! Per-PDF reading position, compatible with the original app's sidecar.
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SIDECAR_BYTES: u64 = 4096;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// One-based PDF page number, as in the original app.
    pub page: usize,
    pub scale: f64,
    pub scroll_top: f32,
    pub scroll_height: f32,
    pub single_page: bool,
}

impl Position {
    fn valid(self) -> bool {
        self.page > 0
            && (0.1..=8.0).contains(&self.scale)
            && self.scale.is_finite()
            && self.scroll_top.is_finite()
            && self.scroll_top >= 0.0
            && self.scroll_height.is_finite()
            && self.scroll_height >= 0.0
    }

    pub fn restored_scroll_top(self, current_height: f32) -> f32 {
        if !self.single_page || self.scroll_height <= 0.0 || !current_height.is_finite() {
            return 0.0;
        }
        (self.scroll_top / self.scroll_height * current_height).clamp(0.0, current_height)
    }
}

pub fn sidecar_path(pdf: &Path) -> PathBuf {
    let mut name = OsString::from(pdf.as_os_str());
    name.push(".readerpos.json");
    PathBuf::from(name)
}

pub fn read(pdf: &Path) -> Option<Position> {
    let sidecar = sidecar_path(pdf);
    let mut bytes = Vec::new();
    std::fs::File::open(sidecar)
        .ok()?
        .take(MAX_SIDECAR_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SIDECAR_BYTES {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let page = usize::try_from(value.get("page")?.as_u64()?).ok()?;
    let scale = value.get("scale")?.as_f64()?;
    let scroll_top = value.get("scrollTop")?.as_f64()? as f32;
    let scroll_height = value.get("scrollHeight")?.as_f64()? as f32;
    let single_page = match value.get("viewMode")?.as_str()? {
        "single" => true,
        "continuous" => false,
        _ => return None,
    };
    let position = Position {
        page,
        scale,
        scroll_top,
        scroll_height,
        single_page,
    };
    position.valid().then_some(position)
}

pub fn write(pdf: &Path, position: Position) -> io::Result<()> {
    if !position.valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid reading position",
        ));
    }
    let view_mode = if position.single_page {
        "single"
    } else {
        "continuous"
    };
    let value = serde_json::json!({
        "page": position.page,
        "scale": position.scale,
        "scrollTop": position.scroll_top,
        "scrollHeight": position.scroll_height,
        "viewMode": view_mode,
    });
    let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    let sidecar = sidecar_path(pdf);
    let mut temp_name = OsString::from(sidecar.as_os_str());
    temp_name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(temp_name);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &sidecar)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub fn clear(pdf: &Path) -> io::Result<()> {
    match std::fs::remove_file(sidecar_path(pdf)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_original_sidecar_and_rejects_corruption() {
        let pdf = std::env::temp_dir().join(format!("opds-reader-{}.pdf", std::process::id()));
        let sidecar = sidecar_path(&pdf);
        let expected = Position {
            page: 4,
            scale: 1.25,
            scroll_top: 200.0,
            scroll_height: 1000.0,
            single_page: true,
        };
        write(&pdf, expected).unwrap();
        assert_eq!(read(&pdf), Some(expected));
        write(&pdf, expected).unwrap();
        assert_eq!(read(&pdf), Some(expected));
        assert_eq!(expected.restored_scroll_top(500.0), 100.0);
        std::fs::write(
            &sidecar,
            br#"{"page":4,"scale":1.25,"scrollTop":200,"scrollHeight":1000,"viewMode":"single","savedAt":123456789}"#,
        )
        .unwrap();
        assert_eq!(read(&pdf), Some(expected));
        std::fs::write(&sidecar, br#"{"page":4,"scale":null}"#).unwrap();
        assert_eq!(read(&pdf), None);
        std::fs::write(&sidecar, vec![b' '; 5000]).unwrap();
        assert_eq!(read(&pdf), None);
        clear(&pdf).unwrap();
        clear(&pdf).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn updating_a_sidecar_does_not_follow_a_symlink() {
        let pdf = std::env::temp_dir().join(format!("opds-reader-link-{}.pdf", std::process::id()));
        let target =
            std::env::temp_dir().join(format!("opds-reader-target-{}", std::process::id()));
        let sidecar = sidecar_path(&pdf);
        std::fs::write(&target, b"keep this file").unwrap();
        std::os::unix::fs::symlink(&target, &sidecar).unwrap();
        let position = Position {
            page: 2,
            scale: 1.0,
            scroll_top: 0.0,
            scroll_height: 0.0,
            single_page: true,
        };
        write(&pdf, position).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"keep this file");
        assert_eq!(read(&pdf), Some(position));
        clear(&pdf).unwrap();
        std::fs::remove_file(target).unwrap();
    }
}
