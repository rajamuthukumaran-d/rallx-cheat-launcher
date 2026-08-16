//! Normal-mode single-instance coordination.
//!
//! The named event is both the single-instance claim and the activation signal.
//! A second normal launch opens and signals it, then exits. The first process
//! polls the event from Slint's event loop so a tray-hidden window can be
//! recreated without touching UI state from a worker thread.

#![allow(unsafe_code)]

use std::fmt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

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
    activation_event: HANDLE,
}

impl SingleInstance {
    pub fn claim() -> Result<Claim, SingleInstanceError> {
        let name = event_name()?;
        let activation_event =
            match unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) } {
                Ok(event) => event,
                Err(err) => {
                    return Err(SingleInstanceError(format!(
                        "could not create activation signal: {err}"
                    )))
                }
            };
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        if already_exists {
            signal_existing(activation_event)?;
            return Ok(Claim::Existing);
        }

        Ok(Claim::Primary(Self { activation_event }))
    }

    pub fn activation_requested(&self) -> bool {
        (unsafe { WaitForSingleObject(self.activation_event, 0) }) == WAIT_OBJECT_0
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.activation_event) }.ok();
    }
}

/// Signals an already-running normal instance before the elevation handoff.
/// This avoids showing another UAC prompt merely to discover that the elevated
/// copy is already running.
pub fn activate_existing() -> Result<bool, SingleInstanceError> {
    let name = event_name()?;
    let activation_event =
        match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) } {
            Ok(event) => event,
            Err(_) => return Ok(false),
        };
    signal_existing(activation_event)?;
    Ok(true)
}

fn signal_existing(activation_event: HANDLE) -> Result<(), SingleInstanceError> {
    // The user-initiated secondary process normally owns Windows' foreground
    // permission. Pass it on before waking the primary so its UI-thread call to
    // SetForegroundWindow is allowed to activate the restored window.
    if let Err(err) = unsafe { AllowSetForegroundWindow(ASFW_ANY) } {
        eprintln!("Could not transfer foreground permission: {err}");
    }

    let result = unsafe { SetEvent(activation_event) };
    unsafe { CloseHandle(activation_event) }.ok();
    result.map_err(|err| SingleInstanceError(format!("could not activate the existing app: {err}")))
}

fn event_name() -> Result<Vec<u16>, SingleInstanceError> {
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
    Ok(wide(&format!(
        r"Local\RallxCheatLauncher_{hash:016x}_Activate"
    )))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_signals_the_primary_instance() {
        assert!(!activate_existing().expect("finds no instance before the first claim"));

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

        drop(primary);
        assert!(!activate_existing().expect("releases the instance claim"));
    }
}
