slint::include_modules!();

use slint::ComponentHandle;

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
fn on_exe_dropped(app_weak: slint::Weak<AppWindow>) -> impl Fn(std::path::PathBuf) + 'static {
    move |path| {
        let app_weak = app_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = app_weak.upgrade() {
                app.invoke_exe_dropped(path.to_string_lossy().to_string().into());
            }
        });
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

    let app = create_window()?;

    app_state::wire(&app, config);
    gamepad::spawn_listener(app.as_weak());

    // Shown explicitly rather than through app.run() because the window has no
    // HWND to hang the file-drop subclass off until it exists.
    let outcome = match app.show() {
        Ok(()) => {
            enable_drag_drop(&app);
            slint::run_event_loop()
        }
        Err(err) => Err(err),
    };
    let _ = app.hide();
    drop(app);

    elevate::finish_requested_restart();

    // Nothing outside this process has happened yet, so a renderer failure here
    // is always safe to restart from.
    renderer::recover(outcome, false)
}
