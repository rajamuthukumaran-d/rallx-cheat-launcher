#![allow(dead_code)]

use serde::{Deserialize, Deserializer, Serialize};

use std::fs;
use std::path::PathBuf;

/// Mirrors the three values the Slint `Theme` global actually owns. `accent` is
/// a `#rrggbb` string, `background` is "dark"/"light" and `style` is
/// "comfortable"/"compact"; anything unrecognised (including the hex background
/// colors earlier builds wrote here) falls back to the default.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThemeConfig {
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_style")]
    pub style: String,
}

fn default_accent() -> String {
    "#5b8cff".to_string()
}

fn default_background() -> String {
    "dark".to_string()
}

fn default_style() -> String {
    "comfortable".to_string()
}

fn default_true() -> bool {
    true
}

fn default_launch_shortcut() -> Option<String> {
    Some("Ctrl+Alt+Shift+F3".to_string())
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent: default_accent(),
            background: default_background(),
            style: default_style(),
        }
    }
}

impl ThemeConfig {
    pub fn is_dark(&self) -> bool {
        !self.background.eq_ignore_ascii_case("light")
    }

    pub fn is_compact(&self) -> bool {
        self.style.eq_ignore_ascii_case("compact")
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.background = if dark { "dark" } else { "light" }.to_string();
    }

    pub fn set_compact(&mut self, compact: bool) {
        self.style = if compact { "compact" } else { "comfortable" }.to_string();
    }

    /// `(r, g, b)` of the accent, falling back to the default accent when the
    /// stored string isn't a `#rrggbb` value.
    pub fn accent_rgb(&self) -> (u8, u8, u8) {
        parse_hex_rgb(&self.accent).unwrap_or((0x5b, 0x8c, 0xff))
    }
}

pub fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((
        u8::from_str_radix(&digits[0..2], 16).ok()?,
        u8::from_str_radix(&digits[2..4], 16).ok()?,
        u8::from_str_radix(&digits[4..6], 16).ok()?,
    ))
}

pub fn format_hex_rgb(r: u8, g: u8, b: u8) -> String {
    format!("#{:02x}{:02x}{:02x}", r, g, b)
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
    /// Command line to start `game_exe` with, as taken from a picked .lnk or
    /// typed by hand. Only ever written into a generated .bat - Rallx itself
    /// never starts the game. `default` because configs predate the field.
    #[serde(default)]
    pub game_args: Option<String>,
    /// Executable used to match the running game when the global shortcut is
    /// pressed. Its lifetime also controls cleanup: once this exact executable
    /// has been seen and then exits, Rallx terminates the trainer it launched.
    /// Rallx itself exits afterward only in launch-option background mode.
    #[serde(default)]
    pub watched_exe: Option<String>,
    pub launch_shortcut: Option<String>,
    pub default_cheats: Vec<CheatConfig>,
    pub close_after_launch: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub trainer_folder: Option<PathBuf>,
    #[serde(default = "default_launch_shortcut")]
    pub default_shortcut: Option<String>,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default = "default_true")]
    pub close_after_launch_global: bool,
    #[serde(default = "default_true")]
    pub confirm_exit: bool,
    /// Whether startup should hand off to an elevated copy of the app. Off by
    /// default: elevation is only needed to reach trainers that run elevated
    /// themselves, and it costs a UAC prompt every launch.
    #[serde(default)]
    pub run_as_admin: bool,
    #[serde(default)]
    pub trainers: Vec<TrainerConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            trainer_folder: None,
            default_shortcut: default_launch_shortcut(),
            theme: ThemeConfig::default(),
            close_after_launch_global: true,
            confirm_exit: true,
            run_as_admin: false,
            trainers: Vec::new(),
        }
    }
}

/// config.json lives next to the executable, so the app is self-contained and
/// needs nothing on disk to find its own settings - the trainer folder is just
/// one more value inside it.
pub fn get_config_path() -> PathBuf {
    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        exe_path.join("config.json")
    } else {
        PathBuf::from("config.json")
    }
}

