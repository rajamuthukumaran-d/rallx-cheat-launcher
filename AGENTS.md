# Rallx Cheat Launcher — Agent Instructions

Shared instructions for any AI coding agent (Claude Code, Gemini CLI, etc.) working
in this repository. Tool-specific entry points (`CLAUDE.md`, `GEMINI.md`) import
this file directly — edit this file, not the copies.

Full product spec lives in [`PRD.md`](./PRD.md). Read it before starting any
non-trivial task. This file covers *how* to work in the repo; `PRD.md` covers
*what* to build.

## Project summary

Rallx Cheat Launcher is a Windows desktop app (Rust + Slint) that lists game
trainers from a user-selected folder and launches them, optionally auto-pressing
configured hotkeys. It must work well on handheld gaming PCs (ROG Ally, Steam
Deck) with touch screen and gamepad input, not just mouse/keyboard.

## Tech stack

- **Language:** Rust (stable toolchain)
- **UI:** [Slint](https://slint.dev/) — `.slint` files for markup, Rust for logic
- **Config storage:** `config.json` next to the app executable, for portable
  behavior (not in `%APPDATA%`, not in the repo). The selected trainer folder
  is a value inside it, so nothing else on disk is needed to find the config.
- **Target platform:** Windows only. Don't add cross-platform abstractions unless
  explicitly asked — optimize for correctness on Windows first.

Suggested crates for known requirements (confirm current versions on crates.io
before pinning — don't assume the below are latest):
- `serde` / `serde_json` — config (de)serialization
- `gilrs` — gamepad/XInput input for controller navigation
- `global-hotkey` — system-wide hotkey registration (e.g. Insert to trigger
  trainer + default cheats mid-game)
- ~~`tray-icon`~~ — not used: Slint's built-in `SystemTrayIcon` element covers
  the tray icon + menu and shares the app's event loop (see `ui/tray.slint`)
- `enigo` or the `windows` crate's `SendInput` — programmatic key press
  injection for "default cheats" (`SendInput` is what `keys.rs` uses)
- `windows` crate — exe icon extraction, any other Win32 interop
- `notify` — only if live-watching the trainer folder becomes a requirement

Don't add a crate for something Slint or `std` already covers.

## Project layout (establish this as the app is scaffolded)

```
src/
  main.rs           # entry point, tray/background vs windowed startup branch
  launch_args.rs    # CLI launch-option parsing + launch-script generation
  background.rs     # tray mode: option -> config resolution, hotkey/tray loop
  config.rs         # config.json schema + load/save
  trainer.rs        # trainer discovery, launch, file import
  elevate.rs        # elevation check + relaunching the app itself via "runas"
  keys.rs           # key-combo parsing + SendInput injection
  hotkey.rs         # global hotkey registration/handling
  clipboard.rs      # Win32 clipboard write (copy launch script)
  dragdrop.rs       # WM_DROPFILES window subclass -> dropped .exe path
  gamepad.rs        # gilrs polling -> UI navigation events
ui/
  *.slint           # Slint component/screen files
  tray.slint        # SystemTrayIcon component used by background mode
mockups/            # contains mockup.html for visual reference
```

Keep UI (`.slint`) and app logic (`.rs`) cleanly separated: business logic
(trainer discovery, launching, config, hotkeys) belongs in Rust modules, not
embedded in Slint callbacks beyond simple event wiring.

## Design assets workflow

Designs are located in [`mockups/mockup.html`](./mockups/mockup.html). When implementing a screen:

1. Open `mockups/mockup.html` in a web browser to view the visual layout, styling, and design references.
2. Treat the mockup as a visual reference, not literal code to paste in — translate it into idiomatic Slint, since the HTML structure does not map directly to Slint.

## Core functional requirements to keep in mind

(Full detail in `PRD.md` — this is a condensed checklist so changes don't
silently violate a requirement from another section.)

- **Home screen:** trainer list (logo from exe, name, version, size, play icon);
  edit mode reveals copy/edit/delete icons; add + settings icons at top; full
  gamepad mapping (A=launch, X=edit, Y=search, Select=delete w/ confirm,
  RB=copy launch script, Start=settings); "close after launching" checkbox.
- **Settings screen:** trainer folder picker, default launch shortcut, theme
  (accent/background/style), global "close app after launching trainer",
  "run as administrator" (applied at next startup) plus a restart-elevated
  button that is disabled once the process is already elevated.
- **Add/Edit trainer popup:** triggered by drag-drop of an exe or the add icon;
  editable name, exe picker, launch shortcut assignment, list of default cheats
  (key/key-combo) entered via a record button that captures live key input.
- **Trainer launching:** click/play/A-button launches the trainer executable,
  resolved relative to the configured trainer folder (trainers are referenced
  by filename only, never full path).
- **Launch-option / background mode:** CLI args like
  `--launch="rdr2-trainer.exe" --hotkey="insert" --defaultcheat="ctrl+num1,num3,ctrl+num5"`.
  Only `--launch` is required — omitted values fall back to that trainer's
  saved shortcut/cheats (then the global default shortcut), so the other flags
  are per-run overrides. `--override` suppresses those fallbacks and takes the
  hotkey and cheats from the command line alone.
  When launched with these args, the app must **not** show its window — it runs
  as a tray-only background process, waits for the hotkey, then launches the
  trainer and injects the default cheat keys.
- **Drag & drop:** dropping an exe onto the app opens the Add Trainer popup;
  confirming moves the file into the trainer folder.
- **Close-after-launch precedence:** the global Settings toggle is the single
  source of truth for whether the *app* closes after a UI-launched trainer.
  A per-trainer "close after launch" flag is only used to *generate* the
  `--closeafterlaunch` CLI flag for a launch script — it must never by itself
  close the app when launching from the UI.

## Coding conventions

- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before
  considering a change done. Fix clippy warnings rather than allowing them,
  unless there's a specific documented reason not to.
- Prefer `Result<T, E>` with a small app-specific error enum over `unwrap()`/
  `expect()` outside of `main`/tests/genuinely-infallible cases.
- Keep Slint callbacks thin: marshal data in/out, delegate real work to Rust
  functions that are unit-testable without a running UI.
- No comments that restate what the code does. Only comment non-obvious
  constraints (e.g. why a keystroke needs a delay, why a Win32 call is used
  over a safe wrapper).
- Don't introduce cross-platform (macOS/Linux) code paths — this is a
  Windows-only app.

## Build & run

Once scaffolded, standard Cargo workflow applies:
- `cargo build` / `cargo run`
- `cargo test` for unit tests on non-UI logic (config parsing, launch-arg
  parsing/generation, keystroke-combo parsing, close-after-launch precedence)
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt`

There is no test suite or CI config yet — if you add one, keep it Windows-only
(`windows-latest` runner if a GitHub Actions workflow is added).

## Things to avoid

- Don't store config.json anywhere other than next to the app executable —
  that's an explicit product requirement, not an implementation detail. Don't
  move it into the trainer folder, and don't reintroduce a separate bootstrap
  pointer file (e.g. `last_folder.txt`) for locating it.
- Don't commit anything under `mockups/temp`.
- Don't build a general plugin/extension system, telemetry, or auto-update
  machinery — out of scope unless requested.
- Don't silently swallow the launch-option CLI args into the normal UI code
  path — background/tray mode and windowed mode are distinct startup branches.
