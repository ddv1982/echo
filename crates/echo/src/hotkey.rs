use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::audio::CancellationToken;

const O_NONBLOCK: i32 = 0o4000;
const EV_KEY: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

/// A canonical hold key: display name plus its evdev key code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldKeySpec {
    pub name: String,
    pub code: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    UnknownKey(String),
    MultiKey(String),
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKey(name) => write!(f, "unknown key {name}"),
            Self::MultiKey(spec) => write!(
                f,
                "hold key must be a single key, got {spec}; bind combos to a desktop shortcut running rec --toggle"
            ),
        }
    }
}

impl std::error::Error for HotkeyError {}

/// Parse a single hold key name. Hold-to-talk watches one key; the edge
/// matcher has no chord state, so multi-key specs are rejected rather than
/// silently OR-matched.
pub fn parse_hold_key(spec: &str) -> Result<HoldKeySpec, HotkeyError> {
    if spec.contains('+') {
        return Err(HotkeyError::MultiKey(spec.to_string()));
    }
    let folded = spec.trim().to_ascii_lowercase().replace(['_', '-'], "");
    let (name, code) = match folded.as_str() {
        "rightctrl" | "rctrl" | "controlr" => ("RightCtrl", 97),
        "leftctrl" | "lctrl" | "controll" | "ctrl" | "control" => ("LeftCtrl", 29),
        "rightshift" | "rshift" => ("RightShift", 54),
        "leftshift" | "lshift" | "shift" => ("LeftShift", 42),
        "super" | "meta" | "win" | "mod4" => ("Super", 125),
        "alt" | "mod1" => ("Alt", 56),
        "space" => ("Space", 57),
        other => return Err(HotkeyError::UnknownKey(other.to_string())),
    };
    Ok(HoldKeySpec {
        name: name.to_string(),
        code,
    })
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

fn resolved_hold_key(
    env: Option<&str>,
    file: &echo_core::Config,
) -> Result<HoldKeySpec, HotkeyError> {
    let name = echo_core::resolve(
        env.map(str::to_string),
        file.hold_key.clone(),
        "RightCtrl".to_string(),
    );
    parse_hold_key(&name)
}

/// Hold key from `ECHO_HOLD_KEY`, the config file, or Right Ctrl.
pub fn hold_key() -> Result<HoldKeySpec, HotkeyError> {
    resolved_hold_key(
        std::env::var("ECHO_HOLD_KEY").ok().as_deref(),
        &crate::settings::file_config(),
    )
}

/// Decode one 24-byte evdev `input_event` (64-bit Linux layout) into
/// (type, code, value).
#[must_use]
pub fn decode_input_event(buf: &[u8; 24]) -> (u16, u16, i32) {
    let ev_type = u16::from_le_bytes([buf[16], buf[17]]);
    let code = u16::from_le_bytes([buf[18], buf[19]]);
    let value = i32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]);
    (ev_type, code, value)
}

/// Map a decoded event to a hold-key edge. Value 1 is press and 0 release;
/// 2 (autorepeat) and non-key events return None.
#[must_use]
pub fn key_edge(hold_code: u16, event: (u16, u16, i32)) -> Option<HotkeyEvent> {
    let (ev_type, code, value) = event;
    if ev_type != EV_KEY || code != hold_code {
        return None;
    }
    match value {
        1 => Some(HotkeyEvent::Down),
        0 => Some(HotkeyEvent::Up),
        _ => None,
    }
}

/// Nonblocking evdev reader for one hold key across every readable device.
pub struct HoldKey {
    files: Vec<fs::File>,
    code: u16,
}

