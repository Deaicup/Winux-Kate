// Windows shell replacement: write the Winlogon\Shell registry value, terminate
// the legacy explorer.exe, and restore it. All destructive operations are gated
// behind the `--shell` flag / WINUX_SHELL_MODE=1 to keep development machines safe.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::{env, mem};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const WINLOGON_SUBKEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon");

/// True if Winux-Kate should take over the shell role (kill explorer, etc.).
pub fn should_take_over_shell() -> bool {
    env::args().any(|a| a == "--shell")
        || env::var("WINUX_SHELL_MODE").map(|v| v == "1").unwrap_or(false)
}

/// Returns true if the current process was launched by winlogon.exe (i.e. we are
/// the actual system shell).
pub fn is_running_as_shell() -> bool {
    let pid = unsafe { GetCurrentProcessId() };
    match parent_process_name(pid) {
        Some(name) => name.eq_ignore_ascii_case("winlogon.exe"),
        None => false,
    }
}

/// Terminate all `explorer.exe` processes via taskkill. Returns true on success.
pub fn kill_explorer() -> bool {
    let status = Command::new("taskkill")
        .args(["/F", "/IM", "explorer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    matches!(status, Ok(s) if s.success())
}

/// True if any explorer.exe process is still alive.
pub fn explorer_running() -> bool {
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if utf16_to_string(&entry.szExeFile).eq_ignore_ascii_case("explorer.exe") {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// Kill explorer and block until every explorer.exe process is gone (Windows
/// may auto-restart the shell, so we re-kill inside the wait loop).
pub fn kill_explorer_blocking(timeout_ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let _ = kill_explorer();
        if !explorer_running() || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Relaunch `explorer.exe` (used as a development safety net).
pub fn restore_explorer() -> bool {
    Command::new("explorer.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok()
}

/// Install Winux-Kate as the system shell for the next logon (requires admin).
pub fn install_as_shell(exe_path: &str) -> Result<(), String> {
    set_shell_value(exe_path)
}

/// Restore `explorer.exe` as the system shell.
pub fn uninstall_shell() -> Result<(), String> {
    set_shell_value("explorer.exe")
}

/// Read the current Winlogon Shell value.
pub fn current_shell() -> Option<String> {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, WINLOGON_SUBKEY, 0, KEY_READ, &mut hkey).0 != 0 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32 * 2;
        let mut ty: REG_VALUE_TYPE = REG_SZ;
        let res = RegQueryValueExW(
            hkey,
            w!("Shell"),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut len),
        );
        let _ = RegCloseKey(hkey);
        if res.0 != 0 {
            return None;
        }
        let nul = len as usize / 2;
        let s = String::from_utf16_lossy(&buf[..nul]);
        Some(s.trim_end_matches('\0').to_string())
    }
}

fn set_shell_value(value: &str) -> Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            WINLOGON_SUBKEY,
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        );
        if status.0 != 0 {
            return Err("failed to open Winlogon registry key (admin required)".into());
        }
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes: Vec<u8> = wide.iter().flat_map(|v| v.to_le_bytes()).collect();
        let res = RegSetValueExW(hkey, w!("Shell"), 0, REG_SZ, Some(&bytes));
        let _ = RegCloseKey(hkey);
        if res.0 != 0 {
            return Err("failed to write Shell registry value".into());
        }
        Ok(())
    }
}

/// Look up the name of the parent process of `pid`.
fn parent_process_name(pid: u32) -> Option<String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut parent_pid = 0u32;
        if Process32FirstW(snap, &mut entry).is_err() {
            let _ = CloseHandle(snap);
            return None;
        }
        loop {
            if entry.th32ProcessID == pid {
                parent_pid = entry.th32ParentProcessID;
                break;
            }
            if Process32NextW(snap, &mut entry).is_err() {
                let _ = CloseHandle(snap);
                return None;
            }
        }

        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry).is_err() {
            let _ = CloseHandle(snap);
            return None;
        }
        let name;
        loop {
            if entry.th32ProcessID == parent_pid {
                name = utf16_to_string(&entry.szExeFile);
                break;
            }
            if Process32NextW(snap, &mut entry).is_err() {
                let _ = CloseHandle(snap);
                return None;
            }
        }
        let _ = CloseHandle(snap);
        Some(name)
    }
}

fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Resolve the absolute path of the running executable (used for install_shell).
pub fn current_exe_path() -> Option<String> {
    env::current_exe().ok().map(|p| p.to_string_lossy().to_string())
}

#[allow(dead_code)]
fn _force_link(h: HANDLE) {
    let _ = h.is_invalid();
}
