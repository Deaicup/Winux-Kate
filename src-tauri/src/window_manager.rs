// Built-in window manager: enumerate external process windows, reparent them
// into the main Winux-Kate window with `SetParent`, position them into slot
// rectangles reported by the frontend, and show/hide them as the active page
// changes.

use crate::state::{hwnd_from_usize, hwnd_to_usize, AppState, ImView, Rect, SlotKind};
use std::collections::HashMap;
use std::mem;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::*;

const WS_EX_APPWINDOW: u32 = 0x0004_0000;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_TOPMOST: u32 = 0x0000_0008;

/// Last z-order applied to Kate's main window (true = pushed to bottom).
/// Guards SetWindowPos calls on the main window so they only fire on state
/// CHANGES: repeated SetWindowPos from the watchdog thread sends messages to
/// the main thread and freezes the UI (regression: do NOT call it every pass).
static MAIN_AT_BOTTOM: AtomicBool = AtomicBool::new(false);

/// Push Kate's main window to the bottom/top of the z-order, skipping the
/// SetWindowPos when the cached state already matches. Safe for watchdog use.
fn set_main_zorder(state: &AppState, bottom: bool) {
    if MAIN_AT_BOTTOM.swap(bottom, Ordering::SeqCst) == bottom {
        return;
    }
    crate::debug_log::dlog("zorder", format!("main window -> {}", if bottom { "BOTTOM" } else { "TOP" }));
    let main_opt = *state.main_hwnd.lock();
    if let Some(main) = main_opt {
        unsafe {
            let _ = SetWindowPos(
                main,
                if bottom { HWND_BOTTOM } else { HWND_TOP },
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

struct ProcInfo {
    name: String,
    parent: u32,
}

/// Take ONE toolhelp snapshot and build a pid -> (name, parent) map. Much
/// cheaper than re-snapshotting per window.
fn snapshot_processes() -> HashMap<u32, ProcInfo> {
    let mut map = HashMap::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(s) => s,
            Err(_) => return map,
        };
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry).is_err() {
            let _ = CloseHandle(snap);
            return map;
        }
        loop {
            let name = utf16_to_string(&entry.szExeFile);
            map.insert(
                entry.th32ProcessID,
                ProcInfo {
                    name,
                    parent: entry.th32ParentProcessID,
                },
            );
            if Process32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
        let _ = CloseHandle(snap);
    }
    map
}

/// True if `pid` is `root` or a descendant of `root` in the process tree.
fn is_descendant(pid: u32, root: u32, map: &HashMap<u32, ProcInfo>) -> bool {
    let mut cur = pid;
    for _ in 0..64 {
        if cur == root {
            return true;
        }
        match map.get(&cur) {
            Some(info) => cur = info.parent,
            None => return false,
        }
    }
    false
}

/// Collect every visible top-level window in a single EnumWindows pass.
fn collect_visible_windows() -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let v = &mut *(lparam.0 as *mut Vec<HWND>);
        if IsWindowVisible(hwnd).as_bool() {
            v.push(hwnd);
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Like collect_visible_windows but returns ALL top-level windows, including
/// hidden ones. Needed because some Electron apps (Trae) create new windows
/// that are initially hidden.
fn collect_all_top_level_windows() -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let v = &mut *(lparam.0 as *mut Vec<HWND>);
        v.push(hwnd);
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

/// Find the first visible top-level window whose process image name matches any
/// of `names` (case-insensitive). Used to attach to already-running IM clients.
pub fn find_window_by_processes(names: &[&str]) -> Option<HWND> {
    let map = snapshot_processes();
    for hwnd in collect_visible_windows() {
        let pid = pid_of(hwnd);
        if let Some(info) = map.get(&pid) {
            for n in names {
                if info.name.eq_ignore_ascii_case(n) {
                    return Some(hwnd);
                }
            }
        }
    }
    None
}

pub fn find_window_by_process(name: &str) -> Option<HWND> {
    find_window_by_processes(&[name])
}

/// True if `hwnd` is a genuine user-facing app window: alive, VISIBLE, has a
/// non-empty title, and is not a system IME window. Hidden helper windows
/// (Electron plugin hosts, "Chrome Legacy Window", etc.) are excluded -- they
/// were previously mis-adopted as new custom-page instances and died at once.
pub fn is_real_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
    }
    let title = window_title(hwnd);
    if title.trim().is_empty() {
        return false;
    }
    let t = title.to_lowercase();
    !(t.contains("msctfime") || t.contains("default ime") || t == "ime")
}

/// All top-level windows (including hidden) whose process name matches any of
/// `names`. Used by custom-page adoption which needs to find windows that
/// Electron apps created in a hidden state.
pub fn find_all_windows_by_processes(names: &[&str]) -> Vec<HWND> {
    let map = snapshot_processes();
    let mut out = Vec::new();
    for hwnd in collect_all_top_level_windows() {
        let pid = pid_of(hwnd);
        if let Some(info) = map.get(&pid) {
            if names.iter().any(|n| info.name.eq_ignore_ascii_case(n)) {
                // Only genuine user-facing windows (visible + titled). Hidden
                // helper windows of Electron apps must not be adopted.
                if !is_real_app_window(hwnd) {
                    continue;
                }
                out.push(hwnd);
            }
        }
    }
    out
}

/// Find a visible top-level window belonging to the process tree rooted at
/// `root_pid` (the launched process or any of its descendants), skipping HWNDs
/// in `exclude`.
pub fn find_window_in_tree(root_pid: u32, exclude: &[usize]) -> Option<HWND> {
    let map = snapshot_processes();
    for hwnd in collect_visible_windows() {
        let raw = hwnd.0 as usize;
        if exclude.contains(&raw) {
            continue;
        }
        let pid = pid_of(hwnd);
        if pid == root_pid || is_descendant(pid, root_pid, &map) {
            return Some(hwnd);
        }
    }
    None
}

/// Poll for a new window in the process tree of `root_pid` to appear.
pub fn wait_for_window_in_tree(
    root_pid: u32,
    exclude: &[usize],
    timeout_secs: u64,
) -> Result<usize, String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(hwnd) = find_window_in_tree(root_pid, exclude) {
            return Ok(hwnd_to_usize(hwnd));
        }
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for window of pid {root_pid}"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Find a visible top-level window whose process name matches any of `names`,
/// skipping HWNDs in `exclude` (used when a launcher hands off to an already-
/// running instance, e.g. VSCode `code --new-window`).
pub fn find_new_window_by_processes(names: &[&str], exclude: &[usize]) -> Option<HWND> {
    let map = snapshot_processes();
    // Use collect_all_top_level_windows (includes hidden) because Electron
    // apps may create new windows that are initially hidden -- but only accept
    // genuine user-facing windows (visible + titled), never helper windows.
    for hwnd in collect_all_top_level_windows() {
        let raw = hwnd.0 as usize;
        if exclude.contains(&raw) {
            continue;
        }
        let pid = pid_of(hwnd);
        if let Some(info) = map.get(&pid) {
            if names.iter().any(|n| info.name.eq_ignore_ascii_case(n)) {
                if !is_real_app_window(hwnd) {
                    continue;
                }
                return Some(hwnd);
            }
        }
    }
    None
}

pub fn wait_for_new_window_by_processes(
    names: &[&str],
    exclude: &[usize],
    timeout_secs: u64,
) -> Result<usize, String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(hwnd) = find_new_window_by_processes(names, exclude) {
            return Ok(hwnd_to_usize(hwnd));
        }
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {names:?} window"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Visible top-level windows that carry a title and do not belong to our own
/// process; candidates for adoption onto the desktop page.
pub fn adoptable_windows(exclude: &[usize], own_pid: u32) -> Vec<(usize, String)> {
    adoptable_windows_filtered(exclude, own_pid, &[])
}

/// Like `adoptable_windows` but also skips windows whose process image name is
/// in `blocked_exes` (lowercase, e.g. "trae cn.exe"), so custom-page apps are
/// never adopted onto the desktop page.
pub fn adoptable_windows_filtered(
    exclude: &[usize],
    own_pid: u32,
    blocked_exes: &[String],
) -> Vec<(usize, String)> {
    let map = snapshot_processes();
    let mut out = Vec::new();
    for hwnd in collect_visible_windows() {
        let raw = hwnd.0 as usize;
        if exclude.contains(&raw) {
            continue;
        }
        // Skip windows already embedded by us (IDE / IM / custom / desktop-page
        // windows are all marked WS_EX_TOOLWINDOW).
        let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 };
        if ex & WS_EX_TOOLWINDOW != 0 {
            continue;
        }
        let pid = pid_of(hwnd);
        if pid == own_pid {
            continue;
        }
        if let Some(info) = map.get(&pid) {
            // Skip if owned by our process tree.
            if is_descendant(pid, own_pid, &map) {
                continue;
            }
            // Skip custom-page apps.
            let name_lower = info.name.to_lowercase();
            if blocked_exes.iter().any(|b| b == &name_lower) {
                continue;
            }
        }
        let title = window_title(hwnd);
        if title.trim().is_empty() {
            continue;
        }
        // Skip system IME / input method windows that shouldn't be embedded.
        let title_lower = title.to_lowercase();
        if title_lower.contains("msctfime") || title_lower.contains("default ime") || title_lower == "ime" {
            continue;
        }
        out.push((raw, title));
    }
    out
}

