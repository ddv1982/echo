use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cleanup::CleanupMode;
use crate::language::LanguageChoice;
use crate::paths::{config_path, set_aside_corrupt, write_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineChoice {
    Whisper,
    Parakeet,
    Fake,
    Auto,
}

impl EngineChoice {
    #[must_use]
    pub fn from_env_var(raw: &str) -> Option<Self> {
        match raw {
            "whisper" => Some(Self::Whisper),
            "parakeet" => Some(Self::Parakeet),
            "fake" => Some(Self::Fake),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub engine: Option<EngineChoice>,
    #[serde(default)]
    pub whisper_model: Option<String>,
    #[serde(default)]
    pub cleanup: Option<CleanupMode>,
    #[serde(default)]
    pub hud: Option<bool>,
    #[serde(default)]
    pub hold_key: Option<String>,
    #[serde(default)]
    pub toggle_shortcut: Option<String>,
    #[serde(default)]
    pub record_seconds: Option<u32>,
    #[serde(default)]
    pub microphone: Option<String>,
    #[serde(default)]
    pub language: Option<LanguageChoice>,
}

impl Config {
    /// Validate and normalize the shortcut syntax shared by toggle and
    /// push-to-talk settings. A chord has at most one non-modifier key and
    /// uses a stable modifier order. A single modifier is also accepted for
    /// compatibility with shipped hold-key values such as `RightCtrl`.
    pub fn canonical_shortcut(spec: &str) -> Result<String, ShortcutError> {
        canonical_shortcut(spec)
    }

    /// Toggle shortcuts must include a modifier so ordinary typing cannot
    /// start or stop a recording.
    pub fn canonical_toggle_shortcut(spec: &str) -> Result<String, ShortcutError> {
        let canonical = canonical_shortcut(spec)?;
        if !canonical.contains('+') {
            return Err(ShortcutError(format!(
                "invalid shortcut {spec}: toggle shortcuts need at least one modifier"
            )));
        }
        Ok(canonical)
    }

    pub fn load() -> Result<Self, String> {
        Self::load_from(config_path())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read(path).map_err(|err| err.to_string())?;
        match serde_json::from_slice::<Self>(&raw) {
            Ok(config) => Ok(config),
            Err(_) => {
                set_aside_corrupt(path);
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(config_path())
    }

    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        write_atomic(path.as_ref(), raw.as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutError(String);

impl std::fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShortcutError {}

fn canonical_shortcut(spec: &str) -> Result<String, ShortcutError> {
    let raw = spec.trim();
    if raw.is_empty() {
        return Err(ShortcutError("shortcut cannot be empty".to_string()));
    }

    let parts = raw.split('+').map(str::trim).collect::<Vec<_>>();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(ShortcutError(format!("invalid shortcut {spec}: empty key")));
    }

    let mut modifiers = [false; 4];
    let mut modifier_tokens = Vec::new();
    let mut terminal = None;
    for part in parts {
        let folded = part.to_ascii_lowercase().replace(['_', '-', ' '], "");
        if let Some((index, single_name)) = modifier(&folded) {
            if modifiers[index] {
                return Err(ShortcutError(format!(
                    "invalid shortcut {spec}: duplicate modifier {part}"
                )));
            }
            modifiers[index] = true;
            modifier_tokens.push(single_name);
            continue;
        }

        let key = terminal_key(&folded)
            .ok_or_else(|| ShortcutError(format!("invalid shortcut {spec}: unknown key {part}")))?;
        if terminal.replace(key).is_some() {
            return Err(ShortcutError(format!(
                "invalid shortcut {spec}: more than one non-modifier key"
            )));
        }
    }

    if terminal.is_none() {
        return if modifier_tokens.len() == 1 {
            Ok(modifier_tokens[0].to_string())
        } else {
            Err(ShortcutError(format!(
                "invalid shortcut {spec}: a modifier chord needs a non-modifier key"
            )))
        };
    }

    let names = ["Super", "Ctrl", "Alt", "Shift"];
    let mut canonical = modifiers
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .map(|(index, _)| names[index].to_string())
        .collect::<Vec<_>>();
    canonical.push(terminal.expect("terminal checked").to_string());
    Ok(canonical.join("+"))
}

fn modifier(folded: &str) -> Option<(usize, &'static str)> {
    match folded {
        "super" | "meta" | "win" | "mod4" | "leftsuper" | "lsuper" | "metaleft" => {
            Some((0, "Super"))
        }
        "rightsuper" | "rsuper" | "metaright" => Some((0, "RightSuper")),
        "ctrl" | "control" | "leftctrl" | "lctrl" | "controll" => Some((1, "LeftCtrl")),
        "rightctrl" | "rctrl" | "controlr" => Some((1, "RightCtrl")),
        "alt" | "mod1" | "leftalt" | "lalt" | "altleft" => Some((2, "Alt")),
        "rightalt" | "ralt" | "altright" | "altgr" => Some((2, "RightAlt")),
        "shift" | "leftshift" | "lshift" | "shiftleft" => Some((3, "LeftShift")),
        "rightshift" | "rshift" | "shiftright" => Some((3, "RightShift")),
        _ => None,
    }
}

fn terminal_key(folded: &str) -> Option<&'static str> {
    match folded {
        "space" | "spacebar" => Some("Space"),
        "enter" | "return" => Some("Enter"),
        "tab" => Some("Tab"),
        "backspace" => Some("Backspace"),
        "delete" | "del" => Some("Delete"),
        "insert" | "ins" => Some("Insert"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" | "pgup" => Some("PageUp"),
        "pagedown" | "pgdown" => Some("PageDown"),
        "up" | "arrowup" => Some("ArrowUp"),
        "down" | "arrowdown" => Some("ArrowDown"),
        "left" | "arrowleft" => Some("ArrowLeft"),
        "right" | "arrowright" => Some("ArrowRight"),
        "escape" | "esc" => Some("Escape"),
        "minus" => Some("Minus"),
        "equal" | "equals" => Some("Equal"),
        "bracketleft" | "leftbracket" => Some("BracketLeft"),
        "bracketright" | "rightbracket" => Some("BracketRight"),
        "backslash" => Some("Backslash"),
        "semicolon" => Some("Semicolon"),
        "quote" | "apostrophe" => Some("Quote"),
        "backquote" | "grave" => Some("Backquote"),
        "comma" => Some("Comma"),
        "period" | "dot" => Some("Period"),
        "slash" => Some("Slash"),
        "capslock" => Some("CapsLock"),
        "menu" | "contextmenu" => Some("Menu"),
        _ => {
            if folded.len() == 1 {
                let byte = folded.as_bytes()[0];
                if byte.is_ascii_alphabetic() {
                    return Some(match byte.to_ascii_uppercase() {
                        b'A' => "A",
                        b'B' => "B",
                        b'C' => "C",
                        b'D' => "D",
                        b'E' => "E",
                        b'F' => "F",
                        b'G' => "G",
                        b'H' => "H",
                        b'I' => "I",
                        b'J' => "J",
                        b'K' => "K",
                        b'L' => "L",
                        b'M' => "M",
                        b'N' => "N",
                        b'O' => "O",
                        b'P' => "P",
                        b'Q' => "Q",
                        b'R' => "R",
                        b'S' => "S",
                        b'T' => "T",
                        b'U' => "U",
                        b'V' => "V",
                        b'W' => "W",
                        b'X' => "X",
                        b'Y' => "Y",
                        b'Z' => "Z",
                        _ => unreachable!(),
                    });
                }
                if byte.is_ascii_digit() {
                    return Some(match byte {
                        b'0' => "0",
                        b'1' => "1",
                        b'2' => "2",
                        b'3' => "3",
                        b'4' => "4",
                        b'5' => "5",
                        b'6' => "6",
                        b'7' => "7",
                        b'8' => "8",
                        b'9' => "9",
                        _ => unreachable!(),
                    });
                }
            }
            folded
                .strip_prefix('f')
                .and_then(|number| match number.parse::<u8>().ok()? {
                    1 => Some("F1"),
                    2 => Some("F2"),
                    3 => Some("F3"),
                    4 => Some("F4"),
                    5 => Some("F5"),
                    6 => Some("F6"),
                    7 => Some("F7"),
                    8 => Some("F8"),
                    9 => Some("F9"),
                    10 => Some("F10"),
                    11 => Some("F11"),
                    12 => Some("F12"),
                    _ => None,
                })
        }
    }
}

#[must_use]
pub fn resolve<T>(env: Option<T>, file: Option<T>, default: T) -> T {
    match (env, file) {
        (Some(value), _) => value,
        (None, Some(value)) => value,
        (None, None) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch_path(label: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "echo-config-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = scratch_path("roundtrip");
        let original = Config {
            engine: Some(EngineChoice::Whisper),
            whisper_model: Some("base.en".into()),
            cleanup: Some(CleanupMode::LocalModel {
                model: "llama3".into(),
            }),
            hud: Some(true),
            hold_key: Some("RightCtrl".into()),
            toggle_shortcut: Some("Super+Alt+Space".into()),
            record_seconds: Some(8),
            microphone: Some("USB Mic".into()),
            language: Some(LanguageChoice::Auto),
        };
        original.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap(), original);
    }

    #[test]
    fn missing_field_is_none() {
        let path = scratch_path("partial");
        fs::write(&path, r#"{"engine":"fake"}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.engine, Some(EngineChoice::Fake));
        assert_eq!(loaded.whisper_model, None);
        assert_eq!(loaded.cleanup, None);
        assert_eq!(loaded.hud, None);
        assert_eq!(loaded.hold_key, None);
        assert_eq!(loaded.toggle_shortcut, None);
        assert_eq!(loaded.record_seconds, None);
        assert_eq!(loaded.microphone, None);
        assert_eq!(loaded.language, None);
    }

    #[test]
    fn unknown_field_is_ignored() {
        let path = scratch_path("unknown");
        fs::write(&path, r#"{"engine":"whisper","future_knob":true}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.engine, Some(EngineChoice::Whisper));
        assert_eq!(
            loaded,
            Config {
                engine: Some(EngineChoice::Whisper),
                ..Config::default()
            }
        );
    }

    #[test]
    fn old_hold_key_config_loads_without_toggle_migration() {
        let path = scratch_path("old-shortcut");
        fs::write(&path, r#"{"hold_key":"RightCtrl"}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.hold_key.as_deref(), Some("RightCtrl"));
        assert_eq!(loaded.toggle_shortcut, None);
        assert_eq!(
            Config::canonical_shortcut(loaded.hold_key.as_deref().unwrap()).unwrap(),
            "RightCtrl"
        );
    }

    #[test]
    fn corrupt_file_yields_defaults_and_sibling() {
        let path = scratch_path("corrupt");
        fs::write(&path, "not json at all").unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert!(path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn invalid_utf8_file_yields_defaults_and_sibling() {
        let path = scratch_path("invalid-utf8");
        fs::write(&path, [0xff, 0xfe, b'{']).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert!(path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn resolve_prefers_env_then_file_then_default() {
        assert_eq!(resolve(Some(1), Some(2), 3), 1);
        assert_eq!(resolve(None, Some(2), 3), 2);
        assert_eq!(resolve::<i32>(None, None, 3), 3);
    }

    #[test]
    fn invalid_engine_yields_defaults_and_sibling() {
        let path = scratch_path("invalid-engine");
        fs::write(&path, r#"{"engine":"Whisper"}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
        assert!(!path.exists(), "invalid engine should be moved aside");
        assert!(path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn shortcut_parser_normalizes_aliases_and_order() {
        assert_eq!(
            Config::canonical_shortcut("alt + space + mod4").unwrap(),
            "Super+Alt+Space"
        );
        assert_eq!(
            Config::canonical_shortcut("control-a")
                .unwrap_err()
                .to_string(),
            "invalid shortcut control-a: unknown key control-a"
        );
        assert_eq!(Config::canonical_shortcut("control + a").unwrap(), "Ctrl+A");
        assert_eq!(
            Config::canonical_shortcut("RightCtrl").unwrap(),
            "RightCtrl"
        );
    }

    #[test]
    fn shortcut_parser_rejects_invalid_chords() {
        for invalid in ["", "Ctrl+Control+A", "A+B", "Ctrl+Alt", "Ctrl++A", "Thumb"] {
            assert!(
                Config::canonical_shortcut(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn toggle_shortcut_requires_a_modifier_and_terminal_key() {
        assert_eq!(
            Config::canonical_toggle_shortcut("alt + space + mod4").unwrap(),
            "Super+Alt+Space"
        );
        for invalid in ["A", "F8", "RightCtrl"] {
            assert!(
                Config::canonical_toggle_shortcut(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }
}
