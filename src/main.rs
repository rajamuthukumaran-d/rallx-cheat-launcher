// Release builds are launched by double-clicking the exe (and in tray mode from
// a shortcut), where a console window would be visible noise. Debug builds keep
// the console so `cargo run` still shows the eprintln! diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use slint::ComponentHandle;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

mod app_state;
mod background;
mod clipboard;
mod config;
mod dialog;
mod dragdrop;
mod elevate;
mod exe_icon;
mod exe_version;
mod gamepad;
mod hotkey;
mod key_capture;
mod keys;
mod launch_args;
mod renderer;
mod trainer;

/// Slint's default renderer fails on some handheld GPU drivers; the software
/// renderer is the fallback both startup branches share.
///
/// This only covers a backend that fails to build at all. The renderer itself
/// is created lazily once the event loop shows a window, so the common failure
/// lands in [`renderer::recover`] instead.
pub fn create_window() -> Result<AppWindow, slint::PlatformError> {
    match AppWindow::new() {
        Ok(app) => Ok(app),
        Err(err) => {
            eprintln!(
                "Failed to initialize default renderer: {err}. Retrying with software renderer..."
            );
            renderer::force_software_backend();
            AppWindow::new()
        }
    }
}

/// Tray mode has no window and is usually started from a shortcut or .bat with
/// no console attached, so a failure there would otherwise be completely
/// silent - the process just wouldn't appear. Every startup failure on that
/// branch gets a dialog as well as stderr.
fn fatal(message: &str, code: i32) -> ! {
    dialog::error(message);
    std::process::exit(code);
}

/// Hands a dropped .exe path to the UI. Runs inside the window procedure, so
/// the work is deferred rather than re-entering the UI from a native message.
///
/// The drop point arrives in physical pixels and is divided by the scale factor
/// on the way in, since every coordinate the UI hit-tests it against is a Slint
/// logical length.
fn on_exe_dropped(app_weak: slint::Weak<AppWindow>) -> impl Fn(dragdrop::Drop) + 'static {
    move |drop| {
        let app_weak = app_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            let scale = app.window().scale_factor();
            app.invoke_exe_dropped(
                drop.path.to_string_lossy().to_string().into(),
                drop.x as f32 / scale,
                drop.y as f32 / scale,
            );
        });
    }
}

fn on_numpad_pressed(app_weak: slint::Weak<AppWindow>) -> impl Fn(String) -> bool + 'static {
    move |combo| {
        let Some(app) = app_weak.upgrade() else {
            return false;
        };
        if !app.get_recording() {
            return false;
        }
        let app_weak = app_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = app_weak.upgrade() {
                app.set_recorded_combo(keys::format_combo_for_display(&combo).into());
            }
        });
        true
    }
}

/// Installs the file-drop handler once the window it needs actually exists.
///
/// `show()` doesn't get us there: Slint creates the native window (and with it
/// the HWND to subclass) on the event loop's first pass, so at startup there is
/// nothing to attach to yet. Hence the poll - it stops on the first success.
///
/// Failing outright costs only this one way of adding a trainer, since the add
/// icon and the Browse… picker are unaffected, so it's reported and ignored.
fn enable_drag_drop(app: &AppWindow) {
    const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
    const MAX_ATTEMPTS: u32 = 40;

    let app_weak = app.as_weak();
    let attempts = std::cell::Cell::new(0u32);
    let timer = std::rc::Rc::new(slint::Timer::default());

    // The closure owns the only surviving handle to the timer it belongs to,
    // which is what keeps the timer alive past this function. stop() doesn't
    // break that cycle - it just takes the timer off the active list, which is
    // all that's wanted, since it never has to fire again.
    let handle = timer.clone();
    timer.start(slint::TimerMode::Repeated, RETRY_INTERVAL, move || {
        let Some(app) = app_weak.upgrade() else {
            handle.stop();
            return;
        };

        match dragdrop::enable(app.window(), on_exe_dropped(app.as_weak())) {
            Ok(()) => handle.stop(),
            Err(dragdrop::DragDropError::NoWindowHandle) if attempts.get() < MAX_ATTEMPTS => {
                attempts.set(attempts.get() + 1);
            }
            Err(err) => {
                eprintln!("Drag and drop unavailable: {err}");
                handle.stop();
            }
        }
    });
}

