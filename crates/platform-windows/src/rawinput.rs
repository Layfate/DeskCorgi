//! Raw Input: reads keyboard and mouse at the device level, so the dog keeps reacting
//! while games or other apps have focus (RIDEV_INPUTSINK). Read-only, no hooks injected.

use deskcorgi_core::keymap::{key, key_from_set1, label_from_set1, KeyId};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC_EX};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK,
    RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{RI_KEY_BREAK, RI_KEY_E0, RI_KEY_E1};

pub fn register(target: HWND) -> bool {
    let devices = [
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: RIDEV_INPUTSINK, hwndTarget: target }, // keyboard
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_INPUTSINK, hwndTarget: target }, // mouse
    ];
    unsafe { RegisterRawInputDevices(devices.as_ptr(), devices.len() as u32, std::mem::size_of::<RAWINPUTDEVICE>() as u32) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub enum Raw {
    /// `label` is what is printed on the key, for the optional key pop-ups.
    Key { id: KeyId, down: bool, vk: u16, label: Option<&'static str> },
    Mouse { dx: i32, dy: i32, relative: bool, buttons: u16 },
}

// Mouse button transition flags (usButtonFlags).
pub const LEFT_DOWN: u16 = 0x0001;
pub const LEFT_UP: u16 = 0x0002;
pub const RIGHT_DOWN: u16 = 0x0004;
pub const RIGHT_UP: u16 = 0x0008;
pub const MIDDLE_DOWN: u16 = 0x0010;
pub const MIDDLE_UP: u16 = 0x0020;
pub const WHEEL: u16 = 0x0400;
pub const HWHEEL: u16 = 0x0800;

/// Decodes a WM_INPUT message.
pub fn read(lparam: LPARAM) -> Option<Raw> {
    let mut data: RAWINPUT = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let got = unsafe {
        GetRawInputData(
            lparam as HRAWINPUT,
            RID_INPUT,
            (&mut data as *mut RAWINPUT).cast(),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if got == u32::MAX || got == 0 {
        return None;
    }
    match data.header.dwType {
        RIM_TYPEKEYBOARD => {
            let kb = unsafe { data.data.keyboard };
            if kb.VKey == 0xFF {
                return None; // fake keys sent as part of escape sequences
            }
            let flags = kb.Flags as u32;
            let down = flags & RI_KEY_BREAK == 0;
            let (id, label) = if flags & RI_KEY_E1 != 0 {
                (Some(key(0, 13)), Some("PAUSE")) // Pause/Break
            } else {
                let mut make = kb.MakeCode;
                let mut e0 = flags & RI_KEY_E0 != 0;
                if make == 0 {
                    // Some keyboards (and software keyboards) leave MakeCode empty.
                    let sc = unsafe { MapVirtualKeyW(kb.VKey as u32, MAPVK_VK_TO_VSC_EX) };
                    make = (sc & 0xFF) as u16;
                    e0 = e0 || (sc & 0xFF00) == 0xE000;
                }
                (key_from_set1(make, e0), label_from_set1(make, e0))
            };
            id.map(|id| Raw::Key { id, down, vk: kb.VKey, label })
        }
        RIM_TYPEMOUSE => {
            let m = unsafe { data.data.mouse };
            let buttons = unsafe { m.Anonymous.Anonymous.usButtonFlags };
            Some(Raw::Mouse { dx: m.lLastX, dy: m.lLastY, relative: m.usFlags & MOUSE_MOVE_ABSOLUTE == 0, buttons })
        }
        _ => None,
    }
}
