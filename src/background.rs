//! Background/tray mode: the branch `main` takes when launch options are
//! present. No window is shown at startup - the app sits in the system tray,
//! waits for its hotkey, then launches the trainer and injects the configured
//! default cheats.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use crate::config::AppConfig;
use crate::keys::{self, KeyCombo};
use crate::launch_args::LaunchOptions;
use crate::{app_state, gamepad, hotkey, trainer, AppWindow, TrayIcon};

/// Head start the trainer gets before the first cheat combo is injected -
/// keystrokes sent while it is still initializing are dropped.
const LAUNCH_SETTLE: Duration = Duration::from_millis(1000);

/// Gap between consecutive cheat combos. Trainers debounce their own hotkeys,
/// so back-to-back combos would register as one.
const CHEAT_INTERVAL: Duration = Duration::from_millis(150);

/// How often the event loop drains the global-hotkey channel.
const HOTKEY_POLL: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundPlan {
    pub folder: PathBuf,
    pub filename: String,
    pub display_name: String,
    /// `None` launches the trainer immediately at startup, since nothing else
    /// would ever trigger it.
    pub hotkey: Option<KeyCombo>,
    pub cheats: Vec<KeyCombo>,
    pub close_after_launch: bool,
}

/// Resolves launch options against config.json: unspecified values fall back to
/// the trainer's own saved shortcut and cheats, then to the global default
/// shortcut.
pub fn plan(options: &LaunchOptions, config: &AppConfig) -> Result<BackgroundPlan, String> {
    let Some(folder) = config.trainer_folder.clone() else {
        return Err("no trainer folder is configured - open Rallx and pick one first".to_string());
    };

    let Some(requested) = options
        .trainer
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Err("--launch is required in background mode".to_string());
    };

    // A hand-written script tends to carry the full path it was copied from,
    // but trainers are always resolved inside the configured folder, so only
    // the file name part is meaningful.
    let requested = Path::new(requested)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(requested);

    // Trainers are referenced by filename, but matching the display name too
    // makes a hand-written script forgiving.
    let saved = config
        .trainers
        .iter()
        .find(|entry| entry.filename.eq_ignore_ascii_case(requested))
        .or_else(|| {
            config
                .trainers
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(requested))
        });

    let filename = saved
        .map(|entry| entry.filename.clone())
        .unwrap_or_else(|| requested.to_string());

    if !folder.join(&filename).is_file() {
        return Err(format!("{filename} was not found in {}", folder.display()));
    }

    let hotkey_source = options
        .hotkey
        .clone()
        .or_else(|| saved.and_then(|entry| entry.launch_shortcut.clone()))
        .or_else(|| config.default_shortcut.clone());

    let hotkey = match hotkey_source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => Some(keys::parse_combo(value).map_err(|err| format!("--hotkey: {err}"))?),
        None => None,
    };

    let cheats = match options.default_cheats.as_deref() {
        Some(list) => {
            keys::parse_combo_list(list).map_err(|err| format!("--defaultcheat: {err}"))?
        }
        None => saved
            .map(|entry| {
                entry
                    .default_cheats
                    .iter()
                    .filter(|cheat| !cheat.key.trim().is_empty())
                    .map(|cheat| keys::parse_combo(&cheat.key))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
            .map_err(|err| format!("saved default cheat: {err}"))?
            .unwrap_or_default(),
    };

    Ok(BackgroundPlan {
        folder,
        display_name: saved.map(|entry| entry.name.clone()).unwrap_or_else(|| {
            PathBuf::from(&filename)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&filename)
                .to_string()
        }),
        filename,
        hotkey,
        cheats,
        close_after_launch: options.close_after_launch,
    })
}

/// Launches the trainer (first trigger only) and injects the default cheats.
/// Runs off the UI thread because the injection sequence sleeps between combos.
fn trigger(plan: Arc<BackgroundPlan>, launched: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        if !launched.swap(true, Ordering::SeqCst) {
            if let Err(err) = trainer::launch_trainer(&plan.folder, &plan.filename) {
                eprintln!("Failed to launch {}: {err}", plan.filename);
                launched.store(false, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(LAUNCH_SETTLE);
        }

        for (index, combo) in plan.cheats.iter().enumerate() {
            if index > 0 {
                std::thread::sleep(CHEAT_INTERVAL);
            }
            if let Err(err) = keys::press(combo) {
                eprintln!("Failed to send {combo}: {err}");
            }
        }

        if plan.close_after_launch {
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        }
    });
}

