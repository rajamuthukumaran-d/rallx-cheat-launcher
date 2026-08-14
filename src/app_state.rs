#![allow(dead_code)]

// In-memory trainer state and the Rust side of the Slint callback wiring.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, SharedString, Timer, TimerMode, VecModel,
};

use crate::config::{AppConfig, CheatConfig, TrainerConfig};
use crate::{
    clipboard, elevate, exe_icon, exe_version, hotkey, keys, launch_args, trainer, AppWindow,
    CheatEntry, Palette, Theme, TrainerItem,
};

// Placeholders the UI shows for an unassigned value; also the sentinels the
// save path treats as "nothing configured" when writing config.json.
const NO_EXE_PLACEHOLDER: &str = "No executable selected";
const NO_GAME_PLACEHOLDER: &str = "No game selected";
const NO_WATCHED_EXE_PLACEHOLDER: &str = "No app selected";
const NOT_SET: &str = "Not set";
const GLOBAL_HOTKEY_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Windowed,
    Background,
}

struct RegisteredWindowHotkey {
    canonical: String,
    id: u32,
    _manager: hotkey::HotkeyManager,
}

const ROW_COLORS: [(u8, u8, u8); 6] = [
    (0x5b, 0x8c, 0xff),
    (0xff, 0x9f, 0x5b),
    (0x5b, 0xff, 0xb0),
    (0xff, 0x5b, 0x8c),
    (0xc8, 0x90, 0x5b),
    (0x8c, 0x5b, 0xff),
];

struct AppState {
    trainers: RefCell<Vec<TrainerItem>>,
    next_trainer_id: Cell<i32>,
    next_cheat_id: Cell<i32>,
    config: RefCell<AppConfig>,
    window_hotkey: RefCell<Option<RegisteredWindowHotkey>>,
    hotkey_poll_timer: Timer,
    hotkey_scan_in_progress: Arc<AtomicBool>,
}

fn row_color(index: usize) -> Color {
    let (r, g, b) = ROW_COLORS[index % ROW_COLORS.len()];
    Color::from_rgb_u8(r, g, b)
}

fn make_cheat(state: &AppState, label: &str, key: &str) -> CheatEntry {
    let id = state.next_cheat_id.get();
    state.next_cheat_id.set(id + 1);
    CheatEntry {
        id,
        label: label.into(),
        key: key.into(),
    }
}

struct NewTrainer<'a> {
    name: &'a str,
    version: &'a str,
    size: &'a str,
    exe: &'a str,
    game_exe: &'a str,
    game_args: &'a str,
    watched_exe: &'a str,
    shortcut: &'a str,
    cheats: Vec<CheatEntry>,
    icon: Option<Image>,
}

fn make_trainer(state: &AppState, fields: NewTrainer) -> TrainerItem {
    let id = state.next_trainer_id.get();
    state.next_trainer_id.set(id + 1);
    let color = row_color(id as usize);
    let letter = fields
        .name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    TrainerItem {
        id,
        name: fields.name.into(),
        version: fields.version.into(),
        size: fields.size.into(),
        exe: fields.exe.into(),
        game_exe: fields.game_exe.into(),
        game_args: fields.game_args.into(),
        watched_exe: fields.watched_exe.into(),
        shortcut: fields.shortcut.into(),
        color,
        letter: letter.into(),
        has_icon: fields.icon.is_some(),
        icon: fields.icon.unwrap_or_default(),
        cheats: ModelRc::new(VecModel::from(fields.cheats)),
    }
}

fn format_size(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

fn config_to_trainer_item(
    state: &AppState,
    cfg: &TrainerConfig,
    exe_path: Option<&std::path::Path>,
) -> TrainerItem {
    let cheats: Vec<CheatEntry> = cfg
        .default_cheats
        .iter()
        .map(|cheat| {
            let key = if cheat.key.is_empty() {
                NOT_SET.to_string()
            } else {
                keys::format_combo_for_display(&cheat.key)
            };
            make_cheat(state, &cheat.label, &key)
        })
        .collect();
    let icon = exe_path.and_then(exe_icon::extract_icon);
    let size = format_size(cfg.size_bytes);
    let shortcut = cfg
        .launch_shortcut
        .as_deref()
        .map(keys::format_combo_for_display)
        .unwrap_or_else(|| NOT_SET.to_string());
    make_trainer(
        state,
        NewTrainer {
            name: &cfg.name,
            version: &cfg.version,
            size: &size,
            exe: &cfg.filename,
            game_exe: cfg.game_exe.as_deref().unwrap_or_default(),
            game_args: cfg.game_args.as_deref().unwrap_or_default(),
            watched_exe: cfg.watched_exe.as_deref().unwrap_or_default(),
            shortcut: &shortcut,
            cheats,
            icon,
        },
    )
}

fn trainer_items_from_config(state: &AppState, config: &AppConfig) -> Vec<TrainerItem> {
    config
        .trainers
        .iter()
        .map(|cfg| {
            let exe_path = config
                .trainer_folder
                .as_ref()
                .map(|f| f.join(&cfg.filename));
            config_to_trainer_item(state, cfg, exe_path.as_deref())
        })
        .collect()
}

/// Re-scans the configured trainer folder, reconciling discovered files
/// against saved metadata, persists the result, and returns the fresh
/// display list. No-op (returns the existing list) if no folder is set.
fn rescan_trainer_folder(state: &AppState) -> Vec<TrainerItem> {
    let mut config = state.config.borrow_mut();
    let Some(folder) = config.trainer_folder.clone() else {
        drop(config);
        return trainer_items_from_config(state, &state.config.borrow());
    };

    if let Ok(trainers) = trainer::sync_trainer_configs(&config.trainers, &folder) {
        config.trainers = trainers;
    }
    // Saved even when the scan failed: config.json is the only record of which
    // folder was picked, so a newly chosen one must survive an unreadable dir.
    let _ = crate::config::save_config(&config);

    trainer_items_from_config(state, &config)
}

fn parse_leading_mb(size: &str) -> f64 {
    size.split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn filtered(trainers: &[TrainerItem], query: &str) -> Vec<TrainerItem> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return trainers.to_vec();
    }
    trainers
        .iter()
        .filter(|t| t.name.to_lowercase().contains(query.as_str()))
        .cloned()
        .collect()
}

