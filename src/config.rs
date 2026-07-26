#![allow(dead_code)]

use serde::{Deserialize, Deserializer, Serialize};

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThemeConfig {
    pub accent: String,
    pub background: String,
    pub style: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct CheatConfig {
    pub label: String,
    pub key: String,
}

// Earlier configs stored default cheats as bare key strings; those are still
// accepted and read back with an empty label.
impl<'de> Deserialize<'de> for CheatConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Key(String),
            Labeled {
                #[serde(default)]
                label: String,
                key: String,
            },
        }

        Ok(match Raw::deserialize(deserializer)? {
            Raw::Key(key) => CheatConfig {
                label: String::new(),
                key,
            },
            Raw::Labeled { label, key } => CheatConfig { label, key },
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrainerConfig {
    pub name: String,
    pub filename: String, // Filename only, not full path
    pub version: String,
    pub size_bytes: u64,
    pub game_exe: Option<String>,
    pub launch_shortcut: Option<String>,
    pub default_cheats: Vec<CheatConfig>,
    pub close_after_launch: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub trainer_folder: Option<PathBuf>,
    pub default_shortcut: Option<String>,
    pub theme: ThemeConfig,
    pub close_after_launch_global: bool,
    pub trainers: Vec<TrainerConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            trainer_folder: None,
            default_shortcut: None,
            theme: ThemeConfig {
                accent: "#3b82f6".to_string(),
                background: "#121214".to_string(),
                style: "dark".to_string(),
            },
            close_after_launch_global: false,
            trainers: Vec::new(),
        }
    }
}

pub fn get_bootstrap_path() -> PathBuf {
    // Store last_folder.txt next to the executable (portable behavior)
    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        exe_path.join("last_folder.txt")
    } else {
        PathBuf::from("last_folder.txt")
    }
}

pub fn get_config_path(folder: &Path) -> PathBuf {
    folder.join("config.json")
}

pub fn load_config() -> (AppConfig, Option<PathBuf>) {
    let bootstrap = get_bootstrap_path();
    let folder_path = if bootstrap.exists() {
        fs::read_to_string(&bootstrap)
            .ok()
            .map(|s| PathBuf::from(s.trim()))
    } else {
        None
    };

    if let Some(ref folder) = folder_path {
        let config_path = get_config_path(folder);
        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                    return (config, Some(folder.clone()));
                }
            }
        }
    }

    // Default configuration if nothing is loaded
    (AppConfig::default(), folder_path)
}

pub fn save_config(config: &AppConfig) -> Result<(), std::io::Error> {
    if let Some(ref folder) = config.trainer_folder {
        // Save the folder path to bootstrap file
        let bootstrap = get_bootstrap_path();
        fs::write(&bootstrap, folder.to_string_lossy().as_ref())?;

        // Ensure folder exists and save config.json inside it
        fs::create_dir_all(folder)?;
        let config_path = get_config_path(folder);
        let content = serde_json::to_string_pretty(config)?;
        fs::write(config_path, content)?;
    }
    Ok(())
}
