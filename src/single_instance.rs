//! Normal-mode single-instance coordination.
//!
//! A second normal launch signals the first process through a named event and
//! exits. The first process polls that event from Slint's event loop so a
//! tray-hidden window can be recreated without touching UI state from a worker
//! thread.

#![allow(unsafe_code)]

use std::fmt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenMutexW, SetEvent, WaitForSingleObject,
    SYNCHRONIZATION_SYNCHRONIZE,
};

#[derive(Debug)]
pub struct SingleInstanceError(String);

impl fmt::Display for SingleInstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SingleInstanceError {}

pub enum Claim {
    Primary(SingleInstance),
    Existing,
}

pub struct SingleInstance {
    mutex: HANDLE,
    activation_event: HANDLE,
}

impl SingleInstance {
    pub fn claim() -> Result<Claim, SingleInstanceError> {
        let names = object_names()?;
        let mutex = unsafe { CreateMutexW(None, false, PCWSTR(names.mutex.as_ptr())) }
            .map_err(|err| SingleInstanceError(format!("could not create instance lock: {err}")))?;
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        let activation_event =
            match unsafe { CreateEventW(None, false, false, PCWSTR(names.event.as_ptr())) } {
                Ok(event) => event,
                Err(err) => {
                    unsafe { CloseHandle(mutex) }.ok();
                    return Err(SingleInstanceError(format!(
                        "could not create activation signal: {err}"
                    )));
                }
            };

        if already_exists {
            let result = unsafe { SetEvent(activation_event) };
            unsafe { CloseHandle(activation_event) }.ok();
            unsafe { CloseHandle(mutex) }.ok();
            result.map_err(|err| {
                SingleInstanceError(format!("could not activate the existing app: {err}"))
            })?;
            return Ok(Claim::Existing);
        }

        Ok(Claim::Primary(Self {
            mutex,
            activation_event,
        }))
    }

    pub fn activation_requested(&self) -> bool {
        (unsafe { WaitForSingleObject(self.activation_event, 0) }) == WAIT_OBJECT_0
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.activation_event) }.ok();
        unsafe { CloseHandle(self.mutex) }.ok();
    }
}

/// Signals an already-running normal instance before the elevation handoff.
/// This avoids showing another UAC prompt merely to discover that the elevated
/// copy is already running.
pub fn activate_existing() -> Result<bool, SingleInstanceError> {
    let names = object_names()?;
    let mutex = match unsafe {
        OpenMutexW(
            SYNCHRONIZATION_SYNCHRONIZE,
            false,
            PCWSTR(names.mutex.as_ptr()),
        )
    } {
        Ok(mutex) => mutex,
        Err(_) => return Ok(false),
    };
    unsafe { CloseHandle(mutex) }.ok();

    // CreateEvent also opens an event created by the primary process. Creating
    // it here closes the tiny race where the primary has claimed its mutex but
    // has not created the companion event yet.
    let activation_event =
        unsafe { CreateEventW(None, false, false, PCWSTR(names.event.as_ptr())) }.map_err(
            |err| SingleInstanceError(format!("could not open activation signal: {err}")),
        )?;
    let result = unsafe { SetEvent(activation_event) };
    unsafe { CloseHandle(activation_event) }.ok();
    result.map_err(|err| {
        SingleInstanceError(format!("could not activate the existing app: {err}"))
    })?;
    Ok(true)
}

struct ObjectNames {
    mutex: Vec<u16>,
    event: Vec<u16>,
}

fn object_names() -> Result<ObjectNames, SingleInstanceError> {
    let exe = std::env::current_exe().map_err(|err| {
        SingleInstanceError(format!("could not identify the app executable: {err}"))
    })?;
    let normalized = exe.to_string_lossy().to_lowercase();

    // A deterministic path suffix lets separately installed portable copies
    // run independently while every shortcut to the same executable converges
    // on one process.
    let hash = normalized
        .encode_utf16()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, unit| {
            (hash ^ u64::from(unit)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    let stem = format!(r"Local\RallxCheatLauncher_{hash:016x}");

    Ok(ObjectNames {
        mutex: wide(&format!("{stem}_Mutex")),
        event: wide(&format!("{stem}_Activate")),
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_signals_the_primary_instance() {
        let Claim::Primary(primary) = SingleInstance::claim().expect("claims test instance") else {
            panic!("test executable unexpectedly already has an instance")
        };
        assert!(!primary.activation_requested());

        assert!(activate_existing().expect("signals the primary"));
        assert!(primary.activation_requested());
        assert!(!primary.activation_requested(), "the event auto-resets");

        assert!(matches!(
            SingleInstance::claim().expect("opens the existing instance"),
            Claim::Existing
        ));
        assert!(primary.activation_requested());
    }
}
