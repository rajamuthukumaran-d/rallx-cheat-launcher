//! Native keyboard capture for keys Slint cannot identify physically.
//!
//! Slint exposes logical key text, so top-row `1` and Numpad `1` both arrive
//! as `"1"`. This window subclass reads Win32 scan codes before Slint handles
//! them and reports physical numpad keys while the recorder is active.

use std::ffi::c_void;
use std::fmt;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_KEYDOWN, WM_NCDESTROY, WM_SYSKEYDOWN};

type Handler = Box<dyn Fn(String) -> bool>;

const SUBCLASS_ID: usize = 2;

#[derive(Debug)]
pub enum KeyCaptureError {
    NoWindowHandle,
    SubclassFailed,
}

impl fmt::Display for KeyCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWindowHandle => write!(f, "the window has not been shown yet"),
            Self::SubclassFailed => write!(f, "could not install the keyboard capture subclass"),
        }
    }
}

fn window_hwnd(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle();
    match HasWindowHandle::window_handle(&handle).ok()?.as_raw() {
        RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut c_void)),
        _ => None,
    }
}

fn numpad_key(scan_code: u8, extended: bool) -> Option<&'static str> {
    Some(match (scan_code, extended) {
        (0x52, false) => "Numpad0",
        (0x4f, false) => "Numpad1",
        (0x50, false) => "Numpad2",
        (0x51, false) => "Numpad3",
        (0x4b, false) => "Numpad4",
        (0x4c, false) => "Numpad5",
        (0x4d, false) => "Numpad6",
        (0x47, false) => "Numpad7",
        (0x48, false) => "Numpad8",
        (0x49, false) => "Numpad9",
        (0x53, false) => "NumpadDecimal",
        (0x4e, false) => "NumpadAdd",
        (0x4a, false) => "NumpadSubtract",
        (0x37, false) => "NumpadMultiply",
        (0x35, true) => "NumpadDivide",
        (0x1c, true) => "NumpadEnter",
        _ => return None,
    })
}

unsafe fn modifier_down(key: u16) -> bool {
    GetKeyState(key as i32) < 0
}

unsafe fn numpad_combo(lparam: LPARAM) -> Option<String> {
    let scan_code = ((lparam.0 >> 16) & 0xff) as u8;
    let extended = (lparam.0 & (1 << 24)) != 0;
    let key = numpad_key(scan_code, extended)?;
    let mut combo = String::new();

    for (down, label) in [
        (modifier_down(VK_CONTROL.0), "Ctrl"),
        (modifier_down(VK_MENU.0), "Alt"),
        (modifier_down(VK_SHIFT.0), "Shift"),
        (modifier_down(VK_LWIN.0) || modifier_down(VK_RWIN.0), "Meta"),
    ] {
        if down {
            combo.push_str(label);
            combo.push_str(" + ");
        }
    }
    combo.push_str(key);
    Some(combo)
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    handler: usize,
) -> LRESULT {
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if let Some(combo) = numpad_combo(lparam) {
                if (*(handler as *const Handler))(combo) {
                    return LRESULT(0);
                }
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        WM_NCDESTROY => {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), subclass_id);
            drop(Box::from_raw(handler as *mut Handler));
            result
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

pub fn enable(
    window: &slint::Window,
    on_numpad: impl Fn(String) -> bool + 'static,
) -> Result<(), KeyCaptureError> {
    let hwnd = window_hwnd(window).ok_or(KeyCaptureError::NoWindowHandle)?;
    let handler: *mut Handler = Box::into_raw(Box::new(Box::new(on_numpad)));

    unsafe {
        if !SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, handler as usize).as_bool() {
            drop(Box::from_raw(handler));
            return Err(KeyCaptureError::SubclassFailed);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::numpad_key;

    #[test]
    fn keypad_scan_codes_are_distinct_from_extended_navigation_keys() {
        assert_eq!(numpad_key(0x4f, false), Some("Numpad1"));
        assert_eq!(numpad_key(0x4f, true), None);
        assert_eq!(numpad_key(0x53, false), Some("NumpadDecimal"));
    }

    #[test]
    fn extended_keypad_keys_are_recognized() {
        assert_eq!(numpad_key(0x35, true), Some("NumpadDivide"));
        assert_eq!(numpad_key(0x1c, true), Some("NumpadEnter"));
        assert_eq!(numpad_key(0x1c, false), None);
    }
}
