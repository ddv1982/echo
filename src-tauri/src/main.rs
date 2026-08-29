use std::collections::BTreeMap;
use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, GlobalShortcuts, NewShortcut, Shortcut,
};
use ashpd::desktop::CreateSessionOptions;
use echo::audio::AudioCapture;
use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{
    DictEntry, Dictionary, History, RunDetail, WhisperRunMode, WhisperRuntimeBackend,
    WhisperRuntimeSource, WhisperTuningTelemetry,
};
use futures_util::StreamExt;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

mod cli;
mod setup;

const APP_ID: &str = "io.github.ddv1982.echo";
const GNOME_MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
static CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());

fn update_file_config(
    update: impl FnOnce(&mut echo_core::Config) -> Result<(), String>,
) -> Result<(), String> {
    let _write = CONFIG_WRITE_LOCK.lock().expect("config write lock");
    let mut config = echo_core::Config::load().unwrap_or_default();
    update(&mut config)?;
    config.save()?;
    echo::settings::reload();
    health_invalidate();
    Ok(())
}
const GNOME_CUSTOM_KEY_SCHEMA: &str =
    "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const ECHO_CUSTOM_KEY_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/echo/";
const ECHO_CUSTOM_KEY_NAME: &str = "Echo Dictation";

struct FixedShortcut;

impl FixedShortcut {
    const ID: &'static str = "toggle-recording";
    const DISPLAY: &'static str = "Super+Alt+Space";
    const PORTAL_TRIGGER: &'static str = "LOGO+ALT+space";
    const GNOME_ACCELERATOR: &'static str = "<Super><Alt>space";

