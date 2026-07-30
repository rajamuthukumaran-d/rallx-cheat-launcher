//! Launch-option parsing for background/tray mode.
//!
//! ```text
//! rallx-cheat-launcher.exe --launch="rdr2-trainer.exe" --hotkey="insert" \
//!     --defaultcheat="ctrl+num1,num3,ctrl+num5" [--closeafterlaunch]
//! ```
//!
//! Presence of any of `--launch` / `--hotkey` / `--defaultcheat` selects the
//! tray branch in `main`; without them the app starts windowed as usual.

use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub trainer: Option<String>,
    pub hotkey: Option<String>,
    /// Raw comma-separated combos, parsed later by `keys::parse_combo_list` so
    /// argument parsing stays independent of the key vocabulary.
    pub default_cheats: Option<String>,
    pub close_after_launch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    MissingValue(String),
    Unknown(String),
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(flag) => write!(f, "--{flag} needs a value, e.g. --{flag}=\"...\""),
            Self::Unknown(arg) => write!(f, "unrecognized launch option \"{arg}\""),
        }
    }
}

impl std::error::Error for ArgError {}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(trimmed);
    unquoted.to_string()
}

/// Parses the arguments after the executable name. `Ok(None)` means "no launch
/// options given" - the caller should start the normal windowed app.
///
/// Both `--flag=value` and `--flag value` are accepted, with or without the
/// leading dashes, since these end up hand-written into shortcuts and .bat
/// files.
pub fn parse<S: AsRef<str>>(args: &[S]) -> Result<Option<LaunchOptions>, ArgError> {
    let mut options = LaunchOptions::default();
    let mut args = args.iter().map(AsRef::as_ref).peekable();

    while let Some(arg) = args.next() {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        let body = arg.trim_start_matches('-');
        let (name, inline_value) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };

        let mut value = || match inline_value {
            Some(value) => Ok(unquote(value)),
            // `next_if` so a flag that turns out to have no value leaves the
            // following option in place instead of eating it.
            None => args
                .next_if(|next| !next.trim_start().starts_with("--"))
                .map(unquote)
                .ok_or_else(|| ArgError::MissingValue(name.to_lowercase())),
        };

        match name.to_lowercase().as_str() {
            "launch" | "trainer" => options.trainer = Some(value()?),
            "hotkey" | "shortcut" => options.hotkey = Some(value()?),
            "defaultcheat" | "defaultcheats" | "cheat" | "cheats" => {
                options.default_cheats = Some(value()?)
            }
            "closeafterlaunch" => options.close_after_launch = true,
            // A bare path is what Explorer passes when a file is dropped onto
            // the app's icon. That is a windowed-mode gesture, not a launch
            // option, so it must not abort startup the way a misspelled flag
            // should. Undashed tokens are checked against the flag names above
            // first, so `launch=t.exe` still parses.
            _ if !arg.starts_with('-') => continue,
            _ => return Err(ArgError::Unknown(arg.to_string())),
        }
    }

    if options.trainer.is_none() && options.hotkey.is_none() && options.default_cheats.is_none() {
        return Ok(None);
    }

    Ok(Some(options))
}

