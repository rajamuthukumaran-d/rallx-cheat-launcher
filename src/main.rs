slint::include_modules!();

mod app_state;
mod config;
mod exe_icon;
mod exe_version;
mod gamepad;
mod hotkey;
mod trainer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let mut launch_exe = None;
    let mut hotkey = None;
    let mut default_cheat = None;

    for arg in args.iter().skip(1) {
        if arg.starts_with("--launch=") {
            launch_exe = Some(arg.trim_start_matches("--launch=").to_string());
        } else if arg.starts_with("--hotkey=") {
            hotkey = Some(arg.trim_start_matches("--hotkey=").to_string());
        } else if arg.starts_with("--defaultcheat=") {
            default_cheat = Some(arg.trim_start_matches("--defaultcheat=").to_string());
        }
    }

    if launch_exe.is_some() || hotkey.is_some() || default_cheat.is_some() {
        println!("Running in background mode:");
        println!("Launch: {:?}", launch_exe);
        println!("Hotkey: {:?}", hotkey);
        println!("Cheats: {:?}", default_cheat);
        return Ok(());
    }

    let app_result = AppWindow::new();
    let app = match app_result {
        Ok(app) => app,
        Err(e) => {
            eprintln!(
                "Failed to initialize default renderer: {}. Retrying with software renderer...",
                e
            );
            std::env::set_var("SLINT_BACKEND", "winit-software");
            AppWindow::new()?
        }
    };

    let (mut config, folder_path) = config::load_config();
    config.trainer_folder = folder_path;
    app_state::wire(&app, config);

    app.run()?;
    Ok(())
}
