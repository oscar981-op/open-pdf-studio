//! Small, bounded native recent-file store. Detected final-file symlinks are rejected.
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_ENTRIES: usize = 20;
const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_PATH_BYTES: usize = 4096;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFile {
    pub path: String,
    pub pinned: bool,
}

pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local/state"))
        })?;
    Some(base.join("open-pdf-studio-rust/recent-files.json"))
}

pub fn read(path: &Path) -> VecDeque<RecentFile> {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return VecDeque::new();
    };
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return VecDeque::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return VecDeque::new();
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return VecDeque::new();
    }
    let Ok(paths) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else {
        return VecDeque::new();
    };
    let mut recent = VecDeque::new();
    for value in paths {
        // Accept the earlier string-only native store while upgrading to
        // pinned entries; unknown or malformed records are ignored.
        let (path, pinned) = match value {
            serde_json::Value::String(path) => (path, false),
            serde_json::Value::Object(fields) => {
                let Some(path) = fields.get("path").and_then(|value| value.as_str()) else {
                    continue;
                };
                let Some(pinned) = fields.get("pinned").and_then(|value| value.as_bool()) else {
                    continue;
                };
                (path.to_owned(), pinned)
            }
            _ => continue,
        };
        if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
            continue;
        }
        if !recent.iter().any(|entry: &RecentFile| entry.path == path) {
            recent.push_back(RecentFile { path, pinned });
            if recent.len() == MAX_ENTRIES {
                break;
            }
        }
    }
    recent
}

pub fn write(path: &Path, recent: &VecDeque<RecentFile>) -> io::Result<()> {
    if recent.len() > MAX_ENTRIES
        || recent.iter().any(|entry| {
            entry.path.is_empty() || entry.path.len() > MAX_PATH_BYTES || entry.path.contains('\0')
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid recent-file list",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    std::fs::create_dir_all(parent)?;
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recent-file store is not a regular file",
            ));
        }
    }
    let value: Vec<_> = recent
        .iter()
        .map(|entry| serde_json::json!({"path":entry.path,"pinned":entry.pinned}))
        .collect();
    let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "recent-file store is too large",
        ));
    }
    let mut temp_name = OsString::from(path.as_os_str());
    temp_name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(temp_name);
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "opds-recent-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn bounded_roundtrip_and_corrupt_file_rejection() {
        let dir = test_dir();
        let path = dir.join("state/recent.json");
        let recent = VecDeque::from([
            RecentFile {
                path: "/tmp/a.pdf".into(),
                pinned: true,
            },
            RecentFile {
                path: "/tmp/b.pdf".into(),
                pinned: false,
            },
        ]);
        write(&path, &recent).unwrap();
        assert_eq!(read(&path), recent);
        std::fs::write(&path, br#"["/tmp/old.pdf"]"#).unwrap();
        assert_eq!(
            read(&path).front().unwrap(),
            &RecentFile {
                path: "/tmp/old.pdf".into(),
                pinned: false,
            }
        );
        std::fs::write(&path, b"{broken").unwrap();
        assert!(read(&path).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_store_does_not_modify_target() {
        use std::os::unix::fs::symlink;
        let dir = test_dir();
        let target = dir.join("target.json");
        std::fs::write(&target, b"keep").unwrap();
        let link = dir.join("recent.json");
        symlink(&target, &link).unwrap();
        let recent = VecDeque::from([RecentFile {
            path: "/tmp/a.pdf".into(),
            pinned: false,
        }]);
        assert!(read(&link).is_empty());
        assert!(write(&link, &recent).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
