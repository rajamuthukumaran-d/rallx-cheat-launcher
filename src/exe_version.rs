#![allow(dead_code)]

// Reads the version resource embedded in an .exe: the VS_FIXEDFILEINFO block
// for the version number the Home screen shows, and the StringFileInfo block
// for the name the exe gives itself. Everything here returns None for a file
// without a version resource - not every trainer .exe has one - so callers can
// fall back to the filename rather than showing a fabricated value.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn version_info(path: &Path) -> Option<Vec<u8>> {
    let path_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_ptr = PCWSTR(path_wide.as_ptr());

    unsafe {
        let size = GetFileVersionInfoSizeW(path_ptr, None);
        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        GetFileVersionInfoW(path_ptr, 0, size, buffer.as_mut_ptr() as *mut c_void).ok()?;
        Some(buffer)
    }
}

/// Looks up one sub-block. The returned pointer points *into* `buffer`, so it's
/// only valid while the caller still holds it.
fn query(buffer: &[u8], sub_block: &str) -> Option<(*const c_void, u32)> {
    let block = wide(sub_block);
    let mut value: *mut c_void = ptr::null_mut();
    let mut len: u32 = 0;

    let found = unsafe {
        VerQueryValueW(
            buffer.as_ptr() as *const c_void,
            PCWSTR(block.as_ptr()),
            &mut value,
            &mut len,
        )
    };

    if !found.as_bool() || value.is_null() || len == 0 {
        return None;
    }
    Some((value.cast_const(), len))
}

pub fn extract_version(path: &Path) -> Option<String> {
    let buffer = version_info(path)?;
    let (value, _) = query(&buffer, "\\")?;

    let info = unsafe { &*(value as *const VS_FIXEDFILEINFO) };
    let major = info.dwFileVersionMS >> 16;
    let minor = info.dwFileVersionMS & 0xFFFF;
    let build = info.dwFileVersionLS >> 16;
    let revision = info.dwFileVersionLS & 0xFFFF;

    Some(format!("{major}.{minor}.{build}.{revision}"))
}

/// The language/code-page pair the string table is keyed on. Only the first
/// translation is used - a trainer shipping several is not worth guessing at.
fn translation(buffer: &[u8]) -> Option<(u16, u16)> {
    let (value, len) = query(buffer, "\\VarFileInfo\\Translation")?;
    if len < 4 {
        return None;
    }

    let entry = unsafe { std::slice::from_raw_parts(value as *const u16, 2) };
    Some((entry[0], entry[1]))
}

fn string_value(buffer: &[u8], sub_block: &str) -> Option<String> {
    // Unlike the binary blocks above, a string value's length comes back as a
    // character count that includes the terminating NUL.
    let (value, len) = query(buffer, sub_block)?;
    let chars = unsafe { std::slice::from_raw_parts(value as *const u16, len as usize) };

    let text = String::from_utf16_lossy(chars.split(|&c| c == 0).next().unwrap_or_default());
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The name the exe calls itself, for pre-filling the Add-trainer form.
/// FileDescription first because that's what Explorer shows as the file's
/// description and it's usually the specific trainer name, where ProductName
/// tends to be the vendor's suite.
pub fn extract_display_name(path: &Path) -> Option<String> {
    let buffer = version_info(path)?;
    let (language, code_page) = translation(&buffer)?;
    let prefix = format!("\\StringFileInfo\\{language:04x}{code_page:04x}");

    ["FileDescription", "ProductName"]
        .into_iter()
        .find_map(|field| string_value(&buffer, &format!("{prefix}\\{field}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Plenty of trainers ship no version resource at all, and every lookup here
    // has to degrade to None rather than a fabricated value.
    #[test]
    fn a_file_without_a_version_resource_yields_nothing() {
        let path =
            std::env::temp_dir().join(format!("rallx-test-noversion-{}.exe", std::process::id()));
        std::fs::write(&path, b"not a real pe").unwrap();

        let version = extract_version(&path);
        let name = extract_display_name(&path);

        std::fs::remove_file(&path).unwrap();

        assert_eq!(version, None);
        assert_eq!(name, None);
    }
}
