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

pub fn launch_trainer(folder: &Path, filename: &str) -> Result<(), std::io::Error> {
    let full_path = folder.join(filename);
    std::process::Command::new(full_path).spawn()?;
    Ok(())
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
