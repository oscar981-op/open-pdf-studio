//! Linux resource-bounded text preparation for strict native DynLex dispatch.
//! This contains parser resource failures, not an OS security sandbox: no claim
//! of filesystem/network isolation. Only the known Rust probe may be launched.
use crate::{hybrid_page::PreparedHybridPage, winding_page::PreparedWindingPage};
use std::{
    error::Error,
    io::{Read, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
const INPUT_LIMIT: usize = 16_000_000;
const OUTPUT_LIMIT: usize = 32 * 1024 * 1024;
const STDERR_LIMIT: usize = 64 * 1024;
const ADDRESS_LIMIT: u64 = 512 * 1024 * 1024;
static WORKER: Mutex<()> = Mutex::new(());
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub enum PreparedScene {
    Text(PreparedWindingPage),
    Hybrid(PreparedHybridPage),
}
impl PreparedScene {
    pub fn render(&self) -> Result<crate::Bitmap> {
        match self {
            Self::Text(scene) => scene.render(),
            Self::Hybrid(scene) => scene.render(),
        }
    }
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Text(scene) => scene.retained_bytes(),
            Self::Hybrid(scene) => scene.retained_bytes(),
        }
    }
    pub fn paint_count(&self) -> usize {
        match self {
            Self::Text(scene) => scene.paint_count(),
            Self::Hybrid(scene) => scene.paint_count(),
        }
    }
    pub fn profile_replay(&self) -> Result<(crate::Bitmap, [f64; 4])> {
        match self {
            Self::Text(scene) => scene.profile_replay(),
            Self::Hybrid(_) => Err("text-only profile stages do not apply to hybrid scenes".into()),
        }
    }
}

/// Source-bound cache used by native dispatch. Recent page/zoom scenes share
/// one 32 MiB retained budget, so returning to a recent zoom need not launch
/// and parse a second worker. A new zoom still requires preparation.
pub struct TextDocument {
    source: std::sync::Arc<Vec<u8>>,
    worker: std::path::PathBuf,
    cache: Mutex<TextCache>,
}
#[derive(Default)]
struct TextCache {
    entries: std::collections::VecDeque<((u32, u32), std::result::Result<PreparedScene, String>)>,
    preparations: usize,
}
impl TextDocument {
    pub fn new(source: std::sync::Arc<Vec<u8>>, worker: std::path::PathBuf) -> Result<Self> {
        if source.is_empty() || source.len() > INPUT_LIMIT {
            return Err("text document source limit".into());
        }
        Ok(Self {
            source,
            worker,
            cache: Mutex::new(TextCache::default()),
        })
    }
    pub fn render(&self, page: u32, scale: f32) -> std::result::Result<crate::Bitmap, String> {
        if page == 0 || page > 4096 || !scale.is_finite() || !(0.01..=8.).contains(&scale) {
            return Err("text document page/scale limit".into());
        }
        let key = (page, scale.to_bits());
        let mut cache = self.cache.lock().map_err(|_| "text cache poisoned")?;
        if let Some(index) = cache.entries.iter().position(|entry| entry.0 == key) {
            let entry = cache.entries.remove(index).unwrap();
            cache.entries.push_back(entry);
        } else {
            cache.preparations += 1;
            let prepared = prepare(&self.worker, &self.source, page, f64::from(scale))
                .map_err(|e| e.to_string());
            cache.entries.push_back((key, prepared));
            while cache.entries.len() > 4
                || cache
                    .entries
                    .iter()
                    .filter_map(|entry| entry.1.as_ref().ok())
                    .map(PreparedScene::retained_bytes)
                    .sum::<usize>()
                    > OUTPUT_LIMIT
            {
                cache.entries.pop_front();
            }
        }
        match &cache.entries.back().unwrap().1 {
            Ok(scene) => scene.render().map_err(|e| e.to_string()),
            Err(error) => Err(error.clone()),
        }
    }
    pub fn cache_stats(&self) -> std::result::Result<(usize, usize), String> {
        let cache = self.cache.lock().map_err(|_| "text cache poisoned")?;
        let bytes = cache
            .entries
            .iter()
            .filter_map(|entry| entry.1.as_ref().ok())
            .map(PreparedScene::retained_bytes)
            .sum();
        Ok((cache.preparations, bytes))
    }
}

