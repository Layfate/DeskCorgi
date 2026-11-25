//! Small Win32 helpers: strings, registry autostart, elevation, dialogs, monitors.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    KEY_SET_VALUE, REG_SZ,
};
use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, SW_SHOWNORMAL};

/// NUL-terminated UTF-16 for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn wide_os(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// Copies `s` into a fixed UTF-16 buffer, truncating and NUL-terminating.
pub fn copy_wide(dst: &mut [u16], s: &str) {
    if dst.is_empty() {
        return;
    }
    let mut n = 0;
    for c in s.encode_utf16() {
        if n + 1 >= dst.len() {
            break;
        }
        dst[n] = c;
        n += 1;
    }
    dst[n] = 0;
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "DeskCorgi";

fn autostart_command() -> Option<Vec<u16>> {
    let exe = std::env::current_exe().ok()?;
    let mut s = OsString::from("\"");
    s.push(exe.as_os_str());
    s.push("\"");
    Some(wide_os(&s))
}

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn open_run_key(access: u32) -> Option<RegKey> {
    let mut key: HKEY = null_mut();
    let path = wide(RUN_KEY);
    let err = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    (err == 0 && !key.is_null()).then_some(RegKey(key))
}

/// True when the Run entry exists and points at this exe.
pub fn autostart_enabled() -> bool {
    let Some(key) = open_run_key(KEY_QUERY_VALUE) else { return false };
    let name = wide(RUN_VALUE);
    let mut buf = [0u16; 1024];
    let mut bytes = (buf.len() * 2) as u32;
    let mut ty = 0u32;
    let err = unsafe { RegQueryValueExW(key.0, name.as_ptr(), null(), &mut ty, buf.as_mut_ptr().cast(), &mut bytes) };
    if err != 0 || ty != REG_SZ {
        return false;
    }
    let len = (bytes as usize / 2).min(buf.len());
    let stored = String::from_utf16_lossy(&buf[..len]);
    let stored = stored.trim_end_matches('\0').trim_matches('"').to_lowercase();
    match std::env::current_exe() {
        Ok(exe) => stored == exe.to_string_lossy().to_lowercase(),
        Err(_) => false,
    }
}

pub fn set_autostart(on: bool) -> bool {
    let Some(key) = open_run_key(KEY_SET_VALUE | KEY_QUERY_VALUE) else { return false };
    let name = wide(RUN_VALUE);
    unsafe {
        if on {
            let Some(cmd) = autostart_command() else { return false };
            RegSetValueExW(key.0, name.as_ptr(), 0, REG_SZ, cmd.as_ptr().cast(), (cmd.len() * 2) as u32) == 0
        } else {
            let err = RegDeleteValueW(key.0, name.as_ptr());
            err == 0 || err == 2 // ERROR_FILE_NOT_FOUND: already off
        }
    }
}

pub fn is_elevated() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

/// Starts an elevated copy of this exe (UAC prompt). Returns false if the user cancelled.
pub fn restart_elevated() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let file = wide_os(exe.as_os_str());
    let verb = wide("runas");
    let args = wide("--replace");
    let r = unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), args.as_ptr(), null(), SW_SHOWNORMAL) };
    r as isize > 32
}

pub fn message(owner: HWND, title: &str, text: &str, warning: bool) {
    let t = wide(title);
    let m = wide(text);
    let icon = if warning { MB_ICONWARNING } else { MB_ICONINFORMATION };
    unsafe {
        MessageBoxW(owner, m.as_ptr(), t.as_ptr(), MB_OK | icon);
    }
}

/// Shows the standard "Open" dialog for a skin file.
pub fn pick_skin_file(owner: HWND) -> Option<PathBuf> {
    let mut file = vec![0u16; 4096];
    // Filter pairs separated by NULs, ending with a double NUL.
    let filter: Vec<u16> = "DeskCorgi skin (*.toml)\0*.toml\0All files (*.*)\0*.*\0\0".encode_utf16().collect();
    let title = wide("Import a DeskCorgi skin");
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    let ok = unsafe { GetOpenFileNameW(&mut ofn) } != 0;
    if !ok {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    (len > 0).then(|| PathBuf::from(OsString::from_wide(&file[..len])))
}

/// Opens a folder in Explorer.
pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let verb = wide("open");
    let p = wide_os(path.as_os_str());
    unsafe {
        ShellExecuteW(null_mut(), verb.as_ptr(), p.as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

/// Work area (screen minus taskbar) of the monitor nearest to a point.
pub fn work_area_near(p: POINT) -> RECT {
    unsafe {
        let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !mon.is_null() && GetMonitorInfoW(mon, &mut mi) != 0 {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1280, bottom: 720 }
        }
    }
}

/// Whether any part of the rectangle is on a connected monitor.
pub fn rect_visible(r: &RECT) -> bool {
    unsafe { !MonitorFromRect(r, MONITOR_DEFAULTTONULL).is_null() }
}

/// Monitor rectangle for the monitor containing most of `r`.
pub fn monitor_rect_for(r: &RECT) -> Option<RECT> {
    unsafe {
        let mon = MonitorFromRect(r, MONITOR_DEFAULTTONULL);
        if mon.is_null() {
            return None;
        }
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        (GetMonitorInfoW(mon, &mut mi) != 0).then_some(mi.rcMonitor)
    }
}
