//! Parsing and injection of key combos ("ctrl+num1", "Insert", "Shift + F5").
//!
//! One parse feeds both consumers: `global-hotkey` registration (via
//! [`KeyCombo::canonical`], whose vocabulary that crate's parser accepts) and
//! Win32 `SendInput` injection (via the virtual-key codes in [`Key`]).

use std::fmt;
use std::thread::sleep;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};

/// How long a synthesized key stays down. Trainers poll `GetAsyncKeyState`
/// rather than reading the message queue, so a down/up pair sent in the same
/// tick is missed entirely.
pub const KEY_DWELL: Duration = Duration::from_millis(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// Name in the vocabulary `global-hotkey`'s parser accepts, so
    /// [`KeyCombo::canonical`] round-trips through `HotKey::from_str`.
    pub name: &'static str,
    pub vk: u16,
    /// Keys on the extended part of the keyboard need `KEYEVENTF_EXTENDEDKEY`,
    /// otherwise Windows delivers their numpad twin (Insert becomes Numpad0).
    pub extended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub modifiers: Modifiers,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyParseError {
    Empty,
    UnknownKey(String),
    MultipleKeys(String),
    ModifiersOnly(String),
}

impl fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty key combo"),
            Self::UnknownKey(token) => write!(f, "unknown key \"{token}\""),
            Self::MultipleKeys(combo) => {
                write!(f, "\"{combo}\" has more than one non-modifier key")
            }
            Self::ModifiersOnly(combo) => write!(f, "\"{combo}\" has no key besides modifiers"),
        }
    }
}

impl std::error::Error for KeyParseError {}

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];

const DIGITS: [&str; 10] = [
    "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8",
    "Digit9",
];

const NUMPAD: [&str; 10] = [
    "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7",
    "Numpad8", "Numpad9",
];

const FKEYS: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

/// `(aliases, canonical name, virtual-key code, extended)`. Aliases are matched
/// after whitespace/underscore/hyphen removal and upper-casing.
const NAMED: &[(&[&str], &str, u16, bool)] = &[
    (&["ESCAPE", "ESC"], "Escape", 0x1B, false),
    (&["ENTER", "RETURN"], "Enter", 0x0D, false),
    (&["SPACE", "SPACEBAR"], "Space", 0x20, false),
    (&["TAB"], "Tab", 0x09, false),
    (&["BACKSPACE"], "Backspace", 0x08, false),
    (&["INSERT", "INS"], "Insert", 0x2D, true),
    (&["DELETE", "DEL"], "Delete", 0x2E, true),
    (&["HOME"], "Home", 0x24, true),
    (&["END"], "End", 0x23, true),
    (&["PAGEUP", "PGUP"], "PageUp", 0x21, true),
    (&["PAGEDOWN", "PGDN"], "PageDown", 0x22, true),
    (&["ARROWUP", "UP"], "ArrowUp", 0x26, true),
    (&["ARROWDOWN", "DOWN"], "ArrowDown", 0x28, true),
    (&["ARROWLEFT", "LEFT"], "ArrowLeft", 0x25, true),
    (&["ARROWRIGHT", "RIGHT"], "ArrowRight", 0x27, true),
    (&["CAPSLOCK"], "CapsLock", 0x14, false),
    (&["NUMLOCK"], "NumLock", 0x90, true),
    (&["SCROLLLOCK"], "ScrollLock", 0x91, false),
    (&["PAUSE", "PAUSEBREAK", "BREAK"], "Pause", 0x13, false),
    (&["PRINTSCREEN", "PRTSC"], "PrintScreen", 0x2C, false),
    (
        &["NUMPADADD", "NUMADD", "NUMPADPLUS", "NUMPLUS"],
        "NumpadAdd",
        0x6B,
        false,
    ),
    (
        &["NUMPADSUBTRACT", "NUMSUBTRACT", "NUMPADMINUS", "NUMMINUS"],
        "NumpadSubtract",
        0x6D,
        false,
    ),
    (
        &["NUMPADMULTIPLY", "NUMMULTIPLY"],
        "NumpadMultiply",
        0x6A,
        false,
    ),
    (&["NUMPADDIVIDE", "NUMDIVIDE"], "NumpadDivide", 0x6F, true),
    (
        &["NUMPADDECIMAL", "NUMDECIMAL"],
        "NumpadDecimal",
        0x6E,
        false,
    ),
    (&["NUMPADENTER", "NUMENTER"], "NumpadEnter", 0x0D, true),
    (&["MINUS"], "Minus", 0xBD, false),
    (&["EQUAL"], "Equal", 0xBB, false),
    (&["BRACKETLEFT"], "BracketLeft", 0xDB, false),
    (&["BRACKETRIGHT"], "BracketRight", 0xDD, false),
    (&["BACKSLASH"], "Backslash", 0xDC, false),
    (&["SEMICOLON"], "Semicolon", 0xBA, false),
    (&["QUOTE"], "Quote", 0xDE, false),
    (&["BACKQUOTE", "TILDE"], "Backquote", 0xC0, false),
    (&["COMMA"], "Comma", 0xBC, false),
    (&["PERIOD"], "Period", 0xBE, false),
    (&["SLASH"], "Slash", 0xBF, false),
];

