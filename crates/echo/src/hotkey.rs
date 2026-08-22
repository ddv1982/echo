use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::audio::CancellationToken;
use evdev::{Device, KeyCode};

const O_NONBLOCK: i32 = 0o4000;
const EV_KEY: u16 = 1;
#[cfg(test)]
type DecodedInputEvent = (u16, u16, i32);
#[cfg(test)]
type ScriptedEventBatch = Result<Vec<DecodedInputEvent>, io::ErrorKind>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

/// Desktop session capability used by the GUI's native shortcut runtime.
/// Detection deliberately consults only `XDG_SESSION_TYPE`: a stale DISPLAY
/// variable in a Wayland session is not evidence that X11 grabs are usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSession {
    Wayland,
    X11,
    Unknown,
}

impl DesktopSession {
    #[must_use]
    pub fn from_xdg_session_type(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("wayland") => Self::Wayland,
            Some("x11") => Self::X11,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBackend {
    Portal,
    X11,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDecision {
    pub backend: NativeBackend,
    pub reason: Option<String>,
}

/// Select a backend from observed capabilities. `portal_version` must come
/// from the live GlobalShortcuts proxy, never from a desktop name/version.
#[must_use]
pub fn select_native_backend(
    session: DesktopSession,
    portal_version: Option<u32>,
) -> BackendDecision {
    match session {
        DesktopSession::Wayland if portal_version.unwrap_or(0) >= 1 => BackendDecision {
            backend: NativeBackend::Portal,
            reason: None,
        },
        DesktopSession::Wayland => BackendDecision {
            backend: NativeBackend::Unsupported,
            reason: Some(
                "Wayland session has no org.freedesktop.portal.GlobalShortcuts interface"
                    .to_string(),
            ),
        },
        DesktopSession::X11 => BackendDecision {
            backend: NativeBackend::X11,
            reason: None,
        },
        DesktopSession::Unknown => BackendDecision {
            backend: NativeBackend::Unsupported,
            reason: Some("unknown or headless desktop session".to_string()),
        },
    }
}

/// Debounces a toggle shortcut into one action per physical press.
#[derive(Debug)]
pub struct ToggleDriver {
    down: bool,
    live: bool,
}

impl Default for ToggleDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ToggleDriver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            down: false,
            live: true,
        }
    }

    #[must_use]
    pub fn on_edge(&mut self, edge: HotkeyEvent) -> bool {
        if !self.live {
            return false;
        }
        match edge {
            HotkeyEvent::Down if !self.down => {
                self.down = true;
                true
            }
            HotkeyEvent::Up => {
                self.down = false;
                false
            }
            HotkeyEvent::Down => false,
        }
    }

    /// Permanently disarm the driver when its listener/session terminates.
    pub fn terminate(&mut self) {
        self.live = false;
        self.down = false;
    }
}

