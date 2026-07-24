// Terminal backend using Windows ConPTY (via the `portable-pty` crate). Each
// session owns a master/slave pair, a spawned child shell, and a reader thread
// that forwards output to the frontend as base64-encoded `pty-data` events.

use base64::Engine;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{AppHandle, Emitter};
use portable_pty::{Child, MasterPty, PtySize, PtyPair};

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send>,
}

static SESSIONS: once_cell::sync::Lazy<Mutex<HashMap<u32, PtySession>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

#[derive(serde::Serialize, Clone)]
struct PtyData {
    id: u32,
    data: String,
}

/// Spawn a new terminal running `cmd` (e.g. "powershell.exe"). Returns its id.
pub fn spawn(cmd: &str, cols: u16, rows: u16, app: AppHandle) -> Result<u32, String> {
    let pty_system = portable_pty::native_pty_system();
    let pair: PtyPair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut builder = portable_pty::CommandBuilder::new(cmd);
    builder.cwd(std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into()));

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|e| format!("spawn: {e}"))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone_reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let session = PtySession {
        master: pair.master,
        writer,
        child,
    };
    SESSIONS.lock().insert(id, session);

    // Reader thread: forward bytes to the frontend.
    let app2 = app.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    let _ = app2.emit("pty-data", PtyData { id, data });
                }
                Err(_) => break,
            }
        }
        let _ = app2.emit("pty-exit", id);
    });

    Ok(id)
}

/// Write user input to the terminal's stdin.
pub fn write(id: u32, data: &[u8]) -> Result<(), String> {
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| format!("no pty session {id}"))?;
    session
        .writer
        .write_all(data)
        .map_err(|e| format!("write: {e}"))?;
    session.writer.flush().ok();
    Ok(())
}

/// Resize the terminal pty.
pub fn resize(id: u32, cols: u16, rows: u16) -> Result<(), String> {
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| format!("no pty session {id}"))?;
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize: {e}"))?;
    Ok(())
}

/// Kill a terminal session.
pub fn kill(id: u32) -> Result<(), String> {
    let mut sessions = SESSIONS.lock();
    if let Some(mut session) = sessions.remove(&id) {
        let _ = session.child.kill();
    }
    Ok(())
}
