#![allow(dead_code)]

pub struct HotkeyManager;

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyManager {
    pub fn new() -> Self {
        HotkeyManager
    }

    pub fn register_hotkey(&mut self, _shortcut: &str) -> Result<(), String> {
        // Stub for global hotkey registration
        Ok(())
    }
}
