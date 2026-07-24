// Tauri command handlers: thin wrappers exposing the backend modules to the
// frontend via `invoke(...)`. Each delegates to the relevant module and, where
// window visibility is affected, triggers a layout refresh on the main thread.

use crate::apps;
use crate::pty;
use crate::shell;
use crate::shortcuts;
use crate::state::{hwnd_from_usize, hwnd_to_usize, AppState, ImView, Rect, SlotKind, SlotRect};
use crate::system;
use crate::window_manager;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

static DESKTOP_SLOT_SEQ: AtomicU32 = AtomicU32::new(1);

fn apply_main(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = app2.state::<AppState>();
        window_manager::apply_layout(&state);
    });
}

// ---------- shell ----------

#[tauri::command]
pub fn install_shell(exe: Option<String>) -> Result<(), String> {
    let path = exe.or_else(shell::current_exe_path).unwrap_or_default();
    shell::install_as_shell(&path)
}

#[tauri::command]
pub fn uninstall_shell() -> Result<(), String> {
    shell::uninstall_shell()
}

#[tauri::command]
pub fn kill_explorer() -> bool {
    shell::kill_explorer()
}

#[tauri::command]
pub fn restore_explorer() -> bool {
    shell::restore_explorer()
}

#[tauri::command]
pub fn is_running_as_shell() -> bool {
    shell::is_running_as_shell()
}

// ---------- window manager ----------

#[tauri::command]
pub fn report_slot_rects(rects: Vec<SlotRect>, state: State<AppState>, app: AppHandle) {
    // Dedup: only re-apply the layout when a rect actually changed. The
    // frontend re-reports on an interval, and each apply_layout foregrounds
    // the custom (Trae) window -- doing it for identical rects is pure churn.
    let mut changed = false;
    {
        let mut slots = state.slots.lock();
        for r in rects {
            // Log custom-page slots only (others are noisy and rarely relevant).
            if r.id.starts_with("custom-") {
                crate::debug_log::dlog(
                    "slots",
                    format!("{}: page {} ({},{}) {}x{}", r.id, r.page, r.rect.x, r.rect.y, r.rect.w, r.rect.h),
                );
            }
            match slots.get(&r.id) {
                Some(old) if old.rect == r.rect && old.page == r.page => {}
                _ => {
                    changed = true;
                    slots.insert(r.id.clone(), r);
                }
            }
        }
    }
    if changed {
        apply_main(&app);
    }
}

#[tauri::command]
pub fn hide_all_external(state: State<AppState>) {
    window_manager::hide_all(&state);
}

#[tauri::command]
pub fn get_current_page(state: State<AppState>) -> u8 {
    *state.current_page.lock()
}

#[tauri::command]
pub fn set_current_page(page: u8, state: State<AppState>, app: AppHandle) -> Result<(), String> {
    let total = state.total_pages();
    if page < 1 || page > total {
        return Err(format!("page must be 1..={total}"));
    }
    crate::debug_log::dlog("page", format!("set_current_page -> {page}"));
    *state.current_page.lock() = page;
    let _ = app.emit("page-changed", page);
    apply_main(&app);
    Ok(())
}

#[tauri::command]
pub fn launch_and_attach(
    exe: String,
    args: Vec<String>,
    proc_name: String,
    slot: String,
    kind: SlotKind,
    page: u8,
    state: State<AppState>,
    app: AppHandle,
) -> Result<usize, String> {
    let (hwnd, _pid) = window_manager::launch_process(&exe, &args)?;
    window_manager::attach_window(hwnd_from_usize(hwnd), true);
    state.managed.lock().insert(
        slot.clone(),
        crate::state::ManagedWindow {
            hwnd,
            slot,
            page,
            kind,
        },
    );
    apply_main(&app);
    Ok(hwnd)
}

#[tauri::command]
pub fn detach_window(slot: String, state: State<AppState>, app: AppHandle) -> Result<(), String> {
    let win = state
        .managed
        .lock()
        .get(&slot)
        .cloned()
        .ok_or_else(|| format!("no managed window in slot {slot}"))?;
    // For Custom (Trae) windows, only do a gentle release (no style changes)
    // to avoid triggering Lifecycle#kill().
    if win.kind == crate::state::SlotKind::Custom {
        window_manager::release_window_gentle(hwnd_from_usize(win.hwnd));
    } else {
        window_manager::detach_window(hwnd_from_usize(win.hwnd));
    }
    state.managed.lock().remove(&slot);
    state.slots.lock().remove(&slot);
    apply_main(&app);
    Ok(())
}

