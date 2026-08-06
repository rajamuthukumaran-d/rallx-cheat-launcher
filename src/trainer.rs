#![allow(dead_code)]

use std::fmt;
use std::path::Path;

use crate::config::TrainerConfig;
use crate::exe_version;

#[derive(Debug)]
pub enum ImportError {
    NotAnExe,
    AlreadyInFolder,
    DestinationExists(String),
    Io(std::io::Error),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnExe => write!(f, "Only .exe files can be added as trainers"),
            Self::AlreadyInFolder => {
                write!(f, "That executable is already in the trainer folder")
            }
            Self::DestinationExists(name) => {
                write!(f, "{name} already exists in the trainer folder")
            }
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl From<std::io::Error> for ImportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

fn is_exe(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// Same directory check that tolerates `.`/`..`/casing/short-path differences
/// by comparing canonicalized paths, falling back to a plain compare when
/// either path can't be canonicalized (e.g. it no longer exists).
fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Validates a user-picked executable against the configured trainer folder
/// *before* anything is moved, so the Add-trainer form can reject the choice
/// while it's still cancellable.
pub fn validate_import(src: &Path, folder: &Path) -> Result<String, ImportError> {
    if !is_exe(src) {
        return Err(ImportError::NotAnExe);
    }

    let filename = src
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or(ImportError::NotAnExe)?
        .to_string();

    if let Some(parent) = src.parent() {
        if same_folder(parent, folder) {
            return Err(ImportError::AlreadyInFolder);
        }
    }

    if folder.join(&filename).exists() {
        return Err(ImportError::DestinationExists(filename));
    }

    Ok(filename)
}

#[derive(Debug)]
pub enum GameExeError {
    NotAnExe,
    NotFound,
    /// A .lnk was picked but the shell could not be asked what it points at.
    UnreadableShortcut(String),
    /// A .lnk resolved to something that isn't a usable .exe.
    ShortcutTargetNotAnExe,
}

impl fmt::Display for GameExeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnExe => write!(
                f,
                "Only .exe files or Windows shortcuts can be set as the game"
            ),
            Self::NotFound => write!(f, "That executable no longer exists"),
            Self::UnreadableShortcut(err) => write!(f, "Could not read that shortcut: {err}"),
            Self::ShortcutTargetNotAnExe => {
                write!(f, "That shortcut does not point at an executable")
            }
        }
    }
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

/// What a user's pick for "the game" resolved to: the executable to start, and
/// the command line to start it with (empty unless a .lnk carried arguments).
#[derive(Debug, Clone)]
pub struct GameSelection {
    pub exe: std::path::PathBuf,
    pub args: String,
}

/// Validates an executable picked as a trainer's game. Unlike a trainer, the
/// game is referenced where it already lives - it is never moved into the
/// trainer folder - so the path is only checked for being a real .exe.
///
/// A .lnk is resolved through the shell first, which is the point of accepting
/// one: store-, launcher- and user-made shortcuts are where a game's launch
/// options normally live, so picking the shortcut fills in both fields at once.
pub fn resolve_game_selection(path: &Path) -> Result<GameSelection, GameExeError> {
    if is_shortcut(path) {
        let (exe, args) = read_shortcut(path)?;
        if !is_exe(&exe) || !exe.is_file() {
            return Err(GameExeError::ShortcutTargetNotAnExe);
        }
        return Ok(GameSelection { exe, args });
    }

    if !is_exe(path) {
        return Err(GameExeError::NotAnExe);
    }
    if !path.is_file() {
        return Err(GameExeError::NotFound);
    }

    Ok(GameSelection {
        exe: path.to_path_buf(),
        args: String::new(),
    })
}