/// A canonical hold chord. For compatibility, `code` remains the literal
/// evdev code for single keys; chord values pack the modifier mask above the
/// terminal key code and are decoded by `HoldKey::open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldKeySpec {
    pub name: String,
    pub code: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    UnknownKey(String),
    InvalidChord(String),
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKey(name) => write!(f, "unknown key {name}"),
            Self::InvalidChord(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for HotkeyError {}

/// Parse the same canonical chord syntax used by the toggle setting and pack
/// its modifier requirements for the evdev listener.
pub fn parse_hold_key(spec: &str) -> Result<HoldKeySpec, HotkeyError> {
    let name = echo_core::Config::canonical_shortcut(spec)
        .map_err(|err| HotkeyError::InvalidChord(err.to_string()))?;
    let mut mask = 0u8;
    let mut key = None;
    for part in name.split('+') {
        match part {
            "Super" if name.contains('+') => mask |= MOD_SUPER,
            "Ctrl" if name.contains('+') => mask |= MOD_CTRL,
            "Alt" if name.contains('+') => mask |= MOD_ALT,
            "Shift" if name.contains('+') => mask |= MOD_SHIFT,
            terminal => key = Some(terminal),
        }
    }
    let terminal = key.ok_or_else(|| HotkeyError::InvalidChord(format!("invalid chord {name}")))?;
    let code = key_code(terminal).ok_or_else(|| HotkeyError::UnknownKey(terminal.to_string()))?;
    let packed = code | (u16::from(mask) << 8);
    Ok(HoldKeySpec { name, code: packed })
}

const MOD_SUPER: u8 = 1;
const MOD_CTRL: u8 = 2;
const MOD_ALT: u8 = 4;
const MOD_SHIFT: u8 = 8;

fn key_code(name: &str) -> Option<u16> {
    Some(match name {
        "Escape" => 1,
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "Minus" => 12,
        "Equal" => 13,
        "Backspace" => 14,
        "Tab" => 15,
        "Q" => 16,
        "W" => 17,
        "E" => 18,
        "R" => 19,
        "T" => 20,
        "Y" => 21,
        "U" => 22,
        "I" => 23,
        "O" => 24,
        "P" => 25,
        "BracketLeft" => 26,
        "BracketRight" => 27,
        "Enter" => 28,
        "LeftCtrl" => 29,
        "A" => 30,
        "S" => 31,
        "D" => 32,
        "F" => 33,
        "G" => 34,
        "H" => 35,
        "J" => 36,
        "K" => 37,
        "L" => 38,
        "Semicolon" => 39,
        "Quote" => 40,
        "Backquote" => 41,
        "LeftShift" => 42,
        "Backslash" => 43,
        "Z" => 44,
        "X" => 45,
        "C" => 46,
        "V" => 47,
        "B" => 48,
        "N" => 49,
        "M" => 50,
        "Comma" => 51,
        "Period" => 52,
        "Slash" => 53,
        "RightShift" => 54,
        "Alt" => 56,
        "Space" => 57,
        "CapsLock" => 58,
        "F1" => 59,
        "F2" => 60,
        "F3" => 61,
        "F4" => 62,
        "F5" => 63,
        "F6" => 64,
        "F7" => 65,
        "F8" => 66,
        "F9" => 67,
        "F10" => 68,
        "F11" => 87,
        "F12" => 88,
        "RightCtrl" => 97,
        "RightAlt" => 100,
        "Home" => 102,
        "ArrowUp" => 103,
        "PageUp" => 104,
        "ArrowLeft" => 105,
        "ArrowRight" => 106,
        "End" => 107,
        "ArrowDown" => 108,
        "PageDown" => 109,
        "Insert" => 110,
        "Delete" => 111,
        "Super" => 125,
        "RightSuper" => 126,
        "Menu" => 127,
        _ => return None,
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
        let code = hold_key().map(|spec| spec.code).unwrap_or(97);
        match probe_hold_devices(code) {
            EvdevAvailability::Ready(devices) => Self::Evdev { devices },
            _ => Self::Cli,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvdevAvailability {
    Ready(Vec<PathBuf>),
    NeedsPermission(String),
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvdevListenerHealth {
    Active,
    NeedsPermission(String),
    Unavailable(String),
    Degraded(String),
}

pub fn readable_event_nodes() -> Result<Vec<PathBuf>, io::Error> {
    let code = hold_key().map(|spec| spec.code).unwrap_or(97);
    match probe_hold_devices(code) {
        EvdevAvailability::Ready(devices) => Ok(devices),
        EvdevAvailability::NeedsPermission(_) | EvdevAvailability::Unavailable(_) => Ok(Vec::new()),
    }
}

pub fn evdev_permission_hint() -> String {
    if Path::new("/dev/input").exists() {
        "raw keyboard input is not available to Echo; use the desktop global shortcut or ask your system administrator to configure access".to_string()
    } else {
        "evdev is unavailable (/dev/input is missing). use echo rec --once or bind a compositor key to that command".to_string()
    }
}

pub fn probe_hold_devices(code: u16) -> EvdevAvailability {
    let entries = match fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            return EvdevAvailability::NeedsPermission(
                "Echo cannot inspect raw keyboard devices; use the desktop global shortcut."
                    .to_string(),
            );
        }
        Err(err) => {
            return EvdevAvailability::Unavailable(format!(
                "Raw keyboard input is unavailable: {err}."
            ));
        }
    };
    let mut devices = Vec::new();
    let mut permission_denied = false;
    let mut saw_event_node = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("event"))
        {
            continue;
        }
        saw_event_node = true;
        match Device::open(&path) {
            Ok(device) if device_supports_hold(&device, code) => devices.push(path),
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                permission_denied = true;
            }
            Err(_) => {}
        }
    }
    devices.sort();
    classify_evdev_probe(devices, permission_denied, saw_event_node)
}

fn classify_evdev_probe(
    devices: Vec<PathBuf>,
    permission_denied: bool,
    saw_event_node: bool,
) -> EvdevAvailability {
    if !devices.is_empty() {
        EvdevAvailability::Ready(devices)
    } else if permission_denied {
        EvdevAvailability::NeedsPermission(
            "Echo cannot read an eligible keyboard; use the desktop global shortcut.".to_string(),
        )
    } else if saw_event_node {
        EvdevAvailability::Unavailable(
            "No readable keyboard supports the configured push-to-talk shortcut.".to_string(),
        )
    } else {
        EvdevAvailability::Unavailable("No raw keyboard devices were found.".to_string())
    }
}

fn device_supports_hold(device: &Device, code: u16) -> bool {
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    key_codes_support_hold(code, |key| keys.contains(KeyCode::new(key)))
}

fn key_codes_support_hold(code: u16, contains: impl Fn(u16) -> bool) -> bool {
    let keyboard = [30, 44, 28, 57].into_iter().all(&contains);
    if !keyboard || !contains(code & 0xff) {
        return false;
    }
    let modifiers = (code >> 8) as u8;
    (!has_modifier(modifiers, MOD_SUPER) || contains(125) || contains(126))
        && (!has_modifier(modifiers, MOD_CTRL) || contains(29) || contains(97))
        && (!has_modifier(modifiers, MOD_ALT) || contains(56) || contains(100))
        && (!has_modifier(modifiers, MOD_SHIFT) || contains(42) || contains(54))
}

fn has_modifier(modifiers: u8, modifier: u8) -> bool {
    modifiers & modifier != 0
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

struct HoldDevice {
    path: PathBuf,
    reader: HoldReader,
    held_modifiers: u8,
    chord_down: bool,
}

enum HoldReader {
    Evdev(Box<Device>),
    RawFixture(fs::File),
    #[cfg(test)]
    Scripted(VecDeque<ScriptedEventBatch>),
}

/// Nonblocking evdev reader for one hold key across eligible keyboards.
pub struct HoldKey {
    devices: Vec<HoldDevice>,
    code: u16,
    required_modifiers: u8,
    active_device: Option<PathBuf>,
    pending_edges: VecDeque<HotkeyEvent>,
}

impl HoldKey {
    pub fn open(devices: &[PathBuf], code: u16) -> io::Result<Self> {
        let mut hold = Self {
            devices: Vec::new(),
            code: code & 0xff,
            required_modifiers: (code >> 8) as u8,
            active_device: None,
            pending_edges: VecDeque::new(),
        };
        let (_, errors) = hold.sync_fixture_devices(devices);
        if hold.devices.is_empty() {
            return Err(errors.into_iter().next().unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "no eligible keyboard remained available",
                )
            }));
        }
        Ok(hold)
    }

    /// Poll every device until the wanted edge arrives or `cancel` fires.
    /// Returns false when cancelled first.
    pub fn wait(&mut self, want: HotkeyEvent, cancel: &CancellationToken) -> io::Result<bool> {
        loop {
            if cancel.is_cancelled() {
                return Ok(false);
            }
            if let Some(edge) = self.pending_edges.pop_front() {
                if edge == want {
                    return Ok(true);
                }
                continue;
            }
            let (edges, errors) = self.poll_edges();
            self.pending_edges.extend(edges);
            if self.devices.is_empty() {
                return Err(errors.into_iter().next().unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "all keyboard devices disconnected")
                }));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn sync_fixture_devices(&mut self, paths: &[PathBuf]) -> (Vec<HotkeyEvent>, Vec<io::Error>) {
        self.sync_devices(paths, |path| {
            fs::OpenOptions::new()
                .read(true)
                .custom_flags(O_NONBLOCK)
                .open(path)
                .map(HoldReader::RawFixture)
        })
    }

    fn sync_system_devices(&mut self, paths: &[PathBuf]) -> (Vec<HotkeyEvent>, Vec<io::Error>) {
        self.sync_devices(paths, |path| {
            let device = Device::open(path)?;
            device.set_nonblocking(true)?;
            Ok(HoldReader::Evdev(Box::new(device)))
        })
    }

    fn sync_devices(
        &mut self,
        paths: &[PathBuf],
        mut open: impl FnMut(&Path) -> io::Result<HoldReader>,
    ) -> (Vec<HotkeyEvent>, Vec<io::Error>) {
        self.devices.retain(|device| paths.contains(&device.path));
        let mut errors = Vec::new();
        for path in paths {
            if self.devices.iter().any(|device| device.path == *path) {
                continue;
            }
            match open(path) {
                Ok(reader) => self.devices.push(HoldDevice {
                    path: path.clone(),
                    reader,
                    held_modifiers: 0,
                    chord_down: false,
                }),
                Err(err) => errors.push(err),
            }
        }
        (Vec::new(), errors)
    }

    fn poll_edges(&mut self) -> (Vec<HotkeyEvent>, Vec<io::Error>) {
        let mut events = Vec::new();
        let mut failed = Vec::new();
        for device in &mut self.devices {
            match &mut device.reader {
                HoldReader::Evdev(reader) => match reader.fetch_events() {
                    Ok(batch) => {
                        events.extend(batch.take(64).map(|event| {
                            (
                                device.path.clone(),
                                (event.event_type().0, event.code(), event.value()),
                            )
                        }));
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                    Err(err) => failed.push((device.path.clone(), err)),
                },
                HoldReader::RawFixture(reader) => {
                    let mut buf = [0u8; 24];
                    for _ in 0..64 {
                        match reader.read(&mut buf) {
                            Ok(24) => {
                                events.push((device.path.clone(), decode_input_event(&buf)));
                            }
                            Ok(_) => break,
                            Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                            Err(err) => {
                                failed.push((device.path.clone(), err));
                                break;
                            }
                        }
                    }
                }
                #[cfg(test)]
                HoldReader::Scripted(batches) => match batches.pop_front() {
                    Some(Ok(batch)) => events.extend(
                        batch
                            .into_iter()
                            .take(64)
                            .map(|event| (device.path.clone(), event)),
                    ),
                    Some(Err(kind)) => failed.push((
                        device.path.clone(),
                        io::Error::new(kind, "scripted evdev read failure"),
                    )),
                    None => {}
                },
            }
        }
        let mut edges = Vec::new();
        let mut logically_active = self.active_device.is_some();
        let mut pending_up = None;
        for (path, event) in events {
            let edge = {
                let Some(device) = self.devices.iter_mut().find(|device| device.path == path)
                else {
                    continue;
                };
                chord_edge(
                    self.code,
                    self.required_modifiers,
                    &mut device.held_modifiers,
                    &mut device.chord_down,
                    event,
                )
            };
            if edge.is_none() {
                continue;
            }
            let any_held = self.devices.iter().any(|device| device.chord_down);
            match (logically_active, any_held) {
                (false, true) => {
                    if let Some(released_path) = pending_up.take() {
                        if released_path == path {
                            edges.extend([HotkeyEvent::Up, HotkeyEvent::Down]);
                        }
                    } else {
                        edges.push(HotkeyEvent::Down);
                    }
                    logically_active = true;
                }
                (true, false) => {
                    logically_active = false;
                    pending_up = Some(path);
                }
                _ => {}
            }
        }
        let mut errors = Vec::new();
        for (path, error) in failed {
            self.devices.retain(|device| device.path != path);
            errors.push(error);
        }
        let any_held = self.devices.iter().any(|device| device.chord_down);
        match (logically_active, any_held) {
            (false, true) => pending_up = None,
            (true, false) => pending_up = self.active_device.clone(),
            _ => {}
        }
        self.active_device = self
            .devices
            .iter()
            .find(|device| device.chord_down)
            .map(|device| device.path.clone());
        if pending_up.is_some() {
            edges.push(HotkeyEvent::Up);
        }
        (edges, errors)
    }

    fn release_active(&mut self) -> Option<HotkeyEvent> {
        self.active_device.take().map(|_| HotkeyEvent::Up)
    }
}