// ---------- pty ----------

#[tauri::command]
pub fn pty_spawn(cmd: String, cols: u16, rows: u16, app: AppHandle) -> Result<u32, String> {
    pty::spawn(&cmd, cols, rows, app)
}

#[tauri::command]
pub fn pty_write(id: u32, data: String) -> Result<(), String> {
    pty::write(id, data.as_bytes())
}

#[tauri::command]
pub fn pty_resize(id: u32, cols: u16, rows: u16) -> Result<(), String> {
    pty::resize(id, cols, rows)
}

#[tauri::command]
pub fn pty_kill(id: u32) -> Result<(), String> {
    pty::kill(id)
}

// ---------- system ----------

#[tauri::command]
pub fn system_status() -> system::SystemStatus {
    system::status()
}

#[tauri::command]
pub fn set_volume(v: f32) {
    system::set_volume(v);
}

#[tauri::command]
pub fn set_mute(muted: bool) {
    system::set_mute(muted);
}

#[tauri::command]
pub fn set_brightness(v: u8) {
    system::set_brightness(v);
}

// ---------- shortcuts / desktop ----------

#[tauri::command]
pub fn list_shortcuts() -> Vec<shortcuts::Shortcut> {
    shortcuts::list_desktop_shortcuts()
}

#[tauri::command]
pub fn launch_app(
    target: String,
    args: String,
    state: State<AppState>,
    app: AppHandle,
) -> Result<String, String> {
    let proc_name = std::path::Path::new(&target)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| target.clone());
    let _ = proc_name;

    // Exclude already-managed HWNDs so we attach the freshly created window.
    let exclude: Vec<usize> = state.managed.lock().values().map(|w| w.hwnd).collect();

    let args_vec: Vec<String> = if args.trim().is_empty() {
        Vec::new()
    } else {
        args.split_whitespace().map(|s| s.to_string()).collect()
    };

    let child = std::process::Command::new(&target)
        .args(&args_vec)
        .spawn()
        .map_err(|e| format!("failed to launch {target}: {e}"))?;
    let pid = child.id();
    drop(child);

    let hwnd = window_manager::wait_for_window_in_tree(pid, &exclude, 20)?;

    window_manager::attach_window(hwnd_from_usize(hwnd), false);
    let slot = format!("desktop-{}", DESKTOP_SLOT_SEQ.fetch_add(1, Ordering::SeqCst));

    // Cascade-position the new desktop window within the main client area.
    let rect = {
        let managed = state.managed.lock();
        let count = managed.values().filter(|w| w.kind == SlotKind::DesktopApp).count() as i32;
        let cr = state
            .main_hwnd
            .lock()
            .clone()
            .and_then(window_manager::client_rect)
            .unwrap_or(Rect { x: 0, y: 0, w: 1280, h: 800 });
        let off = (40 * count) % 240;
        Rect {
            x: off,
            y: off,
            w: (cr.w * 3 / 5).max(320),
            h: (cr.h * 4 / 5).max(240),
        }
    };
    // Place at the initial cascade once; the user can then move/resize freely and
    // the position is preserved across page switches.
    window_manager::move_window(hwnd_from_usize(hwnd), &rect);
    state.slots.lock().insert(
        slot.clone(),
        SlotRect {
            id: slot.clone(),
            rect,
            kind: SlotKind::DesktopApp,
            page: 4,
        },
    );
    state.managed.lock().insert(
        slot.clone(),
        crate::state::ManagedWindow {
            hwnd,
            slot: slot.clone(),
            page: 4,
            kind: SlotKind::DesktopApp,
        },
    );
    apply_main(&app);
    Ok(slot)
}

