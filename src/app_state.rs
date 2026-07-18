#![allow(dead_code)]

// UI-only prototype state: an in-memory trainer list wired to the Slint UI so
// every screen (Home, Settings, Add/Edit, Delete confirm, key recorder) is
// fully interactive. No filesystem discovery, launching, or hotkey
// registration happens here yet - see trainer.rs/hotkey.rs for those, which
// this module deliberately does not call into.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{AppWindow, CheatEntry, TrainerItem};

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

fn make_trainer(
    state: &AppState,
    name: &str,
    version: &str,
    size: &str,
    exe: &str,
    shortcut: &str,
    cheats: Vec<CheatEntry>,
) -> TrainerItem {
    let id = state.next_trainer_id.get();
    state.next_trainer_id.set(id + 1);
    let color = row_color(id as usize);
    let letter = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    TrainerItem {
        id,
        name: name.into(),
        version: version.into(),
        size: size.into(),
        exe: exe.into(),
        shortcut: shortcut.into(),
        color,
        letter: letter.into(),
        cheats: ModelRc::new(VecModel::from(cheats)),
    }
}

fn sample_trainers(state: &AppState) -> Vec<TrainerItem> {
    vec![
        make_trainer(
            state,
            "Elden Ring +25 Trainer",
            "3.2.1",
            "4.1 MB",
            "EldenRing.exe",
            "Ctrl + F1",
            vec![
                make_cheat(state, "Infinite Health", "Numpad 1"),
                make_cheat(state, "Infinite Stamina", "Numpad 2"),
                make_cheat(state, "One Hit Kill", "Numpad 3"),
            ],
        ),
        make_trainer(
            state,
            "Cyberpunk 2077 +30 Trainer",
            "2.13.0",
            "5.8 MB",
            "Cyberpunk2077.exe",
            "Ctrl + F2",
            vec![
                make_cheat(state, "Infinite Health", "Numpad 1"),
                make_cheat(state, "Infinite Ammo", "Numpad 4"),
                make_cheat(state, "God Mode", "Numpad 0"),
            ],
        ),
        make_trainer(
            state,
            "Baldur's Gate 3 Trainer",
            "1.8.0",
            "2.9 MB",
            "bg3.exe",
            "Alt + F3",
            vec![
                make_cheat(state, "Infinite Gold", "Numpad 5"),
                make_cheat(state, "Set Level", "Numpad 6"),
            ],
        ),
        make_trainer(
            state,
            "Hogwarts Legacy +20 Trainer",
            "1.4.2",
            "3.6 MB",
            "HogwartsLegacy.exe",
            "Ctrl + F4",
            vec![
                make_cheat(state, "Infinite Health", "Numpad 1"),
                make_cheat(state, "No Cooldown", "Numpad 7"),
            ],
        ),
        make_trainer(
            state,
            "Starfield Trainer",
            "0.9.3",
            "2.2 MB",
            "Starfield.exe",
            "Ctrl + F5",
            vec![
                make_cheat(state, "Infinite Credits", "Numpad 8"),
                make_cheat(state, "Infinite Oxygen", "Numpad 9"),
            ],
        ),
        make_trainer(
            state,
            "Diablo IV +18 Trainer",
            "1.2.0",
            "3.0 MB",
            "Diablo4.exe",
            "Alt + F6",
            vec![
                make_cheat(state, "Infinite Health", "Numpad 1"),
                make_cheat(state, "Infinite Gold", "Numpad 2"),
            ],
        ),
    ]
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

fn show_toast(app: &AppWindow, message: impl Into<SharedString>) {
    app.set_toast_message(message.into());
    app.set_show_toast(true);
}

fn open_add_form(app: &AppWindow) {
    app.set_editing_id(-1);
    app.set_form_title("Add trainer".into());
    app.set_form_save_label("Add trainer".into());
    app.set_form_color(row_color(0));
    app.set_form_letter("?".into());
    app.set_form_name("".into());
    app.set_form_exe_display("No executable selected".into());
    app.set_form_shortcut("".into());
    app.set_form_shortcut_display("Click to record shortcut".into());
    app.set_form_cheats(ModelRc::new(VecModel::from(Vec::<CheatEntry>::new())));
    app.set_show_add_edit(true);
}

fn open_edit_form(app: &AppWindow, trainer: &TrainerItem) {
    app.set_editing_id(trainer.id);
    app.set_form_title("Edit trainer".into());
    app.set_form_save_label("Save changes".into());
    app.set_form_color(trainer.color);
    app.set_form_letter(trainer.letter.clone());
    app.set_form_name(trainer.name.clone());
    app.set_form_exe_display(trainer.exe.clone());
    app.set_form_shortcut(trainer.shortcut.clone());
    app.set_form_shortcut_display(trainer.shortcut.clone());
    app.set_form_cheats(ModelRc::new(VecModel::from(cheats_to_vec(&trainer.cheats))));
    app.set_show_add_edit(true);
}

fn build_launch_script(trainer: &TrainerItem) -> String {
    let mut script = format!("\"{}\" --hotkey \"{}\"", trainer.exe, trainer.shortcut);
    for cheat in trainer.cheats.iter() {
        script.push_str(&format!(" --cheat \"{}={}\"", cheat.label, cheat.key));
    }
    script
}

pub fn wire(app: &AppWindow, configured_folder: Option<std::path::PathBuf>) {
    let state = Rc::new(AppState {
        trainers: RefCell::new(Vec::new()),
        next_trainer_id: Cell::new(1),
        next_cheat_id: Cell::new(100),
    });
    *state.trainers.borrow_mut() = sample_trainers(&state);
    refresh_trainer_list(app, &state);

    let folder_label = match configured_folder {
        Some(folder) => folder.display().to_string(),
        None => "No folder selected".to_string(),
    };
    app.set_folder_path(folder_label.into());
    app.set_default_shortcut_label("Ctrl + F12".into());

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
            if let Some(trainer) = find_trainer(&state, id) {
                let suffix = if app.get_close_after_launch() {
                    " (app would close)"
                } else {
                    ""
                };
                show_toast(&app, format!("Launching {}…{}", trainer.name, suffix));
            }
        });
    }

    {
        let app_weak = app.as_weak();
        let state = state.clone();
        app.on_copy_script(move |id| {
            let app = app_weak.unwrap();
            if let Some(trainer) = find_trainer(&state, id) {
                let _script = build_launch_script(&trainer);
                show_toast(&app, "Launch script copied");
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
            state.trainers.borrow_mut().retain(|t| t.id != id);
            app.set_delete_confirm_id(-1);
            refresh_trainer_list(&app, &state);
            show_toast(&app, "Trainer deleted");
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
                return;
            }
            let exe_display = app.get_form_exe_display().to_string();
            let exe = if exe_display == "No executable selected" {
                "game.exe".to_string()
            } else {
                exe_display
            };
            let shortcut = app.get_form_shortcut().to_string();
            let shortcut = if shortcut.is_empty() {
                "Not set".to_string()
            } else {
                shortcut
            };
            let cheats = cheats_to_vec(&app.get_form_cheats());
            let editing_id = app.get_editing_id();

            if editing_id < 0 {
                let trainer = make_trainer(&state, &name, "1.0.0", "— MB", &exe, &shortcut, cheats);
                state.trainers.borrow_mut().push(trainer);
                show_toast(&app, "Trainer added");
            } else {
                let mut trainers = state.trainers.borrow_mut();
                if let Some(t) = trainers.iter_mut().find(|t| t.id == editing_id) {
                    t.name = name.clone().into();
                    t.exe = exe.into();
                    t.shortcut = shortcut.into();
                    t.cheats = ModelRc::new(VecModel::from(cheats));
                    t.letter = name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string()
                        .into();
                }
                drop(trainers);
                show_toast(&app, "Trainer updated");
            }

            app.set_show_add_edit(false);
            refresh_trainer_list(&app, &state);
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
        app.on_browse_folder(move || {
            let app = app_weak.unwrap();
            show_toast(&app, "Folder picker would open here");
        });
    }

    {
        let app_weak = app.as_weak();
        app.on_browse_exe(move || {
            let app = app_weak.unwrap();
            if app.get_form_exe_display() == "No executable selected" {
                app.set_form_exe_display("NewGame.exe".into());
            }
            show_toast(&app, "Executable picker would open here");
        });
    }
}