pub fn load_config() -> AppConfig {
    let config_path = get_config_path();
    if let Ok(content) = fs::read_to_string(&config_path) {
        if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
            if config.default_shortcut.is_none() {
                config.default_shortcut = default_launch_shortcut();
            }
            return config;
        }
    }

    AppConfig::default()
}

pub fn save_config(config: &AppConfig) -> Result<(), std::io::Error> {
    let content = serde_json::to_string_pretty(config)?;
    fs::write(get_config_path(), content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(parse_hex_rgb("#5b8cff"), Some((0x5b, 0x8c, 0xff)));
        assert_eq!(format_hex_rgb(0x5b, 0x8c, 0xff), "#5b8cff");
        assert_eq!(parse_hex_rgb("5b8cff"), None);
        assert_eq!(parse_hex_rgb("#5b8cf"), None);
        assert_eq!(parse_hex_rgb("#zzzzzz"), None);
    }

    #[test]
    fn theme_flags_map_to_strings() {
        let mut theme = ThemeConfig::default();
        assert!(theme.is_dark());
        assert!(!theme.is_compact());

        theme.set_dark(false);
        theme.set_compact(true);
        assert_eq!(theme.background, "light");
        assert_eq!(theme.style, "compact");
        assert!(!theme.is_dark());
        assert!(theme.is_compact());
    }

    // Configs written before the theme fields meant dark/comfortable stored a
    // hex background and "dark" as the style; both must degrade to the default
    // rather than being read as light/compact.
    #[test]
    fn legacy_theme_values_fall_back_to_defaults() {
        let theme = ThemeConfig {
            accent: "#3b82f6".to_string(),
            background: "#121214".to_string(),
            style: "dark".to_string(),
        };
        assert!(theme.is_dark());
        assert!(!theme.is_compact());
        assert_eq!(theme.accent_rgb(), (0x3b, 0x82, 0xf6));
    }

    #[test]
    fn missing_fields_use_defaults_without_losing_the_rest() {
        let config: AppConfig =
            serde_json::from_str(r#"{"trainer_folder":"C:\\trainers"}"#).expect("parses");
        assert_eq!(config.trainer_folder, Some(PathBuf::from("C:\\trainers")));
        assert!(config.close_after_launch_global);
        assert!(config.confirm_exit);
        assert_eq!(
            config.default_shortcut.as_deref(),
            Some("Ctrl+Alt+Shift+F3")
        );
        // Elevation is opt-in, so a config written before the setting existed
        // must not start prompting for UAC on the next launch.
        assert!(!config.run_as_admin);
        assert_eq!(config.theme.accent, "#5b8cff");
    }

    #[test]
    fn settings_survive_a_save_load_roundtrip() {
        let mut config = AppConfig {
            default_shortcut: Some("Ctrl + F12".to_string()),
            close_after_launch_global: false,
            confirm_exit: false,
            run_as_admin: true,
            ..AppConfig::default()
        };
        config.theme.accent = "#ff9f5b".to_string();
        config.theme.set_dark(false);
        config.theme.set_compact(true);

        let json = serde_json::to_string(&config).expect("serializes");
        let loaded: AppConfig = serde_json::from_str(&json).expect("parses");

        assert_eq!(loaded.default_shortcut.as_deref(), Some("Ctrl + F12"));
        assert!(!loaded.close_after_launch_global);
        assert!(!loaded.confirm_exit);
        assert!(loaded.run_as_admin);
        assert_eq!(loaded.theme.accent_rgb(), (0xff, 0x9f, 0x5b));
        assert!(!loaded.theme.is_dark());
        assert!(loaded.theme.is_compact());
    }

    #[test]
    fn older_trainer_configs_default_to_no_watched_executable() {
        let config: AppConfig = serde_json::from_str(
            r#"{
                "trainers": [{
                    "name": "Trainer",
                    "filename": "trainer.exe",
                    "version": "1.0",
                    "size_bytes": 1,
                    "game_exe": null,
                    "launch_shortcut": null,
                    "default_cheats": [],
                    "close_after_launch": false
                }]
            }"#,
        )
        .expect("parses");

        assert_eq!(config.trainers[0].watched_exe, None);
    }
}
