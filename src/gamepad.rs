#![allow(dead_code)]

pub struct GamepadManager;

impl Default for GamepadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GamepadManager {
    pub fn new() -> Self {
        GamepadManager
    }

    pub fn poll_events(&mut self) {
        // Stub for gilrs gamepad events polling
    }
}