#[tauri::command]
pub fn close_app(slot: String, state: State<AppState>, app: AppHandle) -> Result<(), String> {
    let hwnd = state
        .managed
        .lock()
        .get(&slot)
        .map(|w| w.hwnd)
        .ok_or_else(|| format!("no managed window in slot {slot}"))?;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            hwnd_from_usize(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }
    // Don't remove from managed/slots immediately: WM_CLOSE is asynchronous and
    // the user may cancel. prune_dead_windows will clean up once the window is
    // actually destroyed (including the corresponding slot entry).
    apply_main(&app);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct DesktopWin {
    pub slot: String,
    pub title: String,
}

/// Reparent every existing visible top-level window (not owned by Winux-Kate)
/// into the desktop page so it behaves like a normal desktop that inherits the
/// already-open apps. Skips windows already embedded (WS_EX_TOOLWINDOW) and
/// windows belonging to custom-page applications (tracked by exe name).
#[tauri::command]
pub fn adopt_existing_windows(state: State<AppState>, app: AppHandle) -> Result<usize, String> {
    let own_pid = std::process::id();
    let main_hwnd = state.main_hwnd.lock().clone();

    let mut exclude: Vec<usize> = state.managed.lock().values().map(|w| w.hwnd).collect();
    exclude.extend(state.ide_instances.lock().iter().map(|i| i.hwnd));
    if let Some(h) = main_hwnd {
        exclude.push(hwnd_to_usize(h));
    }

    // Collect exe names of custom-page apps so their windows are never adopted
    // onto the desktop page (they belong to their own dedicated page).
    let custom_exes: Vec<String> = state
        .custom_pages
        .lock()
        .iter()
        .filter_map(|p| {
            std::path::Path::new(&p.exe)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
        })
        .collect();

    let candidates = window_manager::adoptable_windows_filtered(&exclude, own_pid, &custom_exes);
    let mut count = 0;
    for (hwnd, _title) in candidates {
        window_manager::attach_window(hwnd_from_usize(hwnd), false);
        let slot = format!("desktop-{}", DESKTOP_SLOT_SEQ.fetch_add(1, Ordering::SeqCst));
        let cr = main_hwnd
            .and_then(window_manager::client_rect)
            .unwrap_or(Rect { x: 0, y: 0, w: 1280, h: 800 });
        let managed_count = state
            .managed
            .lock()
            .values()
            .filter(|w| w.kind == SlotKind::DesktopApp)
            .count() as i32;
        let off = (40 * managed_count) % 240;
        let rect = Rect {
            x: off,
            y: off,
            w: (cr.w * 3 / 5).max(320),
            h: (cr.h * 4 / 5).max(240),
        };
        window_manager::move_window(hwnd_from_usize(hwnd), &rect);
        state.slots.lock().insert(
            slot.clone(),
            SlotRect {
                id: slot.clone(),
                rect,
                kind: SlotKind::DesktopApp,
                page: 4,
            },
        );
        state.managed.lock().insert(
            slot.clone(),
            crate::state::ManagedWindow {
                hwnd,
                slot: slot.clone(),
                page: 4,
                kind: SlotKind::DesktopApp,
            },
        );
        count += 1;
    }
    apply_main(&app);
    Ok(count)
}

/// List the windows currently on the desktop page (for the taskbar).
#[tauri::command]
pub fn list_desktop_windows(state: State<AppState>) -> Vec<DesktopWin> {
    let managed = state.managed.lock();
    let mut out = Vec::new();
    for (slot, win) in managed.iter() {
        if win.kind == SlotKind::DesktopApp {
            let title = window_manager::window_title(hwnd_from_usize(win.hwnd));
            out.push(DesktopWin {
                slot: slot.clone(),
                title,
            });
        }
    }
    out
}

/// Bring a desktop window to the front (taskbar click).
#[tauri::command]
pub fn focus_desktop_window(
    slot: String,
    state: State<AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let hwnd = state
        .managed
        .lock()
        .get(&slot)
        .map(|w| w.hwnd)
        .ok_or_else(|| format!("no managed window in slot {slot}"))?;
    // Z-order changes for child windows must happen on the main thread.
    let _ = app.run_on_main_thread(move || {
        window_manager::focus_window(hwnd_from_usize(hwnd));
    });
    Ok(())
}

// ---------- ide ----------

#[tauri::command]
pub fn ide_new(folder: Option<String>, app: AppHandle) -> Result<usize, String> {
    apps::launch_vscode(folder, &app)
}

#[tauri::command]
pub fn ide_cycle(app: AppHandle) {
    apps::cycle_ide(&app);
}

#[tauri::command]
pub fn ide_set_active(index: usize, app: AppHandle) {
    apps::set_ide_active(index, &app);
}

#[derive(serde::Serialize)]
pub struct IdeState {
    pub list: Vec<crate::state::IdeInstance>,
    pub active: usize,
}

#[tauri::command]
pub fn ide_list(state: State<AppState>) -> IdeState {
    IdeState {
        list: state.ide_instances.lock().clone(),
        active: *state.ide_active.lock(),
    }
}

#[tauri::command]
pub fn ide_close(index: usize, app: AppHandle) -> Result<(), String> {
    apps::close_ide(index, &app)
}

// ---------- im ----------

#[tauri::command]
pub fn im_launch(kind: String, app: AppHandle) -> Result<usize, String> {
    apps::launch_im(&app, &kind)
}

#[tauri::command]
pub fn im_toggle(state: State<AppState>, app: AppHandle) -> Result<(), String> {
    let v = {
        let mut v = state.im_view.lock();
        *v = if *v == ImView::Split {
            ImView::WeCom
        } else {
            ImView::Split
        };
        *v
    };
    let _ = app.emit("im-toggle", v);
    apply_main(&app);
    Ok(())
}

#[tauri::command]
pub fn im_set_paths(paths: HashMap<String, String>, app: AppHandle) {
    apps::set_im_paths(paths, &app);
}

// ---------- file system (for the dashboard editor / file viewer) ----------

#[derive(serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[tauri::command]
pub fn list_dir(path: Option<String>) -> Result<Vec<DirEntry>, String> {
    let dir = path.unwrap_or_else(|| {
        std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\".into())
    });
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(&dir) {
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(DirEntry {
                name,
                path: e.path().to_string_lossy().to_string(),
                is_dir,
            });
        }
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

#[tauri::command]
pub fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

// ---------- app detection ----------

#[derive(serde::Serialize)]
pub struct AppDetection {
    pub vscode: bool,
    pub wechat: bool,
    pub qq: bool,
    pub wecom: bool,
}

#[tauri::command]
pub fn detect_apps(app: AppHandle) -> AppDetection {
    AppDetection {
        vscode: apps::detect_vscode(),
        wechat: apps::detect_im("wechat", &app),
        qq: apps::detect_im("qq", &app),
        wecom: apps::detect_im("wecom", &app),
    }
}

// ---------- custom app pages ----------

#[tauri::command]
pub fn list_custom_pages(state: State<AppState>) -> Vec<crate::state::CustomPage> {
    state.custom_pages.lock().clone()
}

#[tauri::command]
pub fn add_custom_page(
    name: String,
    exe: String,
    args: String,
    app: AppHandle,
) -> crate::state::CustomPage {
    apps::add_custom_page(name, exe, args, &app)
}

#[tauri::command]
pub fn remove_custom_page(id: u8, app: AppHandle) {
    apps::remove_custom_page(id, &app);
}

#[tauri::command]
pub fn launch_custom_page(id: u8, app: AppHandle) -> Result<usize, String> {
    apps::launch_custom(id, &app)
}

#[tauri::command]
pub fn launch_custom_new(id: u8, app: AppHandle) -> Result<usize, String> {
    apps::launch_custom_new(id, &app)
}

#[tauri::command]
pub fn cycle_custom(id: u8, app: AppHandle) {
    apps::cycle_custom(id, &app);
}

#[tauri::command]
pub fn set_custom_active(id: u8, index: usize, app: AppHandle) {
    apps::set_custom_active(id, index, &app);
}

#[tauri::command]
pub fn close_custom(id: u8, index: usize, app: AppHandle) -> Result<(), String> {
    apps::close_custom(id, index, &app)
}

#[derive(serde::Serialize)]
pub struct CustomState {
    pub list: Vec<usize>,
    pub active: usize,
}

#[tauri::command]
pub fn custom_state(id: u8, app: AppHandle) -> CustomState {
    let (list, active) = apps::custom_state(id, &app);
    CustomState { list, active }
}

// ---------- shell control ----------

/// Quit Winux-Kate (triggers the Exit handler which releases windows and
/// restores explorer).
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Temporarily hide every embedded overlay window so a modal dialog (e.g. the
/// folder picker) is not covered by a topmost IDE/IM window.
#[tauri::command]
pub fn hide_overlays(state: State<AppState>) {
    window_manager::hide_all(&state);
}

/// Kill standalone WeChat / QQ processes to avoid conflicts when entering the
/// desktop page (where they are not embedded).
#[tauri::command]
pub fn kill_im_processes() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let s = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "Weixin.exe", "/IM", "WeChat.exe", "/IM", "QQ.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    matches!(s, Ok(st) if st.success())
}