fn refresh_trainer_list(app: &AppWindow, state: &AppState) {
    let trainers = state.trainers.borrow();
    let visible = filtered(&trainers, app.get_search_query().as_str());
    let total_size: f64 = trainers.iter().map(|t| parse_leading_mb(&t.size)).sum();

    app.set_visible_trainers(ModelRc::new(VecModel::from(visible)));
    app.set_trainer_count(trainers.len() as i32);
    app.set_total_size_label(SharedString::from(format!("{:.1} MB", total_size)));
}

fn cheats_to_vec(cheats: &ModelRc<CheatEntry>) -> Vec<CheatEntry> {
    cheats.iter().collect()
}

fn find_trainer(state: &AppState, id: i32) -> Option<TrainerItem> {
    state.trainers.borrow().iter().find(|t| t.id == id).cloned()
}

/// Applies a virtual-keyboard edit (insert/backspace) to whichever field
/// `keyboard_target` names ("search" | "name" | "cheat" | "game-args"), then mirrors the
/// result back into `keyboard_preview` so the popup's own display stays in
/// sync. The actual string mutation happens here in Rust rather than in
/// Slint, since Slint's imperative string API has no substring/pop support.
fn apply_keyboard_edit(app: &AppWindow, state: &AppState, edit: impl FnOnce(&mut String)) {
    let mut text = app.get_keyboard_preview().to_string();
    edit(&mut text);
    app.set_keyboard_preview(text.clone().into());

    match app.get_keyboard_target().as_str() {
        "search" => {
            app.set_search_query(text.into());
            refresh_trainer_list(app, state);
        }
        "name" => app.set_form_name(text.into()),
        "game-args" => app.set_form_game_args(text.into()),
        "cheat" => {
            let id = app.get_keyboard_cheat_id();
            let cheats: Vec<CheatEntry> = cheats_to_vec(&app.get_form_cheats())
                .into_iter()
                .map(|mut c| {
                    if c.id == id {
                        c.label = text.clone().into();
                    }
                    c
                })
                .collect();
            app.set_form_cheats(ModelRc::new(VecModel::from(cheats)));
        }
        _ => {}
    }
}

fn show_toast(app: &AppWindow, message: impl Into<SharedString>) {
    app.set_toast_message(message.into());
    app.set_show_toast(true);
}

fn open_add_form(app: &AppWindow) {
    app.set_editing_id(-1);
    app.set_form_title("Add trainer".into());
    app.set_form_save_label("Add trainer".into());
    app.set_form_name("".into());
    app.set_form_exe_display(NO_EXE_PLACEHOLDER.into());
    app.set_form_exe_path("".into());
    app.set_form_game_exe_display(NO_GAME_PLACEHOLDER.into());
    app.set_form_game_exe_path("".into());
    app.set_form_game_args("".into());
    app.set_form_game_expanded(false);
    app.set_form_watched_exe_display(NO_WATCHED_EXE_PLACEHOLDER.into());
    app.set_form_watched_exe_path("".into());
    app.set_form_shortcut("".into());
    app.set_form_shortcut_display("Click to record shortcut".into());
    app.set_form_cheats(ModelRc::new(VecModel::from(Vec::<CheatEntry>::new())));
    app.set_form_focused_index(-1);
    app.set_form_sub_index(0);
    app.set_show_add_edit(true);
}

/// The name to pre-fill the Add-trainer form with: what the exe calls itself,
/// falling back to its filename for the many trainers that ship no version
/// resource.
fn suggested_trainer_name(path: &Path) -> String {
    exe_version::extract_display_name(path).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string()
    })
}

/// Points the open Add-trainer form at `path` (already validated against the
/// trainer folder, so `filename` is what it will be imported as). Shared by the
/// Browse… picker and the drag-and-drop path so both pre-fill identically. The
/// name is only suggested into a still-empty field, which matters for Browse…
/// picking a second exe after the user has typed - a drop always arrives into a
/// form that was just cleared.
fn prefill_form_exe(app: &AppWindow, path: &Path, filename: &str) {
    app.set_form_exe_path(path.to_string_lossy().to_string().into());
    app.set_form_exe_display(filename.into());

    if app.get_form_name().trim().is_empty() {
        app.set_form_name(suggested_trainer_name(path).into());
    }
}

/// Points the form's Game section at a resolved selection. The full path is what
/// gets shown as well as stored: the game stays where it is installed, so the
/// bare filename wouldn't say which copy was picked. Launch options are only
/// overwritten when the selection actually carried some, so re-picking the plain
/// .exe after a .lnk doesn't silently wipe what the .lnk filled in.
///
/// The section is forced open because a drop can land on it while it's
/// collapsed, and a value the user can't see is worse than one they didn't ask
/// to look at.
fn set_form_game_exe(app: &AppWindow, selection: &trainer::GameSelection) {
    let display = selection.exe.to_string_lossy().to_string();
    app.set_form_game_exe_path(display.clone().into());
    app.set_form_game_exe_display(display.into());
    if !selection.args.is_empty() {
        app.set_form_game_args(selection.args.clone().into());
    }
    app.set_form_game_expanded(true);
}