impl HoldKey {
    pub fn open(devices: &[PathBuf], code: u16) -> io::Result<Self> {
        let files = devices
            .iter()
            .map(|path| {
                fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(O_NONBLOCK)
                    .open(path)
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self { files, code })
    }

    /// Poll every device until the wanted edge arrives or `cancel` fires.
    /// Returns false when cancelled first.
    pub fn wait(&mut self, want: HotkeyEvent, cancel: &CancellationToken) -> io::Result<bool> {
        let mut buf = [0u8; 24];
        loop {
            if cancel.is_cancelled() {
                return Ok(false);
            }
            for file in &mut self.files {
                loop {
                    match file.read(&mut buf) {
                        Ok(24) => {
                            if key_edge(self.code, decode_input_event(&buf)) == Some(want) {
                                return Ok(true);
                            }
                        }
                        Ok(_) => break,
                        Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                        Err(err) => return Err(err),
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// What the listener should do after a hold-key edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldAction {
    Start,
    Stop,
}

/// Decides hold-key edges into session actions. Starts only when no session
/// is active anywhere (the toggle lock serializes processes; a key-down must
/// never truncate someone else's recording), and stops only sessions it
/// started, so a stray key-up is a no-op.
#[derive(Debug, Default)]
pub struct HoldDriver {
    started_by_us: bool,
}

impl HoldDriver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn on_edge(&mut self, edge: HotkeyEvent, session_active: bool) -> Option<HoldAction> {
        match edge {
            HotkeyEvent::Down if !self.started_by_us && !session_active => {
                self.started_by_us = true;
                Some(HoldAction::Start)
            }
            HotkeyEvent::Up if self.started_by_us => {
                self.started_by_us = false;
                Some(HoldAction::Stop)
            }
            _ => None,
        }
    }
}

/// The hold-key listener loop: wait for down, report, wait for up, report,
/// until cancelled. Events for the other edge arriving mid-wait are consumed
/// and ignored, which is what keeps autorepeat harmless.
pub fn run_hold_listener(
    hold: &mut HoldKey,
    cancel: &CancellationToken,
    mut on_edge: impl FnMut(HotkeyEvent),
) -> io::Result<()> {
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if !hold.wait(HotkeyEvent::Down, cancel)? {
            return Ok(());
        }
        on_edge(HotkeyEvent::Down);
        if !hold.wait(HotkeyEvent::Up, cancel)? {
            return Ok(());
        }
        on_edge(HotkeyEvent::Up);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_right_ctrl_with_its_evdev_code() {
        let spec = parse_hold_key("RightCtrl").unwrap();
        assert_eq!(spec.name, "RightCtrl");
        assert_eq!(spec.code, 97);
    }

    #[test]
    fn hold_key_prefers_env_then_file_then_right_ctrl() {
        let file = echo_core::Config {
            hold_key: Some("LeftCtrl".into()),
            ..echo_core::Config::default()
        };
        assert_eq!(
            resolved_hold_key(Some("Space"), &file).unwrap().name,
            "Space"
        );
        assert_eq!(resolved_hold_key(None, &file).unwrap().name, "LeftCtrl");
        assert_eq!(
            resolved_hold_key(None, &echo_core::Config::default())
                .unwrap()
                .name,
            "RightCtrl"
        );
    }

    #[test]
    fn rejects_multi_key_specs() {
        assert_eq!(
            parse_hold_key("Super+Alt+Space"),
            Err(HotkeyError::MultiKey("Super+Alt+Space".to_string()))
        );
    }

    #[test]
    fn rejects_unknown_names() {
        assert!(matches!(
            parse_hold_key("Thumb"),
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

    fn event_bytes(ev_type: u16, code: u16, value: i32) -> [u8; 24] {
        let mut buf = [0u8; 24];
        buf[16..18].copy_from_slice(&ev_type.to_le_bytes());
        buf[18..20].copy_from_slice(&code.to_le_bytes());
        buf[20..24].copy_from_slice(&value.to_le_bytes());
        buf
    }

    #[test]
    fn decodes_input_event_fields() {
        let buf = event_bytes(EV_KEY, 97, 1);
        assert_eq!(decode_input_event(&buf), (EV_KEY, 97, 1));
        let buf = event_bytes(0, 0, 0);
        assert_eq!(decode_input_event(&buf), (0, 0, 0));
    }

    #[test]
    fn key_edges_only_for_matching_key_events() {
        assert_eq!(key_edge(97, (EV_KEY, 97, 1)), Some(HotkeyEvent::Down));
        assert_eq!(key_edge(97, (EV_KEY, 97, 0)), Some(HotkeyEvent::Up));
        // Autorepeat is not an edge.
        assert_eq!(key_edge(97, (EV_KEY, 97, 2)), None);
        // Other keys and non-key events are ignored.
        assert_eq!(key_edge(97, (EV_KEY, 30, 1)), None);
        assert_eq!(key_edge(97, (2, 97, 1)), None);
    }

    #[test]
    fn hold_key_finds_edge_in_event_stream() {
        let dir = std::env::temp_dir().join(format!("echo-holdkey-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("events");
        let mut raw = Vec::new();
        raw.extend_from_slice(&event_bytes(2, 0, 5)); // relative motion, ignored
        raw.extend_from_slice(&event_bytes(EV_KEY, 97, 2)); // autorepeat, ignored
        raw.extend_from_slice(&event_bytes(EV_KEY, 97, 1)); // the edge
        fs::write(&path, raw).unwrap();

        let spec = parse_hold_key("RightCtrl").unwrap();
        let mut hold = HoldKey::open(&[path], spec.code).unwrap();
        let cancel = CancellationToken::new();
        assert!(hold.wait(HotkeyEvent::Down, &cancel).unwrap());
    }

    #[test]
    fn hold_key_wait_stops_on_cancel() {
        let dir = std::env::temp_dir().join(format!("echo-holdkey-cancel-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("events");
        fs::write(&path, event_bytes(EV_KEY, 97, 1)).unwrap();

        let spec = parse_hold_key("RightCtrl").unwrap();
        let mut hold = HoldKey::open(&[path], spec.code).unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        // Wants Up but the stream only has Down; only the cancel ends the wait.
        assert!(!hold.wait(HotkeyEvent::Up, &cancel).unwrap());
    }

    #[test]
    fn driver_starts_only_when_free_and_stops_only_its_own() {
        let mut driver = HoldDriver::new();
        assert_eq!(
            driver.on_edge(HotkeyEvent::Down, false),
            Some(HoldAction::Start)
        );
        // A second down while our session runs does nothing.
        assert_eq!(driver.on_edge(HotkeyEvent::Down, true), None);
        assert_eq!(
            driver.on_edge(HotkeyEvent::Up, true),
            Some(HoldAction::Stop)
        );
        // A stray up starts nothing and stops nothing.
        assert_eq!(driver.on_edge(HotkeyEvent::Up, false), None);
        // A down while another process holds the lock is ignored.
        assert_eq!(driver.on_edge(HotkeyEvent::Down, true), None);
        assert_eq!(driver.on_edge(HotkeyEvent::Up, true), None);
    }

    #[test]
    fn listener_reports_edges_from_a_fixture_stream() {
        let dir = std::env::temp_dir().join(format!("echo-holdlisten-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("events");
        let mut raw = Vec::new();
        raw.extend_from_slice(&event_bytes(EV_KEY, 97, 1)); // down
        raw.extend_from_slice(&event_bytes(EV_KEY, 97, 0)); // up
        fs::write(&path, raw).unwrap();

        let spec = parse_hold_key("RightCtrl").unwrap();
        let mut hold = HoldKey::open(&[path], spec.code).unwrap();
        let cancel = CancellationToken::new();
        let listener_cancel = cancel.clone();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_writer = seen.clone();
        let thread = std::thread::spawn(move || {
            let _ = run_hold_listener(&mut hold, &listener_cancel, move |edge| {
                seen_writer.lock().expect("seen lock").push(edge);
            });
        });
        std::thread::sleep(Duration::from_millis(100));
        cancel.cancel();
        let _ = thread.join();
        assert_eq!(
            *seen.lock().expect("seen lock"),
            vec![HotkeyEvent::Down, HotkeyEvent::Up]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