    fn x11_hotkey() -> HotKey {
        HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::Space)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    phase: String,
    last_transcript: Option<String>,
    recording: bool,
    microphone_ready: bool,
    engine_name: String,
    engine_ready: bool,
    injection_name: String,
    injection_ready: bool,
    shortcut: ShortcutStatus,
    cleanup_name: String,
    hud_enabled: bool,
    recording_limit_seconds: Option<u32>,
    recording_policy: RecordingPolicyDto,
    settings_path: String,
    version: String,
    last_error: Option<String>,
    last_run: Option<LastRun>,
    language_warning: Option<String>,
    recording_in_process: bool,
    current_exe: String,
    first_path_hit: Option<String>,
    stale_installs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingPolicyDto {
    minimum_seconds: u32,
    default_seconds: u32,
    maximum_seconds: u32,
    presets_seconds: [u32; 5],
}

fn recording_policy_dto() -> RecordingPolicyDto {
    RecordingPolicyDto {
        minimum_seconds: echo_core::RecordingLimit::MIN.seconds(),
        default_seconds: echo_core::RecordingLimit::DEFAULT.seconds(),
        maximum_seconds: echo_core::RecordingLimit::MAX.seconds(),
        presets_seconds: echo_core::RecordingLimit::PRESETS.map(echo_core::RecordingLimit::seconds),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum ShortcutStatus {
    Probing {
        desired: String,
    },
    Active {
        desired: String,
        effective: String,
        backend: ShortcutBackendName,
        activation: Option<String>,
        verification_identity: String,
    },
    GnomeReady {
        desired: String,
        effective: String,
        detail: String,
        command: String,
        binding: String,
        activation: Option<String>,
        verification_identity: String,
    },
    GnomeSetup {
        desired: String,
        setup: LegacyShortcutSetup,
    },
    Manual {
        desired: String,
        command: String,
        detail: String,
    },
    Failed {
        desired: String,
        detail: String,
    },
    Unsupported {
        desired: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LegacyShortcutState {
    Missing,
    Stale,
    Conflicting,
    Ready,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyShortcutSetup {
    state: LegacyShortcutState,
    detail: String,
    command: String,
    binding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GnomeCustomBinding {
    path: String,
    name: String,
    command: String,
    binding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GnomeShortcutSnapshot {
    paths: Vec<String>,
    bindings: Vec<GnomeCustomBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GsettingsWrite {
    schema: String,
    key: &'static str,
    value: String,
}

/// What the last transcription actually ran, observed from the engine's own
/// output and persisted on the history row. Distinct from the settings
/// fields: one is a request, the other is an observation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastRun {
    engine: String,
    binary: Option<String>,
    model_path: Option<String>,
    multilingual: Option<bool>,
    vad: Option<bool>,
    infer_ms: u64,
    language: Option<String>,
    language_probability: Option<f32>,
    performance: Option<LastRunPerformance>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastRunPerformance {
    mode: WhisperRunMode,
    runtime_source: WhisperRuntimeSource,
    backend: WhisperRuntimeBackend,
    device: Option<String>,
    total_ms: u64,
    audio_encode_ms: u64,
    child_wall_ms: u64,
    parse_ms: u64,
    attempt_count: usize,
    tuning: WhisperTuningTelemetry,
    selection: Option<echo_core::WhisperSelectionTelemetry>,
    recovery: Option<echo_core::WhisperRecoveryTelemetry>,
}

fn project_last_run_performance(detail: &RunDetail) -> Option<LastRunPerformance> {
    let whisper = detail.whisper.as_ref()?;
    Some(LastRunPerformance {
        mode: whisper.mode,
        runtime_source: whisper.runtime.source,
        backend: whisper.runtime.backend,
        device: whisper.runtime.device.clone(),
        total_ms: whisper.total_ms,
        audio_encode_ms: whisper.audio_encode_ms,
        child_wall_ms: whisper
            .attempts
            .iter()
            .map(|attempt| attempt.child_wall_ms)
            .sum(),
        parse_ms: whisper.parse_ms,
        attempt_count: whisper.attempts.len(),
        tuning: whisper.tuning,
        selection: whisper.selection.clone(),
        recovery: whisper.recovery.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SettingSource {
    Env,
    File,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SettingField<T> {
    value: Option<T>,
    effective: T,
    source: SettingSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    engine: SettingField<String>,
    whisper_model: SettingField<String>,
    cleanup: SettingField<String>,
    hud: SettingField<bool>,
    record_seconds: SettingField<u32>,
    language: SettingField<String>,
    whisper_acceleration: SettingField<String>,
}

#[derive(Debug, Default, Clone)]
struct SettingsEnv {
    engine: Option<String>,
    whisper_model: Option<String>,
    cleanup: Option<String>,
    hud: Option<String>,
    record_seconds: Option<String>,
    language: Option<String>,
    whisper_acceleration: Option<String>,
}

#[derive(Debug, Clone)]
struct Health {
    microphone_ready: bool,
    engine_name: String,
    engine_ready: bool,
    injection_name: String,
    injection_ready: bool,
    current_exe: String,
    first_path_hit: Option<String>,
    stale_installs: Vec<String>,
}

static HEALTH: Mutex<Option<(Instant, Health)>> = Mutex::new(None);

/// Microphone, engine, and injection probes open devices and scan PATH, too
/// costly for the frontend's 400 ms status poll. Cache them briefly.
fn health_snapshot() -> Health {
    const TTL: Duration = Duration::from_secs(10);
    let mut cache = HEALTH.lock().expect("health cache lock");
    if let Some((at, health)) = cache.as_ref() {
        if at.elapsed() < TTL {
            return health.clone();
        }
    }
    let (engine_name, engine_ready) = echo::stt::engine_summary();
    let (injection_name, injection_ready) = echo::inject::detection_summary();
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let installs = echo::upgrade::path_installs(&env::var("PATH").unwrap_or_default());
    let first_path_hit = installs
        .first()
        .map(|(path, _)| path.to_string_lossy().into_owned());
    let stale_installs = current_exe
        .as_ref()
        .and_then(|path| echo::upgrade::file_identity(path).ok())
        .map(|current| {
            echo::upgrade::stale_installs(&installs, current)
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let health = Health {
        microphone_ready: AudioCapture::default_input_ready().is_ok(),
        engine_name,
        engine_ready,
        injection_name,
        injection_ready,
        current_exe: current_exe
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        first_path_hit,
        stale_installs,
    };
    *cache = Some((Instant::now(), health.clone()));
    health
}

fn health_invalidate() {
    *HEALTH.lock().expect("health cache lock") = None;
}

static LEGACY_SHORTCUT_CACHE: Mutex<Option<(Instant, String, String, LegacyShortcutSetup)>> =
    Mutex::new(None);

fn legacy_shortcut_setup(
    native: &NativeShortcutState,
    current_exe: &str,
) -> Option<LegacyShortcutSetup> {
    let session = echo::hotkey::DesktopSession::from_xdg_session_type(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    if !needs_legacy_setup(native, session) {
        return None;
    }

    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let executable = stable_shortcut_executable(current_exe, env::var_os("APPIMAGE").as_deref());
    let command = absolute_toggle_command(&executable).unwrap_or_default();
    let binding = FixedShortcut::GNOME_ACCELERATOR.to_string();
    if !desktop
        .split(':')
        .any(|part| matches!(part.to_ascii_lowercase().as_str(), "gnome" | "zorin"))
    {
        return Some(LegacyShortcutSetup {
            state: LegacyShortcutState::Unsupported,
            detail: "This Wayland compositor has no GlobalShortcuts portal; add the command in its keyboard settings."
                .to_string(),
            command,
            binding,
        });
    }
    if command.is_empty() || binding.is_empty() {
        return Some(LegacyShortcutSetup {
            state: LegacyShortcutState::Unsupported,
            detail: "Echo could not derive a stable executable command for GNOME settings."
                .to_string(),
            command,
            binding,
        });
    }

    const TTL: Duration = Duration::from_secs(2);
    let mut cache = LEGACY_SHORTCUT_CACHE
        .lock()
        .expect("legacy shortcut cache lock");
    if let Some((at, cached_command, cached_binding, status)) = cache.as_ref() {
        if at.elapsed() < TTL && cached_command == &command && cached_binding == &binding {
            return Some(status.clone());
        }
    }
    let setup = match read_gnome_shortcuts() {
        Ok(snapshot) => classify_gnome_shortcut(&snapshot, &command, &binding),
        Err(err) => LegacyShortcutSetup {
            state: LegacyShortcutState::Unsupported,
            detail: format!("Cannot inspect GNOME custom shortcuts: {err}"),
            command: command.clone(),
            binding: binding.clone(),
        },
    };
    *cache = Some((Instant::now(), command, binding, setup.clone()));
    Some(setup)
}

fn needs_legacy_setup(native: &NativeShortcutState, session: echo::hotkey::DesktopSession) -> bool {
    matches!(native, NativeShortcutState::PortalAbsent { .. })
        && session == echo::hotkey::DesktopSession::Wayland
}

fn absolute_toggle_command(current_exe: &str) -> Result<String, String> {
    let path = std::path::Path::new(current_exe);
    if !path.is_absolute() {
        return Err("the running executable path is not absolute".to_string());
    }
    let raw = path.to_string_lossy();
    let quoted = if raw
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
    {
        raw.into_owned()
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    };
    Ok(format!("{quoted} rec --toggle"))
}

fn stable_shortcut_executable(current_exe: &str, appimage: Option<&std::ffi::OsStr>) -> String {
    appimage
        .map(std::path::Path::new)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| std::path::Path::new(current_exe))
        .to_string_lossy()
        .into_owned()
}

fn classify_gnome_shortcut(
    snapshot: &GnomeShortcutSnapshot,
    command: &str,
    binding: &str,
) -> LegacyShortcutSetup {
    let target = snapshot
        .bindings
        .iter()
        .find(|entry| entry.path == ECHO_CUSTOM_KEY_PATH);
    let collision = snapshot.bindings.iter().find(|entry| {
        entry.path != ECHO_CUSTOM_KEY_PATH
            && snapshot.paths.contains(&entry.path)
            && gnome_accelerators_match(&entry.binding, binding)
    });
    let setup = |state, detail: String| LegacyShortcutSetup {
        state,
        detail,
        command: command.to_string(),
        binding: binding.to_string(),
    };

    if let Some(entry) = target {
        let occupied =
            !entry.name.is_empty() || !entry.command.is_empty() || !entry.binding.is_empty();
        let echo_command = entry.command.is_empty() || echo_toggle_command(&entry.command, command);
        if occupied && (entry.name != ECHO_CUSTOM_KEY_NAME || !echo_command) {
            return setup(
                LegacyShortcutState::Conflicting,
                "The reserved Echo shortcut slot belongs to another custom shortcut; Echo will not overwrite it."
                    .to_string(),
            );
        }
    }
    if let Some(entry) = collision {
        return setup(
            LegacyShortcutState::Conflicting,
            format!(
                "{} already uses {}; change that shortcut in GNOME Settings first.",
                if entry.name.is_empty() {
                    "Another custom shortcut"
                } else {
                    &entry.name
                },
                binding
            ),
        );
    }
    if !snapshot
        .paths
        .iter()
        .any(|path| path == ECHO_CUSTOM_KEY_PATH)
    {
        return setup(
            LegacyShortcutState::Missing,
            "GNOME has no Echo custom shortcut yet. Close GNOME Settings before setup.".to_string(),
        );
    }
    let Some(entry) = target else {
        return setup(
            LegacyShortcutState::Missing,
            "GNOME has no Echo custom shortcut yet. Close GNOME Settings before setup.".to_string(),
        );
    };
    if entry.name == ECHO_CUSTOM_KEY_NAME
        && entry.command == command
        && gnome_accelerators_match(&entry.binding, binding)
    {
        setup(
            LegacyShortcutState::Ready,
            "GNOME owns this Echo shortcut and its command is current.".to_string(),
        )
    } else {
        setup(
            LegacyShortcutState::Stale,
            "The Echo-owned GNOME shortcut uses an old command or key binding. Close GNOME Settings before repair."
                .to_string(),
        )
    }
}

fn echo_toggle_command(command: &str, desired: &str) -> bool {
    if command == desired {
        return true;
    }
    let Some(parts) = safe_shell_words(command) else {
        return false;
    };
    let mut index = 0;
    if parts.first().map(String::as_str) == Some("/usr/bin/env") {
        index = 1;
        while let Some(part) = parts.get(index) {
            let Some((name, _)) = part.split_once('=') else {
                break;
            };
            if name != "PATH"
                && !(name.starts_with("ECHO_")
                    && name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    }))
            {
                return false;
            }
            index += 1;
        }
    }
    let Some(executable) = parts.get(index) else {
        return false;
    };
    let executable = std::path::Path::new(executable);
    executable.is_absolute()
        && matches!(
            executable.file_name().and_then(|name| name.to_str()),
            Some("echo-desktop" | "echo-app")
        )
        && parts.get(index + 1).map(String::as_str) == Some("rec")
        && parts.get(index + 2).map(String::as_str) == Some("--toggle")
        && parts.len() == index + 3
}

fn safe_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut started = false;
    for character in command.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            started = true;
        } else if quoted {
            if character == '\'' {
                quoted = false;
            } else {
                word.push(character);
            }
        } else if character == '\'' {
            quoted = true;
            started = true;
        } else if character == '\\' {
            escaped = true;
            started = true;
        } else if character.is_ascii_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else if matches!(
            character,
            ';' | '|' | '&' | '$' | '`' | '(' | ')' | '<' | '>' | '"'
        ) || character.is_control()
        {
            return None;
        } else {
            word.push(character);
            started = true;
        }
    }
    if quoted || escaped {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

fn gnome_accelerators_match(left: &str, right: &str) -> bool {
    canonical_gnome_accelerator(left)
        .zip(canonical_gnome_accelerator(right))
        .is_some_and(|(left, right)| left == right)
}

fn canonical_gnome_accelerator(raw: &str) -> Option<String> {
    let mut rest = raw.trim();
    let mut modifiers = [false; 4];
    while let Some(after_open) = rest.strip_prefix('<') {
        let close = after_open.find('>')?;
        let index = match after_open[..close].to_ascii_lowercase().as_str() {
            "super" | "mod4" => 0,
            "ctrl" | "control" | "primary" => 1,
            "alt" | "mod1" => 2,
            "shift" => 3,
            _ => return None,
        };
        if modifiers[index] {
            return None;
        }
        modifiers[index] = true;
        rest = &after_open[close + 1..];
    }
    let terminal = match rest.to_ascii_lowercase().as_str() {
        "enter" | "return" => "Enter".to_string(),
        "up" => "ArrowUp".to_string(),
        "down" => "ArrowDown".to_string(),
        "left" => "ArrowLeft".to_string(),
        "right" => "ArrowRight".to_string(),
        key => key.to_string(),
    };
    if terminal.is_empty() {
        return None;
    }
    let names = ["Super", "Ctrl", "Alt", "Shift"];
    let mut parts = modifiers
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .map(|(index, _)| names[index].to_string())
        .collect::<Vec<_>>();
    parts.push(terminal);
    Some(parts.join("+"))
}

fn read_gnome_shortcuts() -> Result<GnomeShortcutSnapshot, String> {
    let paths = parse_gsettings_strings(&gsettings_get(
        GNOME_MEDIA_KEYS_SCHEMA,
        "custom-keybindings",
    )?);
    let mut inspected = paths.clone();
    if !inspected.iter().any(|path| path == ECHO_CUSTOM_KEY_PATH) {
        inspected.push(ECHO_CUSTOM_KEY_PATH.to_string());
    }
    let mut bindings = Vec::with_capacity(inspected.len());
    for path in inspected {
        let schema = format!("{GNOME_CUSTOM_KEY_SCHEMA}:{path}");
        bindings.push(GnomeCustomBinding {
            path,
            name: gsettings_string(&gsettings_get(&schema, "name")?)?,
            command: gsettings_string(&gsettings_get(&schema, "command")?)?,
            binding: gsettings_string(&gsettings_get(&schema, "binding")?)?,
        });
    }
    Ok(GnomeShortcutSnapshot { paths, bindings })
}

fn gsettings_get(schema: &str, key: &str) -> Result<String, String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_gsettings_strings(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in raw.chars() {
        if !quoted {
            if character == '\'' {
                quoted = true;
                current.clear();
            }
            continue;
        }
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '\'' {
            quoted = false;
            values.push(current.clone());
        } else {
            current.push(character);
        }
    }
    values
}

fn gsettings_string(raw: &str) -> Result<String, String> {
    parse_gsettings_strings(raw)
        .into_iter()
        .next()
        .ok_or_else(|| format!("invalid gsettings string: {raw}"))
}

fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn gvariant_strv(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| gvariant_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn gnome_repair_writes(
    setup: &LegacyShortcutSetup,
    current_paths: &[String],
) -> Result<Vec<GsettingsWrite>, String> {
    let schema = format!("{GNOME_CUSTOM_KEY_SCHEMA}:{ECHO_CUSTOM_KEY_PATH}");
    match setup.state {
        LegacyShortcutState::Ready => Ok(Vec::new()),
        LegacyShortcutState::Missing => Ok(vec![
            GsettingsWrite {
                schema: schema.clone(),
                key: "name",
                value: gvariant_string(ECHO_CUSTOM_KEY_NAME),
            },
            GsettingsWrite {
                schema: schema.clone(),
                key: "command",
                value: gvariant_string(&setup.command),
            },
            GsettingsWrite {
                schema,
                key: "binding",
                value: gvariant_string(&setup.binding),
            },
            GsettingsWrite {
                schema: GNOME_MEDIA_KEYS_SCHEMA.to_string(),
                key: "custom-keybindings",
                value: gvariant_strv(&with_echo_shortcut_path(current_paths.to_vec())),
            },
        ]),
        LegacyShortcutState::Stale => Ok(vec![
            GsettingsWrite {
                schema: schema.clone(),
                key: "name",
                value: gvariant_string(ECHO_CUSTOM_KEY_NAME),
            },
            GsettingsWrite {
                schema: schema.clone(),
                key: "command",
                value: gvariant_string(&setup.command),
            },
            GsettingsWrite {
                schema,
                key: "binding",
                value: gvariant_string(&setup.binding),
            },
        ]),
        LegacyShortcutState::Conflicting | LegacyShortcutState::Unsupported => {
            Err(setup.detail.clone())
        }
    }
}

fn gnome_repair_transaction(
    expected: &GnomeShortcutSnapshot,
    current: &GnomeShortcutSnapshot,
    setup: &LegacyShortcutSetup,
) -> Result<Vec<GsettingsWrite>, String> {
    if current != expected {
        return Err(
            "GNOME shortcuts changed while Echo was preparing the repair; review them and try again."
                .to_string(),
        );
    }
    gnome_repair_writes(setup, &current.paths)
}

fn dconf_keyfile(writes: &[GsettingsWrite]) -> Result<String, String> {
    let mut groups = BTreeMap::<&str, Vec<&GsettingsWrite>>::new();
    for write in writes {
        let group = if write.schema == GNOME_MEDIA_KEYS_SCHEMA {
            "/"
        } else if write.schema == format!("{GNOME_CUSTOM_KEY_SCHEMA}:{ECHO_CUSTOM_KEY_PATH}") {
            "custom-keybindings/echo"
        } else {
            return Err(format!("refusing unexpected dconf schema {}", write.schema));
        };
        groups.entry(group).or_default().push(write);
    }
    let mut keyfile = String::new();
    for (group, writes) in groups {
        keyfile.push_str(&format!("[{group}]\n"));
        for write in writes {
            keyfile.push_str(&format!("{}={}\n", write.key, write.value));
        }
        keyfile.push('\n');
    }
    Ok(keyfile)
}

fn apply_dconf_transaction(writes: &[GsettingsWrite]) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if writes.is_empty() {
        return Ok(());
    }
    let mut child = Command::new("dconf")
        .args(["load", "/org/gnome/settings-daemon/plugins/media-keys/"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot start atomic dconf repair: {err}"))?;
    child
        .stdin
        .take()
        .ok_or("cannot open dconf repair input")?
        .write_all(dconf_keyfile(writes)?.as_bytes())
        .map_err(|err| format!("cannot write dconf repair input: {err}"))?;
    let output = child
        .wait_with_output()
        .map_err(|err| format!("cannot finish atomic dconf repair: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn with_echo_shortcut_path(mut paths: Vec<String>) -> Vec<String> {
    if !paths.iter().any(|path| path == ECHO_CUSTOM_KEY_PATH) {
        paths.push(ECHO_CUSTOM_KEY_PATH.to_string());
    }
    paths
}

#[cfg(test)]
fn apply_gsettings_writes(writes: &[GsettingsWrite]) -> Result<(), String> {
    for write in writes {
        let output = std::process::Command::new("gsettings")
            .args(["set", &write.schema, write.key, &write.value])
            .output()
            .map_err(|err| err.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: String,
    text: String,
    raw: String,
    engine: String,
    started_at: u64,
    infer_ms: u64,
    injection: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DictionaryItem {
    spoken: String,
    written: String,
    created_at: u64,
}

impl From<&DictEntry> for DictionaryItem {
    fn from(entry: &DictEntry) -> Self {
        Self {
            spoken: entry.spoken.clone(),
            written: entry.written.clone(),
            created_at: entry.created_at,
        }
    }
}

#[tauri::command]
fn get_app_status() -> AppStatus {
    let status = echo::status::read();
    let recording_limit =
        project_recording_limit(&status, echo::rec::recording_limit_from_process().limit);
    let health = health_snapshot();
    let shortcut = project_shortcut_status(&native_shortcut_state(), &health.current_exe);
    let last_run = History::load().ok().and_then(|history| {
        history.rows().last().map(|row| LastRun {
            engine: row.engine.to_string(),
            binary: row.detail.binary.clone(),
            model_path: row.detail.model_path.clone(),
            multilingual: row.detail.multilingual,
            vad: row.detail.vad,
            infer_ms: row.infer_ms,
            language: row.detail.language.clone(),
            language_probability: row.detail.language_probability,
            performance: project_last_run_performance(&row.detail),
        })
    });
    let recording_in_process = status.state == "Recording" && echo::rec::recording_in_process();
    AppStatus {
        recording: status.state == "Recording",
        phase: status.state,
        last_transcript: status.last,
        microphone_ready: health.microphone_ready,
        engine_name: health.engine_name,
        engine_ready: health.engine_ready,
        injection_name: health.injection_name,
        injection_ready: health.injection_ready,
        shortcut,
        cleanup_name: echo::cleanup::mode_name(),
        hud_enabled: echo::ui::hud::enabled(),
        recording_limit_seconds: recording_limit.map(echo_core::RecordingLimit::seconds),
        recording_policy: recording_policy_dto(),
        settings_path: echo_core::config_path().to_string_lossy().into_owned(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        last_error: status.error,
        last_run,
        language_warning: echo::stt::language_warning(),
        recording_in_process,
        current_exe: health.current_exe,
        first_path_hit: health.first_path_hit,
        stale_installs: health.stale_installs,
    }
}

fn project_recording_limit(
    status: &echo::status::Status,
    current: echo_core::RecordingLimit,
) -> Option<echo_core::RecordingLimit> {
    if status.state == "Recording" {
        status.recording_limit
    } else {
        Some(current)
    }
}

fn project_shortcut_status(native: &NativeShortcutState, current_exe: &str) -> ShortcutStatus {
    let desired = FixedShortcut::DISPLAY.to_string();
    let activation = echo::status::shortcut_activation();
    match native {
        NativeShortcutState::Probing => ShortcutStatus::Probing { desired },
        NativeShortcutState::Active { backend, effective } => ShortcutStatus::Active {
            desired,
            effective: effective.clone(),
            backend: *backend,
            activation,
            verification_identity: format!("{}:{effective}", backend.as_str()),
        },
        NativeShortcutState::PortalAbsent { detail } => {
            let Some(setup) = legacy_shortcut_setup(native, current_exe) else {
                return ShortcutStatus::Unsupported {
                    desired,
                    detail: detail.clone(),
                };
            };
            let is_gnome = env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .split(':')
                .any(|part| matches!(part.to_ascii_lowercase().as_str(), "gnome" | "zorin"));
            if is_gnome {
                if setup.state == LegacyShortcutState::Ready {
                    ShortcutStatus::GnomeReady {
                        desired: desired.clone(),
                        effective: desired,
                        detail: setup.detail,
                        verification_identity: format!("gnome:{}:{}", setup.binding, setup.command),
                        command: setup.command,
                        binding: setup.binding,
                        activation,
                    }
                } else {
                    ShortcutStatus::GnomeSetup { desired, setup }
                }
            } else {
                if setup.command.is_empty() || setup.binding.is_empty() {
                    return ShortcutStatus::Unsupported {
                        desired,
                        detail: setup.detail,
                    };
                }
                ShortcutStatus::Manual {
                    desired,
                    command: setup.command,
                    detail: setup.detail,
                }
            }
        }
        NativeShortcutState::Failed { detail } => ShortcutStatus::Failed {
            desired,
            detail: detail.clone(),
        },
        NativeShortcutState::Unsupported { detail } => ShortcutStatus::Unsupported {
            desired,
            detail: detail.clone(),
        },
    }
}

#[tauri::command]
fn get_shortcut_status() -> ShortcutStatus {
    project_shortcut_status(&native_shortcut_state(), &current_exe_string())
}

fn current_exe_string() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
fn repair_legacy_shortcut() -> Result<LegacyShortcutSetup, String> {
    let native = native_shortcut_state();
    let advertised = legacy_shortcut_setup(&native, &current_exe_string())
        .ok_or("this session does not need a legacy compositor shortcut")?;
    if advertised.state == LegacyShortcutState::Unsupported {
        return Err(advertised.detail);
    }

    let snapshot = read_gnome_shortcuts()?;
    let setup = classify_gnome_shortcut(&snapshot, &advertised.command, &advertised.binding);
    let current = read_gnome_shortcuts()?;
    let writes = gnome_repair_transaction(&snapshot, &current, &setup)?;
    apply_dconf_transaction(&writes)?;
    *LEGACY_SHORTCUT_CACHE
        .lock()
        .expect("legacy shortcut cache lock") = None;

    let repaired = classify_gnome_shortcut(
        &read_gnome_shortcuts()?,
        &advertised.command,
        &advertised.binding,
    );
    if repaired.state != LegacyShortcutState::Ready {
        return Err(format!(
            "GNOME shortcut repair did not become ready: {}",
            repaired.detail
        ));
    }
    Ok(repaired)
}

#[tauri::command]
fn get_history() -> Result<Vec<HistoryItem>, String> {
    let history = History::load()?;
    Ok(history
        .rows()
        .iter()
        .rev()
        .filter(|row| !row.text.trim().is_empty())
        .map(|row| HistoryItem {
            id: row.id.clone(),
            text: row.text.clone(),
            raw: row.raw.clone(),
            engine: row.engine.to_string(),
            started_at: row.started_at,
            infer_ms: row.infer_ms,
            injection: format!("{:?}", row.inject),
        })
        .collect())
}

#[tauri::command]
fn get_dictionary() -> Result<Vec<DictionaryItem>, String> {
    let dictionary = Dictionary::load()?;
    Ok(dictionary
        .entries()
        .iter()
        .map(DictionaryItem::from)
        .collect())
}

#[tauri::command]
fn add_dictionary_entry(spoken: String, written: String) -> Result<DictionaryItem, String> {
    let spoken = spoken.trim().to_string();
    let written = written.trim().to_string();
    if spoken.is_empty() || written.is_empty() {
        return Err("Both spoken and written forms are required.".to_string());
    }
    let mut dictionary = Dictionary::load()?;
    dictionary
        .add(spoken, written)
        .map(|entry| DictionaryItem::from(&entry))
}

#[tauri::command]
fn remove_dictionary_entry(spoken: String, written: String) -> Result<bool, String> {
    Dictionary::load()?.remove(&spoken, &written)
}

#[tauri::command]
fn toggle_recording() -> Result<(), String> {
    start_recording_thread().map(|_| ())
}

#[tauri::command]
fn stop_recording(activation: String) -> Result<bool, String> {
    echo::rec::stop_shortcut_recording(&activation)
}

/// Live microphone RMS when this process holds the recording session (the
/// GUI's own record button), 0 otherwise.
#[tauri::command]
fn get_recording_level() -> f32 {
    if echo::rec::recording_in_process() {
        echo::audio::process_meter().level()
    } else {
        0.0
    }
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    SysClipboard.set(&text)
}

/// Remove the copies the stale-install scan classifies as stale right now.
/// The webview cannot name paths; the backend re-runs the scan and deletes
/// only what it classified, plus the known user-local leftovers once a stale
/// binary is gone.
#[tauri::command]
fn remove_stale_installs() -> Result<Vec<String>, String> {
    let current = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .ok_or("cannot resolve the running executable")?;
    let path_var = env::var("PATH").unwrap_or_default();
    let home = env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let report = echo::upgrade::remove_stale_installs(&current, &path_var, &home);
    health_invalidate();
    if report.remaining.is_empty() {
        Ok(report
            .removed
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect())
    } else {
        let removed = report
            .removed
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = report
            .remaining
            .iter()
            .map(|(path, err)| format!("{}: {err}", path.display()))
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!("removed {removed}; still present: {remaining}"))
    }
}

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    read_settings()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageOptionDto {
    code: String,
    english_name: String,
    group: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageOptionsDto {
    /// "multilingual" offers Auto plus all 100 languages, "english" is an
    /// English-only Whisper model, "parakeet" is a fixed 25-language
    /// automatic capability with no picker.
    mode: String,
    /// The English-only model's filename when mode is "english".
    model: Option<String>,
    options: Vec<LanguageOptionDto>,
}

#[tauri::command]
fn list_languages() -> LanguageOptionsDto {
    match echo::stt::language_support() {
        echo::stt::LanguageSupport::WhisperMultilingual => LanguageOptionsDto {
            mode: "multilingual".to_string(),
            model: None,
            options: echo_core::Language::all()
                .map(|language| LanguageOptionDto {
                    code: language.code().to_string(),
                    english_name: language.english_name().to_string(),
                    group: if ["en", "de", "es", "fr"].contains(&language.code()) {
                        "common"
                    } else {
                        "all"
                    }
                    .to_string(),
                })
                .collect(),
        },
        echo::stt::LanguageSupport::WhisperEnglishOnly { model } => LanguageOptionsDto {
            mode: "english".to_string(),
            model: Some(model),
            options: vec![LanguageOptionDto {
                code: "en".to_string(),
                english_name: "english".to_string(),
                group: "common".to_string(),
            }],
        },
        echo::stt::LanguageSupport::Parakeet => LanguageOptionsDto {
            mode: "parakeet".to_string(),
            model: None,
            options: echo_core::PARAKEET_LANGUAGES
                .iter()
                .filter_map(|code| echo_core::Language::from_code(code))
                .map(|language| LanguageOptionDto {
                    code: language.code().to_string(),
                    english_name: language.english_name().to_string(),
                    group: "all".to_string(),
                })
                .collect(),
        },
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WhisperModelDto {
    name: String,
    path: String,
    family: String,
    multilingual: bool,
    quantisation: Option<String>,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineAvailabilityDto {
    id: String,
    available: bool,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelInventoryDto {
    whisper: Vec<WhisperModelDto>,
    vad: Vec<String>,
    parakeet: Option<String>,
    engines: Vec<EngineAvailabilityDto>,
}

#[tauri::command]
fn list_models() -> Result<ModelInventoryDto, String> {
    let cache = echo::stt::ModelCache::from_env();
    let inventory = echo::stt::SpeechRuntimeInventory::from_cache(&cache).models;
    Ok(ModelInventoryDto {
        whisper: inventory
            .whisper
            .iter()
            .map(|model| WhisperModelDto {
                name: model.name.clone(),
                path: model.path.to_string_lossy().into_owned(),
                family: model.family.label().to_string(),
                multilingual: model.multilingual,
                quantisation: model.quantisation.clone(),
                size_bytes: model.size_bytes,
            })
            .collect(),
        vad: inventory
            .vad
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        parakeet: inventory
            .parakeet
            .map(|path| path.to_string_lossy().into_owned()),
        engines: echo::stt::engine_availability()
            .into_iter()
            .map(|engine| EngineAvailabilityDto {
                id: engine.id.to_string(),
                available: engine.available,
                reason: engine.reason,
            })
            .collect(),
    })
}

#[tauri::command]
fn set_settings(settings: Settings) -> Result<Settings, String> {
    write_settings(settings)
}

#[tauri::command]
fn get_microphones() -> echo::microphone::MicrophoneSnapshot {
    echo::audio::microphone_snapshot()
}

#[tauri::command]
fn set_microphone(id: Option<String>) -> Result<echo::microphone::MicrophoneSnapshot, String> {
    if env::var("ECHO_MICROPHONE")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Err("ECHO_MICROPHONE controls the microphone in this process".to_string());
    }
    let snapshot = echo::audio::microphone_snapshot();
    let selection = match id {
        None => None,
        Some(raw) => {
            let id = echo::microphone::MicrophoneId::parse(raw)?;
            let device = snapshot
                .devices
                .iter()
                .find(|device| device.id == id)
                .ok_or_else(|| {
                    "that microphone is no longer connected; refresh and choose again".to_string()
                })?;
            Some((id, device.label.clone()))
        }
    };
    update_file_config(|config| {
        update_microphone_config(config, selection);
        Ok(())
    })?;
    Ok(echo::audio::microphone_snapshot())
}

fn update_microphone_config(
    config: &mut echo_core::Config,
    selection: Option<(echo::microphone::MicrophoneId, String)>,
) {
    config.microphone =
        selection.map(
            |(id, last_seen_label)| echo_core::MicrophoneSelection::Device {
                id: id.as_str().to_string(),
                last_seen_label,
            },
        );
}

fn microphone_test(
    capture: Result<AudioCapture, echo::audio::AudioError>,
) -> echo::microphone::MicrophoneTestResult {
    let capture = match capture {
        Ok(capture) => capture,
        Err(error) => {
            return echo::microphone::MicrophoneTestResult::Failed {
                device: None,
                category: error.category(),
                message: error.to_string(),
            };
        }
    };
    let snapshot = echo::audio::microphone_snapshot();
    let device = snapshot
        .devices
        .into_iter()
        .find(|device| device.id == capture.device_id);
    match capture.record(std::time::Duration::from_secs(1), None) {
        Ok(result) => echo::microphone::MicrophoneTestResult::Completed {
            device: device.unwrap_or_else(|| echo::microphone::InputDeviceInfo {
                id: capture.device_id,
                label: capture.device_name,
                is_default: false,
                manufacturer: None,
                device_type: None,
                interface_type: None,
                address: None,
                driver: None,
                extended: Vec::new(),
                host: echo::microphone::AudioHost::Other,
                transport: echo::microphone::InputTransport::Unknown,
                tier: echo::microphone::EndpointTier::Primary,
                hint: String::new(),
            }),
            peak_rms: result.peak_rms,
            outcome: if result.peak_rms > 0.001 {
                echo::microphone::MicrophoneTestOutcome::Heard
            } else {
                echo::microphone::MicrophoneTestOutcome::Silent
            },
        },
        Err(error) => echo::microphone::MicrophoneTestResult::Failed {
            device,
            category: error.category(),
            message: error.to_string(),
        },
    }
}

#[tauri::command]
fn test_input_device(id: Option<String>) -> Result<echo::microphone::MicrophoneTestResult, String> {
    let id = id.map(echo::microphone::MicrophoneId::parse).transpose()?;
    Ok(microphone_test(AudioCapture::open_exact(id.as_ref())))
}

#[tauri::command]
fn test_microphone_fallback() -> echo::microphone::MicrophoneTestResult {
    microphone_test(AudioCapture::open_default())
}

fn read_settings() -> Result<Settings, String> {
    // The picker's default must match what the recorder would do: auto when
    // the resolved model is multilingual, pinned English otherwise.
    let file = echo::settings::file_config();
    let catalog = echo::transcribe::language_catalog(None, &file);
    let language_default = match catalog.selection {
        echo::transcribe::LanguageSelection::EnglishOnly => "en",
        echo::transcribe::LanguageSelection::AutoOrPinned if catalog.model.is_none() => "en",
        echo::transcribe::LanguageSelection::AutoOrPinned
        | echo::transcribe::LanguageSelection::AutomaticOnly => "auto",
    };
    settings_from(&process_settings_env(), &file, language_default)
}

fn write_settings(settings: Settings) -> Result<Settings, String> {
    update_file_config(|config| {
        *config = config_from_values_with_base(&settings, config.clone())?;
        Ok(())
    })?;
    read_settings()
}

fn process_settings_env() -> SettingsEnv {
    SettingsEnv {
        engine: env::var("ECHO_ENGINE").ok(),
        whisper_model: env::var("ECHO_WHISPER_MODEL").ok(),
        cleanup: env::var("ECHO_CLEANUP").ok(),
        hud: env::var("ECHO_HUD").ok(),
        record_seconds: env::var("ECHO_RECORD_SECONDS").ok(),
        language: env::var("ECHO_LANGUAGE").ok(),
        whisper_acceleration: env::var("ECHO_WHISPER_ACCELERATION").ok(),
    }
}

fn settings_from(
    env: &SettingsEnv,
    file: &echo_core::Config,
    language_default: &str,
) -> Result<Settings, String> {
    let env_cleanup = env
        .cleanup
        .as_deref()
        .and_then(|raw| echo_core::CleanupMode::parse(raw).ok())
        .map(|mode| cleanup_name(&mode));
    Ok(Settings {
        engine: setting_field(
            env.engine
                .as_deref()
                .and_then(echo_core::EngineChoice::from_env_var)
                .map(engine_name),
            file.engine.map(engine_name),
            "auto".to_string(),
        ),
        whisper_model: setting_field(
            env.whisper_model.clone().filter(|name| !name.is_empty()),
            file.whisper_model.clone(),
            String::new(),
        ),
        cleanup: setting_field(
            env_cleanup,
            file.cleanup.as_ref().map(cleanup_name),
            "rules".to_string(),
        ),
        hud: hud_field(env.hud.as_deref(), file.hud),
        record_seconds: record_seconds_field(env.record_seconds.as_deref(), file.record_seconds),
        language: setting_field(
            env.language
                .as_deref()
                .and_then(echo_core::LanguageChoice::parse)
                .map(|choice| choice.as_str().to_string()),
            file.language.map(|choice| choice.as_str().to_string()),
            language_default.to_string(),
        ),
        whisper_acceleration: setting_field(
            env.whisper_acceleration
                .as_deref()
                .and_then(echo_core::WhisperAccelerationPreference::parse)
                .map(echo_core::WhisperAccelerationPreference::as_str)
                .map(str::to_string),
            file.whisper_acceleration
                .map(echo_core::WhisperAccelerationPreference::as_str)
                .map(str::to_string),
            echo::stt::whisper_acceleration_factory_default()
                .as_str()
                .to_string(),
        ),
    })
}

#[cfg(test)]
fn config_from_values(settings: &Settings) -> Result<echo_core::Config, String> {
    config_from_values_with_base(settings, echo_core::Config::load().unwrap_or_default())
}

fn config_from_values_with_base(
    settings: &Settings,
    mut config: echo_core::Config,
) -> Result<echo_core::Config, String> {
    config.engine = match settings.engine.value.as_deref() {
        None => None,
        Some(raw) => Some(
            echo_core::EngineChoice::from_env_var(raw)
                .ok_or_else(|| format!("unknown engine {raw}"))?,
        ),
    };
    config.whisper_model = nonempty(settings.whisper_model.value.clone());
    config.cleanup = match settings.cleanup.value.as_deref() {
        None => None,
        Some(raw) => Some(echo_core::CleanupMode::parse(raw).map_err(|err| err.to_string())?),
    };
    config.hud = settings.hud.value;
    config.record_seconds = settings
        .record_seconds
        .value
        .map(|secs| echo_core::RecordingLimit::clamped(u64::from(secs)).seconds());
    config.language = match settings.language.value.as_deref() {
        None => None,
        Some(raw) => Some(
            echo_core::LanguageChoice::parse(raw)
                .ok_or_else(|| format!("unknown language {raw}"))?,
        ),
    };
    config.whisper_acceleration = match settings.whisper_acceleration.value.as_deref() {
        None => None,
        Some(raw) => Some(
            echo_core::WhisperAccelerationPreference::parse(raw)
                .ok_or_else(|| format!("unknown Whisper acceleration {raw}"))?,
        ),
    };
    Ok(config)
}

fn setting_field<T: Clone>(env: Option<T>, file: Option<T>, default: T) -> SettingField<T> {
    let source = if env.is_some() {
        SettingSource::Env
    } else if file.is_some() {
        SettingSource::File
    } else {
        SettingSource::Default
    };
    SettingField {
        value: file.clone(),
        effective: echo_core::resolve(env, file, default),
        source,
    }
}

fn hud_field(env: Option<&str>, file: Option<bool>) -> SettingField<bool> {
    match env {
        Some("0" | "false" | "off") => SettingField {
            value: file,
            effective: false,
            source: SettingSource::Env,
        },
        Some("1" | "true" | "on") => SettingField {
            value: file,
            effective: true,
            source: SettingSource::Env,
        },
        _ => SettingField {
            value: file,
            effective: file != Some(false),
            source: if file.is_some() {
                SettingSource::File
            } else {
                SettingSource::Default
            },
        },
    }
}

fn record_seconds_field(env: Option<&str>, file: Option<u32>) -> SettingField<u32> {
    let resolved = echo_core::resolve_recording_limit(env, file);
    SettingField {
        value: file,
        effective: resolved.limit.seconds(),
        source: match resolved.source {
            echo_core::RecordingLimitSource::Environment => SettingSource::Env,
            echo_core::RecordingLimitSource::File => SettingSource::File,
            echo_core::RecordingLimitSource::Default => SettingSource::Default,
        },
    }
}

fn engine_name(choice: echo_core::EngineChoice) -> String {
    match choice {
        echo_core::EngineChoice::Whisper => "whisper",
        echo_core::EngineChoice::Parakeet => "parakeet",
        echo_core::EngineChoice::Fake => "fake",
        echo_core::EngineChoice::Auto => "auto",
    }
    .to_string()
}

fn cleanup_name(mode: &echo_core::CleanupMode) -> String {
    match mode {
        echo_core::CleanupMode::Off => "off".to_string(),
        echo_core::CleanupMode::Rules => "rules".to_string(),
        echo_core::CleanupMode::LocalModel { model } => format!("local:{model}"),
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|raw| !raw.is_empty())
}

fn start_recording_thread() -> Result<Option<String>, String> {
    echo::rec::toggle_managed_recording()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ShortcutBackendName {
    Portal,
    X11,
}

impl ShortcutBackendName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Portal => "portal",
            Self::X11 => "x11",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NativeShortcutState {
    Probing,
    Active {
        backend: ShortcutBackendName,
        effective: String,
    },
    PortalAbsent {
        detail: String,
    },
    Failed {
        detail: String,
    },
    Unsupported {
        detail: String,
    },
}

static NATIVE_SHORTCUT_STATE: OnceLock<Arc<Mutex<NativeShortcutState>>> = OnceLock::new();

fn native_state_cell() -> &'static Arc<Mutex<NativeShortcutState>> {
    NATIVE_SHORTCUT_STATE.get_or_init(|| Arc::new(Mutex::new(NativeShortcutState::Probing)))
}

fn native_shortcut_state() -> NativeShortcutState {
    native_state_cell()
        .lock()
        .expect("native shortcut state lock")
        .clone()
}

fn set_native_shortcut_state(state: NativeShortcutState) {
    *native_state_cell()
        .lock()
        .expect("native shortcut state lock") = state;
}

fn is_legacy_registry_absence(error: &ashpd::Error) -> bool {
    matches!(
        error,
        ashpd::Error::PortalNotFound(interface)
            if interface.as_str() == "org.freedesktop.host.portal.Registry"
    )
}

fn is_global_shortcuts_absence(error: &ashpd::Error) -> bool {
    matches!(
        error,
        ashpd::Error::PortalNotFound(interface)
            if interface.as_str() == "org.freedesktop.portal.GlobalShortcuts"
    )
}

struct NativeShortcutHandle {
    cancel: echo::audio::CancellationToken,
    thread: JoinHandle<()>,
}

static NATIVE_SHORTCUT: Mutex<Option<NativeShortcutHandle>> = Mutex::new(None);

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum TestShortcutAction {
    Edge(String, echo::hotkey::HotkeyEvent),
    Toggle,
}

#[cfg(test)]
static TEST_SHORTCUT_ACTIONS: Mutex<Option<std::sync::mpsc::Sender<TestShortcutAction>>> =
    Mutex::new(None);

#[cfg(test)]
struct ShortcutRecordingTestEnv {
    dir: std::path::PathBuf,
    old: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

#[cfg(test)]
impl ShortcutRecordingTestEnv {
    fn start(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("echo-shortcut-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/echo/tests/fixtures/claude_code.wav")
            .into_os_string();
        let values = [
            ("ECHO_DATA_DIR", dir.clone().into_os_string()),
            ("ECHO_AUDIO_FIXTURE", fixture),
            ("ECHO_ENGINE", "fake".into()),
            ("ECHO_SKIP_INJECT", "1".into()),
            ("ECHO_HUD", "0".into()),
        ];
        let old = values
            .into_iter()
            .map(|(key, value)| {
                let old = std::env::var_os(key);
                std::env::set_var(key, value);
                (key, old)
            })
            .collect();
        Self { dir, old }
    }

    fn assert_active(&self) {
        assert!(
            echo::rec::session_active(),
            "recording lock was not acquired"
        );
    }

    fn wait_until_inactive(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while echo::rec::session_active() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !echo::rec::session_active(),
            "recording lock was not released"
        );
    }
}

#[cfg(test)]
impl Drop for ShortcutRecordingTestEnv {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while echo::rec::session_active() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        for (key, old) in self.old.drain(..) {
            if let Some(old) = old {
                std::env::set_var(key, old);
            } else {
                std::env::remove_var(key);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
fn report_test_shortcut(action: TestShortcutAction) -> bool {
    let observer = TEST_SHORTCUT_ACTIONS
        .lock()
        .expect("test shortcut observer lock");
    if let Some(observer) = observer.as_ref() {
        let _ = observer.send(action);
        true
    } else {
        false
    }
}
static NATIVE_RECONCILE: Mutex<()> = Mutex::new(());

/// Reconcile serializes teardown and startup. The old worker is cancelled and
/// joined (which closes a portal session or unregisters X11 grabs) before the
/// replacement can register, while the runtime/status mutexes stay unlocked.
fn reconcile_native_shortcuts() {
    reconcile_native_shortcuts_with_recovery(false, false);
}

fn retry_native_shortcuts_after_failure() {
    reconcile_native_shortcuts_with_recovery(true, true);
}

#[tauri::command]
fn retry_shortcut() -> ShortcutStatus {
    if shortcut_retry_needed(&native_shortcut_state()) {
        reconcile_native_shortcuts_with_recovery(false, true);
    }
    get_shortcut_status()
}

fn shortcut_retry_needed(state: &NativeShortcutState) -> bool {
    !matches!(state, NativeShortcutState::Active { .. })
}

fn reconcile_native_shortcuts_with_recovery(recovering: bool, force: bool) {
    let _reconcile = NATIVE_RECONCILE
        .lock()
        .expect("native shortcut reconcile lock");
    let old = {
        let mut guard = NATIVE_SHORTCUT.lock().expect("native shortcut lock");
        if !force
            && guard
                .as_ref()
                .is_some_and(|running| !running.thread.is_finished())
        {
            return;
        }
        guard.take()
    };
    stop_native_handle(old);

    let session = echo::hotkey::DesktopSession::from_xdg_session_type(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    set_native_shortcut_state(NativeShortcutState::Probing);

    if session == echo::hotkey::DesktopSession::Unknown {
        set_native_shortcut_state(NativeShortcutState::Unsupported {
            detail: "unknown or headless desktop session".to_string(),
        });
        return;
    }

    let cancel = echo::audio::CancellationToken::new();
    let thread_cancel = cancel.clone();
    let spawned = std::thread::Builder::new()
        .name(
            match session {
                echo::hotkey::DesktopSession::Wayland => "echo-shortcuts-portal",
                _ => "echo-shortcuts-x11",
            }
            .to_string(),
        )
        .spawn(move || {
            let active = AtomicBool::new(false);
            let result = panic::catch_unwind(AssertUnwindSafe(|| match session {
                echo::hotkey::DesktopSession::Wayland => {
                    run_portal_shortcuts(&thread_cancel, &active)
                }
                echo::hotkey::DesktopSession::X11 => run_x11_shortcuts(&thread_cancel, &active),
                echo::hotkey::DesktopSession::Unknown => Ok(()),
            }));
            let failure = match result {
                Ok(Ok(())) => None,
                Ok(Err(err)) => Some(err),
                Err(_) => Some("native shortcut listener panicked".to_string()),
            };
            let active = active.load(Ordering::SeqCst);
            if let Some(error) = failure {
                if thread_cancel.is_cancelled() {
                    eprintln!("native shortcuts: listener teardown failed: {error}");
                    return;
                }
                mark_native_failure(error);
                if should_retry_native_listener(active, recovering, false) {
                    if let Err(err) = schedule_native_retry(
                        thread_cancel,
                        Duration::from_secs(1),
                        retry_native_shortcuts_after_failure,
                    ) {
                        eprintln!("native shortcuts: {err}");
                    }
                }
            } else if !active
                && should_retry_native_listener(active, recovering, thread_cancel.is_cancelled())
            {
                if let Err(err) = schedule_native_retry(
                    thread_cancel,
                    Duration::from_secs(1),
                    retry_native_shortcuts_after_failure,
                ) {
                    eprintln!("native shortcuts: {err}");
                }
            }
        });
    match spawned {
        Ok(thread) => {
            *NATIVE_SHORTCUT.lock().expect("native shortcut lock") =
                Some(NativeShortcutHandle { cancel, thread });
        }
        Err(err) => mark_native_failure(format!("cannot spawn native shortcut listener: {err}")),
    }
}

fn stop_native_handle(handle: Option<NativeShortcutHandle>) {
    if let Some(handle) = handle {
        handle.cancel.cancel();
        if handle.thread.join().is_err() {
            mark_native_failure("native shortcut listener panicked during teardown".to_string());
        }
    }
}

fn stop_native_shortcuts() {
    let old = NATIVE_SHORTCUT.lock().expect("native shortcut lock").take();
    stop_native_handle(old);
}

fn mark_native_failure(error: String) {
    eprintln!("native shortcuts: {error}");
    set_native_shortcut_state(NativeShortcutState::Failed { detail: error });
}

fn schedule_native_retry(
    cancel: echo::audio::CancellationToken,
    delay: Duration,
    retry: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("echo-shortcuts-retry".to_string())
        .spawn(move || {
            let deadline = Instant::now() + delay;
            while !cancel.is_cancelled() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    if !cancel.is_cancelled() {
                        retry();
                    }
                    break;
                }
                std::thread::sleep(remaining.min(Duration::from_millis(50)));
            }
        })
        .map(|_| ())
        .map_err(|err| format!("cannot schedule native shortcut retry: {err}"))
}

fn should_retry_native_listener(active: bool, recovering: bool, cancelled: bool) -> bool {
    !cancelled && (active || recovering)
}

fn dispatch_shortcut_edge(
    id: &str,
    edge: echo::hotkey::HotkeyEvent,
    toggle: &mut echo::hotkey::ToggleDriver,
) {
    #[cfg(test)]
    report_test_shortcut(TestShortcutAction::Edge(id.to_string(), edge));
    match id {
        FixedShortcut::ID if toggle.on_edge(edge) => match start_recording_thread() {
            Ok(recording_token) => {
                if let Err(err) = echo::status::mark_shortcut_activation(
                    "native-toggle",
                    recording_token.as_deref(),
                ) {
                    eprintln!("toggle shortcut: cannot record provenance: {err}");
                }
                #[cfg(test)]
                report_test_shortcut(TestShortcutAction::Toggle);
            }
            Err(err) => eprintln!("toggle shortcut: cannot change recording: {err}"),
        },
        _ => {}
    }
}

fn run_x11_shortcuts(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
) -> Result<(), String> {
    let mut toggle = echo::hotkey::ToggleDriver::new();
    let result = run_x11_event_loop(cancel, active, |id, edge| {
        dispatch_shortcut_edge(id, edge, &mut toggle);
    });
    toggle.terminate();
    result
}

fn run_x11_event_loop(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
    mut on_edge: impl FnMut(&'static str, echo::hotkey::HotkeyEvent),
) -> Result<(), String> {
    let decision = echo::hotkey::select_native_backend(echo::hotkey::DesktopSession::X11, None);
    debug_assert_eq!(decision.backend, echo::hotkey::NativeBackend::X11);
    let toggle_key = FixedShortcut::x11_hotkey();

    let manager = GlobalHotKeyManager::new()
        .map_err(|err| format!("cannot create X11 global-hotkey manager: {err}"))?;
    while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
    manager
        .register(toggle_key)
        .map_err(|err| format!("X11 toggle shortcut conflict: {err}"))?;
    set_native_shortcut_state(NativeShortcutState::Active {
        backend: ShortcutBackendName::X11,
        effective: FixedShortcut::DISPLAY.to_string(),
    });
    active.store(true, Ordering::SeqCst);

    while !cancel.is_cancelled() {
        match GlobalHotKeyEvent::receiver().recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                if event.id != toggle_key.id() {
                    continue;
                }
                let edge = match event.state {
                    HotKeyState::Pressed => echo::hotkey::HotkeyEvent::Down,
                    HotKeyState::Released => echo::hotkey::HotkeyEvent::Up,
                };
                on_edge(FixedShortcut::ID, edge);
            }
            Err(err) if err.is_timeout() => {}
            Err(err) => {
                let _ = manager.unregister(toggle_key);
                return Err(format!("X11 shortcut listener terminated: {err}"));
            }
        }
    }
    manager
        .unregister(toggle_key)
        .map_err(|err| format!("cannot unregister X11 shortcut: {err}"))
}

fn run_portal_shortcuts(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("cannot start portal runtime: {err}"))?;
    runtime.block_on(async {
        let connection = ashpd::zbus::Connection::session()
            .await
            .map_err(|err| format!("cannot connect to the session bus: {err}"))?;
        run_portal_shortcuts_async(cancel, active, connection).await
    })
}

async fn run_portal_shortcuts_async(
    cancel: &echo::audio::CancellationToken,
    active: &AtomicBool,
    connection: ashpd::zbus::Connection,
) -> Result<(), String> {
    let app_id = APP_ID
        .parse::<ashpd::AppID>()
        .map_err(|err| format!("invalid portal application id: {err}"))?;
    if let Err(err) = ashpd::register_host_app_with_connection(connection.clone(), app_id).await {
        if is_legacy_registry_absence(&err) {
            eprintln!("native shortcuts: host portal Registry is unavailable; probing legacy GlobalShortcuts support");
        } else {
            return Err(format!("portal host registry handshake failed: {err}"));
        }
    }

    // The Registry attempt above intentionally precedes every portal proxy,
    // session and bind operation. New stacks attribute permissions to APP_ID;
    // legacy stacks without Registry can still expose GlobalShortcuts.
    let portal = match GlobalShortcuts::with_connection(connection).await {
        Ok(portal) => portal,
        Err(err) => {
            let detail = format!("Wayland GlobalShortcuts interface is unavailable: {err}");
            if is_global_shortcuts_absence(&err) {
                set_native_shortcut_state(NativeShortcutState::PortalAbsent { detail });
                return Ok(());
            }
            return Err(detail);
        }
    };
    let decision = echo::hotkey::select_native_backend(
        echo::hotkey::DesktopSession::Wayland,
        Some(portal.version()),
    );
    if decision.backend != echo::hotkey::NativeBackend::Portal {
        set_native_shortcut_state(NativeShortcutState::Unsupported {
            detail: decision
                .reason
                .unwrap_or_else(|| "Wayland GlobalShortcuts interface is unavailable".to_string()),
        });
        return Ok(());
    }
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(|err| format!("cannot create GlobalShortcuts session: {err}"))?;
    let session_path = serde_json::to_value(&session)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or("cannot read GlobalShortcuts session path");
    let session_path = match session_path {
        Ok(path) => path,
        Err(err) => return Err(close_portal_after_failure(&session, err.to_string()).await),
    };
    let mut activated = Box::pin(match portal.receive_activated().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal activations: {err}"),
            )
            .await)
        }
    });
    let mut deactivated = Box::pin(match portal.receive_deactivated().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal deactivations: {err}"),
            )
            .await)
        }
    });
    let mut changed = Box::pin(match portal.receive_shortcuts_changed().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal shortcut changes: {err}"),
            )
            .await)
        }
    });
    let mut closed = Box::pin(match session.receive_closed().await {
        Ok(stream) => stream,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("cannot listen for portal session closure: {err}"),
            )
            .await)
        }
    });

    let shortcuts = [
        NewShortcut::new(FixedShortcut::ID, "Start or stop recording")
            .preferred_trigger(FixedShortcut::PORTAL_TRIGGER),
    ];
    let request = tokio::select! {
        result = portal.bind_shortcuts(
            &session,
            &shortcuts,
            None,
            BindShortcutsOptions::default(),
        ) => match result {
            Ok(request) => request,
            Err(err) => return Err(close_portal_after_failure(
                &session,
                format!("cannot bind portal shortcuts: {err}"),
            ).await),
        },
        () = wait_for_native_cancel(cancel) => {
            session.close().await
                .map_err(|err| format!("cannot close cancelled portal shortcut session: {err}"))?;
            return Ok(());
        }
    };
    let response = match request.response() {
        Ok(response) => response,
        Err(err) => {
            return Err(close_portal_after_failure(
                &session,
                format!("portal shortcut registration was rejected: {err}"),
            )
            .await)
        }
    };
    let effective = match effective_portal_shortcut(response.shortcuts()) {
        Ok(effective) => effective,
        Err(err) => return Err(close_portal_after_failure(&session, err).await),
    };
    set_native_shortcut_state(NativeShortcutState::Active {
        backend: ShortcutBackendName::Portal,
        effective,
    });
    active.store(true, Ordering::SeqCst);

    let mut toggle = echo::hotkey::ToggleDriver::new();
    let listener_error = loop {
        if cancel.is_cancelled() {
            break None;
        }
        tokio::select! {
            event = activated.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    dispatch_shortcut_edge(
                        event.shortcut_id(),
                        echo::hotkey::HotkeyEvent::Down,
                        &mut toggle,
                    );
                }
                Some(_) => {}
                None => break Some("portal Activated listener terminated".to_string()),
            },
            event = deactivated.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    dispatch_shortcut_edge(
                        event.shortcut_id(),
                        echo::hotkey::HotkeyEvent::Up,
                        &mut toggle,
                    );
                }
                Some(_) => {}
                None => break Some("portal Deactivated listener terminated".to_string()),
            },
            event = changed.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    match effective_portal_shortcut(event.shortcuts()) {
                        Ok(effective) => set_native_shortcut_state(NativeShortcutState::Active {
                            backend: ShortcutBackendName::Portal,
                            effective,
                        }),
                        Err(err) => break Some(format!("invalid ShortcutsChanged signal: {err}")),
                    }
                }
                Some(_) => {}
                None => break Some("portal ShortcutsChanged listener terminated".to_string()),
            },
            event = closed.next() => match event {
                Some(_) => break Some("portal shortcut session terminated".to_string()),
                None => break Some("portal session listener terminated".to_string()),
            },
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    };
    toggle.terminate();
    let close_result = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
    match close_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(format!("cannot close portal shortcut session: {err}")),
        Err(_) => return Err("timed out closing portal shortcut session".to_string()),
    }
    if let Some(error) = listener_error {
        return Err(error);
    }
    Ok(())
}

async fn wait_for_native_cancel(cancel: &echo::audio::CancellationToken) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn close_portal_after_failure<T>(
    session: &ashpd::desktop::Session<T>,
    primary: String,
) -> String
where
    T: ashpd::desktop::SessionPortal,
{
    match tokio::time::timeout(Duration::from_secs(2), session.close()).await {
        Ok(Ok(())) => primary,
        Ok(Err(err)) => format!("{primary}; portal session cleanup failed: {err}"),
        Err(_) => format!("{primary}; portal session cleanup timed out"),
    }
}

fn effective_portal_shortcut(shortcuts: &[Shortcut]) -> Result<String, String> {
    shortcuts
        .iter()
        .find(|shortcut| shortcut.id() == FixedShortcut::ID)
        .map(Shortcut::trigger_description)
        .filter(|trigger| !trigger.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "portal did not assign an effective trigger for {}",
                FixedShortcut::ID
            )
        })
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        run_desktop();
    } else {
        std::process::exit(cli::run(args));
    }
}

