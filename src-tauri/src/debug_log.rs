// Lightweight file logger for runtime debugging of window management.
// Writes timestamped lines to %USERPROFILE%\.winux-kate\debug.log.
// Enable by calling init() once at startup; dlog() is a no-op before that.

use parking_lot::Mutex;
use std::io::Write;
use std::sync::OnceLock;

static LOG_FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

fn log_path() -> Option<std::path::PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .map(|p| std::path::Path::new(&p).join(".winux-kate").join("debug.log"))
}

/// Initialize the log file (truncates any previous log). Call once at startup.
pub fn init() {
    let path = match log_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let f = std::fs::File::create(path).ok();
    LOG_FILE.get_or_init(|| Mutex::new(f));
}

/// Append a timestamped line: `HH:MM:SS.mmm [tag] message`.
pub fn dlog(tag: &str, msg: impl AsRef<str>) {
    let lock = LOG_FILE.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock();
    let file = match guard.as_mut() {
        Some(f) => f,
        None => return,
    };
    let now = chrono::Local::now();
    let _ = writeln!(
        file,
        "{}.{:03} [{}] {}",
        now.format("%H:%M:%S"),
        now.timestamp_subsec_millis(),
        tag,
        msg.as_ref()
    );
    let _ = file.flush();
}