fn open_edit_form(app: &AppWindow, trainer: &TrainerItem) {
    app.set_editing_id(trainer.id);
    app.set_form_title("Edit trainer".into());
    app.set_form_save_label("Save changes".into());
    app.set_form_name(trainer.name.clone());
    app.set_form_exe_display(trainer.exe.clone());
    // Editing never re-picks the exe (the field is hidden), so there's no
    // source path to move on save.
    app.set_form_exe_path("".into());
    app.set_form_game_exe_path(trainer.game_exe.clone());
    app.set_form_game_exe_display(if trainer.game_exe.is_empty() {
        NO_GAME_PLACEHOLDER.into()
    } else {
        trainer.game_exe.clone()
    });
    app.set_form_game_args(trainer.game_args.clone());
    // Opened only when there's something in it, so the section stays out of the
    // way for the trainers that have no game configured.
    app.set_form_game_expanded(!trainer.game_exe.is_empty());
    app.set_form_watched_exe_path(trainer.watched_exe.clone());
    app.set_form_watched_exe_display(if trainer.watched_exe.is_empty() {
        NO_WATCHED_EXE_PLACEHOLDER.into()
    } else {
        trainer.watched_exe.clone()
    });
    app.set_form_shortcut(trainer.shortcut.clone());
    app.set_form_shortcut_display(trainer.shortcut.clone());
    app.set_form_cheats(ModelRc::new(VecModel::from(cheats_to_vec(&trainer.cheats))));
    app.set_form_focused_index(-1);
    app.set_form_sub_index(0);
    app.set_show_add_edit(true);
}

/// Writes the form's user-editable fields (name, launch shortcut, cheats) onto
/// the config entry for `filename`, creating that entry when the trainer was
/// only just imported. Version and size are left empty for the folder rescan
/// to fill in from the exe itself.
struct TrainerFormValues<'a> {
    name: &'a str,
    game_exe: &'a str,
    game_args: &'a str,
    watched_exe: &'a str,
    shortcut: &'a str,
    cheats: &'a [CheatEntry],
}

fn apply_form_to_config(state: &AppState, filename: &str, form: TrainerFormValues<'_>) {
    let launch_shortcut = match form.shortcut.trim() {
        "" | NOT_SET => None,
        combo => Some(keys::canonicalize_combo(combo)),
    };
    let game_exe = match form.game_exe.trim() {
        "" | NO_GAME_PLACEHOLDER => None,
        path => Some(path.to_string()),
    };
    // Launch options belong to the game, so they go with it rather than
    // outliving a cleared game field.
    let game_args = match form.game_args.trim() {
        "" => None,
        args if game_exe.is_some() => Some(args.to_string()),
        _ => None,
    };
    let watched_exe = match form.watched_exe.trim() {
        "" | NO_WATCHED_EXE_PLACEHOLDER => None,
        path => Some(path.to_string()),
    };
    let default_cheats: Vec<CheatConfig> = form
        .cheats
        .iter()
        .map(|cheat| CheatConfig {
            label: cheat.label.to_string(),
            key: match cheat.key.as_str() {
                NOT_SET => String::new(),
                key => keys::canonicalize_combo(key),
            },
        })
        .collect();

    let mut config = state.config.borrow_mut();
    if let Some(entry) = config.trainers.iter_mut().find(|t| t.filename == filename) {
        entry.name = form.name.to_string();
        entry.game_exe = game_exe;
        entry.game_args = game_args;
        entry.watched_exe = watched_exe;
        entry.launch_shortcut = launch_shortcut;
        entry.default_cheats = default_cheats;
    } else {
        config.trainers.push(TrainerConfig {
            name: form.name.to_string(),
            filename: filename.to_string(),
            version: String::new(),
            size_bytes: 0,
            game_exe,
            game_args,
            watched_exe,
            launch_shortcut,
            default_cheats,
            close_after_launch: false,
        });
    }

    let _ = crate::config::save_config(&config);
}

/// Resolves a stored accent to one of `Palette.accents`, falling back to the
/// first swatch. Settings only ever offers those five, so a value from an older
/// config that isn't among them would otherwise leave no swatch marked selected.
fn palette_accent(app: &AppWindow, (r, g, b): (u8, u8, u8)) -> Color {
    let accents = app.global::<Palette>().get_accents();
    accents
        .iter()
        .find(|c| (c.red(), c.green(), c.blue()) == (r, g, b))
        .or_else(|| accents.iter().next())
        .unwrap_or(Color::from_rgb_u8(r, g, b))
}

/// Pushes the persisted settings into the UI: the plain window properties plus
/// the Slint `Theme` global the whole UI renders from.
fn apply_settings_to_ui(app: &AppWindow, config: &AppConfig) {
    app.set_close_after_launch(config.close_after_launch_global);
    app.set_confirm_exit(config.confirm_exit);
    app.set_run_as_admin(config.run_as_admin);
    app.set_default_shortcut_label(
        config
            .default_shortcut
            .as_deref()
            .map(keys::format_combo_for_display)
            .unwrap_or_else(|| NOT_SET.to_string())
            .into(),
    );

    let theme = app.global::<Theme>();
    theme.set_accent(palette_accent(app, config.theme.accent_rgb()));
    theme.set_dark(config.theme.is_dark());
    theme.set_compact(config.theme.is_compact());
}

