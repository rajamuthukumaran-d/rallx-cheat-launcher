// Slint has no file-drop event, so the drop target is wired at the Win32
// level: the top-level window opts into WM_DROPFILES and a subclass proc pulls
// the dropped path out, handing every other message straight back to winit's
// own window proc.

use std::ffi::{c_void, OsString};
use std::fmt;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Ole::RevokeDragDrop;
use windows::Win32::UI::Shell::{
    DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, RemoveWindowSubclass,
    SetWindowSubclass, HDROP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, MSGFLT_ALLOW, WM_COPYDATA, WM_DROPFILES, WM_NCDESTROY,
};

type Handler = Box<dyn Fn(PathBuf)>;

const SUBCLASS_ID: usize = 1;

/// Not exposed by the `windows` crate: the private message the shell uses to
/// carry the dropped filenames across the process boundary. Without it in the
/// filter, an elevated window is handed a WM_DROPFILES with nothing behind it.
const WM_COPYGLOBALDATA: u32 = 0x0049;

#[derive(Debug)]
pub enum DragDropError {
    /// The window has no HWND yet - it hasn't been shown.
    NoWindowHandle,
    SubclassFailed,
}

impl fmt::Display for DragDropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWindowHandle => write!(f, "the window has not been shown yet"),
            Self::SubclassFailed => write!(f, "could not install the window subclass"),
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

/// Reads the first path out of a drop. Multi-file drops are truncated to one on
/// purpose: the Add-trainer form holds a single executable.
unsafe fn first_dropped_path(hdrop: HDROP) -> Option<PathBuf> {
    let needed = DragQueryFileW(hdrop, 0, None);
    if needed == 0 {
        return None;
    }

    let mut buffer = vec![0u16; needed as usize + 1];
    let copied = DragQueryFileW(hdrop, 0, Some(&mut buffer));
    if copied == 0 {
        return None;
    }

    Some(PathBuf::from(OsString::from_wide(
        &buffer[..copied as usize],
    )))
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
        WM_DROPFILES => {
            let hdrop = HDROP(wparam.0 as *mut c_void);
            let path = first_dropped_path(hdrop);
            DragFinish(hdrop);

            if let Some(path) = path {
                (*(handler as *const Handler))(path);
            }
            LRESULT(0)
        }
        // Last message the window ever gets, so it's the only safe point to
        // take the subclass back off and free the boxed handler.
        WM_NCDESTROY => {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), subclass_id);
            drop(Box::from_raw(handler as *mut Handler));
            result
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

/// Makes `window` accept dropped files, calling `on_file` with the first path of
/// every drop. The window must already be shown - its HWND doesn't exist before
/// that - and must not be hidden again afterwards, since winit destroys the
/// native window (and with it this subclass) on hide.
///
/// `on_file` runs inside the window procedure, so it should hand work off to the
/// event loop rather than re-entering the UI directly.
pub fn enable(
    window: &slint::Window,
    on_file: impl Fn(PathBuf) + 'static,
) -> Result<(), DragDropError> {
    let hwnd = window_hwnd(window).ok_or(DragDropError::NoWindowHandle)?;
    let handler: *mut Handler = Box::into_raw(Box::new(Box::new(on_file)));

    unsafe {
        // winit registers an OLE IDropTarget on every window it creates, and an
        // OLE drop target takes the whole drop: the shell talks to that
        // interface and never posts WM_DROPFILES, however much the window says
        // it accepts files. Slint drops winit's file-drop events on the floor,
        // so nothing is lost by revoking it. (winit revokes it again itself when
        // the window is destroyed; the second call is a harmless no-op.)
        let _ = RevokeDragDrop(hwnd);
        DragAcceptFiles(hwnd, true);

        // UIPI silently drops these when they come from a lower-integrity
        // process, which is every Explorer window once "run as administrator"
        // is on. Failures are ignored: at normal integrity the filter is
        // irrelevant, and refusing to start over it would be worse.
        for message in [WM_DROPFILES, WM_COPYDATA, WM_COPYGLOBALDATA] {
            let _ = ChangeWindowMessageFilterEx(hwnd, message, MSGFLT_ALLOW, None);
        }

        if !SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, handler as usize).as_bool() {
            drop(Box::from_raw(handler));
            DragAcceptFiles(hwnd, false);
            return Err(DragDropError::SubclassFailed);
        }
    }

    Ok(())
}
