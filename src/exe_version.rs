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

/// Looks up one sub-block and returns its bytes, borrowed back out of `buffer`
/// rather than handed over as a raw pointer so the lifetime is checked. The
/// length `VerQueryValue` reports is in UTF-16 characters for a string value
/// and in bytes for everything else, hence `unit` - and it is trusted only as
/// far as `buffer` actually extends, so a truncated or hand-edited resource
/// can't walk off the end.
fn query<'a>(buffer: &'a [u8], sub_block: &str, unit: usize) -> Option<&'a [u8]> {
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

    let offset = (value as usize).checked_sub(buffer.as_ptr() as usize)?;
    let end = (len as usize)
        .checked_mul(unit)?
        .checked_add(offset)?
        .min(buffer.len());
    buffer.get(offset..end)
}

fn query_bytes<'a>(buffer: &'a [u8], sub_block: &str) -> Option<&'a [u8]> {
    query(buffer, sub_block, 1)
}

/// Reads a UTF-16 value a code unit at a time: the block sits at whatever
/// offset into a `Vec<u8>` the resource puts it, which carries no alignment
/// guarantee for `u16`.
fn query_utf16(buffer: &[u8], sub_block: &str) -> Option<Vec<u16>> {
    let bytes = query(buffer, sub_block, size_of::<u16>())?;
    Some(
        bytes
            .chunks_exact(size_of::<u16>())
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect(),
    )
}

/// Marks a block as a real VS_FIXEDFILEINFO rather than whatever else happens
/// to sit at that offset.
const VS_FFI_SIGNATURE: u32 = 0xFEEF_04BD;

pub fn extract_version(path: &Path) -> Option<String> {
    let buffer = version_info(path)?;
    let bytes = query_bytes(&buffer, "\\")?;
    if bytes.len() < size_of::<VS_FIXEDFILEINFO>() {
        return None;
    }

    // Unaligned: the struct is addressed as an offset into a byte buffer.
    let info = unsafe { ptr::read_unaligned(bytes.as_ptr() as *const VS_FIXEDFILEINFO) };
    if info.dwSignature != VS_FFI_SIGNATURE {
        return None;
    }

    let major = info.dwFileVersionMS >> 16;
    let minor = info.dwFileVersionMS & 0xFFFF;
    let build = info.dwFileVersionLS >> 16;
    let revision = info.dwFileVersionLS & 0xFFFF;

    Some(format!("{major}.{minor}.{build}.{revision}"))
}

/// The language/code-page pair the string table is keyed on. Only the first
/// translation is used - a trainer shipping several is not worth guessing at.
fn translation(buffer: &[u8]) -> Option<(u16, u16)> {
    let bytes = query_bytes(buffer, "\\VarFileInfo\\Translation")?;
    if bytes.len() < 4 {
        return None;
    }

    Some((
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
    ))
}

fn string_value(buffer: &[u8], sub_block: &str) -> Option<String> {
    // Unlike the binary blocks above, a string value's length comes back as a
    // character count that includes the terminating NUL.
    let chars = query_utf16(buffer, sub_block)?;

    let text = String::from_utf16_lossy(chars.split(|&c| c == 0).next().unwrap_or_default());
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Code pages to fall back on when the Translation entry doesn't match the key
/// the StringFileInfo block is actually stored under - a common enough mismatch
/// in the wild to be worth a second guess. These two are all that shows up in
/// practice: Unicode and Windows Multilingual.
const FALLBACK_CODE_PAGES: [u16; 2] = [0x04b0, 0x04e4];

/// The name the exe calls itself, for pre-filling the Add-trainer form.
/// FileDescription first because that's what Explorer shows as the file's
/// description and it's usually the specific trainer name, where ProductName
/// tends to be the vendor's suite.
pub fn extract_display_name(path: &Path) -> Option<String> {
    let buffer = version_info(path)?;
    let (language, code_page) = translation(&buffer)?;

    std::iter::once(code_page)
        .chain(
            FALLBACK_CODE_PAGES
                .into_iter()
                .filter(|cp| *cp != code_page),
        )
        .find_map(|cp| {
            let prefix = format!("\\StringFileInfo\\{language:04x}{cp:04x}");
            ["FileDescription", "ProductName"]
                .into_iter()
                .find_map(|field| string_value(&buffer, &format!("{prefix}\\{field}")))
        })
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

    // The one exercise of the happy path: nothing in the repo carries a version
    // resource, and a system exe is the only PE guaranteed to be on any machine
    // this Windows-only app builds on. Covers the assumptions the raw blocks are
    // read under - the fixed-info signature, and a string length that counts
    // UTF-16 characters rather than bytes.
    #[test]
    fn a_system_exe_reports_its_own_version_and_name() {
        let path = std::path::PathBuf::from(r"C:\Windows\System32\notepad.exe");
        if !path.exists() {
            return;
        }

        let version = extract_version(&path).expect("notepad.exe carries a version resource");
        assert_eq!(version.split('.').count(), 4, "got {version}");
        assert!(version.split('.').all(|part| part.parse::<u32>().is_ok()));

        let name = extract_display_name(&path).expect("notepad.exe names itself");
        assert!(!name.is_empty());
        assert_eq!(name.trim(), name);
    }
}
