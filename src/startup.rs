//! Per-user Windows login startup registration.

#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE, WIN32_ERROR,
};
use windows::Win32::Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, CREATE_NO_WINDOW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Rallx Cheat Launcher";
const TASK_NAME: &str = "Rallx Cheat Launcher";
const TASK_DELAY: &str = "0000:15";
const HRESULT_FILE_NOT_FOUND: i32 = 0x8007_0002u32 as i32;
const HRESULT_PATH_NOT_FOUND: i32 = 0x8007_0003u32 as i32;

#[derive(Debug, PartialEq, Eq)]
enum LoginRegistration {
    Disabled,
    Registry,
    ElevatedTask,
}

struct RegistryKey(HKEY);

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Adds or removes Rallx from the current user's Windows login startup list.
/// Elevated startup uses Task Scheduler so Windows can honor a previously
/// approved highest-privilege task without prompting at every sign-in.
pub fn set_enabled(enabled: bool, run_as_admin: bool) -> io::Result<()> {
    match registration_for(enabled, run_as_admin) {
        LoginRegistration::Disabled => remove_all(),
        LoginRegistration::Registry => {
            ensure_process_matches_interactive_user()?;
            enable_registry_startup()?;
            delete_elevated_task()
        }
        LoginRegistration::ElevatedTask => {
            ensure_process_matches_interactive_user()?;
            create_elevated_task()?;
            disable_registry_startup()
        }
    }
}

fn registration_for(enabled: bool, run_as_admin: bool) -> LoginRegistration {
    match (enabled, run_as_admin) {
        (false, _) => LoginRegistration::Disabled,
        (true, false) => LoginRegistration::Registry,
        (true, true) => LoginRegistration::ElevatedTask,
    }
}

fn enable_registry_startup() -> io::Result<()> {
    let key = open_run_key()?;
    let value_name = wide(VALUE_NAME);
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
}