/// The reverse of `apply_settings_to_ui`: reads the current UI state back into
/// `config` and persists it. Called on every settings change.
fn persist_settings_from_ui(app: &AppWindow, state: &AppState) {
    let mut config = state.config.borrow_mut();
    config.close_after_launch_global = app.get_close_after_launch();
    config.confirm_exit = app.get_confirm_exit();
    config.run_as_admin = app.get_run_as_admin();
    config.default_shortcut = match app.get_default_shortcut_label().as_str() {
        "" | NOT_SET => None,
        combo => Some(keys::canonicalize_combo(combo)),
    };

    let theme = app.global::<Theme>();
    let accent = theme.get_accent();
    config.theme.accent =
        crate::config::format_hex_rgb(accent.red(), accent.green(), accent.blue());
    config.theme.set_dark(theme.get_dark());
    config.theme.set_compact(theme.get_compact());

    let _ = crate::config::save_config(&config);
}

struct LaunchScript {
    script: String,
    has_hotkey: bool,
    /// Keys that failed to parse and are therefore missing from `script`. An
    /// unset key is not a failure and never appears here.
    dropped: Vec<String>,
}

fn is_unset(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == NOT_SET
}

/// Builds the command line that reruns this trainer in tray mode. Keys are
/// normalized through `keys::parse_combo` so what lands on the clipboard is
/// exactly what `launch_args` + `background` accept back; anything unparsable
/// is dropped rather than pasted into a script that would refuse to start, and
/// reported to the caller so the omission isn't silent.
fn build_launch_script(trainer: &TrainerItem, close_after_launch: bool) -> LaunchScript {
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "rallx-cheat-launcher.exe".to_string());

    let mut dropped = Vec::new();

    let hotkey = match keys::parse_combo(&trainer.shortcut) {
        Ok(combo) => Some(combo.canonical()),
        Err(_) => {
            if !is_unset(&trainer.shortcut) {
                dropped.push(format!("shortcut \"{}\"", trainer.shortcut));
            }
            None
        }
    };

    let mut cheats = Vec::new();
    for cheat in trainer.cheats.iter() {
        match keys::parse_combo(&cheat.key) {
            Ok(combo) => cheats.push(combo.canonical()),
            Err(_) if is_unset(&cheat.key) => {}
            Err(_) => dropped.push(format!("{} \"{}\"", cheat.label, cheat.key)),
        }
    }

    LaunchScript {
        script: launch_args::build_launch_script(
            &exe,
            &trainer.exe,
            hotkey.as_deref(),
            &cheats,
            close_after_launch,
        ),
        has_hotkey: hotkey.is_some(),
        dropped,
    }
}

fn sync_window_hotkey(state: &AppState) -> Result<(), String> {
    let desired = state
        .config
        .borrow()
        .default_shortcut
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(keys::parse_combo)
        .transpose()
        .map_err(|err| format!("The default launch shortcut is invalid: {err}"))?;
    let desired_canonical = desired.as_ref().map(keys::KeyCombo::canonical);

    if state
        .window_hotkey
        .borrow()
        .as_ref()
        .map(|registered| registered.canonical.as_str())
        == desired_canonical.as_deref()
    {
        return Ok(());
    }

    *state.window_hotkey.borrow_mut() = None;
    let Some(combo) = desired else {
        return Ok(());
    };

    let canonical = combo.canonical();
    let mut manager = hotkey::HotkeyManager::new()?;
    let id = manager.register(&combo)?;
    *state.window_hotkey.borrow_mut() = Some(RegisteredWindowHotkey {
        canonical,
        id,
        _manager: manager,
    });
    Ok(())
}

fn find_matching_watched_trainer<F>(
    candidates: &[(i32, PathBuf)],
    mut is_running: F,
) -> Result<Option<i32>, std::io::Error>
where
    F: FnMut(&Path) -> Result<bool, std::io::Error>,
{
    let mut first_error = None;
    for (id, watched_exe) in candidates {
        match is_running(watched_exe) {
            Ok(true) => return Ok(Some(*id)),
            Ok(false) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) if first_error.is_none() => first_error = Some(err),
            Err(_) => {}
        }
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(None),
    }
}

fn start_window_hotkey_poll(app: &AppWindow, state: &Rc<AppState>) {
    let app_weak = app.as_weak();
    let state_weak = Rc::downgrade(state);

    state
        .hotkey_poll_timer
        .start(TimerMode::Repeated, GLOBAL_HOTKEY_POLL, move || {
            let pressed = hotkey::drain_pressed();
            let Some(state) = state_weak.upgrade() else {
                return;
            };
            let registered_id = state
                .window_hotkey
                .borrow()
                .as_ref()
                .map(|registered| registered.id);
            if !registered_id.is_some_and(|id| pressed.contains(&id)) {
                return;
            }

            let Some(app) = app_weak.upgrade() else {
                return;
            };
            // Recording the configured shortcut necessarily emits its global
            // event too. Drain it above, but never turn that recording action
            // into an accidental trainer launch.
            if app.get_recording() || state.hotkey_scan_in_progress.swap(true, Ordering::SeqCst) {
                return;
            }

            let candidates = state
                .trainers
                .borrow()
                .iter()
                .filter_map(|item| {
                    let watched_exe = item.watched_exe.trim();
                    (!watched_exe.is_empty()).then(|| (item.id, PathBuf::from(watched_exe)))
                })
                .collect::<Vec<_>>();
            let result_weak = app.as_weak();
            let scan_in_progress = state.hotkey_scan_in_progress.clone();

            std::thread::spawn(move || {
                let result =
                    find_matching_watched_trainer(&candidates, trainer::watched_exe_is_running);
                let _ = slint::invoke_from_event_loop(move || {
                    scan_in_progress.store(false, Ordering::SeqCst);
                    let Some(app) = result_weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(Some(id)) => app.invoke_launch_trainer(id),
                        Ok(None) => show_toast(&app, "No running game matches a trainer"),
                        Err(err) => {
                            show_toast(&app, format!("Could not identify the running game: {err}"))
                        }
                    }
                });
            });
        });
}