/// The path and file identity this process was loaded from, recorded at
/// startup so a second launch can tell "same binary" from "replaced by a
/// package upgrade".
struct UpgradeWatch {
    path: std::path::PathBuf,
    identity: echo::upgrade::FileIdentity,
}

fn run_desktop() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.tray_icon = None;
    let result = tauri::Builder::default()
        .manage(setup::SetupService::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let Some(watch) = app.try_state::<UpgradeWatch>() else {
                show_main_window(app);
                return;
            };
            let current = echo::upgrade::file_identity(&watch.path).ok();
            match echo::upgrade::second_launch_decision(watch.identity, current) {
                echo::upgrade::SecondLaunch::Focus => {
                    eprintln!("echo-desktop: second launch; focusing the running window");
                    show_main_window(app);
                }
                echo::upgrade::SecondLaunch::Restart => {
                    // The binary was replaced since this process started.
                    // Hand over to the on-disk build and exit; a failed spawn
                    // falls back to focusing, so an upgrade can never loop.
                    match std::process::Command::new(&watch.path).spawn() {
                        Ok(_) => {
                            eprintln!("echo-desktop: binary changed on disk; restarting into the new build");
                            app.exit(0);
                        }
                        Err(err) => {
                            eprintln!("echo-desktop: restart spawn failed: {err}");
                            show_main_window(app);
                        }
                    }
                }
            }
        }))
        .setup(|app| {
            // Old Echo processes predate the single-instance gate; without a
            // takeover, a new launch coexists with them and the upgrade looks
            // like it never happened. Runs after the gate admitted us, before
            // the tray is built.
            echo::upgrade::terminate_old_echo_processes();
            let open = MenuItem::with_id(app, "open", "Open Echo", true, None::<&str>)?;
            let record =
                MenuItem::with_id(app, "record", "Start / stop recording", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &record, &quit])?;
            let icon = Image::from_bytes(include_bytes!("../icons/tray-24.png"))
                .expect("tray-24.png decodes as RGBA");
            let tray = TrayIconBuilder::new()
                .menu(&menu)
                .icon(icon)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "record" => {
                        let _ = start_recording_thread();
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            let tray_ready = match panic::catch_unwind(AssertUnwindSafe(|| tray.build(app))) {
                Ok(Ok(_)) => true,
                Ok(Err(err)) => {
                    eprintln!("tray icon: {err}");
                    false
                }
                Err(_) => {
                    eprintln!("tray icon: libayatana-appindicator failed to load");
                    false
                }
            };
            app.manage(AtomicBool::new(tray_ready));
            reconcile_native_shortcuts();
            if let Ok(path) = std::env::current_exe().and_then(|path| path.canonicalize()) {
                if let Ok(identity) = echo::upgrade::file_identity(&path) {
                    app.manage(UpgradeWatch { path, identity });
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<AtomicBool>().load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            get_shortcut_status,
            retry_shortcut,
            repair_legacy_shortcut,
            get_history,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            toggle_recording,
            stop_recording,
            get_recording_level,
            copy_text,
            remove_stale_installs,
            get_settings,
            set_settings,
            list_models,
            list_languages,
            setup::get_readiness,
            setup::start_setup,
            setup::repair_managed,
            setup::verify_managed,
            setup::remove_managed,
            setup::cancel_setup,
            get_microphones,
            set_microphone,
            test_input_device,
            test_microphone_fallback,
        ])
        .run(context);
    stop_native_shortcuts();
    result.expect("error while running Echo");
}

#[cfg(test)]
mod portal_runtime_tests;

#[cfg(test)]
mod settings_tests {
    use super::*;
    use echo_core::{Config, EngineChoice};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch_path(label: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "echo-settings-ipc-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    fn custom_binding(path: &str, name: &str, command: &str, binding: &str) -> GnomeCustomBinding {
        GnomeCustomBinding {
            path: path.to_string(),
            name: name.to_string(),
            command: command.to_string(),
            binding: binding.to_string(),
        }
    }

    fn invoke_test_command(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
    ) -> serde_json::Value {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: command.to_string(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::default(),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .unwrap_or_else(|error| panic!("{command} IPC failed: {error}"))
        .deserialize()
        .unwrap()
    }

    #[test]
    fn gnome_accelerators_and_commands_are_stable() {
        assert_eq!(FixedShortcut::DISPLAY, "Super+Alt+Space");
        assert_eq!(FixedShortcut::GNOME_ACCELERATOR, "<Super><Alt>space");
        assert_eq!(FixedShortcut::PORTAL_TRIGGER, "LOGO+ALT+space");
        assert_eq!(
            absolute_toggle_command("/usr/bin/echo-desktop").unwrap(),
            "/usr/bin/echo-desktop rec --toggle"
        );
        assert_eq!(
            absolute_toggle_command("/opt/Echo App/echo-desktop").unwrap(),
            "'/opt/Echo App/echo-desktop' rec --toggle"
        );
        assert_eq!(
            absolute_toggle_command(r"/opt/Echo\ App/echo-desktop").unwrap(),
            r"'/opt/Echo\ App/echo-desktop' rec --toggle"
        );
        assert_eq!(
            absolute_toggle_command("/opt/Echo's/echo-desktop").unwrap(),
            "'/opt/Echo'\\''s/echo-desktop' rec --toggle"
        );
        assert!(absolute_toggle_command("echo-desktop").is_err());
        assert_eq!(
            stable_shortcut_executable(
                "/tmp/.mount_echo/usr/bin/echo-desktop",
                Some(std::ffi::OsStr::new("/home/user/Echo.AppImage")),
            ),
            "/home/user/Echo.AppImage"
        );
        assert_eq!(
            stable_shortcut_executable(
                "/usr/bin/echo-desktop",
                Some(std::ffi::OsStr::new("relative.AppImage")),
            ),
            "/usr/bin/echo-desktop"
        );
    }

    #[test]
    fn echo_command_ownership_requires_an_exact_safe_invocation() {
        let desired = "/usr/bin/echo-desktop rec --toggle";
        assert!(echo_toggle_command(desired, desired));
        assert!(echo_toggle_command(
            "/usr/bin/env PATH=/usr/bin ECHO_ENGINE=whisper /home/user/.local/bin/echo-app rec --toggle",
            desired
        ));
        assert!(echo_toggle_command(
            "'/opt/Echo App/echo-desktop' rec --toggle",
            desired
        ));
        assert!(echo_toggle_command(
            "'/opt/Echo'\\''s/echo-app' rec --toggle",
            desired
        ));
        assert!(!echo_toggle_command(
            "/tmp/not-echo-desktop rec --toggle",
            desired
        ));
        assert!(!echo_toggle_command(
            "/usr/bin/echo-desktop rec --toggle; rm -rf /",
            desired
        ));
        assert!(!echo_toggle_command(
            "/usr/bin/env LD_PRELOAD=/tmp/inject.so /usr/bin/echo-app rec --toggle",
            desired
        ));
        assert!(!echo_toggle_command(
            "'/opt/Echo App/echo-desktop rec --toggle",
            desired
        ));
    }

    #[test]
    fn gnome_accelerator_comparison_is_semantic() {
        assert!(gnome_accelerators_match(
            "<Primary><Mod1>space",
            "<Ctrl><Alt>space"
        ));
        assert!(gnome_accelerators_match(
            "<mod4><alt>Return",
            "<Super><Alt>enter"
        ));
        assert!(gnome_accelerators_match(
            "<Alt><Super>space",
            FixedShortcut::GNOME_ACCELERATOR
        ));
        assert!(!gnome_accelerators_match(
            "<Super><Alt>space",
            "<Super><Alt>Return"
        ));
    }

    #[test]
    fn gnome_shortcut_classifies_missing_stale_conflicting_and_ready() {
        let command = "/usr/bin/echo-desktop rec --toggle";
        let binding = "<Super><Alt>space";
        let empty_target = custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", "");
        let missing = GnomeShortcutSnapshot {
            paths: vec![],
            bindings: vec![empty_target.clone()],
        };
        assert_eq!(
            classify_gnome_shortcut(&missing, command, binding).state,
            LegacyShortcutState::Missing
        );

        let stale = GnomeShortcutSnapshot {
            paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
            bindings: vec![custom_binding(
                ECHO_CUSTOM_KEY_PATH,
                ECHO_CUSTOM_KEY_NAME,
                "/home/user/.local/bin/echo-app rec --toggle",
                binding,
            )],
        };
        assert_eq!(
            classify_gnome_shortcut(&stale, command, binding).state,
            LegacyShortcutState::Stale
        );

        let reserved = GnomeShortcutSnapshot {
            paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
            bindings: vec![custom_binding(
                ECHO_CUSTOM_KEY_PATH,
                "Unrelated action",
                "other-command",
                binding,
            )],
        };
        assert_eq!(
            classify_gnome_shortcut(&reserved, command, binding).state,
            LegacyShortcutState::Conflicting
        );
        let commandeered = GnomeShortcutSnapshot {
            paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
            bindings: vec![custom_binding(
                ECHO_CUSTOM_KEY_PATH,
                ECHO_CUSTOM_KEY_NAME,
                "unrelated-command --dangerous",
                binding,
            )],
        };
        assert_eq!(
            classify_gnome_shortcut(&commandeered, command, binding).state,
            LegacyShortcutState::Conflicting
        );

        let other_path =
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/";
        let collision = GnomeShortcutSnapshot {
            paths: vec![other_path.to_string()],
            bindings: vec![
                empty_target,
                custom_binding(other_path, "Other", "other-command", binding),
            ],
        };
        assert_eq!(
            classify_gnome_shortcut(&collision, command, binding).state,
            LegacyShortcutState::Conflicting
        );
        let semantic_collision = GnomeShortcutSnapshot {
            paths: vec![other_path.to_string()],
            bindings: vec![
                custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", ""),
                custom_binding(other_path, "Other", "other-command", "<Mod4><Mod1>space"),
            ],
        };
        assert_eq!(
            classify_gnome_shortcut(&semantic_collision, command, binding).state,
            LegacyShortcutState::Conflicting
        );

        let ready = GnomeShortcutSnapshot {
            paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
            bindings: vec![custom_binding(
                ECHO_CUSTOM_KEY_PATH,
                ECHO_CUSTOM_KEY_NAME,
                command,
                binding,
            )],
        };
        assert_eq!(
            classify_gnome_shortcut(&ready, command, binding).state,
            LegacyShortcutState::Ready
        );
    }

    #[test]
    fn gnome_repair_is_explicit_idempotent_and_preserves_unrelated_paths() {
        let other_path =
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/";
        let snapshot = GnomeShortcutSnapshot {
            paths: vec![other_path.to_string()],
            bindings: vec![
                custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", ""),
                custom_binding(other_path, "Terminal", "kgx", "<Ctrl><Alt>t"),
            ],
        };
        let setup = classify_gnome_shortcut(
            &snapshot,
            "/usr/bin/echo-desktop rec --toggle",
            "<Super><Alt>space",
        );
        let writes = gnome_repair_writes(&setup, &snapshot.paths).unwrap();
        assert_eq!(writes.len(), 4);
        assert!(writes.iter().all(|write| {
            write.schema == GNOME_MEDIA_KEYS_SCHEMA || write.schema.ends_with(ECHO_CUSTOM_KEY_PATH)
        }));
        assert!(writes
            .last()
            .expect("path-list write")
            .value
            .contains(other_path));
        let keyfile = dconf_keyfile(&writes).unwrap();
        assert!(keyfile.contains("[/]\ncustom-keybindings="));
        assert!(keyfile.contains("[custom-keybindings/echo]\n"));
        let concurrent_path =
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/concurrent/";
        assert_eq!(
            with_echo_shortcut_path(vec![other_path.to_string(), concurrent_path.to_string()]),
            vec![
                other_path.to_string(),
                concurrent_path.to_string(),
                ECHO_CUSTOM_KEY_PATH.to_string()
            ]
        );
        assert_eq!(
            with_echo_shortcut_path(vec![ECHO_CUSTOM_KEY_PATH.to_string()]),
            vec![ECHO_CUSTOM_KEY_PATH.to_string()]
        );
        let mut changed = snapshot.clone();
        changed.paths.push(concurrent_path.to_string());
        assert!(gnome_repair_transaction(&snapshot, &changed, &setup).is_err());
        let mut commandeered = snapshot.clone();
        commandeered.bindings[0].command = "other-command".to_string();
        assert!(gnome_repair_transaction(&snapshot, &commandeered, &setup).is_err());
        assert_eq!(
            gnome_repair_transaction(&snapshot, &snapshot, &setup).unwrap(),
            writes
        );

        let ready = LegacyShortcutSetup {
            state: LegacyShortcutState::Ready,
            detail: String::new(),
            command: setup.command,
            binding: setup.binding,
        };
        assert!(gnome_repair_writes(&ready, &snapshot.paths)
            .unwrap()
            .is_empty());
        let conflict = LegacyShortcutSetup {
            state: LegacyShortcutState::Conflicting,
            detail: "occupied".to_string(),
            command: ready.command,
            binding: ready.binding,
        };
        assert!(gnome_repair_writes(&conflict, &snapshot.paths).is_err());
    }

    #[test]
    fn gnome_repair_restores_an_empty_active_echo_slot() {
        let command = "/usr/bin/echo-desktop rec --toggle";
        let binding = "<Super><Alt>space";
        let snapshot = GnomeShortcutSnapshot {
            paths: vec![ECHO_CUSTOM_KEY_PATH.to_string()],
            bindings: vec![custom_binding(ECHO_CUSTOM_KEY_PATH, "", "", "")],
        };
        let setup = classify_gnome_shortcut(&snapshot, command, binding);
        assert_eq!(setup.state, LegacyShortcutState::Stale);

        let writes = gnome_repair_writes(&setup, &snapshot.paths).unwrap();
        assert_eq!(writes.len(), 3);
        assert!(writes.iter().any(|write| {
            write.key == "name" && write.value == gvariant_string(ECHO_CUSTOM_KEY_NAME)
        }));
        let repaired = GnomeShortcutSnapshot {
            paths: snapshot.paths,
            bindings: vec![custom_binding(
                ECHO_CUSTOM_KEY_PATH,
                ECHO_CUSTOM_KEY_NAME,
                command,
                binding,
            )],
        };
        assert_eq!(
            classify_gnome_shortcut(&repaired, command, binding).state,
            LegacyShortcutState::Ready
        );
    }

    #[test]
    #[ignore = "explicitly repairs the current GNOME user's confirmed Echo shortcut"]
    fn legacy_wayland_host_repairs_only_the_echo_owned_binding() {
        assert_eq!(
            echo::hotkey::DesktopSession::from_xdg_session_type(
                env::var("XDG_SESSION_TYPE").ok().as_deref()
            ),
            echo::hotkey::DesktopSession::Wayland
        );
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        assert!(desktop.to_ascii_lowercase().contains("gnome"));
        let command = "/usr/bin/echo-desktop rec --toggle";
        assert!(std::path::Path::new("/usr/bin/echo-desktop").is_file());
        let binding = FixedShortcut::GNOME_ACCELERATOR.to_string();
        set_native_shortcut_state(NativeShortcutState::Probing);
        let active = AtomicBool::new(false);
        run_portal_shortcuts(&echo::audio::CancellationToken::new(), &active).unwrap();
        assert!(matches!(
            native_shortcut_state(),
            NativeShortcutState::PortalAbsent { .. }
        ));

        let before = read_gnome_shortcuts().unwrap();
        let occupied = before
            .bindings
            .iter()
            .find(|entry| entry.path == ECHO_CUSTOM_KEY_PATH)
            .expect("Echo reserved shortcut slot");
        assert_eq!(occupied.name, ECHO_CUSTOM_KEY_NAME);
        assert!(echo_toggle_command(&occupied.command, command));
        let unrelated_before = before
            .bindings
            .iter()
            .filter(|entry| {
                before.paths.contains(&entry.path) && entry.path != ECHO_CUSTOM_KEY_PATH
            })
            .cloned()
            .collect::<Vec<_>>();

        let schema = format!("{GNOME_CUSTOM_KEY_SCHEMA}:{ECHO_CUSTOM_KEY_PATH}");
        apply_gsettings_writes(&[GsettingsWrite {
            schema,
            key: "command",
            value: gvariant_string("/usr/bin/echo-app rec --toggle"),
        }])
        .unwrap();
        *LEGACY_SHORTCUT_CACHE
            .lock()
            .expect("legacy shortcut cache lock") = None;
        let health = Health {
            microphone_ready: false,
            engine_name: String::new(),
            engine_ready: false,
            injection_name: String::new(),
            injection_ready: false,
            current_exe: "/usr/bin/echo-desktop".to_string(),
            first_path_hit: None,
            stale_installs: Vec::new(),
        };
        *HEALTH.lock().expect("health cache lock") = Some((Instant::now(), health));
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![
                get_app_status,
                repair_legacy_shortcut
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let stale = invoke_test_command(&webview, "get_app_status");
        eprintln!(
            "observed GNOME Echo shortcut state: {}",
            stale["shortcut"]["setup"]["state"]
        );
        assert_eq!(stale["shortcut"]["setup"]["state"], "stale");

        let repaired = invoke_test_command(&webview, "repair_legacy_shortcut");
        assert_eq!(repaired["state"], "ready");
        let ready = invoke_test_command(&webview, "get_app_status");
        assert_eq!(ready["shortcut"]["kind"], "gnome-ready");
        assert_eq!(ready["shortcut"]["effective"], FixedShortcut::DISPLAY);
        assert_eq!(
            invoke_test_command(&webview, "repair_legacy_shortcut")["state"],
            "ready"
        );

        let after = read_gnome_shortcuts().unwrap();
        assert_eq!(
            classify_gnome_shortcut(&after, command, &binding).state,
            LegacyShortcutState::Ready
        );
        let unrelated_after = after
            .bindings
            .iter()
            .filter(|entry| after.paths.contains(&entry.path) && entry.path != ECHO_CUSTOM_KEY_PATH)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(unrelated_after, unrelated_before);
    }

    #[test]
    fn env_beats_file_for_engine_source() {
        let env = SettingsEnv {
            engine: Some("whisper".into()),
            ..SettingsEnv::default()
        };
        let file = Config {
            engine: Some(EngineChoice::Fake),
            ..Config::default()
        };
        let settings = settings_from(&env, &file, "en").unwrap();
        assert_eq!(settings.engine.value.as_deref(), Some("fake"));
        assert_eq!(settings.engine.effective, "whisper");
        assert_eq!(settings.engine.source, SettingSource::Env);
    }

    #[test]
    fn write_then_read_round_trips_file_values() {
        let path = scratch_path("roundtrip");
        let incoming = Settings {
            engine: SettingField {
                value: Some("parakeet".into()),
                effective: "auto".into(),
                source: SettingSource::Default,
            },
            whisper_model: SettingField {
                value: Some("tiny.en".into()),
                effective: "base.en".into(),
                source: SettingSource::Default,
            },
            cleanup: SettingField {
                value: Some("off".into()),
                effective: "rules".into(),
                source: SettingSource::Default,
            },
            hud: SettingField {
                value: Some(false),
                effective: true,
                source: SettingSource::Default,
            },
            record_seconds: SettingField {
                value: Some(8),
                effective: 3,
                source: SettingSource::Default,
            },
            language: SettingField {
                value: Some("de".into()),
                effective: "en".into(),
                source: SettingSource::Default,
            },
            whisper_acceleration: SettingField {
                value: Some("gpu".into()),
                effective: "cpu".into(),
                source: SettingSource::Default,
            },
        };
        config_from_values(&incoming)
            .unwrap()
            .save_to(&path)
            .unwrap();
        let loaded = Config::load_from(&path).unwrap();
        let got = settings_from(&SettingsEnv::default(), &loaded, "en").unwrap();
        assert_eq!(got.engine.value.as_deref(), Some("parakeet"));
        assert_eq!(got.engine.effective, "parakeet");
        assert_eq!(got.engine.source, SettingSource::File);
        assert_eq!(got.whisper_model.value.as_deref(), Some("tiny.en"));
        assert_eq!(got.whisper_model.effective, "tiny.en");
        assert_eq!(got.cleanup.value.as_deref(), Some("off"));
        assert_eq!(got.cleanup.effective, "off");
        assert_eq!(got.hud.value, Some(false));
        assert!(!got.hud.effective);
        assert_eq!(got.record_seconds.value, Some(8));
        assert_eq!(got.record_seconds.effective, 8);
        assert_eq!(got.record_seconds.source, SettingSource::File);
        assert_eq!(got.language.value.as_deref(), Some("de"));
        assert_eq!(got.language.effective, "de");
        assert_eq!(got.language.source, SettingSource::File);
        assert_eq!(got.whisper_acceleration.value.as_deref(), Some("auto"));
        assert_eq!(got.whisper_acceleration.effective, "auto");
        assert_eq!(got.whisper_acceleration.source, SettingSource::File);
    }

    #[test]
    fn dedicated_microphone_update_writes_id_and_clears_legacy_name() {
        let mut config = Config {
            microphone: Some(echo_core::MicrophoneSelection::LegacyName {
                name: "USB Mic".into(),
            }),
            ..Config::default()
        };
        update_microphone_config(
            &mut config,
            Some((
                echo::microphone::MicrophoneId::parse("alsa:usb-one").unwrap(),
                "USB Mic".into(),
            )),
        );
        assert_eq!(
            config.microphone,
            Some(echo_core::MicrophoneSelection::Device {
                id: "alsa:usb-one".into(),
                last_seen_label: "USB Mic".into(),
            })
        );
        update_microphone_config(&mut config, None);
        assert_eq!(config.microphone, None);
    }

    #[test]
    fn settings_patch_preserves_concurrently_owned_microphone_field() {
        let microphone = echo_core::MicrophoneSelection::Device {
            id: "alsa:buds".into(),
            last_seen_label: "Earbuds".into(),
        };
        let base = Config {
            microphone: Some(microphone.clone()),
            ..Config::default()
        };
        let incoming = settings_from(&SettingsEnv::default(), &base, "en").unwrap();
        let updated = config_from_values_with_base(&incoming, base).unwrap();
        assert_eq!(updated.microphone, Some(microphone));
    }

    #[test]
    fn language_defaults_to_english_and_env_wins() {
        let settings = settings_from(&SettingsEnv::default(), &Config::default(), "en").unwrap();
        assert_eq!(settings.language.value, None);
        assert_eq!(settings.language.effective, "en");
        assert_eq!(settings.language.source, SettingSource::Default);

        let env = SettingsEnv {
            language: Some("auto".into()),
            ..SettingsEnv::default()
        };
        let file = Config {
            language: Some(echo_core::LanguageChoice::Pinned(
                echo_core::Language::from_code("de").unwrap(),
            )),
            ..Config::default()
        };
        let settings = settings_from(&env, &file, "en").unwrap();
        assert_eq!(settings.language.value.as_deref(), Some("de"));
        assert_eq!(settings.language.effective, "auto");
        assert_eq!(settings.language.source, SettingSource::Env);

        let invalid = SettingsEnv {
            language: Some("klingon".into()),
            ..SettingsEnv::default()
        };
        let settings = settings_from(&invalid, &file, "en").unwrap();
        assert_eq!(settings.language.effective, "de");
        assert_eq!(settings.language.source, SettingSource::File);
    }

    #[test]
    fn recording_policy_projects_defaults_presets_and_compatibility_values() {
        let policy = recording_policy_dto();
        let serialized = serde_json::to_value(&policy).unwrap();
        assert_eq!(serialized["minimumSeconds"], 1);
        assert_eq!(serialized["defaultSeconds"], 600);
        assert_eq!(serialized["maximumSeconds"], 600);
        assert_eq!(
            serialized["presetsSeconds"],
            serde_json::json!([30, 60, 120, 300, 600])
        );

        let defaults = record_seconds_field(None, None);
        assert_eq!(defaults.effective, 600);
        assert_eq!(defaults.source, SettingSource::Default);

        let custom = record_seconds_field(None, Some(90));
        assert_eq!(custom.effective, 90);
        assert_eq!(custom.source, SettingSource::File);

        let invalid = record_seconds_field(Some("invalid"), Some(61));
        assert_eq!(invalid.effective, 61);
        assert_eq!(invalid.source, SettingSource::File);

        let env = SettingsEnv {
            record_seconds: Some(((u32::MAX as u64) + 1).to_string()),
            ..SettingsEnv::default()
        };
        let file = Config {
            record_seconds: Some(12),
            ..Config::default()
        };
        let settings = settings_from(&env, &file, "en").unwrap();
        assert_eq!(settings.record_seconds.value, Some(12));
        assert_eq!(settings.record_seconds.effective, 600);
        assert_eq!(settings.record_seconds.source, SettingSource::Env);

        let mut incoming = settings;
        incoming.record_seconds.value = Some(u32::MAX);
        assert_eq!(
            config_from_values_with_base(&incoming, Config::default())
                .unwrap()
                .record_seconds,
            Some(600)
        );
    }

    #[test]
    fn active_recording_limit_snapshot_wins_over_current_settings() {
        let active = echo::status::Status {
            state: "Recording".to_string(),
            last: None,
            error: None,
            recording_limit: echo_core::RecordingLimit::new(120),
        };
        assert_eq!(
            project_recording_limit(&active, echo_core::RecordingLimit::MAX)
                .map(echo_core::RecordingLimit::seconds),
            Some(120)
        );

        let legacy = echo::status::Status {
            recording_limit: None,
            ..active.clone()
        };
        assert_eq!(
            project_recording_limit(&legacy, echo_core::RecordingLimit::MAX),
            None
        );

        let idle = echo::status::Status {
            state: "Idle".to_string(),
            ..active
        };
        assert_eq!(
            project_recording_limit(&idle, echo_core::RecordingLimit::MAX)
                .map(echo_core::RecordingLimit::seconds),
            Some(600)
        );
    }

    #[test]
    fn hud_enable_tokens_override_file_false() {
        for token in ["1", "true", "on"] {
            let env = SettingsEnv {
                hud: Some(token.into()),
                ..SettingsEnv::default()
            };
            let file = Config {
                hud: Some(false),
                ..Config::default()
            };
            let settings = settings_from(&env, &file, "en").unwrap();
            assert_eq!(settings.hud.value, Some(false), "token {token}");
            assert!(settings.hud.effective, "token {token}");
            assert_eq!(settings.hud.source, SettingSource::Env, "token {token}");
        }
    }

    #[test]
    fn hud_off_tokens_disable_and_unknown_consults_file() {
        let disabled = Config {
            hud: Some(false),
            ..Config::default()
        };
        let enabled = Config {
            hud: Some(true),
            ..Config::default()
        };
        for token in ["0", "false", "off"] {
            let env = SettingsEnv {
                hud: Some(token.into()),
                ..SettingsEnv::default()
            };
            let settings = settings_from(&env, &enabled, "en").unwrap();
            assert!(!settings.hud.effective, "token {token}");
            assert_eq!(settings.hud.source, SettingSource::Env, "token {token}");
        }
        let unknown = SettingsEnv {
            hud: Some("maybe".into()),
            ..SettingsEnv::default()
        };
        assert!(
            !settings_from(&unknown, &disabled, "en")
                .unwrap()
                .hud
                .effective
        );
        assert_eq!(
            settings_from(&unknown, &disabled, "en").unwrap().hud.source,
            SettingSource::File
        );
        assert!(
            settings_from(&unknown, &enabled, "en")
                .unwrap()
                .hud
                .effective
        );
    }

    #[test]
    fn fixed_reconcile_is_idempotent() {
        #[derive(Default)]
        struct FakeAdapter {
            running: bool,
            calls: Vec<&'static str>,
        }

        fn reconcile(adapter: &mut FakeAdapter) {
            if adapter.running {
                return;
            }
            adapter.calls.push("register");
            adapter.running = true;
        }

        let mut adapter = FakeAdapter::default();
        reconcile(&mut adapter);
        reconcile(&mut adapter);
        assert_eq!(adapter.calls, ["register"]);
    }

    #[test]
    fn portal_effective_trigger_is_distinct_from_fixed_policy() {
        let state = NativeShortcutState::Active {
            backend: ShortcutBackendName::Portal,
            effective: "Alt+F8".to_string(),
        };
        let projected = project_shortcut_status(&state, "/usr/bin/echo-desktop");
        let serialized = serde_json::to_value(&projected).unwrap();
        assert_eq!(serialized["verificationIdentity"], "portal:Alt+F8");
        assert!(serialized.get("verification_identity").is_none());
        assert!(matches!(
            projected,
            ShortcutStatus::Active {
                desired,
                effective,
                verification_identity,
                ..
            } if desired == FixedShortcut::DISPLAY
                && effective == "Alt+F8"
                && verification_identity == "portal:Alt+F8"
        ));
    }

    #[test]
    fn fixed_native_policy_has_one_backend_value_per_surface() {
        let hotkey = FixedShortcut::x11_hotkey();
        assert_eq!(hotkey.key, Code::Space);
        assert!(hotkey.mods.contains(Modifiers::SUPER));
        assert!(hotkey.mods.contains(Modifiers::ALT));
        assert_eq!(FixedShortcut::PORTAL_TRIGGER, "LOGO+ALT+space");
    }

    #[test]
    fn native_retry_runs_after_delay_unless_cancelled() {
        assert!(!shortcut_retry_needed(&NativeShortcutState::Active {
            backend: ShortcutBackendName::X11,
            effective: FixedShortcut::DISPLAY.to_string(),
        }));
        assert!(shortcut_retry_needed(&NativeShortcutState::Failed {
            detail: "listener stopped".to_string(),
        }));
        assert!(!should_retry_native_listener(false, false, false));
        assert!(should_retry_native_listener(true, false, false));
        assert!(should_retry_native_listener(false, true, false));
        assert!(!should_retry_native_listener(true, true, true));

        let (send, receive) = std::sync::mpsc::channel();
        schedule_native_retry(
            echo::audio::CancellationToken::new(),
            Duration::from_millis(20),
            move || send.send(()).unwrap(),
        )
        .unwrap();
        receive
            .recv_timeout(Duration::from_secs(1))
            .expect("native retry callback");

        let cancel = echo::audio::CancellationToken::new();
        cancel.cancel();
        let (send, receive) = std::sync::mpsc::channel();
        schedule_native_retry(cancel, Duration::from_millis(20), move || {
            send.send(()).unwrap()
        })
        .unwrap();
        assert!(receive.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn only_confirmed_global_shortcuts_absence_enables_legacy_setup() {
        let registry_missing = ashpd::Error::PortalNotFound(
            "org.freedesktop.host.portal.Registry".try_into().unwrap(),
        );
        let shortcuts_missing = ashpd::Error::PortalNotFound(
            "org.freedesktop.portal.GlobalShortcuts".try_into().unwrap(),
        );
        assert!(is_legacy_registry_absence(&registry_missing));
        assert!(!is_legacy_registry_absence(&shortcuts_missing));
        assert!(!is_legacy_registry_absence(&ashpd::Error::InvalidAppID));
        assert!(!is_global_shortcuts_absence(&registry_missing));
        assert!(is_global_shortcuts_absence(&shortcuts_missing));
        assert!(!is_global_shortcuts_absence(&ashpd::Error::InvalidAppID));

        let unavailable = NativeShortcutState::Failed {
            detail: "registration failed".to_string(),
        };
        assert!(!needs_legacy_setup(
            &unavailable,
            echo::hotkey::DesktopSession::Wayland
        ));
        let absent = NativeShortcutState::PortalAbsent {
            detail: "portal absent".to_string(),
        };
        assert!(needs_legacy_setup(
            &absent,
            echo::hotkey::DesktopSession::Wayland
        ));
        assert!(!needs_legacy_setup(
            &absent,
            echo::hotkey::DesktopSession::X11
        ));
    }

    #[test]
    #[ignore = "needs an isolated X11 display"]
    fn x11_runtime_registers_and_releases_the_fixed_grab() {
        set_native_shortcut_state(NativeShortcutState::Probing);
        let cancel = echo::audio::CancellationToken::new();
        let listener_cancel = cancel.clone();
        let active = Arc::new(AtomicBool::new(false));
        let listener_active = active.clone();
        let listener =
            std::thread::spawn(move || run_x11_shortcuts(&listener_cancel, &listener_active));

        let deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(native_shortcut_state(), NativeShortcutState::Active { .. })
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            matches!(native_shortcut_state(), NativeShortcutState::Active { .. }),
            "X11 listener did not become healthy"
        );
        assert!(active.load(Ordering::SeqCst));

        let competing = GlobalHotKeyManager::new().unwrap();
        assert!(
            competing.register(FixedShortcut::x11_hotkey()).is_err(),
            "a competing X11 grab should be rejected"
        );

        cancel.cancel();
        listener.join().unwrap().unwrap();
        let after = GlobalHotKeyManager::new().unwrap();
        let released = FixedShortcut::x11_hotkey();
        let deadline = Instant::now() + Duration::from_secs(1);
        while let Err(err) = after.register(released) {
            assert!(
                Instant::now() < deadline,
                "Echo's toggle grab was not released: {err}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        after.unregister(released).unwrap();
    }

    #[test]
    fn last_run_performance_projects_split_whisper_detail() {
        let detail = RunDetail {
            whisper: Some(echo_core::WhisperRunTelemetry {
                mode: WhisperRunMode::ColdFallback,
                total_ms: 1_230,
                audio_encode_ms: 10,
                parse_ms: 4,
                runtime: echo_core::WhisperRuntimeTelemetry {
                    binary: "/usr/bin/whisper-cli".to_string(),
                    source: WhisperRuntimeSource::System,
                    backend: WhisperRuntimeBackend::Cpu,
                    device: Some("Test CPU".to_string()),
                    library_path: None,
                    vulkan_driver_files: None,
                    mesa_shader_cache_dir: None,
                    identity_sha256: None,
                    vulkan_receipt: None,
                },
                tuning: WhisperTuningTelemetry {
                    threads: Some(4),
                    beam_size: Some(5),
                    best_of: Some(5),
                    no_fallback: Some(false),
                },
                attempts: vec![
                    echo_core::WhisperAttemptTelemetry {
                        vad: true,
                        process_start_ms: 1,
                        child_wall_ms: 500,
                        success: false,
                        exit_code: Some(1),
                        retry_reason: Some(echo_core::WhisperRetryReason::VadRejected),
                    },
                    echo_core::WhisperAttemptTelemetry {
                        vad: false,
                        process_start_ms: 1,
                        child_wall_ms: 710,
                        success: true,
                        exit_code: Some(0),
                        retry_reason: None,
                    },
                ],
                recovery: None,
                selection: None,
            }),
            ..RunDetail::default()
        };
        let projected = project_last_run_performance(&detail).unwrap();
        assert_eq!(projected.mode, WhisperRunMode::ColdFallback);
        assert_eq!(projected.child_wall_ms, 1_210);
        assert_eq!(projected.attempt_count, 2);
        assert_eq!(projected.tuning.threads, Some(4));
        assert_eq!(projected.device.as_deref(), Some("Test CPU"));
    }

    #[test]
    #[ignore = "needs nested Xephyr, xmessage, xdotool, and ydotool"]
    fn x11_runtime_routes_press_and_release_while_another_app_is_focused() {
        struct ChildGuard(std::process::Child);

        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let recording_env = ShortcutRecordingTestEnv::start("x11-routing");
        let title = format!("Echo X11 focus probe {}", std::process::id());
        let _other_app = ChildGuard(
            std::process::Command::new("xmessage")
                .args(["-title", &title, "Shortcut focus probe"])
                .spawn()
                .expect("start focus probe"),
        );
        let search = std::process::Command::new("xdotool")
            .args(["search", "--sync", "--name", &title])
            .output()
            .expect("find focus probe");
        assert!(search.status.success());
        let window = String::from_utf8(search.stdout)
            .unwrap()
            .lines()
            .next()
            .expect("focus probe window")
            .to_string();
        assert!(std::process::Command::new("xdotool")
            .args(["windowfocus", &window])
            .status()
            .expect("focus other app")
            .success());
        let focused = std::process::Command::new("xdotool")
            .arg("getwindowfocus")
            .output()
            .expect("read focused window");
        assert_eq!(String::from_utf8(focused.stdout).unwrap().trim(), window);

        set_native_shortcut_state(NativeShortcutState::Probing);
        let cancel = echo::audio::CancellationToken::new();
        let listener_cancel = cancel.clone();
        let (actions, received) = std::sync::mpsc::channel();
        *TEST_SHORTCUT_ACTIONS
            .lock()
            .expect("test shortcut observer lock") = Some(actions);
        let listener = std::thread::spawn(move || {
            let active = AtomicBool::new(false);
            run_x11_shortcuts(&listener_cancel, &active)
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(native_shortcut_state(), NativeShortcutState::Active { .. })
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            matches!(native_shortcut_state(), NativeShortcutState::Active { .. }),
            "X11 listener not ready"
        );

        let send_shortcut = |shortcut: &str| {
            assert!(std::process::Command::new("ydotool")
                .args(["key", "--key-delay", "50", shortcut])
                .status()
                .expect("send hardware-level X11 shortcut")
                .success());
        };
        let receive_expected = |expected| {
            let event = received
                .recv_timeout(Duration::from_secs(2))
                .expect("routed X11 event");
            assert_eq!(event, expected);
        };

        send_shortcut("super+alt+space");
        for expected in [
            TestShortcutAction::Edge(
                FixedShortcut::ID.to_string(),
                echo::hotkey::HotkeyEvent::Down,
            ),
            TestShortcutAction::Toggle,
            TestShortcutAction::Edge(FixedShortcut::ID.to_string(), echo::hotkey::HotkeyEvent::Up),
        ] {
            receive_expected(expected);
        }
        recording_env.assert_active();

        send_shortcut("super+alt+space");
        for expected in [
            TestShortcutAction::Edge(
                FixedShortcut::ID.to_string(),
                echo::hotkey::HotkeyEvent::Down,
            ),
            TestShortcutAction::Toggle,
            TestShortcutAction::Edge(FixedShortcut::ID.to_string(), echo::hotkey::HotkeyEvent::Up),
        ] {
            receive_expected(expected);
        }
        recording_env.wait_until_inactive();

        cancel.cancel();
        listener.join().unwrap().unwrap();
        *TEST_SHORTCUT_ACTIONS
            .lock()
            .expect("test shortcut observer lock") = None;
    }
}
