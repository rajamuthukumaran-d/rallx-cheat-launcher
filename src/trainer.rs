#![allow(dead_code)]

use std::path::Path;

use crate::config::TrainerConfig;

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
        if let Some(found) = existing.iter().find(|t| t.filename == info.filename) {
            let mut cfg = found.clone();
            cfg.size_bytes = info.size_bytes;
            result.push(cfg);
        } else {
            result.push(TrainerConfig {
                name: stem(&info.filename),
                filename: info.filename,
                version: String::new(),
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
            default_cheats: vec!["Numpad1".to_string()],
            close_after_launch: true,
        }];

        let synced = sync_trainer_configs(&existing, &dir).unwrap();

        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].name, "My Trainer");
        assert_eq!(synced[0].version, "1.2.3");
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
}