pub fn wire(app: &AppWindow, config: AppConfig, mode: AppMode) {
    let state = Rc::new(AppState {
        trainers: RefCell::new(Vec::new()),
        next_trainer_id: Cell::new(1),
        next_cheat_id: Cell::new(100),
        config: RefCell::new(config),
        window_hotkey: RefCell::new(None),
        hotkey_poll_timer: Timer::default(),
        hotkey_scan_in_progress: Arc::new(AtomicBool::new(false)),
    });

    let items = rescan_trainer_folder(&state);
    *state.trainers.borrow_mut() = items;
    refresh_trainer_list(app, &state);

    let trainer_folder = state.config.borrow().trainer_folder.clone();
    let folder_label = match trainer_folder {
        Some(ref folder) => folder.display().to_string(),
        None => "No folder selected".to_string(),
    };
    app.set_folder_path(folder_label.into());
    app.set_has_trainer_folder(trainer_folder.is_some());
    // Fixed for the lifetime of the process, so it's read once rather than
    // re-checked whenever Settings opens.
    app.set_is_elevated(elevate::is_elevated());
    apply_settings_to_ui(app, &state.config.borrow());

    if mode == AppMode::Windowed {
        if let Err(err) = sync_window_hotkey(&state) {
            show_toast(app, err);
        }
        start_window_hotkey_poll(app, &state);
    }

    app.on_quit_app(|| {
        let _ = slint::quit_event_loop();
    });

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_restart_as_admin(move || {
            let app = app_weak.unwrap();
            if elevate::is_elevated() {
                return;
            }

            // The elevated copy reads config.json on startup and this process
            // is about to end, so anything still only in the UI is written now.
            persist_settings_from_ui(&app, &state);

            // The relaunch itself waits until the event loop has exited and the
            // hotkey/tray singletons are released - see
            // elevate::finish_requested_restart.
            elevate::request_restart();
            let _ = slint::quit_event_loop();
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_settings_changed(move || {
            let app = app_weak.unwrap();
            persist_settings_from_ui(&app, &state);
            if mode == AppMode::Windowed {
                if let Err(err) = sync_window_hotkey(&state) {
                    show_toast(&app, err);
                }
            }
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_search_changed(move |_query| {
            let app = app_weak.unwrap();
            refresh_trainer_list(&app, &state);
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_launch_trainer(move |id| {
            let app = app_weak.unwrap();
            let Some(trainer) = find_trainer(&state, id) else {
                return;
            };
            let Some(folder) = state.config.borrow().trainer_folder.clone() else {
                show_toast(&app, "No trainer folder selected");
                return;
            };

            // Most trainers need elevation, and that path blocks on the UAC
            // prompt - on the UI thread the whole window would stop redrawing
            // until the user answers it. The result comes back through the
            // event loop.
            let name = trainer.name.to_string();
            let exe = trainer.exe.to_string();
            let watched_exe = trainer.watched_exe.to_string();
            let close_after_launch = app.get_close_after_launch();
            let result_weak = app.as_weak();

            std::thread::spawn(move || {
                let mut result = trainer::launch_trainer(&folder, &exe);

                if let Ok(process) = result.as_mut() {
                    if !watched_exe.is_empty() {
                        let launched_weak = result_weak.clone();
                        let launched_name = name.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = launched_weak.upgrade() {
                                show_toast(
                                    &app,
                                    format!("Launched {launched_name} - waiting for the selected app to close"),
                                );
                            }
                        });

                        let watch_result = trainer::wait_for_watched_exe_exit(
                            Path::new(&watched_exe),
                            std::time::Duration::from_millis(500),
                        );
                        match watch_result {
                            Ok(()) => {
                                let cleanup_result = process.terminate();
                                let cleanup_weak = result_weak.clone();
                                let cleanup_name = name.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    match cleanup_result {
                                        Ok(()) => {
                                            if let Some(app) = cleanup_weak.upgrade() {
                                                show_toast(
                                                    &app,
                                                    format!(
                                                        "Closed {cleanup_name} after the selected app exited"
                                                    ),
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            eprintln!(
                                                "Could not close the launched trainer: {err}"
                                            );
                                            if let Some(app) = cleanup_weak.upgrade() {
                                                show_toast(
                                                    &app,
                                                    format!(
                                                        "Could not close {cleanup_name}: {err}. Close it manually to finish cleanup."
                                                    ),
                                                );
                                            }
                                        }
                                    }
                                });
                            }
                            Err(err) => {
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(app) = result_weak.upgrade() {
                                        show_toast(
                                            &app,
                                            format!("Could not watch the selected app: {err}"),
                                        );
                                    }
                                });
                            }
                        }
                        return;
                    }
                }

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(app) = result_weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(_) if close_after_launch => {
                            let _ = slint::quit_event_loop();
                        }
                        Ok(_) => show_toast(&app, format!("Launched {name}")),
                        Err(err) => show_toast(&app, format!("Failed to launch {name}: {err}")),
                    }
                });
            });
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_refresh_trainers(move || {
            let app = app_weak.unwrap();
            if state.config.borrow().trainer_folder.is_none() {
                show_toast(&app, "No trainer folder selected");
                return;
            }
            let items = rescan_trainer_folder(&state);
            *state.trainers.borrow_mut() = items;
            refresh_trainer_list(&app, &state);
            show_toast(&app, "Trainer list refreshed");
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_copy_script(move |id| {
            let app = app_weak.unwrap();
            if let Some(trainer) = find_trainer(&state, id) {
                // The per-trainer flag exists only to generate this flag - it
                // never closes the app on a UI launch (see PRD.md).
                let close_after_launch = state
                    .config
                    .borrow()
                    .trainers
                    .iter()
                    .find(|entry| entry.filename.eq_ignore_ascii_case(&trainer.exe))
                    .is_some_and(|entry| entry.close_after_launch);

                let built = build_launch_script(&trainer, close_after_launch);
                match clipboard::set_text(&built.script) {
                    Err(err) => show_toast(&app, format!("Could not copy script: {err}")),
                    Ok(()) if !built.dropped.is_empty() => show_toast(
                        &app,
                        format!(
                            "Copied without {} - not a usable key",
                            built.dropped.join(", ")
                        ),
                    ),
                    // Without a hotkey the script launches the trainer the
                    // moment it runs, which is a different thing from what the
                    // Home screen's play button does.
                    Ok(()) if !built.has_hotkey => {
                        show_toast(&app, "Copied - no shortcut set, so it launches immediately")
                    }
                    Ok(()) => show_toast(&app, "Launch script copied"),
                }
            }
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_request_delete(move |id| {
            let app = app_weak.unwrap();
            if let Some(trainer) = find_trainer(&state, id) {
                app.set_delete_confirm_id(id);
                app.set_delete_confirm_name(trainer.name);
            }
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_confirm_delete(move |id| {
            let app = app_weak.unwrap();
            app.set_delete_confirm_id(-1);

            let Some(item) = find_trainer(&state, id) else {
                return;
            };
            let folder = state.config.borrow().trainer_folder.clone();

            if let Some(folder) = folder {
                if let Err(err) = trainer::delete_trainer_file(&folder, &item.exe) {
                    show_toast(&app, format!("Failed to delete {}: {err}", item.exe));
                    return;
                }
                {
                    let mut config = state.config.borrow_mut();
                    config.trainers.retain(|t| t.filename != item.exe.as_str());
                    let _ = crate::config::save_config(&config);
                }
                let items = rescan_trainer_folder(&state);
                *state.trainers.borrow_mut() = items;
            } else {
                state.trainers.borrow_mut().retain(|t| t.id != id);
            }

            refresh_trainer_list(&app, &state);
            show_toast(&app, format!("Deleted {}", item.name));
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_open_add(move || {
            let app = app_weak.unwrap();
            open_add_form(&app);
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_open_edit(move |id| {
            let app = app_weak.unwrap();
            if let Some(trainer) = find_trainer(&state, id) {
                open_edit_form(&app, &trainer);
            }
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_save_form(move || {
            let app = app_weak.unwrap();
            let name = app.get_form_name().to_string();
            if name.trim().is_empty() {
                show_toast(&app, "Enter a name for the trainer");
                return;
            }
            let shortcut = app.get_form_shortcut().to_string();
            let game_exe = app.get_form_game_exe_path().to_string();
            let game_args = app.get_form_game_args().to_string();
            let watched_exe = app.get_form_watched_exe_path().to_string();
            let cheats = cheats_to_vec(&app.get_form_cheats());
            let editing_id = app.get_editing_id();

            let Some(folder) = state.config.borrow().trainer_folder.clone() else {
                show_toast(&app, "Select a trainer folder in Settings first");
                return;
            };

            // Adding imports the picked exe into the trainer folder first, so
            // that the config entry it's about to get is keyed on the filename
            // as it now exists there. Editing keeps the trainer's own file.
            let filename = if editing_id < 0 {
                let exe_path = app.get_form_exe_path().to_string();
                if exe_path.is_empty() {
                    show_toast(&app, "Pick a trainer executable");
                    return;
                }
                match trainer::import_trainer(Path::new(&exe_path), &folder) {
                    Ok(filename) => filename,
                    Err(err) => {
                        show_toast(&app, err.to_string());
                        return;
                    }
                }
            } else {
                let Some(trainer) = find_trainer(&state, editing_id) else {
                    return;
                };
                trainer.exe.to_string()
            };

            apply_form_to_config(
                &state,
                &filename,
                TrainerFormValues {
                    name: name.trim(),
                    game_exe: &game_exe,
                    game_args: &game_args,
                    watched_exe: &watched_exe,
                    shortcut: &shortcut,
                    cheats: &cheats,
                },
            );

            let items = rescan_trainer_folder(&state);
            *state.trainers.borrow_mut() = items;

            app.set_show_add_edit(false);
            app.set_form_focused_index(-1);
            app.set_form_sub_index(0);
            refresh_trainer_list(&app, &state);
            show_toast(
                &app,
                if editing_id < 0 {
                    "Trainer added"
                } else {
                    "Trainer updated"
                },
            );
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_add_cheat_row(move || {
            let app = app_weak.unwrap();
            // Cheat ids only need to be unique within the currently-open form,
            // so deriving from length keeps this callback self-contained.
            let mut cheats = cheats_to_vec(&app.get_form_cheats());
            let next_id = cheats.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            cheats.push(CheatEntry {
                id: next_id,
                label: "".into(),
                key: "Not set".into(),
            });
            app.set_form_cheats(ModelRc::new(VecModel::from(cheats)));
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_remove_cheat_row(move |id| {
            let app = app_weak.unwrap();
            let cheats: Vec<CheatEntry> = cheats_to_vec(&app.get_form_cheats())
                .into_iter()
                .filter(|c| c.id != id)
                .collect();
            app.set_form_cheats(ModelRc::new(VecModel::from(cheats)));
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_set_cheat_label(move |id, label| {
            let app = app_weak.unwrap();
            let cheats: Vec<CheatEntry> = cheats_to_vec(&app.get_form_cheats())
                .into_iter()
                .map(|mut c| {
                    if c.id == id {
                        c.label = label.clone();
                    }
                    c
                })
                .collect();
            app.set_form_cheats(ModelRc::new(VecModel::from(cheats)));
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_set_cheat_key(move |id, key| {
            let app = app_weak.unwrap();
            let cheats: Vec<CheatEntry> = cheats_to_vec(&app.get_form_cheats())
                .into_iter()
                .map(|mut c| {
                    if c.id == id {
                        c.key = key.clone();
                    }
                    c
                })
                .collect();
            app.set_form_cheats(ModelRc::new(VecModel::from(cheats)));
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_browse_folder(move || {
            let app = app_weak.unwrap();
            let Some(folder) = rfd::FileDialog::new().pick_folder() else {
                return;
            };

            state.config.borrow_mut().trainer_folder = Some(folder.clone());
            let items = rescan_trainer_folder(&state);
            *state.trainers.borrow_mut() = items;
            refresh_trainer_list(&app, &state);

            app.set_folder_path(folder.display().to_string().into());
            app.set_has_trainer_folder(true);
            show_toast(&app, "Trainer folder updated");
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_browse_exe(move || {
            let app = app_weak.unwrap();
            let Some(folder) = state.config.borrow().trainer_folder.clone() else {
                show_toast(&app, "Select a trainer folder in Settings first");
                return;
            };

            let Some(path) = rfd::FileDialog::new()
                .add_filter("Executable", &["exe"])
                .pick_file()
            else {
                return;
            };

            // Rejected here rather than on save so the user can correct the
            // choice while the form is still open.
            let filename = match trainer::validate_import(&path, &folder) {
                Ok(filename) => filename,
                Err(err) => {
                    show_toast(&app, err.to_string());
                    return;
                }
            };

            prefill_form_exe(&app, &path, &filename);
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_browse_game_exe(move || {
            let app = app_weak.unwrap();
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Game or shortcut", &["exe", "lnk"])
                .pick_file()
            else {
                return;
            };

            match trainer::resolve_game_selection(&path) {
                Ok(selection) => set_form_game_exe(&app, &selection),
                Err(err) => show_toast(&app, err.to_string()),
            }
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_clear_game_exe(move || {
            let app = app_weak.unwrap();
            app.set_form_game_exe_path("".into());
            app.set_form_game_exe_display(NO_GAME_PLACEHOLDER.into());
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_browse_watched_exe(move || {
            let app = app_weak.unwrap();
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Executable", &["exe"])
                .pick_file()
            else {
                return;
            };

            match trainer::validate_watched_exe(&path) {
                Ok(path) => {
                    let display = path.to_string_lossy().to_string();
                    app.set_form_watched_exe_path(display.clone().into());
                    app.set_form_watched_exe_display(display.into());
                }
                Err(err) => show_toast(&app, err.to_string()),
            }
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_clear_watched_exe(move || {
            let app = app_weak.unwrap();
            app.set_form_watched_exe_path("".into());
            app.set_form_watched_exe_display(NO_WATCHED_EXE_PLACEHOLDER.into());
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_generate_game_bat(move || {
            let app = app_weak.unwrap();

            let editing_id = app.get_editing_id();
            if editing_id < 0 {
                show_toast(&app, "Save the trainer before generating its .bat file");
                return;
            }
            let Some(item) = find_trainer(&state, editing_id) else {
                return;
            };
            let trainer_filename = item.exe.to_string();
            let close_after_launch = state
                .config
                .borrow()
                .trainers
                .iter()
                .find(|entry| entry.filename.eq_ignore_ascii_case(&trainer_filename))
                .is_some_and(|entry| entry.close_after_launch);

            let launcher = match std::env::current_exe() {
                Ok(path) => path,
                Err(err) => {
                    show_toast(&app, format!("Could not locate Rallx: {err}"));
                    return;
                }
            };

            match trainer::generate_game_bat(
                Path::new(&app.get_form_game_exe_path().to_string()),
                app.get_form_game_args().as_ref(),
                &launcher,
                &trainer_filename,
                close_after_launch,
            ) {
                Ok(path) => show_toast(
                    &app,
                    format!(
                        "Created {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                ),
                Err(err) => show_toast(&app, err.to_string()),
            }
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_exe_dropped(move |dropped, x, y| {
            let app = app_weak.unwrap();
            let path = PathBuf::from(dropped.to_string());

            // A drop onto one of the open form's executable rows fills that
            // field instead of starting a new trainer, so it's resolved before
            // the popup guard below - which exists to stop an unrelated drop
            // from replacing what the user is part-way through.
            match app.invoke_form_drop_zone(x, y).as_str() {
                "game" => {
                    match trainer::resolve_game_selection(&path) {
                        Ok(selection) => set_form_game_exe(&app, &selection),
                        Err(err) => show_toast(&app, err.to_string()),
                    }
                    return;
                }
                "watched" => {
                    match trainer::validate_watched_exe(&path) {
                        Ok(path) => {
                            let display = path.to_string_lossy().to_string();
                            app.set_form_watched_exe_path(display.clone().into());
                            app.set_form_watched_exe_display(display.into());
                        }
                        Err(err) => show_toast(&app, err.to_string()),
                    }
                    return;
                }
                "trainer" => {
                    let Some(folder) = state.config.borrow().trainer_folder.clone() else {
                        show_toast(&app, "Select a trainer folder in Settings first");
                        return;
                    };
                    match trainer::validate_import(&path, &folder) {
                        Ok(filename) => prefill_form_exe(&app, &path, &filename),
                        Err(err) => show_toast(&app, err.to_string()),
                    }
                    return;
                }
                _ => {}
            }

            if app.invoke_popup_open() {
                return;
            }

            let Some(folder) = state.config.borrow().trainer_folder.clone() else {
                show_toast(&app, "Select a trainer folder in Settings first");
                return;
            };

            // Same pre-import validation the Browse… picker runs, done before
            // the form opens so a bad drop doesn't leave an empty popup behind.
            let filename = match trainer::validate_import(&path, &folder) {
                Ok(filename) => filename,
                Err(err) => {
                    show_toast(&app, err.to_string());
                    return;
                }
            };

            app.invoke_clear_for_drop();
            open_add_form(&app);
            prefill_form_exe(&app, &path, &filename);
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_keyboard_char(move |ch| {
            let app = app_weak.unwrap();
            apply_keyboard_edit(&app, &state, |s| s.push_str(ch.as_str()));
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_keyboard_backspace(move || {
            let app = app_weak.unwrap();
            apply_keyboard_edit(&app, &state, |s| {
                s.pop();
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The user-visible half of the version-resource fallback: a trainer that
    // names itself gets that name, and the many that don't must still land on
    // something readable rather than an empty field.
    #[test]
    fn a_trainer_without_a_version_resource_is_named_after_its_file() {
        let path =
            std::env::temp_dir().join(format!("rallx-test-noname-{}.exe", std::process::id()));
        std::fs::write(&path, b"not a real pe").unwrap();

        let name = suggested_trainer_name(&path);
        std::fs::remove_file(&path).unwrap();

        assert_eq!(name, format!("rallx-test-noname-{}", std::process::id()));
    }

    fn item(shortcut: &str, cheats: &[(&str, &str)]) -> TrainerItem {
        let cheats: Vec<CheatEntry> = cheats
            .iter()
            .enumerate()
            .map(|(index, (label, key))| CheatEntry {
                id: index as i32,
                label: (*label).into(),
                key: (*key).into(),
            })
            .collect();
        TrainerItem {
            id: 1,
            name: "RDR2".into(),
            version: "1.0".into(),
            size: "3.0 MB".into(),
            exe: "rdr2-trainer.exe".into(),
            game_exe: "".into(),
            game_args: "".into(),
            watched_exe: "".into(),
            shortcut: shortcut.into(),
            color: Color::from_rgb_u8(0, 0, 0),
            letter: "R".into(),
            has_icon: false,
            icon: Image::default(),
            cheats: ModelRc::new(VecModel::from(cheats)),
        }
    }

    #[test]
    fn a_usable_shortcut_and_cheats_are_normalized_into_the_script() {
        let built = build_launch_script(&item("insert", &[("Health", "ctrl+num1")]), false);

        assert!(built.has_hotkey);
        assert!(built.dropped.is_empty());
        assert!(built.script.contains("--launch=\"rdr2-trainer.exe\""));
        assert!(built.script.contains("--hotkey=\"Insert\""));
        assert!(built.script.contains("--defaultcheat=\"Ctrl+Numpad1\""));
        assert!(!built.script.contains("--closeafterlaunch"));
    }

    // An unassigned key is a legitimate "nothing configured", not a value the
    // user should be warned about losing.
    #[test]
    fn unset_keys_are_omitted_without_being_reported() {
        let built = build_launch_script(&item(NOT_SET, &[("Health", NOT_SET), ("Ammo", "")]), true);

        assert!(!built.has_hotkey);
        assert!(built.dropped.is_empty(), "{:?}", built.dropped);
        assert!(!built.script.contains("--hotkey"));
        assert!(!built.script.contains("--defaultcheat"));
        assert!(built.script.contains("--closeafterlaunch"));
    }

    // A key that was set but can't be parsed is silently missing from the
    // script, so the caller has to be able to say so.
    #[test]
    fn unparsable_keys_are_dropped_and_reported() {
        let built = build_launch_script(
            &item("banana", &[("Health", "ctrl+num1"), ("Ammo", "durian")]),
            false,
        );

        assert!(!built.has_hotkey);
        assert_eq!(built.dropped, ["shortcut \"banana\"", "Ammo \"durian\""]);
        assert!(!built.script.contains("--hotkey"));
        assert!(built.script.contains("--defaultcheat=\"Ctrl+Numpad1\""));
    }

    #[test]
    fn global_hotkey_selects_the_first_running_watched_executable() {
        let candidates = vec![
            (10, PathBuf::from(r"C:\Games\First.exe")),
            (20, PathBuf::from(r"C:\Games\Second.exe")),
            (30, PathBuf::from(r"C:\Games\Third.exe")),
        ];
        let mut checked = Vec::new();

        let matched = find_matching_watched_trainer(&candidates, |path| {
            checked.push(path.to_path_buf());
            Ok(path.ends_with("Second.exe"))
        })
        .unwrap();

        assert_eq!(matched, Some(20));
        assert_eq!(
            checked,
            candidates[..2]
                .iter()
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_deleted_watched_executable_does_not_block_another_match() {
        let candidates = vec![
            (10, PathBuf::from(r"C:\Games\Deleted.exe")),
            (20, PathBuf::from(r"C:\Games\Running.exe")),
        ];

        let matched = find_matching_watched_trainer(&candidates, |path| {
            if path.ends_with("Deleted.exe") {
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            } else {
                Ok(true)
            }
        })
        .unwrap();

        assert_eq!(matched, Some(20));
    }
}
