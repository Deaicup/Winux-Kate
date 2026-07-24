// Desktop shortcut enumeration and `.lnk` resolution via the Shell COM
// `IShellLinkW` + `IPersistFile` interfaces. Icons are extracted through
// `SHGetFileInfoW` and converted to base64 PNG data URLs for the frontend.

use base64::Engine;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::{env, mem};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDIBits, GetObjectW, ReleaseDC, HGDIOBJ, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, WIN32_FIND_DATAW};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, ShellLink, SLGP_RAWPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

#[derive(serde::Serialize, Clone)]
pub struct Shortcut {
    pub name: String,
    pub target: String,
    pub args: String,
    pub icon: String,
}

pub fn list_desktop_shortcuts() -> Vec<Shortcut> {
    let user_desktop = env::var("USERPROFILE")
        .map(|p| format!("{p}\\Desktop"))
        .unwrap_or_default();
    let public_desktop = "C:\\Users\\Public\\Desktop".to_string();

    let mut result = Vec::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    for dir in [user_desktop, public_desktop] {
        if dir.is_empty() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_lnk = path
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("lnk"))
                    .unwrap_or(false);
                if is_lnk {
                    if let Some(s) = resolve_lnk(&path) {
                        result.push(s);
                    }
                }
            }
        }
    }
    unsafe {
        CoUninitialize();
    }
    result
}

fn resolve_lnk(path: &std::path::Path) -> Option<Shortcut> {
    unsafe {
        let shelllink: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = shelllink.cast().ok()?;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        let mut target_buf = [0u16; 260];
        let mut args_buf = [0u16; 1024];
        let mut find_data: WIN32_FIND_DATAW = mem::zeroed();
        let _ = shelllink.GetPath(&mut target_buf, &mut find_data, SLGP_RAWPATH.0 as u32);
        let _ = shelllink.GetArguments(&mut args_buf);

        let target = utf16_to_string(&target_buf);
        let args = utf16_to_string(&args_buf);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let icon = icon_for_path(&target);

        Some(Shortcut {
            name,
            target,
            args,
            icon,
        })
    }
}

fn icon_for_path(target: &str) -> String {
    if target.is_empty() {
        return String::new();
    }
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut shfi: SHFILEINFOW = mem::zeroed();
        let _ = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi as *mut _),
            mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if shfi.hIcon.is_invalid() {
            return String::new();
        }
        let png = hicon_to_png(shfi.hIcon);
        let _ = DestroyIcon(shfi.hIcon);
        png.unwrap_or_default()
    }
}

fn hicon_to_png(hicon: HICON) -> Option<String> {
    unsafe {
        let mut ii: ICONINFO = mem::zeroed();
        GetIconInfo(hicon, &mut ii).ok()?;
        let hbm = ii.hbmColor;
        if hbm.is_invalid() {
            return None;
        }
        let mut bmp: BITMAP = mem::zeroed();
        if GetObjectW(
            HGDIOBJ(hbm.0),
            mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut BITMAP as *mut core::ffi::c_void),
        ) == 0
        {
            return None;
        }
        let w = bmp.bmWidth;
        let h = bmp.bmHeight;
        if w <= 0 || h <= 0 {
            return None;
        }
        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let size = (w as usize) * (h as usize) * 4;
        let mut pixels = vec![0u8; size];
        let hdc = GetDC(HWND(core::ptr::null_mut()));
        let res = GetDIBits(
            hdc,
            hbm,
            0,
            h as u32,
            Some(pixels.as_mut_ptr() as *mut core::ffi::c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        let _ = ReleaseDC(HWND(core::ptr::null_mut()), hdc);
        if res == 0 {
            return None;
        }
        // BGRA -> RGBA
        for chunk in pixels.chunks_mut(4) {
            chunk.swap(0, 2);
        }

        let img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            image::ImageBuffer::from_raw(w as u32, h as u32, pixels)?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(out.into_inner());
        Some(format!("data:image/png;base64,{b64}"))
    }
}

fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[allow(dead_code)]
fn _link_marker(_: &OsStr) {}