pub fn run(
    app: AppWindow,
    options: &LaunchOptions,
    config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = Arc::new(plan(options, &config)?);
    let launched = Arc::new(AtomicBool::new(false));

    app_state::wire(&app, config);
    // The window stays hidden until the tray asks for it; closing it returns to
    // the tray rather than ending the process, which is what keeps the hotkey
    // alive. "Exit" in the tray menu is the way out.
    app.window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);

    let tray = TrayIcon::new()?;
    tray.set_trainer_name(plan.display_name.as_str().into());

    {
        let app_weak = app.as_weak();
        let gamepad_started = std::cell::Cell::new(false);
        tray.on_show_window(move || {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            if let Err(err) = app.show() {
                eprintln!("Failed to show the window: {err}");
                return;
            }
            // Gamepad polling drives the visible UI only, so it is started on
            // the first show instead of costing a thread while in the tray.
            if !gamepad_started.replace(true) {
                gamepad::spawn_listener(app.as_weak());
            }
        });
    }

    {
        let plan = plan.clone();
        let launched = launched.clone();
        tray.on_launch_trainer(move || trigger(plan.clone(), launched.clone()));
    }

    tray.on_quit_app(|| {
        let _ = slint::quit_event_loop();
    });

    tray.show()?;

    // Kept alive for the whole event loop; dropping either would silently stop
    // the hotkey from firing.
    let mut _manager = None;
    let poll_timer = Timer::default();

    match plan.hotkey {
        Some(combo) => {
            let mut manager = hotkey::HotkeyManager::new()?;
            let id = manager.register(&combo)?;
            _manager = Some(manager);

            let plan = plan.clone();
            let launched = launched.clone();
            poll_timer.start(TimerMode::Repeated, HOTKEY_POLL, move || {
                if hotkey::was_pressed(id) {
                    trigger(plan.clone(), launched.clone());
                }
            });
        }
        None => trigger(plan.clone(), launched.clone()),
    }

    slint::run_event_loop_until_quit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CheatConfig, TrainerConfig};

    fn trainer_folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rallx-test-bg-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rdr2-trainer.exe"), b"exe").unwrap();
        dir
    }

    fn config(folder: &std::path::Path) -> AppConfig {
        AppConfig {
            trainer_folder: Some(folder.to_path_buf()),
            default_shortcut: Some("Ctrl+F12".to_string()),
            trainers: vec![TrainerConfig {
                name: "RDR2".to_string(),
                filename: "rdr2-trainer.exe".to_string(),
                version: "1.0".to_string(),
                size_bytes: 3,
                game_exe: None,
                launch_shortcut: Some("Insert".to_string()),
                default_cheats: vec![
                    CheatConfig {
                        label: "Health".to_string(),
                        key: "Numpad1".to_string(),
                    },
                    CheatConfig {
                        label: "Unset".to_string(),
                        key: String::new(),
                    },
                ],
                close_after_launch: true,
            }],
            ..AppConfig::default()
        }
    }

    fn options(trainer: &str) -> LaunchOptions {
        LaunchOptions {
            trainer: Some(trainer.to_string()),
            ..LaunchOptions::default()
        }
    }

    #[test]
    fn cli_values_win_over_saved_ones() {
        let folder = trainer_folder("cli");
        let plan = plan(
            &LaunchOptions {
                trainer: Some("rdr2-trainer.exe".to_string()),
                hotkey: Some("ctrl+num9".to_string()),
                default_cheats: Some("num1,ctrl+num2".to_string()),
                close_after_launch: true,
            },
            &config(&folder),
        )
        .unwrap();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(plan.hotkey.unwrap().canonical(), "Ctrl+Numpad9");
        let cheats: Vec<String> = plan.cheats.iter().map(KeyCombo::canonical).collect();
        assert_eq!(cheats, ["Numpad1", "Ctrl+Numpad2"]);
        assert!(plan.close_after_launch);
    }

    #[test]
    fn omitted_values_fall_back_to_the_saved_trainer() {
        let folder = trainer_folder("saved");
        let plan = plan(&options("rdr2-trainer.exe"), &config(&folder)).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(plan.hotkey.unwrap().canonical(), "Insert");
        // The cheat with no key assigned is skipped rather than failing.
        let cheats: Vec<String> = plan.cheats.iter().map(KeyCombo::canonical).collect();
        assert_eq!(cheats, ["Numpad1"]);
        assert_eq!(plan.display_name, "RDR2");
        // Per-trainer close_after_launch only generates the CLI flag; it must
        // not switch the behavior on by itself.
        assert!(!plan.close_after_launch);
    }

    #[test]
    fn falls_back_to_the_global_shortcut_then_to_no_hotkey() {
        let folder = trainer_folder("global");
        let mut cfg = config(&folder);
        cfg.trainers[0].launch_shortcut = None;
        let with_global = plan(&options("rdr2-trainer.exe"), &cfg).unwrap();

        cfg.default_shortcut = None;
        let without = plan(&options("rdr2-trainer.exe"), &cfg).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(with_global.hotkey.unwrap().canonical(), "Ctrl+F12");
        assert_eq!(without.hotkey, None);
    }

    #[test]
    fn matches_a_trainer_by_display_name_and_ignores_case() {
        let folder = trainer_folder("byname");
        let by_name = plan(&options("rdr2"), &config(&folder)).unwrap();
        let by_file = plan(&options("RDR2-Trainer.EXE"), &config(&folder)).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(by_name.filename, "rdr2-trainer.exe");
        assert_eq!(by_file.filename, "rdr2-trainer.exe");
    }

    // Copying the trainer's full path out of Explorer is the obvious thing to
    // do, so it resolves to the same entry as the bare filename.
    #[test]
    fn accepts_a_full_path_by_using_its_file_name() {
        let folder = trainer_folder("fullpath");
        let plan = plan(
            &options("C:\\Users\\me\\Downloads\\Trainer\\rdr2-trainer.exe"),
            &config(&folder),
        )
        .unwrap();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(plan.filename, "rdr2-trainer.exe");
        assert_eq!(plan.display_name, "RDR2");
    }

    #[test]
    fn rejects_a_trainer_that_is_not_in_the_folder() {
        let folder = trainer_folder("missing");
        let err = plan(&options("nope.exe"), &config(&folder)).unwrap_err();
        std::fs::remove_dir_all(&folder).unwrap();

        assert!(err.contains("nope.exe"), "{err}");
    }

    #[test]
    fn rejects_an_unparsable_combo() {
        let folder = trainer_folder("badkey");
        let err = plan(
            &LaunchOptions {
                trainer: Some("rdr2-trainer.exe".to_string()),
                default_cheats: Some("num1,banana".to_string()),
                ..LaunchOptions::default()
            },
            &config(&folder),
        )
        .unwrap_err();
        std::fs::remove_dir_all(&folder).unwrap();

        assert!(err.contains("banana"), "{err}");
    }
}
