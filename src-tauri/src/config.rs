// Persistence for user-defined custom app pages, stored as JSON under
// %USERPROFILE%\.winux-kate\pages.json.

use crate::state::CustomPage;
use std::path::PathBuf;

fn config_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .map(|p| PathBuf::from(p).join(".winux-kate"))
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("pages.json"))
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SavedPage {
    name: String,
    exe: String,
    args: String,
}

/// Load custom pages from disk, assigning ids 5, 6, ... (built-in pages are 1-4).
pub fn load() -> Vec<CustomPage> {
    let path = match config_path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let data = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let saved: Vec<SavedPage> = serde_json::from_str(&data).unwrap_or_default();
    saved
        .into_iter()
        .enumerate()
        .map(|(i, s)| CustomPage {
            id: 5 + i as u8,
            name: s.name,
            exe: s.exe,
            args: s.args,
        })
        .collect()
}

/// Persist the current custom pages (without runtime-only fields like id/hwnd).
pub fn save(pages: &[CustomPage]) {
    let dir = match config_dir() {
        Some(d) => d,
        None => return,
    };
    let _ = std::fs::create_dir_all(&dir);
    let saved: Vec<SavedPage> = pages
        .iter()
        .map(|p| SavedPage {
            name: p.name.clone(),
            exe: p.exe.clone(),
            args: p.args.clone(),
        })
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&saved) {
        let _ = std::fs::write(dir.join("pages.json"), json);
    }
}
