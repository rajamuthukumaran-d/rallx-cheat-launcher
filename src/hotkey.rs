//! System-wide hotkey registration, used by background/tray mode to trigger a
//! trainer launch while a game has focus.

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::keys::KeyCombo;

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
}

impl HotkeyManager {
    pub fn new() -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new()
            .map_err(|err| format!("could not initialize global hotkeys: {err}"))?;
        Ok(Self {
            manager,
            registered: Vec::new(),
        })
    }

    /// Registers `combo` system-wide and returns the id its events carry.
    pub fn register(&mut self, combo: &KeyCombo) -> Result<u32, String> {
        let canonical = combo.canonical();
        let hotkey: HotKey = canonical
            .parse()
            .map_err(|err| format!("{canonical} is not a usable hotkey: {err}"))?;
        self.manager.register(hotkey).map_err(|err| {
            format!("could not register {canonical} (another app may already own it): {err}")
        })?;
        self.registered.push(hotkey);
        Ok(hotkey.id())
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let _ = self.manager.unregister_all(&self.registered);
    }
}

/// Drains pending hotkey events and reports whether `id` was pressed. Key-up
/// events are discarded so one physical press triggers exactly once.
pub fn was_pressed(id: u32) -> bool {
    let receiver = GlobalHotKeyEvent::receiver();
    let mut pressed = false;
    while let Ok(event) = receiver.try_recv() {
        if event.id() == id && event.state() == HotKeyState::Pressed {
            pressed = true;
        }
    }
    pressed
}
