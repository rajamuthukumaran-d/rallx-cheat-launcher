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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let options = match launch_args::parse(&args) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    let app = create_window()?;

    // Launch options select tray mode: the window is constructed but stays
    // hidden until the tray icon asks for it.
    if let Some(options) = options {
        if let Err(err) = background::run(app, &options, config::load_config()) {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return Ok(());
    }

    app_state::wire(&app, config::load_config());
    gamepad::spawn_listener(app.as_weak());

    app.run()?;
    Ok(())
}
