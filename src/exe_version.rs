#![allow(dead_code)]

// Reads the FileVersion resource embedded in an .exe (the VS_FIXEDFILEINFO
// block every PE built with version info carries) so the Home screen can
// show the trainer's real version. Returns None for anything without a
// version resource - not every trainer .exe has one - so callers can hide
// the version entirely rather than showing a fabricated value.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};

pub fn extract_version(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_ptr = PCWSTR(wide.as_ptr());

    unsafe {
        let size = GetFileVersionInfoSizeW(path_ptr, None);
        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        GetFileVersionInfoW(path_ptr, 0, size, buffer.as_mut_ptr() as *mut c_void).ok()?;

        let mut fixed_info_ptr: *mut c_void = ptr::null_mut();
        let mut fixed_info_len: u32 = 0;
        let root: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();

        let found = VerQueryValueW(
            buffer.as_ptr() as *const c_void,
            PCWSTR(root.as_ptr()),
            &mut fixed_info_ptr,
            &mut fixed_info_len,
        );
        if !found.as_bool() || fixed_info_ptr.is_null() {
            return None;
        }

        let info = &*(fixed_info_ptr as *const VS_FIXEDFILEINFO);
        let major = info.dwFileVersionMS >> 16;
        let minor = info.dwFileVersionMS & 0xFFFF;
        let build = info.dwFileVersionLS >> 16;
        let revision = info.dwFileVersionLS & 0xFFFF;

        Some(format!("{major}.{minor}.{build}.{revision}"))
    }
}