const VK_CONTROL: u16 = 0x11;
const VK_SHIFT: u16 = 0x10;
const VK_MENU: u16 = 0x12;
const VK_LWIN: u16 = 0x5B;

fn punctuation(c: char) -> Option<&'static str> {
    Some(match c {
        '-' => "MINUS",
        '=' => "EQUAL",
        '[' => "BRACKETLEFT",
        ']' => "BRACKETRIGHT",
        '\\' => "BACKSLASH",
        ';' => "SEMICOLON",
        '\'' => "QUOTE",
        '`' => "BACKQUOTE",
        ',' => "COMMA",
        '.' => "PERIOD",
        '/' => "SLASH",
        _ => return None,
    })
}

fn normalize(token: &str) -> String {
    token
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

pub fn parse_key(token: &str) -> Result<Key, KeyParseError> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(KeyParseError::Empty);
    }

    // Single-character tokens are matched before normalization, which would
    // otherwise strip the punctuation keys `-` and `_`.
    let normalized = match trimmed.chars().collect::<Vec<_>>().as_slice() {
        [c] => punctuation(*c)
            .map(str::to_string)
            .unwrap_or_else(|| normalize(trimmed)),
        _ => normalize(trimmed),
    };

    if let Some(index) = LETTERS.iter().position(|name| *name == normalized) {
        return Ok(Key {
            name: LETTERS[index],
            vk: 0x41 + index as u16,
            extended: false,
        });
    }

    if let Some(digit) = digit_after(&normalized, &["", "DIGIT"]) {
        return Ok(Key {
            name: DIGITS[digit],
            vk: 0x30 + digit as u16,
            extended: false,
        });
    }

    if let Some(digit) = digit_after(&normalized, &["NUMPAD", "NUM"]) {
        return Ok(Key {
            name: NUMPAD[digit],
            vk: 0x60 + digit as u16,
            extended: false,
        });
    }

    if let Some(index) = FKEYS.iter().position(|name| *name == normalized) {
        return Ok(Key {
            name: FKEYS[index],
            vk: 0x70 + index as u16,
            extended: false,
        });
    }

    for (aliases, name, vk, extended) in NAMED {
        if aliases.contains(&normalized.as_str()) {
            return Ok(Key {
                name,
                vk: *vk,
                extended: *extended,
            });
        }
    }

    Err(KeyParseError::UnknownKey(trimmed.to_string()))
}

/// Single digit following any of `prefixes`, e.g. `("NUM1", ["NUMPAD", "NUM"])
/// -> Some(1)`.
fn digit_after(normalized: &str, prefixes: &[&str]) -> Option<usize> {
    prefixes.iter().find_map(|prefix| {
        let rest = normalized.strip_prefix(prefix)?;
        match rest.chars().collect::<Vec<_>>().as_slice() {
            [c] if c.is_ascii_digit() => Some(*c as usize - '0' as usize),
            _ => None,
        }
    })
}

pub fn parse_combo(combo: &str) -> Result<KeyCombo, KeyParseError> {
    if combo.trim().is_empty() {
        return Err(KeyParseError::Empty);
    }

    let mut modifiers = Modifiers::default();
    let mut key = None;

    for token in combo.split('+') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match normalize(token).as_str() {
            "CTRL" | "CONTROL" => modifiers.ctrl = true,
            "ALT" | "OPTION" => modifiers.alt = true,
            "SHIFT" => modifiers.shift = true,
            "WIN" | "SUPER" | "META" | "CMD" | "COMMAND" => modifiers.win = true,
            _ => {
                if key.is_some() {
                    return Err(KeyParseError::MultipleKeys(combo.trim().to_string()));
                }
                key = Some(parse_key(token)?);
            }
        }
    }

    match key {
        Some(key) => Ok(KeyCombo { modifiers, key }),
        None => Err(KeyParseError::ModifiersOnly(combo.trim().to_string())),
    }
}

/// Parses the comma-separated list used by `--defaultcheat`.
pub fn parse_combo_list(list: &str) -> Result<Vec<KeyCombo>, KeyParseError> {
    list.split(',')
        .filter(|entry| !entry.trim().is_empty())
        .map(parse_combo)
        .collect()
}

