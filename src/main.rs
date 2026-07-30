slint::include_modules!();

mod app_state;
mod background;
mod clipboard;
mod config;
mod exe_icon;
mod exe_version;
mod gamepad;
mod hotkey;
mod keys;
mod launch_args;
mod trainer;

/// Slint's default renderer fails on some handheld GPU drivers; the software
/// renderer is the fallback both startup branches share.
fn create_window() -> Result<AppWindow, slint::PlatformError> {
    match AppWindow::new() {
        Ok(app) => Ok(app),
        Err(err) => {
            eprintln!(
                "Failed to initialize default renderer: {err}. Retrying with software renderer..."
            );
            std::env::set_var("SLINT_BACKEND", "winit-software");
            AppWindow::new()
        }
    }
}

/// Tray mode has no window and is usually started from a shortcut or .bat with
/// no console attached, so a failure there would otherwise be completely
/// silent - the process just wouldn't appear. Every startup failure on that
/// branch gets a dialog as well as stderr.
fn fatal(message: &str, code: i32) -> ! {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    eprintln!("{message}");

    let title: Vec<u16> = "Rallx Cheat Launcher\0".encode_utf16().collect();
    let body: Vec<u16> = format!("{message}\0").encode_utf16().collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        )
    };

    std::process::exit(code);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let options = match launch_args::parse(&args) {
        Ok(options) => options,
        Err(err) => fatal(&err.to_string(), 2),
    };

    // Launch options select tray mode: the window is constructed but stays
    // hidden until the tray icon asks for it.
    if let Some(options) = options {
        let app = match create_window() {
            Ok(app) => app,
            Err(err) => fatal(&format!("Could not create the window: {err}"), 1),
        };
        if let Err(err) = background::run(app, &options, config::load_config()) {
            fatal(&err.to_string(), 1);
        }
        return Ok(());
    }

    let app = create_window()?;

    app_state::wire(&app, config::load_config());
    gamepad::spawn_listener(app.as_weak());

    app.run()?;
    Ok(())
}