pub fn window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        utf16_to_string(&buf[..n as usize])
    }
}

/// Reliably bring a window to the foreground. Plain SetForegroundWindow fails
/// when the calling process is not the foreground process (Windows foreground
/// lock); attaching our input queue to the current foreground thread bypasses
/// that restriction. This is the ONLY window operation we perform on custom
/// (Trae) windows -- it does not move/resize/hide them, so their
/// self-protection (Lifecycle#kill) is not triggered.
pub fn force_foreground(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let cur_thread = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != cur_thread;
        if attached {
            let _ = AttachThreadInput(cur_thread, fg_thread, true);
        }
        let ok = SetForegroundWindow(hwnd).as_bool();
        if attached {
            let _ = AttachThreadInput(cur_thread, fg_thread, false);
        }
        crate::debug_log::dlog(
            "foreground",
            format!(
                "SetForegroundWindow({:#x}) = {} (prev fg = {:#x}, attached = {})",
                hwnd.0 as usize, ok, fg.0 as usize, attached
            ),
        );
        ok
    }
}

/// Bring a managed window to the top of the z-order, show it and give it
/// keyboard focus (child windows do not always grab focus automatically).
pub fn focus_window(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Restore a window as a normal (non-topmost) desktop window and show it, used
/// when returning windows to the normal desktop on exit.
pub fn release_window(hwnd: HWND) {
    release_window_impl(hwnd, false)
}

/// Gentle release: only show the window (no style changes), for Electron apps
/// (Trae) that self-kill on external style modifications.
pub fn release_window_gentle(hwnd: HWND) {
    release_window_impl(hwnd, true);
}

fn release_window_impl(hwnd: HWND, gentle: bool) {
    unsafe {
        if !gentle {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let new_style = (style & !WS_POPUP.0) | WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0;
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_TOOLWINDOW) as isize);
            let _ = SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            );
        } else {
            // Gentle: do nothing (Trae kills on any external window op).
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
}