/// Builds the command line that reproduces `trainer` in tray mode - the string
/// the Home screen's copy button puts on the clipboard.
pub fn build_launch_script(
    exe: &str,
    trainer_filename: &str,
    hotkey: Option<&str>,
    cheats: &[String],
    close_after_launch: bool,
) -> String {
    let mut script = format!("\"{exe}\" --launch=\"{trainer_filename}\"");
    if let Some(hotkey) = hotkey.filter(|value| !value.trim().is_empty()) {
        script.push_str(&format!(" --hotkey=\"{hotkey}\""));
    }
    if !cheats.is_empty() {
        script.push_str(&format!(" --defaultcheat=\"{}\"", cheats.join(",")));
    }
    if close_after_launch {
        script.push_str(" --closeafterlaunch");
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_means_windowed_mode() {
        assert_eq!(parse::<&str>(&[]), Ok(None));
    }

    #[test]
    fn parses_the_documented_command_line() {
        let options = parse(&[
            "--launch=rdr2-trainer.exe",
            "--hotkey=insert",
            "--defaultcheat=ctrl+num1,num3,ctrl+num5",
        ])
        .unwrap()
        .unwrap();

        assert_eq!(options.trainer.as_deref(), Some("rdr2-trainer.exe"));
        assert_eq!(options.hotkey.as_deref(), Some("insert"));
        assert_eq!(
            options.default_cheats.as_deref(),
            Some("ctrl+num1,num3,ctrl+num5")
        );
        assert!(!options.close_after_launch);
    }

    // A shortcut target keeps the quotes as part of the argument when the whole
    // `--launch="a b.exe"` token is itself quoted.
    #[test]
    fn strips_quotes_and_accepts_bare_or_separated_forms() {
        let quoted = parse(&["--launch=\"rdr2 trainer.exe\""]).unwrap().unwrap();
        assert_eq!(quoted.trainer.as_deref(), Some("rdr2 trainer.exe"));

        let bare = parse(&["launch=t.exe", "hotkey=insert"]).unwrap().unwrap();
        assert_eq!(bare.trainer.as_deref(), Some("t.exe"));
        assert_eq!(bare.hotkey.as_deref(), Some("insert"));

        let separated = parse(&["--launch", "t.exe"]).unwrap().unwrap();
        assert_eq!(separated.trainer.as_deref(), Some("t.exe"));
    }

    #[test]
    fn close_after_launch_is_a_bare_flag() {
        let options = parse(&["--launch=t.exe", "--closeafterlaunch"])
            .unwrap()
            .unwrap();
        assert!(options.close_after_launch);
    }

    // Dropping a file on the app's icon passes its path as a bare argument.
    // That has to fall through to windowed mode, not kill startup.
    #[test]
    fn a_bare_path_is_ignored_rather_than_rejected() {
        assert_eq!(parse(&["C:\\Users\\me\\Downloads\\trainer.exe"]), Ok(None));

        let alongside = parse(&["C:\\drop\\t.exe", "--launch=t.exe"])
            .unwrap()
            .unwrap();
        assert_eq!(alongside.trainer.as_deref(), Some("t.exe"));
    }

    #[test]
    fn reports_missing_values_and_unknown_flags() {
        assert_eq!(
            parse(&["--launch"]),
            Err(ArgError::MissingValue("launch".into()))
        );
        assert_eq!(
            parse(&["--launch", "--hotkey=insert"]),
            Err(ArgError::MissingValue("launch".into()))
        );
        assert_eq!(
            parse(&["--nonsense=1"]),
            Err(ArgError::Unknown("--nonsense=1".into()))
        );
    }

    #[test]
    fn launch_script_round_trips_through_the_parser() {
        let script = build_launch_script(
            "C:\\Rallx\\rallx-cheat-launcher.exe",
            "rdr2-trainer.exe",
            Some("Insert"),
            &["Ctrl+Numpad1".to_string(), "Numpad3".to_string()],
            true,
        );
        assert_eq!(
            script,
            "\"C:\\Rallx\\rallx-cheat-launcher.exe\" --launch=\"rdr2-trainer.exe\" \
             --hotkey=\"Insert\" --defaultcheat=\"Ctrl+Numpad1,Numpad3\" --closeafterlaunch"
        );

        let args: Vec<&str> = script.split(" --").skip(1).collect();
        let args: Vec<String> = args.iter().map(|arg| format!("--{arg}")).collect();
        let options = parse(&args).unwrap().unwrap();
        assert_eq!(options.trainer.as_deref(), Some("rdr2-trainer.exe"));
        assert_eq!(options.hotkey.as_deref(), Some("Insert"));
        assert_eq!(
            options.default_cheats.as_deref(),
            Some("Ctrl+Numpad1,Numpad3")
        );
        assert!(options.close_after_launch);
    }

    #[test]
    fn launch_script_omits_empty_sections() {
        let script = build_launch_script("rallx.exe", "t.exe", None, &[], false);
        assert_eq!(script, "\"rallx.exe\" --launch=\"t.exe\"");
    }
}
