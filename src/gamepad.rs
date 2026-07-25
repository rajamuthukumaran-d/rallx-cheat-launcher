#![allow(dead_code)]

// Polls gilrs on a background thread and forwards gamepad input to the UI
// thread via Weak::upgrade_in_event_loop, since gilrs has no async/event-loop
// integration of its own. A (Button::South) launches the focused trainer row;
// D-Pad up/down moves the focused row, mirroring the keyboard's arrow keys.

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
                            app.invoke_launch_focused_trainer();
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
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}