/// Reads a .lnk's target path and arguments via IShellLinkW. There is no
/// documented on-disk format for shell links that's safe to parse by hand -
/// resolution involves the link tracking service - so this goes through COM.
fn read_shortcut(path: &Path) -> Result<(std::path::PathBuf, String), GameExeError> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLR_NO_UI};

    fn err(e: windows::core::Error) -> GameExeError {
        GameExeError::UnreadableShortcut(e.message())
    }

    fn from_wide(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    unsafe {
        // Slint's event loop does not initialize COM on this thread, and a
        // second COM-using feature must not be able to tear it down for the
        // first - so the uninit below is paired with this call only when this
        // call is the one that did the initializing.
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owns_com = init.is_ok();

        let result = (|| -> Result<(std::path::PathBuf, String), GameExeError> {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err)?;
            let file: IPersistFile = link.cast().map_err(err)?;

            let wide = wide(path);
            file.Load(
                PCWSTR(wide.as_ptr()),
                windows::Win32::System::Com::STGM_READ,
            )
            .map_err(err)?;

            // GetPath does not run link tracking itself. Resolve first so a
            // target that moved or was renamed can still be found, but never
            // let the Shell put up a dialog over the trainer form.
            link.Resolve(None, SLR_NO_UI.0 as u32).map_err(err)?;

            // MAX_PATH is what IShellLinkW documents for both buffers; a longer
            // target simply comes back truncated rather than overflowing.
            let mut target = [0u16; 260];
            // Zero requests the normal resolved path. SLGP_RAWPATH would return
            // a possibly nonexistent path with environment variables intact.
            link.GetPath(&mut target, std::ptr::null_mut(), 0)
                .map_err(err)?;

            let mut args = [0u16; 260];
            link.GetArguments(&mut args).map_err(err)?;

            Ok((
                std::path::PathBuf::from(from_wide(&target)),
                from_wide(&args).trim().to_string(),
            ))
        })();

        if owns_com {
            CoUninitialize();
        }
        result
    }
}

/// Marker written as the .bat's first line so a regenerate can tell its own
/// output apart from a file the user (or the game) put there under the same
/// name, and refuse to clobber the latter.
const BAT_MARKER: &str = "@rem Generated by Rallx Cheat Launcher";

#[derive(Debug)]
pub enum BatError {
    NoGameExe,
    NoTrainer,
    /// A file of that name is already there and isn't one of ours.
    WouldOverwrite(String),
    Io(std::io::Error),
}

impl fmt::Display for BatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoGameExe => write!(f, "Pick the game executable first"),
            Self::NoTrainer => write!(f, "Pick a trainer executable first"),
            Self::WouldOverwrite(name) => {
                write!(f, "{name} already exists and wasn't generated by Rallx")
            }
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl From<std::io::Error> for BatError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// cmd expands `%FOO%` while reading each line, so a literal percent in a path
/// or launch option has to be doubled or it silently disappears.
fn bat_escape(value: &str) -> String {
    value.replace('%', "%%")
}

/// Renders the .bat that starts the game and then hands off to Rallx in tray
/// mode. Split out from [`generate_game_bat`] so the exact text is testable
/// without touching the filesystem.
pub fn build_game_bat(
    game_exe: &Path,
    game_args: &str,
    launcher_exe: &Path,
    trainer_filename: &str,
    close_after_launch: bool,
) -> String {
    let game = bat_escape(&game_exe.display().to_string());
    let args = bat_escape(game_args.trim());
    let launcher = bat_escape(&launcher_exe.display().to_string());
    let trainer = bat_escape(trainer_filename);
    let close_flag = if close_after_launch {
        " --closeafterlaunch"
    } else {
        ""
    };

    let game_line = if args.is_empty() {
        format!("start \"\" \"{game}\"")
    } else {
        format!("start \"\" \"{game}\" {args}")
    };

    // `start ""` (empty title) so the quoted path isn't swallowed as the window
    // title, and `cd /d "%~dp0"` so the game gets its own folder as the working
    // directory however the .bat itself was invoked.
    //
    // Shortcut and cheat flags are omitted so background mode falls back to the
    // trainer's saved values. The per-trainer close flag is different: its only
    // purpose is to generate --closeafterlaunch, so it must be emitted here.
    format!(
        "{BAT_MARKER}\r\n\
         @echo off\r\n\
         cd /d \"%~dp0\"\r\n\
         {game_line}\r\n\
         start \"\" \"{launcher}\" --launch=\"{trainer}\"{close_flag}\r\n"
    )
}