/// Release every embedded window back to the normal desktop (on exit).
pub fn release_all(state: &AppState) {
    let hwnds: Vec<usize> = state
        .managed
        .lock()
        .values()
        .map(|w| w.hwnd)
        .collect();
    for h in hwnds {
        // Custom (Trae) windows must be released gently: any style change
        // triggers the app's self-protection (Lifecycle#kill).
        let is_custom = state
            .managed
            .lock()
            .values()
            .any(|w| w.hwnd == h && w.kind == SlotKind::Custom);
        if is_custom {
            release_window_gentle(hwnd_from_usize(h));
        } else {
            release_window(hwnd_from_usize(h));
        }
    }
    let ide: Vec<usize> = state.ide_instances.lock().iter().map(|i| i.hwnd).collect();
    for h in ide {
        release_window(hwnd_from_usize(h));
    }
    let custom: Vec<usize> = state
        .custom_instances
        .lock()
        .values()
        .flatten()
        .copied()
        .collect();
    for h in custom {
        release_window_gentle(hwnd_from_usize(h));
    }
}

/// Size the main shell window to cover the whole primary monitor (including the
/// area normally reserved for the taskbar) so there is no empty strip.
pub fn set_main_fullscreen(hwnd: HWND) {
    unsafe {
        let cx = GetSystemMetrics(SM_CXSCREEN);
        let cy = GetSystemMetrics(SM_CYSCREEN);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            cx,
            cy,
            SWP_NOZORDER | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
    }
}

/// Prepare an external window for embedding via OVERLAY mode: it stays a
/// top-level window in its own process (so keyboard/IME input keeps working).
/// `strip_decorations` removes the title bar for fullscreen slots; otherwise the
/// frame is kept so desktop-page windows remain draggable/resizable. The window
/// is NOT reparented and NOT made topmost (to avoid covering other pages).
/// `gentle`: when true, do NOT modify window styles (only reposition). Needed
/// for Electron apps (Trae) which kill themselves if their window styles are
/// changed externally.
pub fn attach_window(hwnd: HWND, strip_decorations: bool) {
    attach_window_impl(hwnd, strip_decorations, false)
}

