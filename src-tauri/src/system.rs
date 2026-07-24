// System status and control: clock, master audio volume/mute, display
// brightness (via PowerShell + WMI), Bluetooth radio presence, and Wi-Fi SSID
// (via `netsh`). All best-effort; failures degrade to defaults.

use chrono::Local;
use std::mem;
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::Win32::Devices::Bluetooth::{
    BluetoothFindFirstRadio, BluetoothFindRadioClose, BLUETOOTH_FIND_RADIO_PARAMS,
};
use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(serde::Serialize, Clone)]
pub struct SystemStatus {
    pub time: String,
    pub date: String,
    pub volume: f32,
    pub muted: bool,
    pub brightness: u8,
    pub bluetooth_on: bool,
    pub wifi_ssid: String,
    pub wifi_connected: bool,
}

pub fn status() -> SystemStatus {
    let now = Local::now();
    let (vol, muted) = audio_state().unwrap_or((0.0, false));
    let brightness = read_brightness().unwrap_or(255);
    let bluetooth_on = bluetooth_on();
    let (wifi_ssid, wifi_connected) = wifi_info();

    SystemStatus {
        time: now.format("%H:%M:%S").to_string(),
        date: now.format("%Y-%m-%d %a").to_string(),
        volume: vol,
        muted,
        brightness,
        bluetooth_on,
        wifi_ssid,
        wifi_connected,
    }
}

pub fn set_volume(v: f32) {
    if let Some(ep) = audio_endpoint() {
        let clamped = v.clamp(0.0, 1.0);
        unsafe {
            let _ = ep.SetMasterVolumeLevelScalar(clamped, std::ptr::null());
        }
    }
}

pub fn set_mute(muted: bool) {
    if let Some(ep) = audio_endpoint() {
        unsafe {
            let _ = ep.SetMute(BOOL::from(muted), std::ptr::null());
        }
    }
}

pub fn set_brightness(v: u8) {
    let v = v.min(100);
    let script = format!(
        "(Get-WmiObject -Namespace root/wmi -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1,{v})"
    );
    let _ = ps(&[&script]);
    *BRIGHTNESS.lock() = Some(v);
}

fn audio_endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()?;
        let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
        Some(endpoint)
    }
}

fn audio_state() -> Option<(f32, bool)> {
    let ep = audio_endpoint()?;
    unsafe {
        let vol = ep.GetMasterVolumeLevelScalar().ok()?;
        let muted = ep.GetMute().ok().map(|b| b.as_bool())?;
        Some((vol, muted))
    }
}

fn bluetooth_on() -> bool {
    unsafe {
        let mut hradio: HANDLE = mem::zeroed();
        let mut params: BLUETOOTH_FIND_RADIO_PARAMS = mem::zeroed();
        params.dwSize = mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32;
        let hfind = BluetoothFindFirstRadio(&params, &mut hradio).ok();
        match hfind {
            Some(h) => {
                let _ = BluetoothFindRadioClose(h);
                if !hradio.is_invalid() {
                    let _ = CloseHandle(hradio);
                }
                true
            }
            None => false,
        }
    }
}

fn wifi_info() -> (String, bool) {
    let out = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(o) = out else {
        return (String::new(), false);
    };
    let s = String::from_utf8_lossy(&o.stdout);
    for line in s.lines() {
        let t = line.trim();
        if t.starts_with("BSSID") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("SSID") {
            if rest.starts_with(' ') || rest.starts_with(':') {
                if let Some((_k, v)) = t.split_once(':') {
                    let val = v.trim();
                    if !val.is_empty() {
                        return (val.to_string(), true);
                    }
                }
            }
        }
    }
    (String::new(), false)
}

static BRIGHTNESS: once_cell::sync::Lazy<parking_lot::Mutex<Option<u8>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

fn read_brightness() -> Option<u8> {
    if let Some(v) = *BRIGHTNESS.lock() {
        return Some(v);
    }
    let out = ps(&[
        "(Get-WmiObject -Namespace root/wmi -Class WmiMonitorBrightness).CurrentBrightness",
    ])?;
    let v = out.trim().parse::<u8>().ok()?;
    *BRIGHTNESS.lock() = Some(v);
    Some(v)
}

fn ps(args: &[&str]) -> Option<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(args.join(" "))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()
}