struct Running(Child);
impl Running {
    fn terminate_group(&mut self) {
        // SAFETY: the child was assigned a new process group before exec. The
        // parent has not reaped it yet, so its pid cannot be reused here.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        // All normal paths reap explicitly. Error paths still kill/wait the
        // immediate child; resource worker code itself never creates children.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One worker globally at a time: no multiplicative 512 MiB parser concurrency.
/// `executable` must be the application-owned truetype_page_probe binary, never
/// a path supplied by the PDF. No in-process PDF parser runs in this function.
pub fn prepare(executable: &Path, pdf: &[u8], page: u32, scale: f64) -> Result<PreparedScene> {
    prepare_deadline(executable, pdf, page, scale, Duration::from_secs(10))
}

fn prepare_deadline(
    executable: &Path,
    pdf: &[u8],
    page: u32,
    scale: f64,
    wall: Duration,
) -> Result<PreparedScene> {
    let profile = std::env::var_os("OPDS_TEXT_PARENT_PHASES").is_some();
    let profile_start = Instant::now();
    if pdf.is_empty()
        || pdf.len() > INPUT_LIMIT
        || page == 0
        || page > 65536
        || !scale.is_finite()
        || scale <= 0.
        || scale > 16.
    {
        return Err("text worker request outside limits".into());
    }
    let _serial = WORKER.lock().map_err(|_| "text worker lock poisoned")?;
    let mut command = Command::new(executable);
    command
        .args([
            "-",
            &page.to_string(),
            &scale.to_string(),
            "-",
            "--scene-worker",
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: after fork this closure only invokes async-signal-safe libc calls
    // with stack values. No locks, heap allocation or Rust logging before exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for (resource, value) in [
                (libc::RLIMIT_AS, ADDRESS_LIMIT),
                (libc::RLIMIT_CPU, 6),
                (libc::RLIMIT_STACK, 8 * 1024 * 1024),
                (libc::RLIMIT_CORE, 0),
                (libc::RLIMIT_FSIZE, 0),
                (libc::RLIMIT_NOFILE, 32),
            ] {
                let limit = libc::rlimit {
                    rlim_cur: value,
                    rlim_max: value,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Running(command.spawn()?);
    let profile_spawn = Instant::now();
    let mut input = child.0.stdin.take().ok_or("worker stdin")?;
    let output = child.0.stdout.take().ok_or("worker stdout")?;
    let errors = child.0.stderr.take().ok_or("worker stderr")?;
    let started = Instant::now();
    let (status, written, bytes, diagnostic, timed_out) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(pdf));
        let reader = scope.spawn(move || {
            let mut bytes = Vec::with_capacity(OUTPUT_LIMIT + 1);
            output
                .take(OUTPUT_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let stderr = scope.spawn(move || {
            let mut bytes = Vec::with_capacity(STDERR_LIMIT + 1);
            errors
                .take(STDERR_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let mut timeout = false;
        let status = loop {
            // waitid WNOWAIT observes termination without reaping: group cleanup
            // cannot race pid reuse, and inherited pipes cannot block joins.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.0.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result != 0 {
                child.terminate_group();
                break child.0.wait();
            }
            if unsafe { info.si_pid() } != 0 {
                child.terminate_group();
                break child.0.wait();
            }
            if started.elapsed() >= wall {
                timeout = true;
                child.terminate_group();
                break child.0.wait();
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        (status, writer.join(), reader.join(), stderr.join(), timeout)
    });
    let profile_wait = Instant::now();
    if timed_out {
        return Err("text worker wall timeout".into());
    }
    let status = status?;
    let diagnostic = diagnostic.map_err(|_| "worker stderr reader panic")??;
    if diagnostic.len() > STDERR_LIMIT {
        return Err("text worker stderr limit".into());
    }
    if !status.success() {
        return Err(format!(
            "text worker failed ({status}): {}",
            String::from_utf8_lossy(&diagnostic)
        )
        .into());
    }
    written.map_err(|_| "worker input writer panic")??;
    let bytes = bytes.map_err(|_| "worker scene reader panic")??;
    let scene = if bytes.starts_with(b"PHYG") {
        PreparedScene::Hybrid(PreparedHybridPage::decode(&bytes)?)
    } else {
        PreparedScene::Text(PreparedWindingPage::decode(&bytes)?)
    };
    let profile_decoded = Instant::now();
    if profile {
        eprintln!(
            "OPDS_TEXT_PARENT_PHASES spawn={:.3} wait_and_transfer={:.3} decode={:.3} ms",
            (profile_spawn - profile_start).as_secs_f64() * 1000.,
            (profile_wait - profile_spawn).as_secs_f64() * 1000.,
            (profile_decoded - profile_wait).as_secs_f64() * 1000.,
        );
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    struct Script(std::path::PathBuf);
    impl Script {
        fn new(body: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "opds-worker-test-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, format!("#!/bin/sh\ncat >/dev/null\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }
    impl Drop for Script {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn run(script: &Script) -> Result<PreparedScene> {
        prepare_deadline(&script.0, b"%PDF-1.7", 1, 1., Duration::from_millis(250))
    }
    #[test]
    fn worker_failure_timeout_and_pipe_limits() {
        for body in [
            "exit 9",
            "kill -ABRT $$",
            "printf PWPG",
            "head -c 33554433 /dev/zero",
            "head -c 65537 /dev/zero >&2",
        ] {
            let script = Script::new(body);
            assert!(run(&script).is_err(), "accepted {body}");
        }
        let script = Script::new("sleep 30 & wait");
        let started = Instant::now();
        assert!(run(&script).err().unwrap().to_string().contains("timeout"));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "inherited pipe leaked"
        );
    }
    #[test]
    fn cache_is_source_bound_and_serializes_concurrent_replays() {
        let scene = PreparedWindingPage::new(16, 16).unwrap().encode().unwrap();
        let octal = scene
            .iter()
            .map(|b| format!("\\{:03o}", b))
            .collect::<String>();
        let script = Script::new(&format!("printf '{octal}'"));
        let document =
            TextDocument::new(std::sync::Arc::new(b"%PDF-1.7".to_vec()), script.0.clone()).unwrap();
        std::thread::scope(|s| {
            let calls: Vec<_> = (0..4)
                .map(|_| s.spawn(|| document.render(1, 1.).unwrap().rgba))
                .collect();
            for call in calls {
                assert_eq!(call.join().unwrap(), vec![255; 1024]);
            }
        });
        assert_eq!(document.cache_stats().unwrap().0, 1);
        document.render(1, 2.).unwrap();
        assert_eq!(document.cache_stats().unwrap().0, 2);
        document.render(1, 1.).unwrap();
        assert_eq!(document.cache_stats().unwrap().0, 2);
        assert!(document.cache_stats().unwrap().1 <= OUTPUT_LIMIT);
        assert!(document.render(1, f32::NAN).is_err());
        assert_eq!(document.cache_stats().unwrap().0, 2);
        let bad = Script::new("exit 3");
        let rejected =
            TextDocument::new(std::sync::Arc::new(b"%PDF-1.7".to_vec()), bad.0.clone()).unwrap();
        assert!(rejected.render(1, 1.).is_err());
        assert!(rejected.render(1, 1.).is_err());
        assert_eq!(rejected.cache_stats().unwrap(), (1, 0));
    }

    #[test]
    fn child_resource_limits_and_valid_scene() {
        let scene = PreparedWindingPage::new(16, 16).unwrap().encode().unwrap();
        let octal = scene
            .iter()
            .map(|b| format!("\\{:03o}", b))
            .collect::<String>();
        // /bin/sh is only a fake worker in tests, never a PDF interpreter.
        let script=Script::new(&format!("[ \"$(ulimit -v)\" = 524288 ] || exit 3\n[ \"$(ulimit -t)\" = 6 ] || exit 4\n[ \"$(ulimit -s)\" = 8192 ] || exit 5\n[ \"$(ulimit -n)\" = 32 ] || exit 6\n[ \"$(ulimit -c)\" = 0 ] || exit 7\n[ \"$(ulimit -f)\" = 0 ] || exit 8\nprintf '{octal}'"));
        let page = run(&script).unwrap();
        assert_eq!(page.render().unwrap().rgba, vec![255; 16 * 16 * 4]);
        for scale in [f64::NAN, f64::INFINITY, 0., -1., 17.] {
            assert!(prepare(&script.0, b"x", 1, scale).is_err());
        }
        assert!(prepare(&script.0, b"", 1, 1.).is_err());
        assert!(prepare(&script.0, b"x", 0, 1.).is_err());
        let large = vec![0; INPUT_LIMIT + 1];
        assert!(prepare(&script.0, &large, 1, 1.).is_err());
    }

    #[test]
    fn hybrid_worker_revalidates_opaque_image_and_scene() {
        let text = PreparedWindingPage::new(16, 16).unwrap();
        let mut pixels = vec![0_u8; 16 * 16 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[12, 34, 56, 255]);
        }
        let bytes = PreparedHybridPage::new(
            crate::Bitmap {
                width: 16,
                height: 16,
                rgba: pixels.clone(),
            },
            text,
        )
        .unwrap()
        .encode()
        .unwrap();
        let octal = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|b| format!("\\{:03o}", b))
                .collect::<String>()
        };
        let script = Script::new(&format!("printf '{}'", octal(&bytes)));
        let scene = run(&script).unwrap();
        assert_eq!(scene.render().unwrap().rgba, pixels);
        let document =
            TextDocument::new(std::sync::Arc::new(b"%PDF-1.7".to_vec()), script.0.clone()).unwrap();
        assert_eq!(document.render(1, 1.).unwrap().rgba, pixels);
        assert_eq!(document.render(1, 1.).unwrap().rgba, pixels);
        assert_eq!(document.cache_stats().unwrap().0, 1);
        for at in [4, 16, 24, 35, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[at] ^= 1;
            let script = Script::new(&format!("printf '{}'", octal(&bad)));
            assert!(run(&script).is_err(), "accepted hybrid mutation at {at}");
        }
    }
}
