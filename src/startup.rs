//! Per-user Windows login startup registration.

#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Rallx Cheat Launcher";

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Adds or removes Rallx from the current user's Windows login startup list.
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let key = open_run_key()?;
    let value_name = wide(VALUE_NAME);

    if enabled {
        let executable = std::env::current_exe()?;
        let command = registration_command(&executable);
        let data = wide_bytes(&command);
        status_result(unsafe {
            windows::Win32::System::Registry::RegSetValueExW(
                key.0,
                PCWSTR(value_name.as_ptr()),
                0,
                REG_SZ,
                Some(&data),
            )
        })
    } else {
        let status = unsafe { RegDeleteValueW(key.0, PCWSTR(value_name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            status_result(status)
        }
    }
}

fn open_run_key() -> io::Result<RegistryKey> {
    let path = wide(RUN_KEY);
    let mut key = HKEY::default();
    status_result(unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    })?;
    Ok(RegistryKey(key))
}

fn registration_command(executable: &Path) -> OsString {
    let mut command = OsString::from("\"");
    command.push(executable.as_os_str());
    command.push("\"");
    command
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn wide_bytes(value: &OsStr) -> Vec<u8> {
    value
        .encode_wide()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn status_result(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.0 as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_command_quotes_the_executable_path() {
        let command = registration_command(Path::new(r"C:\Program Files\Rallx\rallx.exe"));
        assert_eq!(
            command,
            OsString::from(r#""C:\Program Files\Rallx\rallx.exe""#)
        );
    }

    #[test]
    fn registry_string_is_utf16_little_endian_and_null_terminated() {
        assert_eq!(wide_bytes(OsStr::new("A")), [65, 0, 0, 0]);
    }
}