/// Gentle attach: only reposition, never touch window styles. For custom-page
/// Electron apps that self-kill on external style changes.
pub fn attach_window_gentle(hwnd: HWND) {
    attach_window_impl(hwnd, false, true);
}

fn attach_window_impl(hwnd: HWND, strip_decorations: bool, gentle: bool) {
    unsafe {
        if !gentle {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let new_style = if strip_decorations {
                let mask = !(WS_POPUP.0 | WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0
                    | WS_MINIMIZEBOX.0
                    | WS_MAXIMIZEBOX.0);
                (style & mask) | WS_VISIBLE.0
            } else {
                style | WS_VISIBLE.0
            };
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let new_ex = (ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            );
        } else {
            // Gentle: do nothing (Trae kills on any external window op).
            // We only track the hwnd; the window stays as-is.
        }
    }
}

/// Detach a window: hide it and drop the always-on-top flag (overlay mode).
pub fn detach_window(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let new_style = (style & !WS_POPUP.0) | WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0;
        let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_TOOLWINDOW) as isize);
        let _ = SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn move_window(hwnd: HWND, rect: &Rect) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            SWP_NOACTIVATE,
        );
    }
}

/// Move a window to (x, y) keeping its current size (used for IM windows which
/// should not be stretched to the slot).
pub fn move_window_origin(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            x,
            y,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE,
        );
    }
}

/// Show a window and bring it to the top of the z-order, without moving it (for
/// desktop-page windows whose user-moved position must be preserved).
fn show_topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

