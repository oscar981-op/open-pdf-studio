//! Explicit, optional PdfCraft comparison route. PdfCraft 0.7.1 currently
//! rasterizes through Hayro/Vello; this never participates in DynLex.
use opds_engine_probe::Raster;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_SOURCE: usize = 128_000_000;
const MAX_PIXELS: usize = 8_000_000;
const TIMEOUT: Duration = Duration::from_secs(20);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub fn available() -> bool {
    executable().is_some()
}

fn executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("OPDS_PDFCRAFT_CLI") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    if let Some(path) = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|directory| directory.join("pdfcraft-cli"))
    }) {
        if path.is_file() {
            return Some(path);
        }
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join("pdfcraft-cli"))
        .find(|path| path.is_file())
}

struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "opds-pdfcraft-{}-{stamp}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parse_pam(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let length = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if length > (MAX_PIXELS * 4 + 256) as u64 {
        return Err("PdfCraft-uitvoer overschrijdt de bitmaplimiet".into());
    }
    let data = fs::read(path).map_err(|error| error.to_string())?;
    let end = data
        .windows(7)
        .take(256)
        .position(|window| window == b"ENDHDR\n")
        .map(|position| position + 7)
        .ok_or("PdfCraft leverde geen geldige PAM-header")?;
    let header = std::str::from_utf8(&data[..end]).map_err(|error| error.to_string())?;
    let mut lines = header.lines();
    if lines.next() != Some("P7") {
        return Err("PdfCraft leverde geen PAM P7".into());
    }
    let width = lines
        .next()
        .and_then(|line| line.strip_prefix("WIDTH "))
        .ok_or("PAM-breedte ontbreekt")?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let height = lines
        .next()
        .and_then(|line| line.strip_prefix("HEIGHT "))
        .ok_or("PAM-hoogte ontbreekt")?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    if lines.next() != Some("DEPTH 4")
        || lines.next() != Some("MAXVAL 255")
        || lines.next() != Some("TUPLTYPE RGB_ALPHA")
        || lines.next() != Some("ENDHDR")
        || lines.next().is_some()
    {
        return Err("PdfCraft leverde een ongeldige PAM-header".into());
    }
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or("PdfCraft-bitmapmaat overloopt")?;
    if count == 0 || count > MAX_PIXELS || data.len() - end != count * 4 {
        return Err("PdfCraft-bitmap heeft ongeldige lengte".into());
    }
    Ok((width, height, data[end..].to_vec()))
}

pub fn render(
    source: &[u8],
    page: usize,
    scale: f32,
    page_width_pt: f32,
    page_height_pt: f32,
    total_pages: usize,
) -> Result<Raster, String> {
    if source.is_empty()
        || source.len() > MAX_SOURCE
        || page == 0
        || page > 4096
        || !scale.is_finite()
        || !(0.01..=8.0).contains(&scale)
    {
        return Err("PdfCraft-invoer valt buiten de veiligheidslimiet".into());
    }
    let program = executable().ok_or("Stel OPDS_PDFCRAFT_CLI in op de PdfCraft-CLI")?;
    let started = Instant::now();
    let work = WorkDir::new()?;
    let input = work.path("input.pdf");
    let output = work.path("output.pam");
    let diagnostics = work.path("stderr.txt");
    fs::write(&input, source).map_err(|error| error.to_string())?;
    let stderr = fs::File::create(&diagnostics).map_err(|error| error.to_string())?;
    let mut command = Command::new(program);
    command
        .arg("render")
        .arg(&input)
        .arg("--page")
        .arg(page.to_string())
        .arg("--dpi")
        .arg((f64::from(scale) * 72.0).to_string())
        .arg("--out")
        .arg(&output)
        .stdout(Stdio::null())
        .stderr(stderr);
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let limits = [
                (libc::RLIMIT_AS, 1024 * 1024 * 1024),
                (libc::RLIMIT_CPU, 15),
                (libc::RLIMIT_FSIZE, 64 * 1024 * 1024),
            ];
            for (resource, value) in limits {
                let limit = libc::rlimit {
                    rlim_cur: value,
                    rlim_max: value,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= TIMEOUT {
            #[cfg(target_os = "linux")]
            unsafe {
                // The child was made a session leader in pre_exec, so kill
                // its whole process group if a future CLI spawns helpers.
                let _ = libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err("PdfCraft-rendering duurde te lang".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if fs::metadata(&diagnostics)
        .map_err(|error| error.to_string())?
        .len()
        > 8192
    {
        return Err("PdfCraft-diagnostiek overschrijdt de limiet".into());
    }
    let log = fs::read(&diagnostics).map_err(|error| error.to_string())?;
    if !status.success() || log.windows(8).any(|window| window == b"warning:") {
        return Err("PdfCraft weigerde de pagina of meldde onvolledige inhoud".into());
    }
    let (width, height, rgba) = parse_pam(&output)?;
    Ok(Raster {
        width,
        height,
        rgba,
        page_width_pt,
        page_height_pt,
        total_pages,
        parse_ms: 0,
        render_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pam_parser_accepts_only_exact_bounded_rgba() {
        let work = WorkDir::new().unwrap();
        let file = work.path("test.pam");
        let header = b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n";
        fs::write(&file, [header.as_slice(), &[1, 2, 3, 255]].concat()).unwrap();
        assert_eq!(parse_pam(&file).unwrap(), (1, 1, vec![1, 2, 3, 255]));
        fs::write(&file, [header.as_slice(), &[1, 2, 3]].concat()).unwrap();
        assert!(parse_pam(&file).is_err());
        fs::write(&file, b"P7\nWIDTH 1\nHEIGHT 1\nENDHDR\n\x01\x02\x03\xff").unwrap();
        assert!(parse_pam(&file).is_err());
    }
}
