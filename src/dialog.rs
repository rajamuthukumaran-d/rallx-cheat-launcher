//! Message boxes for the paths that have no other way to reach the user:
//! background/tray mode has no window, and a shortcut- or .bat-launched
//! process has no console for stderr either.

#![allow(unsafe_code)]

use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONWARNING, MB_OK, MESSAGEBOX_STYLE,
};

fn show(message: &str, style: MESSAGEBOX_STYLE) {
    let title: Vec<u16> = "Rallx Cheat Launcher\0".encode_utf16().collect();
    let body: Vec<u16> = format!("{message}\0").encode_utf16().collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            style | MB_OK,
        )
    };
}

pub fn error(message: &str) {
    eprintln!("{message}");
    show(message, MB_ICONERROR);
}

pub fn warning(message: &str) {
    eprintln!("{message}");
    show(message, MB_ICONWARNING);
}