fn show(hwnd: HWND) {
    unsafe {
        // SW_SHOWNOACTIVATE shows the window without activating it (keeps Kate's
        // focus). For minimized windows, pin_overlays watchdog already has an
        // IsIconic check + SW_RESTORE to restore them.
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

fn hide(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Toggle the WS_EX_TOPMOST state of a window. Topmost windows always stay
/// above non-topmost windows in the z-order, so Kate's main window (which is
/// NOT topmost) can never cover them -- this is what keeps embedded IM/IDE
/// windows visible after the user clicks Kate's topbar and Kate grabs the
/// foreground. `HWND_TOP` alone is insufficient: it only tops the non-topmost
/// band, so any later foreground change to Kate re-covers the window.
fn set_topmost(hwnd: HWND, topmost: bool) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Move a window off-screen but keep it alive/visible (NOT topmost), so embedded
/// apps do not close or recreate their window in response to SW_HIDE when
/// switching pages, and so it does not cover windows on the current page.
fn park_offscreen(hwnd: HWND) {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        // Place the window outside the virtual screen (top-left corner, shifted
        // further up-left) so it is invisible on any monitor configuration.
        let x = vx - 32000;
        let y = vy - 32000;
        let _ = SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Check if there is a live (IsWindow + not IME-reused) active custom window
/// for the given page. Returns the HWND if live, None otherwise.
fn live_custom_hwnd(state: &AppState, page: u8) -> Option<HWND> {
    let inst = state.custom_instances.lock();
    let active = state.custom_active.lock();
    if let Some(list) = inst.get(&page) {
        if let Some(&idx) = active.get(&page) {
            if let Some(&h) = list.get(idx) {
                let hwnd = hwnd_from_usize(h);
                if unsafe { !IsWindow(hwnd).as_bool() } {
                    return None;
                }
                // Guard against HWND reuse by IME windows.
                let title = window_title(hwnd);
                let t = title.to_lowercase();
                if t.contains("msctfime") || t.contains("default ime") || t == "ime" {
                    return None;
                }
                return Some(hwnd);
            }
        }
    }
    None
}

/// Re-apply the current layout: show + position windows on the active page,
/// hide everything else. Safe to call repeatedly (used by the watchdog to keep
/// embedded windows pinned in their slots and on top).
pub fn apply_layout(state: &AppState) {
    let page = *state.current_page.lock();
    let im_view = *state.im_view.lock();
    crate::debug_log::dlog("layout", format!("apply_layout: page = {page}"));
    // If the Kate main window is minimized, hide ALL embedded windows to avoid
    // them covering the screen (which looks like a freeze). The user restored
    // Kate by clicking the taskbar; windows reappear on next apply_layout.
    let main_minimized = state
        .main_hwnd
        .lock()
        .map(|h| unsafe { IsIconic(h).as_bool() })
        .unwrap_or(false);
    if main_minimized {
        let managed = state.managed.lock();
        for win in managed.values() {
            if win.kind == SlotKind::Custom {
                continue;
            }
            let h = hwnd_from_usize(win.hwnd);
            if win.kind == SlotKind::DesktopApp {
                hide(h);
            } else {
                park_offscreen(h);
            }
        }
        drop(managed);
        let ide_inst = state.ide_instances.lock();
        for inst in ide_inst.iter() {
            park_offscreen(hwnd_from_usize(inst.hwnd));
        }
        return;
    }
    // For non-custom pages, ensure Kate's main window is at the TOP of the
    // z-order so it covers embedded windows. On page 5+ we push Kate to the
    // bottom so the custom app window is visible/clickable -- as long as
    // there's a live active custom window for the page.
    if page >= 5 {
        if let Some(custom_hwnd) = live_custom_hwnd(state, page) {
            crate::debug_log::dlog(
                "layout",
                format!("page {page}: live custom = {:#x}, foregrounding", custom_hwnd.0 as usize),
            );
            // Bring the custom window to the foreground (AttachThreadInput
            // bypasses the foreground lock), then push Kate to the bottom so
            // the custom window is visible and clickable. We push Kate down
            // unconditionally: the window is alive, so even if activation was
            // denied the user can click it once Kate is below it.
            let _ = force_foreground(custom_hwnd);
            set_main_zorder(state, true);
        } else {
            crate::debug_log::dlog("layout", format!("page {page}: no live custom window"));
            // No live custom window: keep Kate on top so stray system windows
            // (MSCTFIME UI, etc.) don't leak through.
            set_main_zorder(state, false);
        }
    } else {
        // Page < 5: Kate on top.
        set_main_zorder(state, false);
    }
    let slots = state.slots.lock();
    let managed = state.managed.lock();

    let ide_inst = state.ide_instances.lock();
    let ide_active = *state.ide_active.lock();
    for (i, inst) in ide_inst.iter().enumerate() {
        let h = hwnd_from_usize(inst.hwnd);
        if page == 2 && i == ide_active {
            if let Some(slot) = slots.get("ide") {
                move_window(h, &slot.rect);
            }
            show(h);
            // Keep the active IDE window above Kate (topmost) so clicking
            // Kate's topbar doesn't cover it. Kate is not topmost, so a
            // topmost IDE window stays visible regardless of focus changes.
            set_topmost(h, true);
        } else {
            // Drop topmost before parking; park_offscreen uses HWND_BOTTOM
            // which also clears topmost, but be explicit for clarity.
            set_topmost(h, false);
            park_offscreen(h);
        }
    }
    drop(ide_inst);

    for (slot_id, win) in managed.iter() {
        if win.kind == SlotKind::Custom {
            continue;
        }
        let h = hwnd_from_usize(win.hwnd);
        let visible = match win.kind {
            SlotKind::WeChat => page == 3 && im_view == ImView::Split,
            SlotKind::Qq => page == 3 && im_view == ImView::Split,
            SlotKind::WeCom => page == 3 && im_view == ImView::WeCom,
            SlotKind::DesktopApp => page == 4,
            SlotKind::Custom => page == win.page,
            SlotKind::Terminal | SlotKind::Ide => false,
        };
        if visible {
            match win.kind {
                SlotKind::DesktopApp => {
                    show_topmost(h);
                    // Desktop-page windows also need topmost so Kate's topbar
                    // clicks don't cover them (show_topmost only uses HWND_TOP).
                    set_topmost(h, true);
                }
                SlotKind::WeChat | SlotKind::Qq | SlotKind::WeCom => {
                    // IM windows keep their natural size (login + main window are
                    // different sizes); just place them at the slot's top-left.
                    let base = im_base_slot(win.kind);
                    if let Some(slot) = slots.get(base) {
                        move_window_origin(h, slot.rect.x, slot.rect.y);
                    }
                    show(h);
                    // Make IM windows topmost so Kate's main window (non-topmost)
                    // can never cover them when the user clicks Kate's topbar.
                    // Previously only HWND_TOP was used, which Kate re-covered on
                    // any focus change -- the root cause of "IM windows hidden".
                    set_topmost(h, true);
                }
                _ => {
                    if let Some(slot) = slots.get(slot_id) {
                        move_window(h, &slot.rect);
                    }
                    show(h);
                    set_topmost(h, true);
                }
            }
        } else {
            // Drop topmost first so the window doesn't linger above other pages
            // while parked. park_offscreen's HWND_BOTTOM also clears topmost, but
            // desktop windows are hidden (not parked) and would keep topmost.
            set_topmost(h, false);
            // Desktop windows are truly hidden (they have a taskbar entry);
            // embedded windows are parked off-screen so their apps stay alive.
            if win.kind == SlotKind::DesktopApp {
                hide(h);
            } else {
                park_offscreen(h);
            }
        }
    }
}

fn im_base_slot(kind: SlotKind) -> &'static str {
    match kind {
        SlotKind::WeChat => "wechat",
        SlotKind::Qq => "qq",
        SlotKind::WeCom => "wecom",
        _ => "",
    }
}

/// Watchdog pass: keep non-current embedded windows parked off-screen and
/// restore minimized IM/IDE windows. Custom-page windows are NOT touched here
/// (apply_layout handles their sizing/positioning; interfering causes flicker
/// and window recreation in Electron apps).
pub fn pin_overlays(state: &AppState) {
    let page = *state.current_page.lock();
    let im_view = *state.im_view.lock();
    unsafe {
        // Kate's z-order is managed by apply_layout (called on page switch and
        // slot rect updates). Do NOT call SetWindowPos on Kate's main window
        // here -- it sends messages to the main thread and can cause freezes
        // when the main thread is busy.
        for win in state.managed.lock().values() {
            let h = hwnd_from_usize(win.hwnd);
            let on_page = match win.kind {
                SlotKind::WeChat | SlotKind::Qq => page == 3 && im_view == ImView::Split,
                SlotKind::WeCom => page == 3 && im_view == ImView::WeCom,
                SlotKind::DesktopApp => page == 4,
                SlotKind::Custom => page == win.page,
                _ => false,
            };
            if on_page {
                if win.kind == SlotKind::DesktopApp {
                    continue;
                }
                if win.kind == SlotKind::Custom {
                    continue;
                }
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                // Re-assert topmost if an IM client dropped it (some clients
                // reset their own window styles on focus changes). GetWindowLong
                // check avoids a SetWindowPos every 800ms when already topmost,
                // so this is near-zero cost in the steady state.
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
                if (ex & WS_EX_TOPMOST) == 0 {
                    set_topmost(h, true);
                }
            } else if win.kind == SlotKind::DesktopApp {
                let _ = ShowWindow(h, SW_HIDE);
            } else if win.kind != SlotKind::Custom {
                park_offscreen(h);
            }
            // Custom windows: do NOTHING (handled by custom_instances loop below)
        }

        // IDE windows: only the active one on page 2 stays visible; others park.
        let ide = state.ide_instances.lock();
        let active = *state.ide_active.lock();
        for (i, inst) in ide.iter().enumerate() {
            let h = hwnd_from_usize(inst.hwnd);
            if page == 2 && i == active {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                // Re-assert topmost if VSCode dropped it.
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
                if (ex & WS_EX_TOPMOST) == 0 {
                    set_topmost(h, true);
                }
            } else {
                park_offscreen(h);
            }
        }
        drop(ide);

        // Custom-page windows: the active instance of the current page is sized
        // to its slot; all others (inactive + other pages) are parked off-screen.
        let custom_snap: Vec<(u8, Vec<usize>, usize)> = state
            .custom_instances
            .lock()
            .iter()
            .map(|(k, v)| {
                let active = *state.custom_active.lock().get(k).unwrap_or(&0);
                (*k, v.clone(), active)
            })
            .collect();
        let slots = state.slots.lock();
        let mut found_live_custom = false;
        for (id, hwnds, active) in custom_snap {
            for (i, &h) in hwnds.iter().enumerate() {
                let hh = hwnd_from_usize(h);
                // Skip dead windows (Trae may have killed + recreated them).
                if !IsWindow(hh).as_bool() {
                    continue;
                }
                // Guard against HWND reuse: after Trae kills a window, the HWND
                // may be recycled by a system window (e.g. MSCTFIME UI). Check
                // the title to ensure it's still our app window.
                let title = window_title(hh);
                let title_lower = title.to_lowercase();
                if title_lower.contains("msctfime")
                    || title_lower.contains("default ime")
                    || title_lower == "ime"
                {
                    // HWND was reused by an IME window; treat as dead.
                    continue;
                }
                if id == page && i == active {
                    found_live_custom = true;
                    // Only act when Trae is NOT the foreground window -- this
                    // keeps the watchdog from touching anything every 800ms.
                    // force_foreground (AttachThreadInput + SetForegroundWindow)
                    // is the ONLY safe operation on Trae windows -- ShowWindow
                    // (HIDE/MINIMIZE/RESTORE) and SetWindowPos (move) all trigger
                    // Lifecycle#kill(). Do NOT call ShowWindow at all, even if the
                    // window is iconic -- foreground activation restores it.
                    if GetForegroundWindow() != hh {
                        crate::debug_log::dlog(
                            "watchdog",
                            format!("custom {:#x} (page {id}) not foreground, correcting", hh.0 as usize),
                        );
                        // Ensure Kate is at the BOTTOM BEFORE foregrounding. If
                        // Kate was raised (e.g. user hit Ctrl+Alt+R, or a system
                        // event), Trae would be covered even after a successful
                        // force_foreground. set_main_zorder is cache-guarded, so
                        // this is a no-op when Kate is already at the bottom --
                        // no repeated SetWindowPos, no main-thread flooding.
                        set_main_zorder(state, true);
                        // Foreground Trae regardless of Kate's z-order; even if
                        // the foreground lock denies activation, Kate is already
                        // at the bottom so Trae remains visible and clickable.
                        let _ = force_foreground(hh);
                    }
                }
                // For inactive / other-page windows: do NOTHING. Any ShowWindow
                // or SetWindowPos on Trae windows triggers Lifecycle#kill().
            }
        }
        drop(slots);

        // If we're on a custom page (>= 5) but no live active window was found,
        // restore Kate to HWND_TOP so system windows (MSCTFIME UI, etc.) don't
        // leak through and appear above Kate. Cached: only fires on the
        // bottom -> top transition, not every watchdog pass.
        if page >= 5 && !found_live_custom {
            set_main_zorder(state, false);
        }
    }
}

/// Discover IM windows that appeared after the login window (e.g. the main
/// window) and attach them too, so both login + main stay in the slot.
pub fn discover_im_windows(state: &AppState) {
    let page = *state.current_page.lock();
    let im_view = *state.im_view.lock();
    if page != 3 {
        return;
    }
    discover_for_kind(state, SlotKind::WeChat, &["Weixin.exe", "WeChat.exe"], "wechat", im_view == ImView::Split);
    discover_for_kind(state, SlotKind::Qq, &["QQ.exe"], "qq", im_view == ImView::Split);
    discover_for_kind(state, SlotKind::WeCom, &["WXWork.exe"], "wecom", im_view == ImView::WeCom);
}

fn discover_for_kind(state: &AppState, kind: SlotKind, names: &[&str], base: &str, active: bool) {
    if !active {
        return;
    }
    let has = state.managed.lock().values().any(|w| w.kind == kind);
    if !has {
        return;
    }
    let hwnds = find_all_windows_by_processes(names);
    let managed_hwnds: Vec<usize> = state.managed.lock().values().map(|w| w.hwnd).collect();
    let mut idx = state.managed.lock().values().filter(|w| w.kind == kind).count();
    for h in hwnds {
        let raw = hwnd_to_usize(h);
        if managed_hwnds.contains(&raw) {
            continue;
        }
        attach_window(h, true);
        let slot = format!("{base}-{idx}");
        idx += 1;
        state.managed.lock().insert(
            slot.clone(),
            crate::state::ManagedWindow {
                hwnd: raw,
                slot,
                page: 3,
                kind,
            },
        );
    }
}

/// Discover custom-page windows that reappeared (e.g. the app recreated its
/// window) and re-attach them. Also prunes dead hwnds from custom_instances so
/// switching pages doesn't relaunch the app.
pub fn discover_custom_windows(state: &AppState) {
    let custom_pages = state.custom_pages.lock().clone();
    for cp in &custom_pages {
        // Prune dead hwnds from the instance list (including IME-reused HWNDs).
        {
            let mut inst = state.custom_instances.lock();
            if let Some(list) = inst.get_mut(&cp.id) {
                list.retain(|h| {
                    let hwnd = hwnd_from_usize(*h);
                    if unsafe { !IsWindow(hwnd).as_bool() } {
                        return false;
                    }
                    let title = window_title(hwnd);
                    let t = title.to_lowercase();
                    !(t.contains("msctfime") || t.contains("default ime") || t == "ime")
                });
                if list.is_empty() {
                    inst.remove(&cp.id);
                    state.custom_active.lock().remove(&cp.id);
                } else {
                    // Clamp active index to valid range after pruning.
                    let mut active = state.custom_active.lock();
                    let cur = *active.get(&cp.id).unwrap_or(&0);
                    if cur >= list.len() {
                        active.insert(cp.id, list.len() - 1);
                    }
                }
            }
        }
        // Prune dead managed entries for this page.
        {
            let mut managed = state.managed.lock();
            let dead: Vec<String> = managed
                .iter()
                .filter(|(_, w)| {
                    w.kind == SlotKind::Custom
                        && w.page == cp.id
                        && !unsafe { IsWindow(hwnd_from_usize(w.hwnd)).as_bool() }
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in &dead {
                managed.remove(k);
            }
        }
        // NOTE: We do NOT auto-adopt new windows the app opens itself. Doing
        // so caused infinite instance growth (Trae kills + restarts -> new
        // window -> Kate adopts -> Trae kills again). Users add instances via
        // the "接管新窗口" button instead.
    }
}

/// Remove managed/IDE entries whose underlying window has been destroyed (e.g.
/// the user closed VSCode via its own X button). Returns true if the IDE list
/// changed. Custom-page hwnds are NOT pruned here -- discover_custom_windows
/// replaces them if the app recreated its window, so we don't lose the page.
pub fn prune_dead_windows(state: &AppState) -> bool {
    let mut changed = false;
    unsafe {
        {
            let mut inst = state.ide_instances.lock();
            let before = inst.len();
            inst.retain(|i| IsWindow(hwnd_from_usize(i.hwnd)).as_bool());
            if inst.len() != before {
                changed = true;
                let mut a = state.ide_active.lock();
                if *a >= inst.len() {
                    *a = if inst.is_empty() { 0 } else { inst.len() - 1 };
                }
            }
        }
        {
            let mut m = state.managed.lock();
            // Collect slot keys of dead (non-Custom) windows before removing
            // them, so we can clean up the slots map too.
            let dead_slots: Vec<String> = m
                .iter()
                .filter(|(_, w)| {
                    if w.kind == SlotKind::Custom {
                        return false;
                    }
                    !IsWindow(hwnd_from_usize(w.hwnd)).as_bool()
                })
                .map(|(k, _)| k.clone())
                .collect();
            m.retain(|_, w| {
                // Keep Custom entries even if the hwnd is temporarily invalid
                // (the app may recreate its window; discover_custom_windows
                // will reattach). Other kinds are pruned normally.
                if w.kind == SlotKind::Custom {
                    return true;
                }
                IsWindow(hwnd_from_usize(w.hwnd)).as_bool()
            });
            drop(m);
            // Clean up slots for the dead windows.
            if !dead_slots.is_empty() {
                let mut slots = state.slots.lock();
                for k in &dead_slots {
                    slots.remove(k);
                }
            }
        }
    }
    changed
}

/// Hide every embedded window (used when opening a modal dialog or exiting).
pub fn hide_all(state: &AppState) {
    let managed = state.managed.lock();
    for win in managed.values() {
        if win.kind == SlotKind::Custom {
            continue;
        }
        hide(hwnd_from_usize(win.hwnd));
    }
    drop(managed);
    let ide = state.ide_instances.lock();
    for inst in ide.iter() {
        hide(hwnd_from_usize(inst.hwnd));
    }
    drop(ide);
    // Custom windows: do NOTHING (any ShowWindow op triggers Trae kill).
}

/// Return the client-area size of `hwnd` (in pixels), origin (0,0).
pub fn client_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut r).ok()?;
    }
    Some(Rect {
        x: 0,
        y: 0,
        w: r.right - r.left,
        h: r.bottom - r.top,
    })
}

/// Launch an external executable and wait for its main window to appear, then
/// return the window handle. Uses process-tree matching so launchers that spawn
/// child processes (WeChat, VSCode, ...) are handled.
pub fn launch_process(exe: &str, args: &[String]) -> Result<(usize, u32), String> {
    let mut cmd = Command::new(exe);
    for a in args {
        cmd.arg(a);
    }
    let child = cmd.spawn().map_err(|e| format!("failed to launch {exe}: {e}"))?;
    let pid = child.id();
    drop(child);
    let hwnd = wait_for_window_in_tree(pid, &[], 20)?;
    Ok((hwnd, pid))
}

fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
