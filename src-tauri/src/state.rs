// Global application state shared between Tauri commands, the hotkey hook and
// the window manager.

use parking_lot::Mutex;
use std::collections::HashMap;
use windows::Win32::Foundation::HWND;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Terminal,
    Ide,
    WeChat,
    Qq,
    WeCom,
    DesktopApp,
    Custom,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SlotRect {
    pub id: String,
    pub rect: Rect,
    pub kind: SlotKind,
    pub page: u8,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ManagedWindow {
    pub hwnd: usize,
    pub slot: String,
    pub page: u8,
    pub kind: SlotKind,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct IdeInstance {
    pub hwnd: usize,
    pub folder: Option<String>,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImView {
    Split,
    #[serde(rename = "wecom")]
    WeCom,
}

/// A user-defined page that embeds a single chosen application fullscreen.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CustomPage {
    pub id: u8,
    pub name: String,
    pub exe: String,
    pub args: String,
}

pub struct AppState {
    pub current_page: Mutex<u8>,
    pub slots: Mutex<HashMap<String, SlotRect>>,
    /// slot_id -> managed external window
    pub managed: Mutex<HashMap<String, ManagedWindow>>,
    pub ide_instances: Mutex<Vec<IdeInstance>>,
    pub ide_active: Mutex<usize>,
    pub im_view: Mutex<ImView>,
    pub im_paths: Mutex<HashMap<String, String>>,
    pub main_hwnd: Mutex<Option<HWND>>,
    pub custom_pages: Mutex<Vec<CustomPage>>,
    /// custom page id -> list of embedded window hwnds (multiple instances)
    pub custom_instances: Mutex<HashMap<u8, Vec<usize>>>,
    /// custom page id -> index of the active instance
    pub custom_active: Mutex<HashMap<u8, usize>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_page: Mutex::new(1),
            slots: Mutex::new(HashMap::new()),
            managed: Mutex::new(HashMap::new()),
            ide_instances: Mutex::new(Vec::new()),
            ide_active: Mutex::new(0),
            im_view: Mutex::new(ImView::Split),
            im_paths: Mutex::new(HashMap::new()),
            main_hwnd: Mutex::new(None),
            custom_pages: Mutex::new(crate::config::load()),
            custom_instances: Mutex::new(HashMap::new()),
            custom_active: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_main_hwnd(&self, hwnd: HWND) {
        *self.main_hwnd.lock() = Some(hwnd);
    }

    /// Total number of pages: 4 built-in + custom pages.
    pub fn total_pages(&self) -> u8 {
        4 + self.custom_pages.lock().len() as u8
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to convert a stored `usize` back into an `HWND`.
pub fn hwnd_from_usize(v: usize) -> HWND {
    HWND(v as *mut core::ffi::c_void)
}

/// Helper to convert an `HWND` into a storable `usize`.
pub fn hwnd_to_usize(h: HWND) -> usize {
    h.0 as usize
}

// HWND wraps a raw pointer so it is not `Send`/`Sync` by default. A window
// handle is effectively an integer and is safe to share across threads, so we
// assert it here to satisfy Tauri's `State<T>: Send + Sync` requirement.
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}
