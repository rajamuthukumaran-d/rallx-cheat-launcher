//! Minimal Win32 clipboard writer for the Home screen's "copy launch script"
//! action. Slint exposes no clipboard API, and this is the only clipboard use
//! in the app, so it doesn't warrant a dependency.

#![allow(unsafe_code)]

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

pub fn set_text(text: &str) -> Result<(), windows::core::Error> {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = std::mem::size_of_val(utf16.as_slice());

    unsafe {
        OpenClipboard(None)?;

        let result = (|| -> Result<HGLOBAL, windows::core::Error> {
            EmptyClipboard()?;
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let destination = GlobalLock(handle);
            if destination.is_null() {
                let _ = GlobalFree(handle);
                return Err(windows::core::Error::from_win32());
            }
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), destination.cast::<u16>(), utf16.len());
            let _ = GlobalUnlock(handle);
            Ok(handle)
        })();

        let outcome = match result {
            // Ownership of the handle passes to the clipboard on success only;
            // freeing it afterwards would corrupt the clipboard contents.
            Ok(handle) => match SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(handle.0)) {
                Ok(_) => Ok(()),
                Err(err) => {
                    let _ = GlobalFree(handle);
                    Err(err)
                }
            },
            Err(err) => Err(err),
        };

        // The write's own result wins: a failure to close afterwards must not
        // mask why the text never made it onto the clipboard.
        let closed = CloseClipboard();
        outcome.and(closed)
    }
}
