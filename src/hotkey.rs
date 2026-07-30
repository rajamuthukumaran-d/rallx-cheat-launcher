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

/// Drains pending hotkey events and returns the ids that were pressed, each at
/// most once. Key-up events are discarded so one physical press reports
/// exactly once.
///
/// The `global-hotkey` receiver is process-wide, so this hands back every id it
/// saw rather than filtering to one: filtering here would consume and discard
/// events belonging to any other registration.
pub fn drain_pressed() -> Vec<u32> {
    let receiver = GlobalHotKeyEvent::receiver();
    let mut pressed = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        if event.state() == HotKeyState::Pressed && !pressed.contains(&event.id()) {
            pressed.push(event.id());
        }
    }
    pressed
}