fn disable_registry_startup() -> io::Result<()> {
    let key = open_run_key()?;
    let value_name = wide(VALUE_NAME);
    let status = unsafe { RegDeleteValueW(key.0, PCWSTR(value_name.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        status_result(status)
    }
}

fn create_elevated_task() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let command = registration_command(&executable);
    let output = run_schtasks(&[
        OsStr::new("/Create"),
        OsStr::new("/TN"),
        OsStr::new(TASK_NAME),
        OsStr::new("/TR"),
        command.as_os_str(),
        OsStr::new("/SC"),
        OsStr::new("ONLOGON"),
        OsStr::new("/DELAY"),
        OsStr::new(TASK_DELAY),
        OsStr::new("/RL"),
        OsStr::new("HIGHEST"),
        OsStr::new("/IT"),
        OsStr::new("/F"),
    ])?;
    output_result("create the elevated login task", output)
}

fn delete_elevated_task() -> io::Result<()> {
    if !elevated_task_exists()? {
        return Ok(());
    }

    let output = run_schtasks(&[
        OsStr::new("/Delete"),
        OsStr::new("/TN"),
        OsStr::new(TASK_NAME),
        OsStr::new("/F"),
    ])?;
    output_result("delete the elevated login task", output)
}

fn elevated_task_exists() -> io::Result<bool> {
    let output = run_schtasks(&[
        OsStr::new("/Query"),
        OsStr::new("/TN"),
        OsStr::new(TASK_NAME),
        OsStr::new("/HRESULT"),
    ])?;
    match classify_task_query(output.status.success(), output.status.code()) {
        TaskQueryResult::Exists => Ok(true),
        TaskQueryResult::Missing => Ok(false),
        TaskQueryResult::Failed => {
            output_result("query the elevated login task", output)?;
            unreachable!("a successful task query was classified as failed")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TaskQueryResult {
    Exists,
    Missing,
    Failed,
}

fn classify_task_query(success: bool, exit_code: Option<i32>) -> TaskQueryResult {
    if success {
        TaskQueryResult::Exists
    } else if matches!(
        exit_code,
        Some(HRESULT_FILE_NOT_FOUND | HRESULT_PATH_NOT_FOUND)
    ) {
        TaskQueryResult::Missing
    } else {
        TaskQueryResult::Failed
    }
}

fn remove_all() -> io::Result<()> {
    let registry_result = disable_registry_startup();
    let task_result = delete_elevated_task();

    match (registry_result, task_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), Ok(())) | (Ok(()), Err(err)) => Err(err),
        (Err(registry_err), Err(task_err)) => Err(io::Error::other(format!(
            "could not remove the registry entry ({registry_err}) or scheduled task ({task_err})"
        ))),
    }
}

fn run_schtasks(arguments: &[&OsStr]) -> io::Result<Output> {
    Command::new("schtasks.exe")
        .args(arguments)
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
}

fn output_result(operation: &str, output: Output) -> io::Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let message = if detail.is_empty() {
        format!(
            "could not {operation} (exit code {:?})",
            output.status.code()
        )
    } else {
        format!("could not {operation}: {detail}")
    };
    Err(io::Error::other(message))
}

fn ensure_process_matches_interactive_user() -> io::Result<()> {
    let shell_window = unsafe { GetShellWindow() };
    if shell_window.0.is_null() {
        return Err(io::Error::other(
            "could not identify the signed-in Windows user because Explorer is not ready",
        ));
    }

    let mut shell_process_id = 0;
    unsafe { GetWindowThreadProcessId(shell_window, Some(&mut shell_process_id)) };
    if shell_process_id == 0 {
        return Err(io::Error::last_os_error());
    }

    let shell_process = OwnedHandle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, shell_process_id) }
            .map_err(|err| io::Error::other(err.to_string()))?,
    );
    let current_user = process_user_token(unsafe { GetCurrentProcess() })?;
    let interactive_user = process_user_token(shell_process.0)?;

    let same_user = unsafe {
        EqualSid(
            token_user_sid(&current_user),
            token_user_sid(&interactive_user),
        )
        .is_ok()
    };
    if same_user {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows login startup cannot be configured while Rallx is using administrator credentials for a different account; sign in with an administrator account or turn off Run as administrator",
        ))
    }
}

fn process_user_token(process: HANDLE) -> io::Result<Vec<usize>> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|err| io::Error::other(err.to_string()))?;
    let token = OwnedHandle(token);

    let mut byte_count = 0;
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut byte_count) };
    if byte_count == 0 {
        return Err(io::Error::last_os_error());
    }

    let word_count = (byte_count as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; word_count];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            byte_count,
            &mut byte_count,
        )
    }
    .map_err(|err| io::Error::other(err.to_string()))?;
    Ok(buffer)
}

fn token_user_sid(buffer: &[usize]) -> windows::Win32::Security::PSID {
    unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
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

    #[test]
    fn administrator_mode_selects_the_elevated_login_task() {
        assert_eq!(
            registration_for(true, true),
            LoginRegistration::ElevatedTask
        );
        assert_eq!(registration_for(true, false), LoginRegistration::Registry);
        assert_eq!(registration_for(false, true), LoginRegistration::Disabled);
    }

    #[test]
    fn task_query_ignores_only_not_found_hresult_values() {
        assert_eq!(
            classify_task_query(false, Some(HRESULT_FILE_NOT_FOUND)),
            TaskQueryResult::Missing
        );
        assert_eq!(
            classify_task_query(false, Some(HRESULT_PATH_NOT_FOUND)),
            TaskQueryResult::Missing
        );
        assert_eq!(
            classify_task_query(false, Some(0x8007_0005u32 as i32)),
            TaskQueryResult::Failed
        );
        assert_eq!(classify_task_query(false, None), TaskQueryResult::Failed);
        assert_eq!(classify_task_query(true, Some(0)), TaskQueryResult::Exists);
    }
}