/// Installs physical numpad capture once Slint has created the HWND.
fn enable_key_capture(app: &AppWindow) {
    const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
    const MAX_ATTEMPTS: u32 = 40;

    let app_weak = app.as_weak();
    let attempts = std::cell::Cell::new(0u32);
    let timer = std::rc::Rc::new(slint::Timer::default());
    let handle = timer.clone();

    timer.start(slint::TimerMode::Repeated, RETRY_INTERVAL, move || {
        let Some(app) = app_weak.upgrade() else {
            handle.stop();
            return;
        };

        match key_capture::enable(app.window(), on_numpad_pressed(app.as_weak())) {
            Ok(()) => handle.stop(),
            Err(key_capture::KeyCaptureError::NoWindowHandle) if attempts.get() < MAX_ATTEMPTS => {
                attempts.set(attempts.get() + 1);
            }
            Err(err) => {
                eprintln!("Physical numpad capture unavailable: {err}");
                handle.stop();
            }
        }
    });
}

/// Shows the normal app window and starts the input integrations that require
/// a native HWND. A tray-started process has no HWND until its first Open
/// action, so these cannot be installed unconditionally during startup.
fn show_windowed_app(
    app: &AppWindow,
    window_features_started: &Cell<bool>,
) -> Result<(), slint::PlatformError> {
    app.window().set_minimized(false);
    app.show()?;
    if !window_features_started.replace(true) {
        enable_drag_drop(app);
        enable_key_capture(app);
        gamepad::spawn_listener(app.as_weak());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let options = match launch_args::parse(&args) {
        Ok(options) => options,
        Err(err) => fatal(&err.to_string(), 2),
    };

    let config = config::load_config();

    // Integrity level is fixed when a process is created, so honoring "run as
    // administrator" means handing the whole startup over to a fresh elevated
    // copy - before either branch below builds anything. Both branches get it:
    // tray mode is where key injection needs elevation most.
    if elevate::wants_startup_elevation(&config) {
        match elevate::relaunch_as_admin() {
            Ok(()) => return Ok(()),
            // Declining UAC leaves the app usable (and able to turn the setting
            // back off) instead of refusing to start at all.
            Err(elevate::ElevateError::Cancelled) => {}
            Err(err) => dialog::warning(&format!(
                "Could not start as administrator: {err}\n\n\
                 Continuing without administrator rights."
            )),
        }
    }

    // Launch options select tray mode: the window is constructed but stays
    // hidden until the tray icon asks for it.
    if let Some(options) = options {
        let app = match create_window() {
            Ok(app) => app,
            Err(err) => fatal(&format!("Could not create the window: {err}"), 1),
        };
        if let Err(err) = background::run(app, &options, config) {
            fatal(&err.to_string(), 1);
        }
        return Ok(());
    }

    let start_in_background = config.run_in_background;
    let app = create_window()?;

    app_state::wire(&app, config, app_state::AppMode::Windowed);
    let tray = WindowedTrayIcon::new()?;
    let window_features_started = Rc::new(Cell::new(false));

    {
        let app_weak = app.as_weak();
        let window_features_started = window_features_started.clone();
        tray.on_show_window(move || {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            if let Err(err) = show_windowed_app(&app, &window_features_started) {
                dialog::error(&format!("Could not show Rallx: {err}"));
            }
        });
    }
    tray.on_quit_app(|| {
        let _ = slint::quit_event_loop();
    });

    {
        let tray_weak = tray.as_weak();
        app.on_run_in_background_changed(move |enabled| {
            let Some(tray) = tray_weak.upgrade() else {
                return;
            };
            let result = if enabled { tray.show() } else { tray.hide() };
            if let Err(err) = result {
                dialog::error(&format!("Could not update the system tray icon: {err}"));
            }
        });
    }

    // run_event_loop_until_quit is deliberate: once minimized, no app window
    // remains visible, but the normal-mode tray and global game-matching
    // hotkey must keep running until Exit is chosen.
    app.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });

    let minimize_timer = slint::Timer::default();
    {
        let app_weak = app.as_weak();
        minimize_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                let Some(app) = app_weak.upgrade() else {
                    return;
                };
                if app.get_run_in_background() && app.window().is_minimized() {
                    // Clear the native minimized state before hiding so Open
                    // from the tray restores a normal window rather than a
                    // hidden-but-still-minimized one.
                    app.window().set_minimized(false);
                    let _ = app.hide();
                }
            },
        );
    }

    let outcome = if start_in_background {
        tray.show()?;
        slint::run_event_loop_until_quit()
    } else {
        match show_windowed_app(&app, &window_features_started) {
            Ok(()) => slint::run_event_loop_until_quit(),
            Err(err) => Err(err),
        }
    };
    let _ = app.hide();
    drop(minimize_timer);
    drop(tray);
    drop(app);

    elevate::finish_requested_restart();

    // Nothing outside this process has happened yet, so a renderer failure here
    // is always safe to restart from.
    renderer::recover(outcome, false)
}