impl KeyCombo {
    /// `"Ctrl+Numpad1"` - the form written into generated launch scripts, and
    /// the form handed to `global_hotkey::hotkey::HotKey::from_str`.
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (active, name) in [
            (self.modifiers.ctrl, "Ctrl"),
            (self.modifiers.alt, "Alt"),
            (self.modifiers.shift, "Shift"),
            (self.modifiers.win, "Super"),
        ] {
            if active {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(self.key.name);
        out
    }

    fn modifier_vks(&self) -> Vec<u16> {
        let mut vks = Vec::new();
        if self.modifiers.ctrl {
            vks.push(VK_CONTROL);
        }
        if self.modifiers.alt {
            vks.push(VK_MENU);
        }
        if self.modifiers.shift {
            vks.push(VK_SHIFT);
        }
        if self.modifiers.win {
            vks.push(VK_LWIN);
        }
        vks
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

fn key_input(vk: u16, extended: bool, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), std::io::Error> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Presses and releases a combo system-wide. Modifiers go down first and come
/// up last, mirroring what a real keyboard produces.
pub fn press(combo: &KeyCombo) -> Result<(), std::io::Error> {
    let modifiers = combo.modifier_vks();

    let mut down: Vec<INPUT> = modifiers
        .iter()
        .map(|vk| key_input(*vk, *vk == VK_LWIN, false))
        .collect();
    down.push(key_input(combo.key.vk, combo.key.extended, false));
    send(&down)?;

    sleep(KEY_DWELL);

    let mut up = vec![key_input(combo.key.vk, combo.key.extended, true)];
    up.extend(
        modifiers
            .iter()
            .rev()
            .map(|vk| key_input(*vk, *vk == VK_LWIN, true)),
    );
    send(&up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numpad_spellings_to_the_same_key() {
        for spelling in ["num1", "Num 1", "NUMPAD1", "numpad_1", "Numpad 1"] {
            let key = parse_key(spelling).expect(spelling);
            assert_eq!(key.name, "Numpad1");
            assert_eq!(key.vk, 0x61);
            assert!(!key.extended);
        }
    }

    #[test]
    fn digits_are_not_confused_with_numpad_keys() {
        assert_eq!(parse_key("1").unwrap().vk, 0x31);
        assert_eq!(parse_key("Digit1").unwrap().name, "Digit1");
        assert_eq!(parse_key("num1").unwrap().vk, 0x61);
    }

    #[test]
    fn navigation_keys_are_flagged_extended() {
        assert!(parse_key("insert").unwrap().extended);
        assert!(parse_key("Page Up").unwrap().extended);
        assert!(!parse_key("F5").unwrap().extended);
        assert!(!parse_key("num0").unwrap().extended);
    }

    #[test]
    fn parses_modifier_combos_in_any_order_or_casing() {
        let combo = parse_combo("ctrl+num1").unwrap();
        assert!(combo.modifiers.ctrl);
        assert_eq!(combo.key.name, "Numpad1");

        let spaced = parse_combo("Ctrl + Shift + F5").unwrap();
        assert!(spaced.modifiers.ctrl && spaced.modifiers.shift);
        assert_eq!(spaced.key.name, "F5");

        // The key recorder writes "Ctrl + Alt + X"; it has to parse back.
        assert_eq!(
            parse_combo("Ctrl + Alt + X").unwrap().canonical(),
            "Ctrl+Alt+X"
        );
    }

    #[test]
    fn canonical_round_trips_through_the_parser() {
        for input in [
            "ctrl+num1",
            "insert",
            "Shift+F12",
            "win+d",
            "alt + page down",
        ] {
            let once = parse_combo(input).expect(input);
            let twice = parse_combo(&once.canonical()).expect(input);
            assert_eq!(once, twice);
        }
    }

    // The canonical spelling is fed straight to global-hotkey's own parser, so
    // a name it rejects would only fail at runtime on hotkey registration.
    #[test]
    fn canonical_names_are_accepted_by_global_hotkey() {
        for input in [
            "ctrl+num1",
            "insert",
            "shift+f12",
            "alt+pagedown",
            "win+d",
            "numpadadd",
            "ctrl+shift+arrowup",
            "a",
            "9",
        ] {
            let combo = parse_combo(input).expect(input);
            let canonical = combo.canonical();
            assert!(
                canonical.parse::<global_hotkey::hotkey::HotKey>().is_ok(),
                "global-hotkey rejected {canonical}"
            );
        }
    }

    #[test]
    fn rejects_combos_without_a_key_or_with_two() {
        assert!(matches!(
            parse_combo("ctrl+shift"),
            Err(KeyParseError::ModifiersOnly(_))
        ));
        assert!(matches!(
            parse_combo("ctrl+a+b"),
            Err(KeyParseError::MultipleKeys(_))
        ));
        assert!(matches!(parse_combo(""), Err(KeyParseError::Empty)));
        assert!(matches!(
            parse_combo("ctrl+banana"),
            Err(KeyParseError::UnknownKey(_))
        ));
    }

    #[test]
    fn parses_the_defaultcheat_list() {
        let combos = parse_combo_list("ctrl+num1,num3, ctrl+num5 ,").unwrap();
        let names: Vec<String> = combos.iter().map(KeyCombo::canonical).collect();
        assert_eq!(names, ["Ctrl+Numpad1", "Numpad3", "Ctrl+Numpad5"]);
    }
}
