//! Running the launcher itself with administrator rights.
//!
//! Windows fixes a process's integrity level at creation, so "run as admin"
//! can only ever mean *relaunching*: a fresh copy is started through the
//! shell's `runas` verb and this one goes away. That is why the setting needs
//! both a toggle (applied on the next startup) and an explicit restart button.

#![allow(unsafe_code)]

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::AppConfig;

/// Set on the relaunched copy so a `runas` that somehow comes back unelevated
/// can't spawn copies of itself forever. Best-effort: it relies on the
/// elevation broker passing this process's environment through, which is why
/// [`is_elevated`] is still the primary guard.
const GUARD_ENV: &str = "RALLX_ELEVATION_ATTEMPTED";

/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)` - the user dismissed the UAC prompt.
/// A deliberate answer rather than a failure, so it gets its own variant.
const E_CANCELLED: u32 = 0x8007_04C7;

#[derive(Debug)]
pub enum ElevateError {
    Cancelled,
    Failed(String),
}

impl fmt::Display for ElevateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "the administrator prompt was dismissed"),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

/// Whether this process is running elevated. Gates both the startup handoff and
/// the Settings restart button, and explains why injected cheats get dropped.
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::TOKEN_QUERY;
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = Default::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        CloseHandle(token).ok();

        queried && elevation.TokenIsElevated != 0
    }
}

/// Whether startup should hand off to an elevated copy before doing anything
/// else.
pub fn wants_startup_elevation(config: &AppConfig) -> bool {
    config.run_as_admin && !is_elevated() && std::env::var_os(GUARD_ENV).is_none()
}

/// Set by the Settings screen's restart button and acted on only once the event
/// loop has exited - see [`finish_requested_restart`].
static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn request_restart() {
    RESTART_REQUESTED.store(true, Ordering::SeqCst);
}

/// Carries out a restart the UI asked for. Must be called *after* the event
/// loop has exited and every OS-level singleton this process holds (the global
/// hotkey registration, the tray icon) has been dropped, or the elevated copy
/// fails to claim them. Does not return when the relaunch succeeds.
pub fn finish_requested_restart() {
    if !RESTART_REQUESTED.load(Ordering::SeqCst) {
        return;
    }

    match relaunch_as_admin() {
        Ok(()) => std::process::exit(0),
        // The window is already gone by this point, so the user is told why
        // the app closed without coming back rather than being left guessing.
        Err(ElevateError::Cancelled) => crate::dialog::warning(
            "Restart as administrator was cancelled, so Rallx Cheat Launcher has closed.",
        ),
        Err(err) => {
            crate::dialog::error(&format!("Could not restart as administrator: {err}"));
        }
    }
}

/// Starts an elevated copy of this executable with the same command line. The
/// caller decides what to do next - startup falls back to running unelevated,
/// the restart path exits.
pub fn relaunch_as_admin() -> Result<(), ElevateError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().map_err(|err| ElevateError::Failed(err.to_string()))?;
    let directory = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    let arguments = current_arguments();

    std::env::set_var(GUARD_ENV, "1");

    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let directory = wide(&directory.to_string_lossy());
    let parameters = wide(&arguments);

    // SEE_MASK_NOASYNC keeps this call blocked until the user has answered the
    // UAC prompt; without it the request is abandoned when the caller exits,
    // which is exactly what both callers do a moment later.
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpDirectory: PCWSTR(directory.as_ptr()),
        lpParameters: if arguments.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(parameters.as_ptr())
        },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    let result = unsafe { ShellExecuteExW(&mut info) };

    if !info.hProcess.is_invalid() {
        unsafe { CloseHandle(info.hProcess) }.ok();
    }

    result.map_err(|err| {
        if err.code().0 as u32 == E_CANCELLED {
            ElevateError::Cancelled
        } else {
            ElevateError::Failed(err.message())
        }
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// This process's command line with `argv[0]` removed, so the relaunch is
/// character-for-character what this copy was started with. Rebuilding it from
/// `env::args` would mean re-quoting, and values like
/// `--defaultcheat="ctrl+num1,num3"` are easy to get wrong that way.
fn current_arguments() -> String {
    use windows::Win32::System::Environment::GetCommandLineW;

    let line = unsafe { GetCommandLineW().to_string() }.unwrap_or_default();
    strip_program_name(&line).to_string()
}

/// `argv[0]` follows its own parsing rule on Windows: quoted means "up to the
/// next quote", unquoted means "up to the first whitespace", and backslash
/// escapes don't apply to it either way.
fn strip_program_name(line: &str) -> &str {
    let rest = if let Some(after_quote) = line.strip_prefix('"') {
        match after_quote.find('"') {
            Some(end) => &after_quote[end + 1..],
            None => "",
        }
    } else {
        match line.find(char::is_whitespace) {
            Some(end) => &line[end..],
            None => "",
        }
    };

    rest.trim_start()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_an_unquoted_program_name() {
        assert_eq!(
            strip_program_name("rallx.exe --launch=\"a b.exe\" --hotkey=\"Insert\""),
            "--launch=\"a b.exe\" --hotkey=\"Insert\""
        );
    }

    // The usual shape when the app is started from a shortcut or .bat: the
    // path is quoted because it contains spaces, and the closing quote is not
    // the end of the line.
    #[test]
    fn strips_a_quoted_program_name_containing_spaces() {
        assert_eq!(
            strip_program_name("\"C:\\Program Files\\Rallx\\rallx.exe\" --launch=\"t.exe\""),
            "--launch=\"t.exe\""
        );
    }

    #[test]
    fn a_bare_program_name_leaves_no_arguments() {
        assert_eq!(strip_program_name("rallx.exe"), "");
        assert_eq!(strip_program_name("\"C:\\a b\\rallx.exe\""), "");
        assert_eq!(strip_program_name("rallx.exe   "), "");
    }

    #[test]
    fn startup_elevation_needs_the_setting_and_an_unelevated_process() {
        let mut config = AppConfig {
            run_as_admin: false,
            ..AppConfig::default()
        };
        assert!(!wants_startup_elevation(&config));

        config.run_as_admin = true;
        // Written against the live token so the result holds whether or not the
        // test runner itself was started elevated: an already-elevated process
        // must never ask again.
        assert_eq!(wants_startup_elevation(&config), !is_elevated());
    }
}
