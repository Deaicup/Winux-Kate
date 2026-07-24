// Global low-level keyboard hook (WH_KEYBOARD_LL) running on a dedicated thread
// with its own message loop. Captures Ctrl+Tab / Ctrl+Shift+Tab even when an
// embedded external window has focus, and drives page switching + in-page
// navigation by mutating AppState and emitting Tauri events.

use crate::state::{AppState, ImView};
use crate::{apps, window_manager};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

static APP: once_cell::sync::Lazy<Mutex<Option<AppHandle>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

static TAB_PRESSED: AtomicBool = AtomicBool::new(false);

const VK_TAB: i32 = 0x09;
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12; // Alt
const VK_R: i32 = 0x52;

pub fn start(handle: AppHandle) {
    *APP.lock() = Some(handle);
    std::thread::spawn(|| unsafe {
        let hinst = GetModuleHandleW(None)
            .ok()
            .map(|m| HINSTANCE(m.0))
            .unwrap_or(HINSTANCE(core::ptr::null_mut()));
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_ll), hinst, 0).ok();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(core::ptr::null_mut()), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
        if let Some(h) = hook {
            let _ = UnhookWindowsHookEx(h);
        }
    });
}

unsafe extern "system" fn keyboard_ll(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Only do any work when Ctrl is held; pass every other key straight through
    // immediately so typing / IME in embedded windows is never delayed.
    if code == HC_ACTION as i32
        && (GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000) != 0
    {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if kb.vkCode == VK_TAB as u32 {
            let is_down = wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize;
            let is_up = wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize;
            if is_down {
                if !TAB_PRESSED.swap(true, Ordering::SeqCst) {
                    let shift_down = (GetAsyncKeyState(VK_SHIFT) as u16 & 0x8000) != 0;
                    let app = APP.lock().clone();
                    if let Some(app) = app {
                        if shift_down {
                            handle_shift_tab(&app);
                        } else {
                            handle_tab(&app);
                        }
                    }
                }
                return LRESULT(1); // swallow
            }
            if is_up {
                TAB_PRESSED.store(false, Ordering::SeqCst);
                return LRESULT(1);
            }
        } else if kb.vkCode == VK_R as u32
            && (GetAsyncKeyState(VK_MENU) as u16 & 0x8000) != 0
            && wparam.0 == WM_KEYDOWN as usize
        {
            // Ctrl+Alt+R: restore / bring the shell window back to the front in
            // case it was minimized or hidden behind overlays.
            let app = APP.lock().clone();
            if let Some(app) = app {
                let m = app.state::<AppState>().main_hwnd.lock().clone();
                if let Some(m) = m {
                    unsafe {
                        let _ = ShowWindow(m, SW_RESTORE);
                        let _ = SetForegroundWindow(m);
                    }
                }
            }
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn handle_tab(app: &AppHandle) {
    let state = app.state::<AppState>();
    let total = state.total_pages().max(1);
    let page = {
        let mut p = state.current_page.lock();
        let cur = (*p).min(total).max(1);
        *p = cur % total + 1;
        *p
    };
    crate::debug_log::dlog("hotkey", format!("Ctrl+Tab -> page {page}"));
    let _ = app.emit("page-changed", page);
    apply_on_main(app);
}

fn handle_shift_tab(app: &AppHandle) {
    let state = app.state::<AppState>();
    let page = *state.current_page.lock();
    crate::debug_log::dlog("hotkey", format!("Ctrl+Shift+Tab on page {page}"));
    match page {
        2 => {
            // Cycle VSCode instances (or request a new one at the end).
            apps::cycle_ide(app);
        }
        3 => {
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
            apply_on_main(app);
        }
        p if p >= 5 => {
            // Custom page: cycle instances (or launch a new one at the end).
            apps::cycle_custom(p, app);
        }
        _ => {
            // Page 1, 4: go to the previous page.
            let total = state.total_pages().max(1);
            let p = {
                let mut p = state.current_page.lock();
                let cur = (*p).min(total).max(1);
                *p = if cur <= 1 { total } else { cur - 1 };
                *p
            };
            let _ = app.emit("page-changed", p);
            apply_on_main(app);
        }
    }
}

fn apply_on_main(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = app2.state::<AppState>();
        window_manager::apply_layout(&state);
    });
}
