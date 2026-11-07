//! Wine/Proton runtime detection and diagnostics.
//!
//! The Windows build also runs under Wine, so compile-time `cfg` values cannot
//! distinguish real Windows from a compatibility prefix. Wine exposes a
//! private marker from ntdll specifically for runtime identification.

#![allow(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

static IS_WINE: OnceLock<bool> = OnceLock::new();

pub fn is_wine() -> bool {
    *IS_WINE.get_or_init(|| {
        use windows::core::{s, w};
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

        let Ok(ntdll) = (unsafe { GetModuleHandleW(w!("ntdll.dll")) }) else {
            return false;
        };

        unsafe { GetProcAddress(ntdll, s!("wine_get_version")) }.is_some()
    })
}

/// Release builds have no console, and Heroic's game log does not reliably
/// include child-process stderr. Keep compatibility diagnostics beside the
/// executable, where the portable config already lives.
pub fn log(message: &str) {
    if !is_wine() {
        return;
    }

    let path = crate::config::get_config_path().with_file_name("rallx-proton.log");
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "{message}");
}
