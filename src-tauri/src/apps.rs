// Launching and lifecycle management for the embedded external applications:
// VSCode (multi-instance IDE) and the IM clients (WeChat / QQ / WXWork).

use crate::state::{hwnd_from_usize, hwnd_to_usize, AppState, CustomPage, IdeInstance, ManagedWindow, SlotKind};
use crate::window_manager;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

/// Locate a VSCode `Code.exe` installation.
pub fn locate_vscode() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(format!("{local}\\Programs\\Microsoft VS Code\\Code.exe"));
    }
    candidates.push(r"C:\Program Files\Microsoft VS Code\Code.exe".to_string());
    candidates.push(r"C:\Program Files (x86)\Microsoft VS Code\Code.exe".to_string());
    candidates
        .into_iter()
        .find(|c| std::path::Path::new(c).exists())
}

/// Launch a new VSCode instance (optionally opening `folder`), embed it, and
/// register it as a new IDE page.
pub fn launch_vscode(folder: Option<String>, app: &AppHandle) -> Result<usize, String> {
    let exe = locate_vscode().ok_or_else(|| "VSCode (Code.exe) not found".to_string())?;

    let existing: Vec<usize> = app
        .state::<AppState>()
        .ide_instances
        .lock()
        .iter()
        .map(|i| i.hwnd)
        .collect();

    let mut args: Vec<String> = vec!["--new-window".into()];
    if let Some(f) = &folder {
        args.push(f.clone());
    }
    let child = std::process::Command::new(&exe)
        .args(&args)
        .spawn()
        .map_err(|e| format!("failed to launch VSCode: {e}"))?;
    let pid = child.id();
    drop(child);

    // `code.exe --new-window` is a CLI that often hands off to an already-running
    // VSCode instance, so the new window lives in the existing Code.exe process
    // (not the CLI pid's tree). Try the process tree first, then fall back to a
    // name-based scan for a new Code.exe window.
    let hwnd = match window_manager::wait_for_window_in_tree(pid, &existing, 8) {
        Ok(h) => h,
        Err(_) => {
            window_manager::wait_for_new_window_by_processes(&["Code.exe"], &existing, 20)?
        }
    };

    let state = app.state::<AppState>();
    window_manager::attach_window(hwnd_from_usize(hwnd), true);

    let title = folder
        .as_ref()
        .and_then(|f| {
            std::path::Path::new(f)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "VSCode".into());

    let idx = {
        let mut inst = state.ide_instances.lock();
        let idx = inst.len();
        inst.push(IdeInstance {
            hwnd,
            folder,
            title,
        });
        idx
    };
    *state.ide_active.lock() = idx;
    let _ = app.emit("ide-active-changed", idx);
    apply(app);
    Ok(hwnd)
}

/// Set the active IDE instance by index (used when clicking a tab).
pub fn set_ide_active(index: usize, app: &AppHandle) {
    let state = app.state::<AppState>();
    let len = state.ide_instances.lock().len();
    if index < len {
        *state.ide_active.lock() = index;
        let _ = app.emit("ide-active-changed", index);
        apply(app);
    }
}

/// Ctrl+Shift+Tab on page 2: cycle to the next VSCode instance, or request a
/// new one when at the end.
pub fn cycle_ide(app: &AppHandle) {
    let state = app.state::<AppState>();
    let len = state.ide_instances.lock().len();
    if len == 0 {
        let _ = app.emit("ide-request-new", ());
        return;
    }
    let mut active = state.ide_active.lock();
    if *active >= len - 1 {
        drop(active);
        let _ = app.emit("ide-request-new", ());
    } else {
        *active += 1;
        let a = *active;
        drop(active);
        let _ = app.emit("ide-active-changed", a);
        apply(app);
    }
}

/// Close an IDE instance by index (sends WM_CLOSE).
pub fn close_ide(index: usize, app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let hwnd_opt = state
        .ide_instances
        .lock()
        .get(index)
        .map(|i| i.hwnd);
    if let Some(h) = hwnd_opt {
        unsafe {
            let _ = PostMessageW(hwnd_from_usize(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    // Don't remove from ide_instances immediately: WM_CLOSE is asynchronous and
    // the user may cancel. prune_dead_windows will clean up once the window is
    // actually destroyed and clamp the active index.
    apply(app);
    Ok(())
}

/// Launch (or attach to an already running) IM client and embed every window of
/// its process (login + main) so both stay in the slot at natural size.
pub fn launch_im(app: &AppHandle, kind_str: &str) -> Result<usize, String> {
    let (names, base, kind) = match kind_str {
        "wechat" => (&["Weixin.exe", "WeChat.exe"][..], "wechat", SlotKind::WeChat),
        "qq" => (&["QQ.exe"][..], "qq", SlotKind::Qq),
        "wecom" => (&["WXWork.exe"][..], "wecom", SlotKind::WeCom),
        other => return Err(format!("unknown im kind: {other}")),
    };

    let state = app.state::<AppState>();

    // Collect all visible windows of the process; launch it first if none exist.
    let mut hwnds: Vec<HWND> = window_manager::find_all_windows_by_processes(names);
    if hwnds.is_empty() {
        let exe = {
            let paths = state.im_paths.lock();
            paths.get(kind_str).cloned().or_else(|| guess_im_path(kind_str))
        };
        let exe = exe.ok_or_else(|| {
            format!("{kind_str} not running and no path configured (set via im_set_paths)")
        })?;
        let child = std::process::Command::new(&exe)
            .spawn()
            .map_err(|e| format!("failed to launch {kind_str}: {e}"))?;
        let pid = child.id();
        drop(child);
        let deadline = std::time::Instant::now() + Duration::from_secs(25);
        while std::time::Instant::now() < deadline {
            hwnds = window_manager::find_all_windows_by_processes(names);
            if !hwnds.is_empty() {
                break;
            }
            // Some clients use a launcher process; fall back to the process tree.
            if let Some(h) = window_manager::find_window_in_tree(pid, &[]) {
                hwnds = vec![h];
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if hwnds.is_empty() {
            return Err(format!("timed out waiting for {kind_str} window"));
        }
    }

    // Attach every window of the process (login + main). attach_window makes
    // Win32 calls, so we must NOT hold the managed lock while calling it --
    // that would block the watchdog thread which also needs managed.
    let existing: Vec<usize> = state.managed.lock().values().map(|w| w.hwnd).collect();
    let new_hwnds: Vec<HWND> = hwnds
        .iter()
        .filter(|h| !existing.contains(&hwnd_to_usize(**h)))
        .copied()
        .collect();
    for h in &new_hwnds {
        window_manager::attach_window(*h, true);
    }
    // Re-lock to insert the new managed entries.
    let mut managed = state.managed.lock();
    let mut idx = managed.values().filter(|w| w.kind == kind).count();
    let mut first = 0usize;
    for h in &new_hwnds {
        let raw = hwnd_to_usize(*h);
        // Re-check in case another thread added it while we didn't hold the lock.
        if managed.values().any(|w| w.hwnd == raw) {
            continue;
        }
        let slot = format!("{base}-{idx}");
        idx += 1;
        if first == 0 {
            first = raw;
        }
        managed.insert(
            slot.clone(),
            ManagedWindow {
                hwnd: raw,
                slot,
                page: 3,
                kind,
            },
        );
    }
    drop(managed);
    apply(app);
    Ok(first)
}

/// Store user-configured IM executable paths.
pub fn set_im_paths(paths: HashMap<String, String>, app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut store = state.im_paths.lock();
    for (k, v) in paths {
        store.insert(k, v);
    }
}

fn guess_im_path(kind: &str) -> Option<String> {
    let filename = match kind {
        "wechat" => "Weixin.exe",
        "qq" => "QQ.exe",
        "wecom" => "WXWork.exe",
        _ => return None,
    };
    if let Some(p) = find_exe(filename) {
        return Some(p);
    }
    // Older WeChat uses WeChat.exe.
    if kind == "wechat" {
        return find_exe("WeChat.exe");
    }
    None
}

/// Search common Tencent / WXWork install roots for `filename` (depth-limited).
fn find_exe(filename: &str) -> Option<String> {
    let mut roots: Vec<String> = vec![
        r"C:\Program Files\Tencent".into(),
        r"C:\Program Files (x86)\Tencent".into(),
        r"C:\Program Files\WXWork".into(),
        r"C:\Program Files (x86)\WXWork".into(),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(format!("{local}\\Programs"));
        roots.push(format!("{local}\\Tencent"));
    }
    for root in roots {
        if std::path::Path::new(&root).exists() {
            if let Some(p) = find_in_dir(std::path::Path::new(&root), filename, 0) {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn find_in_dir(dir: &std::path::Path, filename: &str, depth: u32) -> Option<std::path::PathBuf> {
    if depth > 4 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_in_dir(&p, filename, depth + 1) {
                return Some(f);
            }
        } else if p
            .file_name()
            .map(|n| n.eq_ignore_ascii_case(filename))
            .unwrap_or(false)
        {
            return Some(p);
        }
    }
    None
}

// ---------- app detection (for download hints) ----------

pub fn detect_vscode() -> bool {
    locate_vscode().is_some()
}

pub fn detect_im(kind: &str, app: &AppHandle) -> bool {
    let names: &[&str] = match kind {
        "wechat" => &["Weixin.exe", "WeChat.exe"],
        "qq" => &["QQ.exe"],
        "wecom" => &["WXWork.exe"],
        _ => return false,
    };
    if window_manager::find_window_by_processes(names).is_some() {
        return true;
    }
    let state = app.state::<AppState>();
    if state.im_paths.lock().get(kind).is_some() {
        return true;
    }
    guess_im_path(kind).is_some()
}

// ---------- custom app pages ----------

/// Launch (or reuse) the application bound to a custom page. If an instance is
/// already running, it is reused (no new window). Uses name-based window scanning
/// because Electron apps hand off to an already-running instance.
pub fn launch_custom(page_id: u8, app: &AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    crate::debug_log::dlog("custom", format!("launch_custom(page {page_id})"));
    // Reuse existing instance if any window is still alive.
    if let Some(hwnds) = state.custom_instances.lock().get(&page_id) {
        for &h in hwnds {
            if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd_from_usize(h)).as_bool() } {
                crate::debug_log::dlog("custom", format!("page {page_id}: reuse live hwnd {h:#x}"));
                return Ok(h);
            }
        }
    }
    // Before launching a new process, check if the app already has a visible
    // window (e.g. Trae is already running). If so, adopt it instead of starting
    // a new process (which would make the running instance open a 2nd window).
    let (exe, _args) = {
        let pages = state.custom_pages.lock();
        let p = pages
            .iter()
            .find(|p| p.id == page_id)
            .ok_or_else(|| format!("no custom page {page_id}"))?;
        (p.exe.clone(), p.args.clone())
    };
    let proc_name = std::path::Path::new(&exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| exe.clone());
    let names = [proc_name.as_str()];
    // Exclude all hwnds already in the instance list for this page.
    let exclude: Vec<usize> = state
        .custom_instances
        .lock()
        .get(&page_id)
        .map(|v| v.clone())
        .unwrap_or_default();
    if let Some(h) = window_manager::find_new_window_by_processes(&names, &exclude) {
        let raw = hwnd_to_usize(h);
        crate::debug_log::dlog("custom", format!("page {page_id}: adopted existing window {raw:#x}"));
        window_manager::attach_window_gentle(h);
        let slot = format!("custom-{page_id}");
        state.managed.lock().insert(
            slot.clone(),
            ManagedWindow {
                hwnd: raw,
                slot,
                page: page_id,
                kind: SlotKind::Custom,
            },
        );
        {
            let mut inst = state.custom_instances.lock();
            let list = inst.entry(page_id).or_default();
            list.push(raw);
            let idx = list.len() - 1;
            state.custom_active.lock().insert(page_id, idx);
        }
        apply(app);
        return Ok(raw);
    }
    // No existing window to adopt; launch_custom_new handles the error message.
    crate::debug_log::dlog("custom", format!("page {page_id}: no existing window, spawn new"));
    launch_custom_new(page_id, app)
}

/// Force-launch a new instance of the custom page's app. For Electron apps
/// (Trae) we DO NOT spawn a new process (they self-kill). Instead we try to
/// adopt any existing window of the same process that isn't yet managed. If
/// none, we fall back to spawning (for non-Electron apps).
pub fn launch_custom_new(page_id: u8, app: &AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let (exe, args) = {
        let pages = state.custom_pages.lock();
        let p = pages
            .iter()
            .find(|p| p.id == page_id)
            .ok_or_else(|| format!("no custom page {page_id}"))?;
        (p.exe.clone(), p.args.clone())
    };
    let proc_name = std::path::Path::new(&exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| exe.clone());
    // First prune dead hwnds from the instance list so the exclude list is
    // accurate (Trae may have recreated a window with a new hwnd).
    {
        let mut inst = state.custom_instances.lock();
        if let Some(list) = inst.get_mut(&page_id) {
            list.retain(|h| {
                let hwnd = hwnd_from_usize(*h);
                if unsafe {
                    !windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd).as_bool()
                } {
                    return false;
                }
                let title = window_manager::window_title(hwnd);
                let t = title.to_lowercase();
                !(t.contains("msctfime") || t.contains("default ime") || t == "ime")
            });
        }
    }
    let exclude: Vec<usize> = state
        .custom_instances
        .lock()
        .get(&page_id)
        .map(|v| v.clone())
        .unwrap_or_default();
    // Find ALL matching windows, then pick one not in exclude.
    let names = [proc_name.as_str()];
    let all = window_manager::find_all_windows_by_processes(&names);
    crate::debug_log::dlog(
        "custom",
        format!("page {page_id}: launch_custom_new found {} candidate(s), exclude {:?}", all.len(), exclude),
    );
    let new_hwnd = all.into_iter().find(|h| {
        let raw = hwnd_to_usize(*h);
        !exclude.contains(&raw)
    });
    if let Some(h) = new_hwnd {
        let raw = hwnd_to_usize(h);
        crate::debug_log::dlog("custom", format!("page {page_id}: adopted unmanaged window {raw:#x}"));
        window_manager::attach_window_gentle(h);
        let slot = format!("custom-{page_id}");
        state.managed.lock().insert(
            slot.clone(),
            ManagedWindow {
                hwnd: raw,
                slot,
                page: page_id,
                kind: SlotKind::Custom,
            },
        );
        {
            let mut inst = state.custom_instances.lock();
            let list = inst.entry(page_id).or_default();
            list.push(raw);
            let idx = list.len() - 1;
            state.custom_active.lock().insert(page_id, idx);
        }
        apply(app);
        return Ok(raw);
    }
    // No unmanaged window found: spawn a new process and wait for its window.
    crate::debug_log::dlog("custom", format!("page {page_id}: spawning {exe} {args}"));
    let args_vec: Vec<String> = if args.trim().is_empty() {
        Vec::new()
    } else {
        args.split_whitespace().map(|s| s.to_string()).collect()
    };
    let _ = std::process::Command::new(&exe)
        .args(&args_vec)
        .spawn()
        .map_err(|e| format!("failed to launch {exe}: {e}"))?;
    // Wait for a new window of this process to appear (up to 20s). Trae may
    // kill+restart, so we keep polling until a window not in exclude shows up.
    let raw = match window_manager::wait_for_new_window_by_processes(&names, &exclude, 20) {
        Ok(r) => r,
        Err(_) => {
            crate::debug_log::dlog("custom", format!("page {page_id}: timed out waiting for window"));
            return Err(format!("timed out waiting for {proc_name} window"));
        }
    };
    crate::debug_log::dlog("custom", format!("page {page_id}: new window appeared {raw:#x}"));
    let hwnd = hwnd_from_usize(raw);
    window_manager::attach_window_gentle(hwnd);
    let slot = format!("custom-{page_id}");
    state.managed.lock().insert(
        slot.clone(),
        ManagedWindow {
            hwnd: raw,
            slot,
            page: page_id,
            kind: SlotKind::Custom,
        },
    );
    {
        let mut inst = state.custom_instances.lock();
        let list = inst.entry(page_id).or_default();
        list.push(raw);
        let idx = list.len() - 1;
        state.custom_active.lock().insert(page_id, idx);
    }
    apply(app);
    Ok(raw)
}

/// Cycle through instances of a custom page (Ctrl+Shift+Tab), like cycle_ide.
/// Only cycles existing windows; never launches a new process (Electron apps
/// like Trae self-kill when Kate spawns/attaches).
pub fn cycle_custom(page_id: u8, app: &AppHandle) {
    let state = app.state::<AppState>();
    // Prune dead windows first (including HWNDs reused by IME windows).
    {
        let mut inst = state.custom_instances.lock();
        if let Some(list) = inst.get_mut(&page_id) {
            list.retain(|h| {
                let hwnd = hwnd_from_usize(*h);
                if unsafe {
                    !windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd).as_bool()
                } {
                    return false;
                }
                // Guard against HWND reuse by system IME windows.
                let title = window_manager::window_title(hwnd);
                let t = title.to_lowercase();
                !(t.contains("msctfime") || t.contains("default ime") || t == "ime")
            });
            if list.is_empty() {
                inst.remove(&page_id);
                state.custom_active.lock().remove(&page_id);
            } else {
                let mut active = state.custom_active.lock();
                let cur = *active.get(&page_id).unwrap_or(&0);
                if cur >= list.len() {
                    active.insert(page_id, list.len() - 1);
                }
            }
        }
    }
    let len = state
        .custom_instances
        .lock()
        .get(&page_id)
        .map(|v| v.len())
        .unwrap_or(0);
    if len == 0 {
        // No windows to cycle; do nothing (don't spawn a new process).
        return;
    }
    let cur = *state.custom_active.lock().get(&page_id).unwrap_or(&0);
    let next = (cur + 1) % len;
    crate::debug_log::dlog("custom", format!("page {page_id}: cycle {cur} -> {next} (len {len})"));
    state.custom_active.lock().insert(page_id, next);
    let _ = app.emit("custom-active-changed", page_id);
    apply(app);
}

/// Get the list + active index for a custom page (for the frontend tabs).
pub fn custom_state(page_id: u8, app: &AppHandle) -> (Vec<usize>, usize) {
    let state = app.state::<AppState>();
    let list = state
        .custom_instances
        .lock()
        .get(&page_id)
        .cloned()
        .unwrap_or_default();
    let active = *state.custom_active.lock().get(&page_id).unwrap_or(&0);
    (list, active)
}

/// Set the active instance index for a custom page (tab click). Clamps the
/// index to the valid range and brings the window to the foreground.
pub fn set_custom_active(page_id: u8, index: usize, app: &AppHandle) {
    let state = app.state::<AppState>();
    let len = state
        .custom_instances
        .lock()
        .get(&page_id)
        .map(|v| v.len())
        .unwrap_or(0);
    if len == 0 || index >= len {
        return;
    }
    crate::debug_log::dlog("custom", format!("page {page_id}: set_active -> {index} (len {len})"));
    state.custom_active.lock().insert(page_id, index);
    let _ = app.emit("custom-active-changed", page_id);
    apply(app);
}

/// Close a custom-page instance: send WM_CLOSE to the window and remove it
/// from the instance list only if the window actually closes. The app itself
/// keeps running (we don't kill the process), only the window is closed.
pub fn close_custom(page_id: u8, index: usize, app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    // Get the hwnd to close.
    let hwnd_opt = state
        .custom_instances
        .lock()
        .get(&page_id)
        .and_then(|v| v.get(index).copied());
    if let Some(h) = hwnd_opt {
        unsafe {
            let _ = PostMessageW(hwnd_from_usize(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        // WM_CLOSE is asynchronous; wait briefly to see if the window actually
        // closes before cleaning up state. If the user cancels (window stays
        // alive), we leave the state intact so the window remains managed.
        std::thread::sleep(Duration::from_millis(500));
        let dead = !unsafe {
            windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd_from_usize(h)).as_bool()
        };
        if dead {
            // Remove from managed map (any entry with this hwnd for this page).
            let mut managed = state.managed.lock();
            let dead_keys: Vec<String> = managed
                .iter()
                .filter(|(_, w)| w.kind == SlotKind::Custom && w.page == page_id && w.hwnd == h)
                .map(|(k, _)| k.clone())
                .collect();
            for k in &dead_keys {
                managed.remove(k);
            }
            drop(managed);
            // Remove from instance list + clamp active.
            let mut inst = state.custom_instances.lock();
            if let Some(list) = inst.get_mut(&page_id) {
                if index < list.len() {
                    list.remove(index);
                }
                if list.is_empty() {
                    inst.remove(&page_id);
                    state.custom_active.lock().remove(&page_id);
                } else {
                    let mut active = state.custom_active.lock();
                    let cur = *active.get(&page_id).unwrap_or(&0);
                    if cur >= list.len() {
                        active.insert(page_id, list.len() - 1);
                    }
                }
            }
            drop(inst);
        }
    }
    let _ = app.emit("custom-active-changed", page_id);
    apply(app);
    Ok(())
}

/// Add a user-defined app page and persist it.
pub fn add_custom_page(name: String, exe: String, args: String, app: &AppHandle) -> CustomPage {
    let state = app.state::<AppState>();
    let mut pages = state.custom_pages.lock();
    let id = pages.iter().map(|p| p.id).max().map(|m| m + 1).unwrap_or(5);
    let page = CustomPage {
        id,
        name,
        exe,
        args,
    };
    pages.push(page.clone());
    drop(pages);
    crate::config::save(&state.custom_pages.lock());
    let _ = app.emit("custom-pages-changed", ());
    page
}

/// Remove a custom page and persist the change.
pub fn remove_custom_page(id: u8, app: &AppHandle) {
    let state = app.state::<AppState>();
    // Release managed Custom windows for this page back to the desktop.
    {
        let managed = state.managed.lock();
        let to_release: Vec<usize> = managed
            .values()
            .filter(|w| w.kind == SlotKind::Custom && w.page == id)
            .map(|w| w.hwnd)
            .collect();
        drop(managed);
        for h in to_release {
            window_manager::release_window_gentle(hwnd_from_usize(h));
        }
    }
    // Clean managed entries for this page.
    state
        .managed
        .lock()
        .retain(|_, w| !(w.kind == SlotKind::Custom && w.page == id));
    {
        let mut pages = state.custom_pages.lock();
        pages.retain(|p| p.id != id);
        crate::config::save(&pages);
    }
    state.custom_instances.lock().remove(&id);
    state.custom_active.lock().remove(&id);
    let mut cur = state.current_page.lock();
    if *cur == id {
        *cur = 1;
        let _ = app.emit("page-changed", 1u8);
    }
    drop(cur);
    let _ = app.emit("custom-pages-changed", ());
    apply(app);
}

fn apply(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = app2.state::<AppState>();
        window_manager::apply_layout(&state);
    });
}