/// Writes the launch .bat next to `game_exe`, named after it (`Game.exe` ->
/// `Game.bat`), and returns where it landed.
pub fn generate_game_bat(
    game_exe: &Path,
    game_args: &str,
    launcher_exe: &Path,
    trainer_filename: &str,
    close_after_launch: bool,
) -> Result<std::path::PathBuf, BatError> {
    if game_exe.as_os_str().is_empty() {
        return Err(BatError::NoGameExe);
    }
    if trainer_filename.trim().is_empty() {
        return Err(BatError::NoTrainer);
    }

    let stem = game_exe
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(BatError::NoGameExe)?;
    let folder = game_exe.parent().ok_or(BatError::NoGameExe)?;
    let dest = folder.join(format!("{stem}.bat"));

    match std::fs::read(&dest) {
        Ok(existing) if !existing.starts_with(BAT_MARKER.as_bytes()) => {
            return Err(BatError::WouldOverwrite(format!("{stem}.bat")));
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(BatError::Io(err)),
    }

    std::fs::write(
        &dest,
        build_game_bat(
            game_exe,
            game_args,
            launcher_exe,
            trainer_filename,
            close_after_launch,
        ),
    )?;
    Ok(dest)
}

/// Moves a validated executable into the trainer folder and returns its
/// filename. Falls back to copy+delete because `fs::rename` fails when the
/// source sits on a different volume than the trainer folder.
pub fn import_trainer(src: &Path, folder: &Path) -> Result<String, ImportError> {
    let filename = validate_import(src, folder)?;
    std::fs::create_dir_all(folder)?;
    let dest = folder.join(&filename);

    if std::fs::rename(src, &dest).is_err() {
        std::fs::copy(src, &dest)?;
        std::fs::remove_file(src)?;
    }

    Ok(filename)
}

#[derive(Debug, Clone)]
pub struct TrainerInfo {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
}

pub fn discover_trainers(folder: &Path) -> Result<Vec<TrainerInfo>, std::io::Error> {
    let mut trainers = Vec::new();
    if folder.is_dir() {
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "exe") {
                if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                    let metadata = entry.metadata()?;
                    trainers.push(TrainerInfo {
                        name: filename.to_string(),
                        filename: filename.to_string(),
                        size_bytes: metadata.len(),
                    });
                }
            }
        }
    }
    Ok(trainers)
}

/// `CreateProcess` refuses to elevate, and virtually every trainer ships with a
/// `requireAdministrator` manifest, so a plain spawn fails with ERROR_ELEVATION_
/// REQUIRED. Only that case falls back to the shell, which raises the UAC
/// prompt on the user's behalf.
const ERROR_ELEVATION_REQUIRED: i32 = 740;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// Spawned directly, so it runs at this process's integrity level.
    Direct,
    /// Elevated via the shell, so it runs at high integrity - which matters to
    /// the caller because UIPI then blocks keystroke injection from a
    /// non-elevated Rallx.
    Elevated,
}

enum ProcessHandle {
    Direct(std::process::Child),
    Elevated(std::os::windows::io::OwnedHandle),
}

impl ProcessHandle {
    fn raw(&self) -> windows::Win32::Foundation::HANDLE {
        use std::os::windows::io::AsRawHandle;

        match self {
            Self::Direct(child) => windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
            Self::Elevated(handle) => windows::Win32::Foundation::HANDLE(handle.as_raw_handle()),
        }
    }
}

/// A launched trainer whose process handle remains available for readiness and
/// liveness checks. Dropping this value closes the handle but does not stop the
/// trainer.
pub struct LaunchedTrainer {
    mode: LaunchMode,
    process: ProcessHandle,
}

impl LaunchedTrainer {
    pub fn mode(&self) -> LaunchMode {
        self.mode
    }

