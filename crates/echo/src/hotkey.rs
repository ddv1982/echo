use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    UnknownKey(String),
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKey(name) => write!(f, "unknown key {name}"),
        }
    }
}

impl std::error::Error for HotkeyError {}

pub fn parse_keyspec(spec: &str) -> Result<KeySpec, HotkeyError> {
    let keys = spec
        .split('+')
        .map(canonicalize)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(KeySpec { keys })
}

fn canonicalize(name: &str) -> Result<String, HotkeyError> {
    let folded = name.trim().to_ascii_lowercase().replace(['_', '-'], "");
    let canon = match folded.as_str() {
        "rightctrl" | "rctrl" | "controlr" => "RightCtrl",
        "leftctrl" | "lctrl" | "controll" => "LeftCtrl",
        "ctrl" | "control" => "Ctrl",
        "rightshift" | "rshift" => "RightShift",
        "leftshift" | "lshift" | "shift" => "Shift",
        "super" | "meta" | "win" | "mod4" => "Super",
        "alt" | "mod1" => "Alt",
        "space" => "Space",
        other => return Err(HotkeyError::UnknownKey(other.to_string())),
    };
    Ok(canon.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeySource {
    Cli,
    Evdev { devices: Vec<PathBuf> },
}

impl HotkeySource {
    #[must_use]
    pub fn detect() -> Self {
        match readable_event_nodes() {
            Ok(devices) if !devices.is_empty() => Self::Evdev { devices },
            _ => Self::Cli,
        }
    }
}

pub fn readable_event_nodes() -> Result<Vec<PathBuf>, io::Error> {
    let dir = Path::new("/dev/input");
    let mut nodes = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.starts_with("event") {
            continue;
        }
        if fs::File::open(&path).is_ok() {
            nodes.push(path);
        }
    }
    Ok(nodes)
}

pub fn evdev_permission_hint() -> String {
    if Path::new("/dev/input").exists() {
        "evdev is not readable. add the user to the input group: sudo usermod -aG input $USER"
            .to_string()
    } else {
        "evdev is unavailable (/dev/input is missing). use echo rec --once or bind a compositor key to that command".to_string()
    }
}

/// Blocking evdev read. Returns the first Down/Up for the hold key.
pub fn next_evdev_event(devices: &[PathBuf], hold: &KeySpec) -> io::Result<HotkeyEvent> {
    let mut files: Vec<fs::File> = devices
        .iter()
        .map(fs::File::open)
        .collect::<io::Result<_>>()?;
    let codes = evdev_codes(hold);
    let mut buf = [0u8; 24];
    loop {
        for file in &mut files {
            match file.read(&mut buf) {
                Ok(24) => {
                    let ev_type = u16::from_le_bytes([buf[16], buf[17]]);
                    let code = u16::from_le_bytes([buf[18], buf[19]]);
                    let value = i32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]);
                    if ev_type != 1 || !codes.contains(&code) {
                        continue;
                    }
                    if value == 1 {
                        return Ok(HotkeyEvent::Down);
                    }
                    if value == 0 {
                        return Ok(HotkeyEvent::Up);
                    }
                }
                Ok(_) => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) => return Err(err),
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn evdev_codes(spec: &KeySpec) -> Vec<u16> {
    spec.keys
        .iter()
        .filter_map(|key| match key.as_str() {
            "RightCtrl" => Some(97),
            "LeftCtrl" | "Ctrl" => Some(29),
            "RightShift" => Some(54),
            "Shift" => Some(42),
            "Super" => Some(125),
            "Alt" => Some(56),
            "Space" => Some(57),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_right_ctrl() {
        let spec = parse_keyspec("RightCtrl").unwrap();
        assert_eq!(spec.keys, ["RightCtrl"]);
    }

    #[test]
    fn parses_super_alt_space() {
        let spec = parse_keyspec("Super+Alt+Space").unwrap();
        assert_eq!(spec.keys, ["Super", "Alt", "Space"]);
    }

    #[test]
    fn rejects_unknown_names() {
        assert!(matches!(
            parse_keyspec("Hyper+Thumb"),
            Err(HotkeyError::UnknownKey(_))
        ));
    }

    #[test]
    fn detect_without_evdev_is_cli() {
        if Path::new("/dev/input").exists()
            && !readable_event_nodes().unwrap_or_default().is_empty()
        {
            assert!(matches!(HotkeySource::detect(), HotkeySource::Evdev { .. }));
        } else {
            assert_eq!(HotkeySource::detect(), HotkeySource::Cli);
            assert!(!evdev_permission_hint().is_empty());
        }
    }
}