pub fn run_evdev_supervisor(
    code: u16,
    cancel: &CancellationToken,
    mut on_health: impl FnMut(EvdevListenerHealth),
    mut on_edge: impl FnMut(HotkeyEvent),
) {
    run_evdev_supervisor_loop(
        code,
        cancel,
        Duration::from_millis(500),
        || probe_hold_devices(code),
        |hold, paths| hold.sync_system_devices(paths),
        &mut on_health,
        &mut on_edge,
    );
}

fn run_evdev_supervisor_loop(
    code: u16,
    cancel: &CancellationToken,
    scan_interval: Duration,
    mut probe: impl FnMut() -> EvdevAvailability,
    mut sync: impl FnMut(&mut HoldKey, &[PathBuf]) -> (Vec<HotkeyEvent>, Vec<io::Error>),
    mut on_health: impl FnMut(EvdevListenerHealth),
    mut on_edge: impl FnMut(HotkeyEvent),
) {
    let mut hold = HoldKey {
        devices: Vec::new(),
        code: code & 0xff,
        required_modifiers: (code >> 8) as u8,
        active_device: None,
        pending_edges: VecDeque::new(),
    };
    let mut last_scan = Instant::now() - Duration::from_secs(1);
    let mut last_health = None;
    while !cancel.is_cancelled() {
        if last_scan.elapsed() >= scan_interval {
            let availability = probe();
            let health = match availability {
                EvdevAvailability::Ready(paths) => {
                    let (edges, errors) = sync(&mut hold, &paths);
                    edges.into_iter().for_each(&mut on_edge);
                    if hold.devices.is_empty() {
                        EvdevListenerHealth::Degraded(errors.first().map_or_else(
                            || "Keyboard devices changed while Echo was opening them; reconnecting."
                                .to_string(),
                            |err| format!(
                                "Cannot open an eligible keyboard: {err}; reconnecting."
                            ),
                        ))
                    } else {
                        EvdevListenerHealth::Active
                    }
                }
                EvdevAvailability::NeedsPermission(detail) => {
                    let (edges, _) = sync(&mut hold, &[]);
                    edges.into_iter().for_each(&mut on_edge);
                    EvdevListenerHealth::NeedsPermission(detail)
                }
                EvdevAvailability::Unavailable(detail) => {
                    let (edges, _) = sync(&mut hold, &[]);
                    edges.into_iter().for_each(&mut on_edge);
                    EvdevListenerHealth::Unavailable(detail)
                }
            };
            if last_health.as_ref() != Some(&health) {
                on_health(health.clone());
                last_health = Some(health);
            }
            last_scan = Instant::now();
        }
        let (edges, errors) = hold.poll_edges();
        edges.into_iter().for_each(&mut on_edge);
        if let Some(error) = errors.first() {
            let health = EvdevListenerHealth::Degraded(format!(
                "A keyboard disconnected or stopped responding: {error}; reconnecting."
            ));
            if last_health.as_ref() != Some(&health) {
                on_health(health.clone());
                last_health = Some(health);
            }
            last_scan = Instant::now() - Duration::from_secs(1);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(edge) = hold.release_active() {
        on_edge(edge);
    }
}

fn chord_edge(
    terminal_code: u16,
    required_modifiers: u8,
    held_modifiers: &mut u8,
    chord_down: &mut bool,
    event: (u16, u16, i32),
) -> Option<HotkeyEvent> {
    let (ev_type, code, value) = event;
    if ev_type != EV_KEY || value == 2 {
        return None;
    }
    let modifier = modifier_mask(code);
    if value == 1 {
        *held_modifiers |= modifier;
        if code == terminal_code
            && !*chord_down
            && *held_modifiers & required_modifiers == required_modifiers
        {
            *chord_down = true;
            return Some(HotkeyEvent::Down);
        }
    } else if value == 0 {
        let ends_chord =
            *chord_down && (code == terminal_code || modifier & required_modifiers != 0);
        *held_modifiers &= !modifier;
        if ends_chord {
            *chord_down = false;
            return Some(HotkeyEvent::Up);
        }
    }
    None
}

fn modifier_mask(code: u16) -> u8 {
    match code {
        125 | 126 => MOD_SUPER,
        29 | 97 => MOD_CTRL,
        56 | 100 => MOD_ALT,
        42 | 54 => MOD_SHIFT,
        _ => 0,
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
    fn parses_and_normalizes_chords() {
        let spec = parse_hold_key("alt+space+meta").unwrap();
        assert_eq!(spec.name, "Super+Alt+Space");
        assert_eq!(spec.code & 0xff, 57);
        assert_eq!((spec.code >> 8) as u8, MOD_SUPER | MOD_ALT);
    }

    #[test]
    fn rejects_unknown_names() {
        assert!(matches!(
            parse_hold_key("Thumb"),
            Err(HotkeyError::InvalidChord(_))
        ));
    }

    #[test]
    fn chord_edges_require_modifiers_and_ignore_repeat() {
        let spec = parse_hold_key("Ctrl+Shift+A").unwrap();
        let terminal_code = spec.code & 0xff;
        let required_modifiers = (spec.code >> 8) as u8;
        let mut held_modifiers = 0;
        let mut chord_down = false;
        let mut edge = |event| {
            chord_edge(
                terminal_code,
                required_modifiers,
                &mut held_modifiers,
                &mut chord_down,
                event,
            )
        };
        assert_eq!(edge((EV_KEY, 30, 1)), None);
        assert_eq!(edge((EV_KEY, 29, 1)), None);
        assert_eq!(edge((EV_KEY, 42, 1)), None);
        assert_eq!(edge((EV_KEY, 30, 1)), Some(HotkeyEvent::Down));
        assert_eq!(edge((EV_KEY, 30, 1)), None);
        assert_eq!(edge((EV_KEY, 30, 2)), None);
        assert_eq!(edge((EV_KEY, 42, 0)), Some(HotkeyEvent::Up));
        assert_eq!(edge((EV_KEY, 42, 1)), None);
        assert_eq!(edge((EV_KEY, 30, 1)), Some(HotkeyEvent::Down));
        assert_eq!(edge((EV_KEY, 30, 0)), Some(HotkeyEvent::Up));
    }

    #[test]
    fn keyboard_capabilities_must_include_the_whole_chord() {
        let spec = parse_hold_key("Ctrl+Shift+A").unwrap();
        let keyboard = [28, 29, 30, 42, 44, 57];
        assert!(key_codes_support_hold(spec.code, |key| keyboard.contains(&key)));
        let missing_shift = [28, 29, 30, 44, 57];
        assert!(!key_codes_support_hold(spec.code, |key| missing_shift.contains(&key)));
        let mouse = [272, 273, 274];
        assert!(!key_codes_support_hold(spec.code, |key| mouse.contains(&key)));
    }

    #[test]
    fn evdev_probe_distinguishes_permission_and_device_failures() {
        let keyboard = PathBuf::from("/dev/input/event4");
        assert_eq!(
            classify_evdev_probe(vec![keyboard.clone()], true, true),
            EvdevAvailability::Ready(vec![keyboard])
        );
        assert!(matches!(
            classify_evdev_probe(Vec::new(), true, true),
            EvdevAvailability::NeedsPermission(_)
        ));
        assert!(matches!(
            classify_evdev_probe(Vec::new(), false, true),
            EvdevAvailability::Unavailable(detail) if detail.contains("No readable keyboard")
        ));
        assert!(matches!(
            classify_evdev_probe(Vec::new(), false, false),
            EvdevAvailability::Unavailable(detail) if detail.contains("No raw keyboard")
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
    fn device_states_do_not_combine_and_removal_releases_the_active_chord() {
        let dir = std::env::temp_dir().join(format!("echo-holdkey-devices-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let first = dir.join("first");
        let second = dir.join("second");
        fs::write(&first, event_bytes(EV_KEY, 29, 1)).unwrap();
        let mut second_events = Vec::new();
        second_events.extend_from_slice(&event_bytes(EV_KEY, 42, 1));
        second_events.extend_from_slice(&event_bytes(EV_KEY, 30, 1));
        fs::write(&second, second_events).unwrap();

        let spec = parse_hold_key("Ctrl+Shift+A").unwrap();
        let mut hold = HoldKey::open(&[first.clone(), second.clone()], spec.code).unwrap();
        assert!(hold.poll_edges().0.is_empty());

        let complete = dir.join("complete");
        let mut complete_events = Vec::new();
        complete_events.extend_from_slice(&event_bytes(EV_KEY, 29, 1));
        complete_events.extend_from_slice(&event_bytes(EV_KEY, 42, 1));
        complete_events.extend_from_slice(&event_bytes(EV_KEY, 30, 1));
        fs::write(&complete, complete_events).unwrap();
        assert!(hold
            .sync_fixture_devices(std::slice::from_ref(&complete))
            .1
            .is_empty());
        assert_eq!(hold.poll_edges().0, vec![HotkeyEvent::Down]);
        let missing = dir.join("disappeared");
        let (edges, errors) = hold.sync_fixture_devices(&[missing]);
        assert!(edges.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(hold.poll_edges().0, vec![HotkeyEvent::Up]);

        let mut rearmed_events = Vec::new();
        rearmed_events.extend_from_slice(&event_bytes(EV_KEY, 29, 1));
        rearmed_events.extend_from_slice(&event_bytes(EV_KEY, 42, 1));
        rearmed_events.extend_from_slice(&event_bytes(EV_KEY, 30, 1));
        rearmed_events.extend_from_slice(&event_bytes(EV_KEY, 30, 0));
        fs::write(&complete, rearmed_events).unwrap();
        assert!(hold.sync_fixture_devices(&[complete]).1.is_empty());
        assert_eq!(
            hold.poll_edges().0,
            vec![HotkeyEvent::Down, HotkeyEvent::Up]
        );
    }

    #[test]
    fn supervisor_teardown_releases_an_active_hold_once() {
        let dir = std::env::temp_dir().join(format!("echo-holdkey-release-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("events");
        fs::write(&path, event_bytes(EV_KEY, 97, 1)).unwrap();
        let mut hold = HoldKey::open(&[path], parse_hold_key("RightCtrl").unwrap().code).unwrap();
        assert_eq!(hold.poll_edges().0, vec![HotkeyEvent::Down]);
        assert_eq!(hold.release_active(), Some(HotkeyEvent::Up));
        assert_eq!(hold.release_active(), None);
    }

    #[test]
    fn supervisor_reconnects_after_device_removal() {
        let dir =
            std::env::temp_dir().join(format!("echo-supervisor-hotplug-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let first = dir.join("first");
        let second = dir.join("second");
        fs::write(&first, event_bytes(EV_KEY, 97, 1)).unwrap();
        let mut second_events = Vec::new();
        second_events.extend_from_slice(&event_bytes(EV_KEY, 97, 1));
        second_events.extend_from_slice(&event_bytes(EV_KEY, 97, 0));
        fs::write(&second, second_events).unwrap();

        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let mut probes = 0;
        let seen = std::cell::RefCell::new(Vec::new());
        run_evdev_supervisor_loop(
            97,
            &cancel,
            Duration::ZERO,
            || {
                probes += 1;
                match probes {
                    1 => EvdevAvailability::Ready(vec![first.clone()]),
                    2 => EvdevAvailability::Unavailable("keyboard removed".to_string()),
                    _ => EvdevAvailability::Ready(vec![second.clone()]),
                }
            },
            HoldKey::sync_fixture_devices,
            |_| {},
            |edge| {
                let mut events = seen.borrow_mut();
                events.push(edge);
                if events.len() == 4 {
                    stop.cancel();
                }
            },
        );
        assert_eq!(
            *seen.borrow(),
            vec![
                HotkeyEvent::Down,
                HotkeyEvent::Up,
                HotkeyEvent::Down,
                HotkeyEvent::Up
            ]
        );
    }

    #[test]
    fn supervisor_read_failure_releases_the_active_hold_and_degrades() {
        let path = PathBuf::from("/dev/input/scripted");
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let seen = std::cell::RefCell::new(Vec::new());
        let health = std::cell::RefCell::new(Vec::new());
        run_evdev_supervisor_loop(
            97,
            &cancel,
            Duration::from_secs(1),
            || EvdevAvailability::Ready(vec![path.clone()]),
            |hold, paths| {
                if hold.devices.is_empty() && !paths.is_empty() {
                    hold.devices.push(HoldDevice {
                        path: path.clone(),
                        reader: HoldReader::Scripted(VecDeque::from([
                            Ok(vec![(EV_KEY, 97, 1)]),
                            Err(io::ErrorKind::BrokenPipe),
                        ])),
                        held_modifiers: 0,
                        chord_down: false,
                    });
                }
                (Vec::new(), Vec::new())
            },
            |state| health.borrow_mut().push(state),
            |edge| {
                let mut events = seen.borrow_mut();
                events.push(edge);
                if edge == HotkeyEvent::Up {
                    stop.cancel();
                }
            },
        );
        assert_eq!(*seen.borrow(), vec![HotkeyEvent::Down, HotkeyEvent::Up]);
        assert!(health
            .borrow()
            .iter()
            .any(|state| matches!(state, EvdevListenerHealth::Degraded(_))));
    }

    #[test]
    fn supervisor_cancellation_releases_the_active_hold() {
        let path = PathBuf::from("/dev/input/scripted");
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let seen = std::cell::RefCell::new(Vec::new());
        run_evdev_supervisor_loop(
            97,
            &cancel,
            Duration::from_secs(1),
            || EvdevAvailability::Ready(vec![path.clone()]),
            |hold, paths| {
                if hold.devices.is_empty() && !paths.is_empty() {
                    hold.devices.push(HoldDevice {
                        path: path.clone(),
                        reader: HoldReader::Scripted(VecDeque::from([Ok(vec![(EV_KEY, 97, 1)])])),
                        held_modifiers: 0,
                        chord_down: false,
                    });
                }
                (Vec::new(), Vec::new())
            },
            |_| {},
            |edge| {
                seen.borrow_mut().push(edge);
                if edge == HotkeyEvent::Down {
                    stop.cancel();
                }
            },
        );
        assert_eq!(*seen.borrow(), vec![HotkeyEvent::Down, HotkeyEvent::Up]);
    }

    #[test]
    fn overlapping_keyboards_keep_the_hold_active_until_both_release() {
        let mut hold = HoldKey {
            devices: vec![
                HoldDevice {
                    path: PathBuf::from("/dev/input/first"),
                    reader: HoldReader::Scripted(VecDeque::from([
                        Ok(vec![(EV_KEY, 97, 1)]),
                        Ok(vec![(EV_KEY, 97, 0)]),
                    ])),
                    held_modifiers: 0,
                    chord_down: false,
                },
                HoldDevice {
                    path: PathBuf::from("/dev/input/second"),
                    reader: HoldReader::Scripted(VecDeque::from([
                        Ok(Vec::new()),
                        Ok(vec![(EV_KEY, 97, 1)]),
                        Ok(vec![(EV_KEY, 97, 0)]),
                    ])),
                    held_modifiers: 0,
                    chord_down: false,
                },
            ],
            code: 97,
            required_modifiers: 0,
            active_device: None,
            pending_edges: VecDeque::new(),
        };
        assert_eq!(hold.poll_edges().0, vec![HotkeyEvent::Down]);
        assert!(hold.poll_edges().0.is_empty());
        assert_eq!(hold.poll_edges().0, vec![HotkeyEvent::Up]);
    }

    #[test]
    fn same_keyboard_release_and_repress_preserves_both_edges() {
        let mut hold = HoldKey {
            devices: vec![HoldDevice {
                path: PathBuf::from("/dev/input/keyboard"),
                reader: HoldReader::Scripted(VecDeque::from([Ok(vec![
                    (EV_KEY, 97, 0),
                    (EV_KEY, 97, 1),
                ])])),
                held_modifiers: 0,
                chord_down: true,
            }],
            code: 97,
            required_modifiers: 0,
            active_device: Some(PathBuf::from("/dev/input/keyboard")),
            pending_edges: VecDeque::new(),
        };
        assert_eq!(
            hold.poll_edges().0,
            vec![HotkeyEvent::Up, HotkeyEvent::Down]
        );
    }

    #[test]
    fn overlapping_keyboard_removal_and_read_failure_transfer_the_hold() {
        let make_hold = |first_reader, second_reader| HoldKey {
            devices: vec![
                HoldDevice {
                    path: PathBuf::from("/dev/input/first"),
                    reader: first_reader,
                    held_modifiers: 0,
                    chord_down: true,
                },
                HoldDevice {
                    path: PathBuf::from("/dev/input/second"),
                    reader: second_reader,
                    held_modifiers: 0,
                    chord_down: true,
                },
            ],
            code: 97,
            required_modifiers: 0,
            active_device: Some(PathBuf::from("/dev/input/first")),
            pending_edges: VecDeque::new(),
        };

        let mut removed = make_hold(
            HoldReader::Scripted(VecDeque::new()),
            HoldReader::Scripted(VecDeque::new()),
        );
        assert!(removed
            .sync_devices(&[PathBuf::from("/dev/input/second")], |_| unreachable!())
            .0
            .is_empty());
        assert!(removed.poll_edges().0.is_empty());
        assert!(removed.sync_devices(&[], |_| unreachable!()).0.is_empty());
        assert_eq!(removed.poll_edges().0, vec![HotkeyEvent::Up]);

        let mut queued_handoff = make_hold(
            HoldReader::Scripted(VecDeque::new()),
            HoldReader::Scripted(VecDeque::from([
                Ok(vec![(EV_KEY, 97, 1)]),
                Ok(vec![(EV_KEY, 97, 0)]),
            ])),
        );
        queued_handoff.devices[1].chord_down = false;
        assert!(queued_handoff
            .sync_devices(&[PathBuf::from("/dev/input/second")], |_| unreachable!())
            .0
            .is_empty());
        assert!(queued_handoff.poll_edges().0.is_empty());
        assert_eq!(queued_handoff.poll_edges().0, vec![HotkeyEvent::Up]);

        let mut failed = make_hold(
            HoldReader::Scripted(VecDeque::from([Err(io::ErrorKind::BrokenPipe)])),
            HoldReader::Scripted(VecDeque::from([Ok(Vec::new()), Ok(vec![(EV_KEY, 97, 0)])])),
        );
        let (edges, errors) = failed.poll_edges();
        assert!(edges.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(failed.poll_edges().0, vec![HotkeyEvent::Up]);
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
    fn backend_selection_uses_session_and_observed_interface_version() {
        assert_eq!(
            select_native_backend(DesktopSession::Wayland, Some(1)).backend,
            NativeBackend::Portal
        );
        assert_eq!(
            select_native_backend(DesktopSession::Wayland, Some(0)).backend,
            NativeBackend::Unsupported
        );
        assert_eq!(
            select_native_backend(DesktopSession::Wayland, None).backend,
            NativeBackend::Unsupported
        );
        // X11 wins even if a portal service happens to be reachable.
        assert_eq!(
            select_native_backend(DesktopSession::X11, Some(2)).backend,
            NativeBackend::X11
        );
        let headless = select_native_backend(DesktopSession::Unknown, Some(2));
        assert_eq!(headless.backend, NativeBackend::Unsupported);
        assert!(headless.reason.is_some());
    }

    #[test]
    fn session_detection_does_not_infer_from_display_variables() {
        assert_eq!(
            DesktopSession::from_xdg_session_type(Some("Wayland")),
            DesktopSession::Wayland
        );
        assert_eq!(
            DesktopSession::from_xdg_session_type(Some("x11")),
            DesktopSession::X11
        );
        assert_eq!(
            DesktopSession::from_xdg_session_type(None),
            DesktopSession::Unknown
        );
        assert_eq!(
            DesktopSession::from_xdg_session_type(Some("tty")),
            DesktopSession::Unknown
        );
    }

    #[test]
    fn toggle_driver_ignores_repeats_rearms_and_stops_after_termination() {
        let mut driver = ToggleDriver::new();
        assert!(driver.on_edge(HotkeyEvent::Down));
        assert!(!driver.on_edge(HotkeyEvent::Down));
        assert!(!driver.on_edge(HotkeyEvent::Up));
        assert!(driver.on_edge(HotkeyEvent::Down));
        driver.terminate();
        assert!(!driver.on_edge(HotkeyEvent::Down));
        assert!(!driver.on_edge(HotkeyEvent::Up));
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