    pub fn is_running(&mut self) -> Result<bool, std::io::Error> {
        use windows::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::WaitForSingleObject;

        if let ProcessHandle::Direct(child) = &mut self.process {
            return child.try_wait().map(|status| status.is_none());
        }

        match unsafe { WaitForSingleObject(self.process.raw(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            WAIT_FAILED => Err(std::io::Error::last_os_error()),
            result => Err(std::io::Error::other(format!(
                "unexpected process wait result {}",
                result.0
            ))),
        }
    }

    /// Waits for the trainer's GUI thread to finish its initial input setup.
    /// `Ok(false)` means the timeout elapsed; callers may still continue after
    /// a grace period because some programs never expose a standard GUI queue.
    pub fn wait_for_input_idle(
        &self,
        timeout: std::time::Duration,
    ) -> Result<bool, std::io::Error> {
        use windows::Win32::Foundation::WAIT_TIMEOUT;
        use windows::Win32::System::Threading::WaitForInputIdle;

        let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;
        match unsafe { WaitForInputIdle(self.process.raw(), timeout_ms) } {
            0 => Ok(true),
            result if result == WAIT_TIMEOUT.0 => Ok(false),
            _ => Err(std::io::Error::last_os_error()),
        }
    }
}

pub fn launch_trainer(folder: &Path, filename: &str) -> Result<LaunchedTrainer, std::io::Error> {
    let full_path = folder.join(filename);
    match std::process::Command::new(&full_path)
        .current_dir(folder)
        .spawn()
    {
        Ok(child) => Ok(LaunchedTrainer {
            mode: LaunchMode::Direct,
            process: ProcessHandle::Direct(child),
        }),
        Err(err) if err.raw_os_error() == Some(ERROR_ELEVATION_REQUIRED) => {
            launch_elevated(&full_path, folder).map(|handle| LaunchedTrainer {
                mode: LaunchMode::Elevated,
                process: ProcessHandle::Elevated(handle),
            })
        }
        Err(err) => Err(err),
    }
}

fn wide(value: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn launch_elevated(
    exe: &Path,
    folder: &Path,
) -> Result<std::os::windows::io::OwnedHandle, std::io::Error> {
    use std::os::windows::io::FromRawHandle;

    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let exe = wide(exe);
    let folder = wide(folder);

    // ShellExecuteW (and ShellExecuteEx without SEE_MASK_NOASYNC) returns
    // before elevation finishes, and Windows abandons the pending UAC request
    // if the requesting process exits first - which is exactly what
    // --closeafterlaunch does a second later. NOASYNC keeps this call blocked
    // until the user has answered the prompt.
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe.as_ptr()),
        lpDirectory: PCWSTR(folder.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    unsafe { ShellExecuteExW(&mut info) }.map_err(|err| {
        std::io::Error::other(format!("could not launch elevated: {}", err.message()))
    })?;

    if info.hProcess.is_invalid() {
        return Err(std::io::Error::other(
            "elevated trainer launch returned no process handle",
        ));
    }

    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(info.hProcess.0) })
}

/// Deletes a trainer's executable from the trainer folder. A file that has
/// already vanished counts as success - the caller only needs it gone, and its
/// config entry still has to be dropped either way.
pub fn delete_trainer_file(folder: &Path, filename: &str) -> Result<(), std::io::Error> {
    match std::fs::remove_file(folder.join(filename)) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn stem(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string()
}

/// Reconciles the configured trainer list against what's actually on disk:
/// files still present keep their saved metadata (name, shortcut, cheats),
/// new files get a fresh default entry, and files that vanished are dropped.
pub fn sync_trainer_configs(
    existing: &[TrainerConfig],
    folder: &Path,
) -> Result<Vec<TrainerConfig>, std::io::Error> {
    let discovered = discover_trainers(folder)?;
    let mut result = Vec::with_capacity(discovered.len());

    for info in discovered {
        // Version is read straight off the exe, same as size - never
        // user-editable, so it's refreshed every sync rather than preserved.
        let version =
            exe_version::extract_version(&folder.join(&info.filename)).unwrap_or_default();

        if let Some(found) = existing.iter().find(|t| t.filename == info.filename) {
            let mut cfg = found.clone();
            cfg.size_bytes = info.size_bytes;
            cfg.version = version;
            result.push(cfg);
        } else {
            result.push(TrainerConfig {
                name: stem(&info.filename),
                filename: info.filename,
                version,
                size_bytes: info.size_bytes,
                game_exe: None,
                game_args: None,
                launch_shortcut: None,
                default_cheats: Vec::new(),
                close_after_launch: false,
            });
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_exe(dir: &Path, name: &str, contents: &[u8]) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    #[test]
    fn discover_trainers_finds_only_exe_files() {
        let dir = std::env::temp_dir().join(format!("rallx-test-discover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_exe(&dir, "trainer.exe", b"abc");
        write_exe(&dir, "readme.txt", b"not an exe");

        let found = discover_trainers(&dir).unwrap();

        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].filename, "trainer.exe");
        assert_eq!(found[0].size_bytes, 3);
    }

    // A game is referenced in place, so the checks are the opposite of a
    // trainer import: living outside the trainer folder is the normal case and
    // must not be rejected, but a path that isn't a real .exe must be.
    #[test]
    fn resolve_game_selection_accepts_any_real_exe_and_rejects_the_rest() {
        let dir = std::env::temp_dir().join(format!("rallx-test-game-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_exe(&dir, "game.exe", b"abc");
        write_exe(&dir, "notes.txt", b"abc");

        let exe = resolve_game_selection(&dir.join("game.exe"));
        let txt = resolve_game_selection(&dir.join("notes.txt"));
        let missing = resolve_game_selection(&dir.join("absent.exe"));

        std::fs::remove_dir_all(&dir).unwrap();

        // A plain .exe carries no launch options - only a .lnk can supply them.
        assert!(matches!(exe, Ok(ref sel) if sel.args.is_empty()));
        assert!(matches!(txt, Err(GameExeError::NotAnExe)));
        assert!(matches!(missing, Err(GameExeError::NotFound)));
    }

    #[test]
    fn game_bat_starts_the_game_then_the_trainer() {
        let bat = build_game_bat(
            Path::new("C:\\Games\\RDR2\\RDR2.exe"),
            "-dx12 -windowed",
            Path::new("C:\\Rallx\\rallx-cheat-launcher.exe"),
            "rdr2-trainer.exe",
            true,
        );

        let lines: Vec<&str> = bat.lines().collect();
        assert_eq!(lines[0], BAT_MARKER);
        assert_eq!(lines[2], "cd /d \"%~dp0\"");
        assert_eq!(
            lines[3],
            "start \"\" \"C:\\Games\\RDR2\\RDR2.exe\" -dx12 -windowed"
        );
        assert_eq!(
            lines[4],
            "start \"\" \"C:\\Rallx\\rallx-cheat-launcher.exe\" --launch=\"rdr2-trainer.exe\" --closeafterlaunch"
        );
    }

    #[test]
    fn game_bat_omits_empty_options_and_doubles_percent_signs() {
        let bat = build_game_bat(
            Path::new("C:\\100%% Games\\g.exe"),
            "   ",
            Path::new("rallx.exe"),
            "t.exe",
            false,
        );

        let lines: Vec<&str> = bat.lines().collect();
        assert_eq!(lines[3], "start \"\" \"C:\\100%%%% Games\\g.exe\"");
        assert!(!bat.contains("--closeafterlaunch"));
    }

    #[test]
    fn generate_game_bat_writes_next_to_the_game_and_wont_clobber_a_foreign_file() {
        let dir = scratch("bat");
        write_exe(&dir, "Game.exe", b"abc");

        let written = generate_game_bat(
            &dir.join("Game.exe"),
            "-dx11",
            Path::new("rallx.exe"),
            "t.exe",
            false,
        )
        .unwrap();
        let regenerated = generate_game_bat(
            &dir.join("Game.exe"),
            "",
            Path::new("rallx.exe"),
            "t.exe",
            false,
        );

        // A .bat the user already had under that name is never overwritten.
        std::fs::write(dir.join("Game.bat"), "echo mine\r\n").unwrap();
        let foreign = generate_game_bat(
            &dir.join("Game.exe"),
            "",
            Path::new("rallx.exe"),
            "t.exe",
            false,
        );

        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(written.file_name().unwrap(), "Game.bat");
        assert!(regenerated.is_ok());
        assert!(matches!(foreign, Err(BatError::WouldOverwrite(name)) if name == "Game.bat"));
    }

    #[test]
    fn generate_game_bat_wont_clobber_a_non_utf8_foreign_file() {
        let dir = scratch("bat-non-utf8");
        write_exe(&dir, "Game.exe", b"abc");
        let dest = dir.join("Game.bat");
        let original = b"echo mine\r\n\xff";
        std::fs::write(&dest, original).unwrap();

        let result = generate_game_bat(
            &dir.join("Game.exe"),
            "",
            Path::new("rallx.exe"),
            "t.exe",
            false,
        );
        let contents = std::fs::read(&dest).unwrap();

        std::fs::remove_dir_all(&dir).unwrap();

        assert!(matches!(result, Err(BatError::WouldOverwrite(name)) if name == "Game.bat"));
        assert_eq!(contents, original);
    }

    #[test]
    fn generate_game_bat_needs_both_a_game_and_a_trainer() {
        assert!(matches!(
            generate_game_bat(Path::new(""), "", Path::new("rallx.exe"), "t.exe", false),
            Err(BatError::NoGameExe)
        ));
        assert!(matches!(
            generate_game_bat(
                Path::new("C:\\g\\game.exe"),
                "",
                Path::new("rallx.exe"),
                "  ",
                false,
            ),
            Err(BatError::NoTrainer)
        ));
    }

    #[test]
    fn sync_trainer_configs_preserves_existing_metadata() {
        let dir = std::env::temp_dir().join(format!("rallx-test-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_exe(&dir, "trainer.exe", b"abcd");

        let existing = vec![TrainerConfig {
            name: "My Trainer".to_string(),
            filename: "trainer.exe".to_string(),
            version: "1.2.3".to_string(),
            size_bytes: 0,
            game_exe: Some("Game.exe".to_string()),
            game_args: None,
            launch_shortcut: Some("Ctrl+F1".to_string()),
            default_cheats: vec![crate::config::CheatConfig {
                label: "Infinite Health".to_string(),
                key: "Numpad1".to_string(),
            }],
            close_after_launch: true,
        }];

        let synced = sync_trainer_configs(&existing, &dir).unwrap();

        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].name, "My Trainer");
        // Version is re-read from the exe on every sync rather than kept
        // from config - the fixture file has no version resource, so it
        // comes back empty rather than the stale "1.2.3".
        assert_eq!(synced[0].version, "");
        assert_eq!(synced[0].size_bytes, 4);
        assert_eq!(synced[0].launch_shortcut.as_deref(), Some("Ctrl+F1"));
    }

    #[test]
    fn sync_trainer_configs_adds_new_and_drops_missing() {
        let dir = std::env::temp_dir().join(format!("rallx-test-sync2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_exe(&dir, "new-trainer.exe", b"abcde");

        let existing = vec![TrainerConfig {
            name: "Gone".to_string(),
            filename: "gone.exe".to_string(),
            version: String::new(),
            size_bytes: 0,
            game_exe: None,
            game_args: None,
            launch_shortcut: None,
            default_cheats: Vec::new(),
            close_after_launch: false,
        }];

        let synced = sync_trainer_configs(&existing, &dir).unwrap();

        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].filename, "new-trainer.exe");
        assert_eq!(synced[0].name, "new-trainer");
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rallx-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validate_import_rejects_exe_already_in_trainer_folder() {
        let folder = scratch("import-same");
        write_exe(&folder, "trainer.exe", b"abc");

        let err = validate_import(&folder.join("trainer.exe"), &folder).unwrap_err();

        std::fs::remove_dir_all(&folder).unwrap();

        assert!(matches!(err, ImportError::AlreadyInFolder));
    }

    #[test]
    fn validate_import_rejects_non_exe() {
        let folder = scratch("import-ext");
        let src = scratch("import-ext-src");
        std::fs::write(src.join("notes.txt"), b"abc").unwrap();

        let err = validate_import(&src.join("notes.txt"), &folder).unwrap_err();

        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::remove_dir_all(&src).unwrap();

        assert!(matches!(err, ImportError::NotAnExe));
    }

    #[test]
    fn validate_import_rejects_duplicate_filename() {
        let folder = scratch("import-dup");
        let src = scratch("import-dup-src");
        write_exe(&folder, "trainer.exe", b"abc");
        write_exe(&src, "trainer.exe", b"def");

        let err = validate_import(&src.join("trainer.exe"), &folder).unwrap_err();

        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::remove_dir_all(&src).unwrap();

        assert!(matches!(err, ImportError::DestinationExists(name) if name == "trainer.exe"));
    }

    #[test]
    fn delete_trainer_file_removes_exe_and_tolerates_a_missing_one() {
        let folder = scratch("delete");
        write_exe(&folder, "trainer.exe", b"abc");

        delete_trainer_file(&folder, "trainer.exe").unwrap();
        let gone = !folder.join("trainer.exe").exists();
        let second = delete_trainer_file(&folder, "trainer.exe");

        std::fs::remove_dir_all(&folder).unwrap();

        assert!(gone);
        assert!(second.is_ok());
    }

    #[test]
    fn import_trainer_moves_exe_into_folder() {
        let folder = scratch("import-move");
        let src = scratch("import-move-src");
        write_exe(&src, "trainer.exe", b"abcd");

        let filename = import_trainer(&src.join("trainer.exe"), &folder).unwrap();
        let moved = folder.join("trainer.exe").exists();
        let source_gone = !src.join("trainer.exe").exists();

        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::remove_dir_all(&src).unwrap();

        assert_eq!(filename, "trainer.exe");
        assert!(moved);
        assert!(source_gone);
    }
}
