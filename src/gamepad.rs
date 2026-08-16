#![allow(dead_code)]

// Polls gilrs on a background thread and forwards gamepad input to the UI
// thread via Weak::upgrade_in_event_loop, since gilrs has no async/event-loop
// integration of its own. A (Button::South) launches the focused trainer row,
// or accepts/confirms whichever popup is currently on top; B (Button::East)
// cancels/closes that popup, or backs out of Settings back to Home. D-Pad
// up/down moves the focused row (home), focused control (settings), or
// focused field/button (add/edit form). D-Pad left/right cycles the value of
// the focused settings control (accent color, background, row style) in
// place, selects a fullscreen window-menu action, or moves between the
// fields/buttons within the add/edit form's focused row. X (West) edits, Y
// (North) opens/starts typing into search,
// Select deletes (with confirm), RB (RightTrigger) copies the launch script,
// and Start opens settings — mirroring the home screen's gamepad hint bar.
// While the virtual keyboard is open, X/Y are repurposed to backspace/space,
// LT (LeftTrigger2) held is a momentary shift, and RT (RightTrigger2) closes
// the keyboard — see AppWindow's show-keyboard-gated functions for why the
// same buttons do double duty instead of needing dedicated ones. A passes
// via-gamepad=true so AppWindow knows this accept came from an actual
// gamepad press and may open the virtual keyboard — the physical Return key
// (handled by AppWindow's global key-scope) reuses the same dispatcher with
// via-gamepad=false so it doesn't.

use std::time::Duration;

use gilrs::{Button, Event, EventType, Gilrs};

use crate::AppWindow;

pub fn spawn_listener(app_weak: slint::Weak<AppWindow>) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(gilrs) => gilrs,
            Err(_) => return,
        };

        loop {
            while let Some(Event { event, .. }) = gilrs.next_event() {
                match event {
                    EventType::ButtonPressed(Button::South, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_gamepad_accept(true);
                        });
                    }
                    EventType::ButtonPressed(Button::East, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_gamepad_cancel();
                        });
                    }
                    EventType::ButtonPressed(Button::DPadDown, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_move_selection(1);
                        });
                    }
                    EventType::ButtonPressed(Button::DPadUp, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_move_selection(-1);
                        });
                    }
                    EventType::ButtonPressed(Button::DPadRight, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_move_horizontal(1);
                        });
                    }
                    EventType::ButtonPressed(Button::DPadLeft, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_move_horizontal(-1);
                        });
                    }
                    EventType::ButtonPressed(Button::West, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_edit_focused_trainer();
                        });
                    }
                    EventType::ButtonPressed(Button::North, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_open_search_keyboard();
                        });
                    }
                    EventType::ButtonPressed(Button::Select, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_delete_focused_trainer();
                        });
                    }
                    EventType::ButtonPressed(Button::RightTrigger, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_copy_focused_trainer();
                        });
                    }
                    EventType::ButtonPressed(Button::Start, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_open_settings_view();
                        });
                    }
                    EventType::ButtonPressed(Button::LeftTrigger2, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_keyboard_shift_hold(true);
                        });
                    }
                    EventType::ButtonReleased(Button::LeftTrigger2, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_keyboard_shift_hold(false);
                        });
                    }
                    EventType::ButtonPressed(Button::RightTrigger2, _) => {
                        let _ = app_weak.upgrade_in_event_loop(|app| {
                            app.invoke_keyboard_right_trigger();
                        });
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}
