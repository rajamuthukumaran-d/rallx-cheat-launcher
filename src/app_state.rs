#![allow(dead_code)]

// UI-only prototype state: an in-memory trainer list wired to the Slint UI so
// every screen (Home, Settings, Add/Edit, Delete confirm, key recorder) is
// fully interactive. Trainer folder selection and discovery are real
// (see trainer::sync_trainer_configs); launching and hotkey registration are
// not wired up yet - see trainer.rs/hotkey.rs for those.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use slint::{Color, ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};

use crate::config::{AppConfig, CheatConfig, TrainerConfig};
use crate::{
    clipboard, exe_icon, keys, launch_args, trainer, AppWindow, CheatEntry, Palette, Theme,
    TrainerItem,
};

// Placeholders the UI shows for an unassigned value; also the sentinels the
// save path treats as "nothing configured" when writing config.json.
const NO_EXE_PLACEHOLDER: &str = "No executable selected";
const NOT_SET: &str = "Not set";

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
                NOT_SET
            } else {
                &cheat.key
            };
            make_cheat(state, &cheat.label, key)
        })
        .collect();
    let icon = exe_path.and_then(exe_icon::extract_icon);
    let size = format_size(cfg.size_bytes);
    make_trainer(
        state,
        NewTrainer {
            name: &cfg.name,
            version: &cfg.version,
            size: &size,
            exe: &cfg.filename,
            shortcut: cfg.launch_shortcut.as_deref().unwrap_or(NOT_SET),
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
/// `keyboard_target` names ("search" | "name" | "cheat"), then mirrors the
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
    app.set_form_shortcut("".into());
    app.set_form_shortcut_display("Click to record shortcut".into());
    app.set_form_cheats(ModelRc::new(VecModel::from(Vec::<CheatEntry>::new())));
    app.set_form_focused_index(-1);
    app.set_form_sub_index(0);
    app.set_show_add_edit(true);
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
fn apply_form_to_config(
    state: &AppState,
    filename: &str,
    name: &str,
    shortcut: &str,
    cheats: &[CheatEntry],
) {
    let launch_shortcut = match shortcut.trim() {
        "" | NOT_SET => None,
        combo => Some(combo.to_string()),
    };
    let default_cheats: Vec<CheatConfig> = cheats
        .iter()
        .map(|cheat| CheatConfig {
            label: cheat.label.to_string(),
            key: match cheat.key.as_str() {
                NOT_SET => String::new(),
                key => key.to_string(),
            },
        })
        .collect();

    let mut config = state.config.borrow_mut();
    if let Some(entry) = config.trainers.iter_mut().find(|t| t.filename == filename) {
        entry.name = name.to_string();
        entry.launch_shortcut = launch_shortcut;
        entry.default_cheats = default_cheats;
    } else {
        config.trainers.push(TrainerConfig {
            name: name.to_string(),
            filename: filename.to_string(),
            version: String::new(),
            size_bytes: 0,
            game_exe: None,
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
    app.set_default_shortcut_label(
        config
            .default_shortcut
            .as_deref()
            .unwrap_or(NOT_SET)
            .to_string()
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
    config.default_shortcut = match app.get_default_shortcut_label().as_str() {
        "" | NOT_SET => None,
        combo => Some(combo.to_string()),
    };

    let theme = app.global::<Theme>();
    let accent = theme.get_accent();
    config.theme.accent =
        crate::config::format_hex_rgb(accent.red(), accent.green(), accent.blue());
    config.theme.set_dark(theme.get_dark());
    config.theme.set_compact(theme.get_compact());

    let _ = crate::config::save_config(&config);
}

/// Builds the command line that reruns this trainer in tray mode. Keys are
/// normalized through `keys::parse_combo` so what lands on the clipboard is
/// exactly what `launch_args` + `background` accept back; anything unparsable
/// is dropped rather than pasted into a script that would refuse to start.
fn build_launch_script(trainer: &TrainerItem, close_after_launch: bool) -> String {
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "rallx-cheat-launcher.exe".to_string());

    let hotkey = keys::parse_combo(&trainer.shortcut)
        .ok()
        .map(|combo| combo.canonical());

    let cheats: Vec<String> = trainer
        .cheats
        .iter()
        .filter_map(|cheat| keys::parse_combo(&cheat.key).ok())
        .map(|combo| combo.canonical())
        .collect();

    launch_args::build_launch_script(
        &exe,
        &trainer.exe,
        hotkey.as_deref(),
        &cheats,
        close_after_launch,
    )
}

pub fn wire(app: &AppWindow, config: AppConfig) {
    let state = Rc::new(AppState {
        trainers: RefCell::new(Vec::new()),
        next_trainer_id: Cell::new(1),
        next_cheat_id: Cell::new(100),
        config: RefCell::new(config),
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
    apply_settings_to_ui(app, &state.config.borrow());

    app.on_quit_app(|| {
        let _ = slint::quit_event_loop();
    });

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_settings_changed(move || {
            let app = app_weak.unwrap();
            persist_settings_from_ui(&app, &state);
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

            match trainer::launch_trainer(&folder, &trainer.exe) {
                Ok(()) if app.get_close_after_launch() => {
                    let _ = slint::quit_event_loop();
                }
                Ok(()) => show_toast(&app, format!("Launched {}", trainer.name)),
                Err(err) => show_toast(&app, format!("Failed to launch {}: {err}", trainer.name)),
            }
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
                    .find(|entry| entry.filename == trainer.exe.as_str())
                    .is_some_and(|entry| entry.close_after_launch);

                match clipboard::set_text(&build_launch_script(&trainer, close_after_launch)) {
                    Ok(()) => show_toast(&app, "Launch script copied"),
                    Err(err) => show_toast(&app, format!("Could not copy script: {err}")),
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

            apply_form_to_config(&state, &filename, name.trim(), &shortcut, &cheats);

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

            app.set_form_exe_path(path.to_string_lossy().to_string().into());
            app.set_form_exe_display(filename.into());

            if app.get_form_name().trim().is_empty() {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                app.set_form_name(stem.into());
            }
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
