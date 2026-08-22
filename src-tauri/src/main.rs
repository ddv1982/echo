use std::collections::{BTreeMap, HashMap};
use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, GlobalShortcuts, NewShortcut, Shortcut,
};
use ashpd::desktop::CreateSessionOptions;
use echo::audio::AudioCapture;
use echo::inject::{Pasteboard, SysClipboard};
use echo::stt::fetch::{self, DownloadStage};
use echo_core::{DictEntry, Dictionary, History};
use futures_util::StreamExt;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

const DEFAULT_TOGGLE_SHORTCUT: &str = "Super+Alt+Space";
const APP_ID: &str = "io.github.ddv1982.echo";
const TOGGLE_SHORTCUT_ID: &str = "toggle-recording";
const HOLD_SHORTCUT_ID: &str = "push-to-talk";
const GNOME_MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GNOME_CUSTOM_KEY_SCHEMA: &str =
    "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const ECHO_CUSTOM_KEY_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/echo/";
const ECHO_CUSTOM_KEY_NAME: &str = "Echo Dictation";

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
    shortcut: String,
    cleanup_name: String,
    hud_enabled: bool,
    max_record_seconds: u64,
    settings_path: String,
    version: String,
    last_error: Option<String>,
    last_run: Option<LastRun>,
    language_warning: Option<String>,
    recording_in_process: bool,
    current_exe: String,
    first_path_hit: Option<String>,
    stale_installs: Vec<String>,
    hold_listener: String,
    hold_listener_error: Option<String>,
    shortcut_backend: String,
    shortcut_healthy: bool,
    shortcut_error: Option<String>,
    requested_shortcut: String,
    requested_hold_shortcut: String,
    effective_hold_shortcut: Option<String>,
    legacy_shortcut: Option<LegacyShortcutSetup>,
    shortcut_activation: Option<String>,
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
    toggle_shortcut: SettingField<String>,
    hold_key: SettingField<String>,
    record_seconds: SettingField<u32>,
    microphone: SettingField<String>,
    language: SettingField<String>,
}

#[derive(Debug, Default, Clone)]
struct SettingsEnv {
    engine: Option<String>,
    whisper_model: Option<String>,
    cleanup: Option<String>,
    hud: Option<String>,
    toggle_shortcut: Option<String>,
    hold_key: Option<String>,
    record_seconds: Option<String>,
    microphone: Option<String>,
    language: Option<String>,
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
        microphone_ready: AudioCapture::open_default().is_ok(),
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
    native: &NativeShortcutStatus,
    current_exe: &str,
) -> Option<LegacyShortcutSetup> {
    if native.healthy
        || !native.global_shortcuts_absent
        || echo::hotkey::DesktopSession::from_xdg_session_type(
            env::var("XDG_SESSION_TYPE").ok().as_deref(),
        ) != echo::hotkey::DesktopSession::Wayland
    {
        return None;
    }

    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let executable = stable_shortcut_executable(
        current_exe,
        env::var_os("APPIMAGE").as_deref(),
    );
    let command = absolute_toggle_command(&executable).unwrap_or_default();
    let binding = gnome_accelerator(&native.requested_toggle).unwrap_or_default();
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

fn gnome_accelerator(shortcut: &str) -> Result<String, String> {
    let canonical =
        echo_core::Config::canonical_toggle_shortcut(shortcut).map_err(|err| err.to_string())?;
    let parts = canonical.split('+').collect::<Vec<_>>();
    let terminal = parts.last().copied().ok_or("shortcut has no key")?;
    let mut accelerator = String::new();
    for modifier in parts.iter().take(parts.len().saturating_sub(1)) {
        accelerator.push_str(match *modifier {
            "Super" => "<Super>",
            "Ctrl" => "<Ctrl>",
            "Alt" => "<Alt>",
            "Shift" => "<Shift>",
            other => return Err(format!("unsupported GNOME modifier {other}")),
        });
    }
    let terminal = match terminal {
        "Space" => "space".to_string(),
        "Enter" => "Return".to_string(),
        "ArrowUp" => "Up".to_string(),
        "ArrowDown" => "Down".to_string(),
        "ArrowLeft" => "Left".to_string(),
        "ArrowRight" => "Right".to_string(),
        key if key.len() == 1 => key.to_ascii_lowercase(),
        key => key.to_string(),
    };
    accelerator.push_str(&terminal);
    Ok(accelerator)
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
    let mut parts = Vec::new();
    while let Some(after_open) = rest.strip_prefix('<') {
        let close = after_open.find('>')?;
        parts.push(match after_open[..close].to_ascii_lowercase().as_str() {
            "super" | "mod4" => "Super".to_string(),
            "ctrl" | "control" | "primary" => "Ctrl".to_string(),
            "alt" | "mod1" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            _ => return None,
        });
        rest = &after_open[close + 1..];
    }
    let terminal = match rest.to_ascii_lowercase().as_str() {
        "return" => "Enter".to_string(),
        "up" => "ArrowUp".to_string(),
        "down" => "ArrowDown".to_string(),
        "left" => "ArrowLeft".to_string(),
        "right" => "ArrowRight".to_string(),
        key => key.to_string(),
    };
    if terminal.is_empty() {
        return None;
    }
    parts.push(terminal);
    echo_core::Config::canonical_toggle_shortcut(&parts.join("+")).ok()
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
    let health = health_snapshot();
    let shortcut_status = native_shortcut_status();
    let evdev_status = evdev_listener_state();
    let legacy_shortcut = legacy_shortcut_setup(&shortcut_status, &health.current_exe);
    let shortcut = projected_toggle_shortcut(&shortcut_status, legacy_shortcut.as_ref());
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
        max_record_seconds: echo::rec::MAX_RECORD_SECONDS,
        settings_path: echo_core::config_path().to_string_lossy().into_owned(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        last_error: status.error,
        last_run,
        language_warning: echo::stt::language_warning(),
        recording_in_process,
        current_exe: health.current_exe,
        first_path_hit: health.first_path_hit,
        stale_installs: health.stale_installs,
        hold_listener: evdev_status.projection().to_string(),
        hold_listener_error: evdev_status.error().map(str::to_string),
        shortcut_backend: shortcut_status.backend.as_str().to_string(),
        shortcut_healthy: shortcut_status.healthy,
        shortcut_error: shortcut_status.error.clone(),
        requested_shortcut: shortcut_status.requested_toggle,
        requested_hold_shortcut: shortcut_status.requested_hold,
        effective_hold_shortcut: shortcut_status.effective_hold,
        legacy_shortcut,
        shortcut_activation: echo::status::shortcut_activation(),
    }
}

fn projected_toggle_shortcut(
    native: &NativeShortcutStatus,
    legacy: Option<&LegacyShortcutSetup>,
) -> String {
    native.effective_toggle.clone().unwrap_or_else(|| {
        if legacy.is_some_and(|setup| setup.state == LegacyShortcutState::Ready) {
            native.requested_toggle.clone()
        } else {
            format!("Unavailable ({})", native.requested_toggle)
        }
    })
}

#[tauri::command]
fn repair_legacy_shortcut() -> Result<LegacyShortcutSetup, String> {
    let native = native_shortcut_status();
    let health = health_snapshot();
    let advertised = legacy_shortcut_setup(&native, &health.current_exe)
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
    start_recording_thread()
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelOfferDto {
    id: String,
    label: String,
    filename: String,
    url: String,
    size_bytes: u64,
    runtime_mb: Option<u32>,
    multilingual: bool,
    installed: bool,
}

#[tauri::command]
fn list_model_offers() -> Vec<ModelOfferDto> {
    let cache = echo::stt::ModelCache::from_env();
    fetch::OFFERS
        .iter()
        .map(|offer| ModelOfferDto {
            id: offer.id.to_string(),
            label: offer.label.to_string(),
            filename: offer.filename.to_string(),
            url: offer.url.to_string(),
            size_bytes: offer.size_bytes,
            runtime_mb: offer.runtime_mb,
            multilingual: offer.multilingual,
            installed: cache.path(offer.filename).is_file(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgressDto {
    id: String,
    received: u64,
    total: u64,
    /// "downloading", "verifying", "done", "failed", or "cancelled".
    stage: String,
    error: Option<String>,
}

static DOWNLOADS: Mutex<Option<HashMap<String, Arc<AtomicBool>>>> = Mutex::new(None);

#[tauri::command]
fn download_model(id: String, app: tauri::AppHandle) -> Result<(), String> {
    let offer = fetch::offer(&id).ok_or_else(|| format!("unknown model offer {id}"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    DOWNLOADS
        .lock()
        .expect("downloads lock")
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), cancel.clone());
    std::thread::Builder::new()
        .name(format!("echo-download-{id}"))
        .spawn(move || {
            let dir = echo::stt::ModelCache::from_env().dir().to_path_buf();
            let emit = |stage: &str, received: u64, total: u64, error: Option<String>| {
                let _ = app.emit(
                    "model-download-progress",
                    DownloadProgressDto {
                        id: id.clone(),
                        received,
                        total,
                        stage: stage.to_string(),
                        error,
                    },
                );
            };
            let result = fetch::download(
                offer,
                &dir,
                |progress| {
                    let stage = match progress.stage {
                        DownloadStage::Downloading => "downloading",
                        DownloadStage::Verifying => "verifying",
                        DownloadStage::Done => "done",
                    };
                    emit(stage, progress.received, progress.total, None);
                },
                &cancel,
            );
            match result {
                Ok(_) => emit("done", offer.size_bytes, offer.size_bytes, None),
                Err(fetch::FetchError::Cancelled) => emit("cancelled", 0, offer.size_bytes, None),
                Err(err) => emit("failed", 0, offer.size_bytes, Some(err.to_string())),
            }
            DOWNLOADS
                .lock()
                .expect("downloads lock")
                .as_mut()
                .map(|active| active.remove(&id));
        })
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
fn cancel_download(id: String) -> bool {
    DOWNLOADS
        .lock()
        .expect("downloads lock")
        .as_ref()
        .and_then(|active| active.get(&id))
        .map(|cancel| {
            cancel.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_or(false)
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
    let inventory = cache.inventory();
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InputDeviceDto {
    name: String,
    is_default: bool,
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<InputDeviceDto>, String> {
    Ok(echo::audio::list_input_devices()
        .into_iter()
        .map(|device| InputDeviceDto {
            name: device.name,
            is_default: device.is_default,
        })
        .collect())
}

#[tauri::command]
fn test_input_device(name: Option<String>) -> Result<f32, String> {
    let capture =
        echo::audio::AudioCapture::open(name.as_deref()).map_err(|err| err.to_string())?;
    let result = capture
        .record(std::time::Duration::from_secs(1), None)
        .map_err(|err| err.to_string())?;
    Ok(result.peak_rms)
}

fn read_settings() -> Result<Settings, String> {
    // The picker's default must match what the recorder would do: auto when
    // the resolved model is multilingual, pinned English otherwise.
    let language_default = match echo::stt::resolved_language(
        None,
        &echo_core::Config::default(),
        echo::stt::resolved_model_multilingual(),
    ) {
        echo_core::LanguageChoice::Auto => "auto",
        _ => "en",
    };
    settings_from(
        &process_settings_env(),
        &echo::settings::file_config(),
        language_default,
    )
}

fn write_settings(settings: Settings) -> Result<Settings, String> {
    config_from_values(&settings)?.save()?;
    echo::settings::reload();
    health_invalidate();
    *LEGACY_SHORTCUT_CACHE
        .lock()
        .expect("legacy shortcut cache lock") = None;
    reconcile_native_shortcuts();
    read_settings()
}

fn process_settings_env() -> SettingsEnv {
    SettingsEnv {
        engine: env::var("ECHO_ENGINE").ok(),
        whisper_model: env::var("ECHO_WHISPER_MODEL").ok(),
        cleanup: env::var("ECHO_CLEANUP").ok(),
        hud: env::var("ECHO_HUD").ok(),
        toggle_shortcut: env::var("ECHO_TOGGLE_SHORTCUT").ok(),
        hold_key: env::var("ECHO_HOLD_KEY").ok(),
        record_seconds: env::var("ECHO_RECORD_SECONDS").ok(),
        microphone: env::var("ECHO_MICROPHONE").ok(),
        language: env::var("ECHO_LANGUAGE").ok(),
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
        toggle_shortcut: shortcut_field(
            "toggle shortcut",
            env.toggle_shortcut.as_deref(),
            file.toggle_shortcut.as_deref(),
            DEFAULT_TOGGLE_SHORTCUT,
            echo_core::Config::canonical_toggle_shortcut,
        )?,
        hold_key: shortcut_field(
            "push-to-talk shortcut",
            env.hold_key.as_deref(),
            file.hold_key.as_deref(),
            "RightCtrl",
            echo_core::Config::canonical_shortcut,
        )?,
        record_seconds: record_seconds_field(
            env.record_seconds
                .as_deref()
                .and_then(|raw| raw.parse::<u64>().ok()),
            file.record_seconds,
        ),
        microphone: setting_field(
            env.microphone.clone().filter(|name| !name.is_empty()),
            file.microphone.clone(),
            String::new(),
        ),
        language: setting_field(
            env.language
                .as_deref()
                .and_then(echo_core::LanguageChoice::parse)
                .map(|choice| choice.as_str().to_string()),
            file.language.map(|choice| choice.as_str().to_string()),
            language_default.to_string(),
        ),
    })
}

fn config_from_values(settings: &Settings) -> Result<echo_core::Config, String> {
    let mut config = echo_core::Config::load().unwrap_or_default();
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
    config.toggle_shortcut = canonical_shortcut_value(
        "toggle shortcut",
        settings.toggle_shortcut.value.as_deref(),
        echo_core::Config::canonical_toggle_shortcut,
    )?;
    config.hold_key = canonical_shortcut_value(
        "push-to-talk shortcut",
        settings.hold_key.value.as_deref(),
        echo_core::Config::canonical_shortcut,
    )?;
    config.record_seconds = settings
        .record_seconds
        .value
        .map(|secs| secs.clamp(1, echo::rec::MAX_RECORD_SECONDS as u32));
    config.microphone = nonempty(settings.microphone.value.clone());
    config.language = match settings.language.value.as_deref() {
        None => None,
        Some(raw) => Some(
            echo_core::LanguageChoice::parse(raw)
                .ok_or_else(|| format!("unknown language {raw}"))?,
        ),
    };
    Ok(config)
}

fn shortcut_field<E: std::fmt::Display>(
    label: &str,
    env: Option<&str>,
    file: Option<&str>,
    default: &str,
    canonicalize: fn(&str) -> Result<String, E>,
) -> Result<SettingField<String>, String> {
    let env = env
        .map(|raw| {
            canonicalize(raw).map_err(|err| format!("invalid {label} from environment: {err}"))
        })
        .transpose()?;
    let file = file
        .map(|raw| canonicalize(raw).map_err(|err| format!("invalid saved {label}: {err}")))
        .transpose()?;
    Ok(setting_field(env, file, default.to_string()))
}

fn canonical_shortcut_value<E: std::fmt::Display>(
    label: &str,
    value: Option<&str>,
    canonicalize: fn(&str) -> Result<String, E>,
) -> Result<Option<String>, String> {
    value
        .map(|raw| canonicalize(raw).map_err(|err| format!("invalid {label}: {err}")))
        .transpose()
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

fn record_seconds_field(env: Option<u64>, file: Option<u32>) -> SettingField<u32> {
    let field = setting_field(
        env.map(|secs| secs.clamp(1, echo::rec::MAX_RECORD_SECONDS) as u32),
        file,
        3,
    );
    SettingField {
        effective: field
            .effective
            .clamp(1, echo::rec::MAX_RECORD_SECONDS as u32),
        ..field
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

fn start_recording_thread() -> Result<(), String> {
    echo::rec::toggle_managed_recording()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutBackendName {
    Portal,
    X11,
    Unsupported,
}

impl ShortcutBackendName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Portal => "portal",
            Self::X11 => "x11",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone)]
struct NativeShortcutStatus {
    backend: ShortcutBackendName,
    healthy: bool,
    global_shortcuts_absent: bool,
    error: Option<String>,
    requested_toggle: String,
    effective_toggle: Option<String>,
    requested_hold: String,
    effective_hold: Option<String>,
}

impl Default for NativeShortcutStatus {
    fn default() -> Self {
        Self {
            backend: ShortcutBackendName::Unsupported,
            healthy: false,
            global_shortcuts_absent: false,
            error: Some("native shortcut backend has not started".to_string()),
            requested_toggle: DEFAULT_TOGGLE_SHORTCUT.to_string(),
            effective_toggle: None,
            requested_hold: "RightCtrl".to_string(),
            effective_hold: None,
        }
    }
}

static NATIVE_SHORTCUT_STATUS: OnceLock<Arc<Mutex<NativeShortcutStatus>>> = OnceLock::new();

fn native_status_cell() -> &'static Arc<Mutex<NativeShortcutStatus>> {
    NATIVE_SHORTCUT_STATUS.get_or_init(|| Arc::new(Mutex::new(NativeShortcutStatus::default())))
}

fn native_shortcut_status() -> NativeShortcutStatus {
    native_status_cell()
        .lock()
        .expect("native shortcut status lock")
        .clone()
}

fn update_native_status(update: impl FnOnce(&mut NativeShortcutStatus)) {
    update(
        &mut native_status_cell()
            .lock()
            .expect("native shortcut status lock"),
    );
}

fn set_effective_shortcuts(status: &mut NativeShortcutStatus, toggle: String, hold: String) {
    status.effective_toggle = Some(toggle);
    status.effective_hold = Some(hold);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeHoldState {
    Probing,
    Healthy,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvdevFallbackPlan {
    StopForNative,
    Start,
}

fn evdev_fallback_plan(native: NativeHoldState) -> EvdevFallbackPlan {
    if native != NativeHoldState::Failed {
        EvdevFallbackPlan::StopForNative
    } else {
        EvdevFallbackPlan::Start
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EvdevListenerState {
    StoppedForNative,
    Probing,
    Active,
    NeedsPermission(String),
    Unavailable(String),
    Failed(String),
}

impl EvdevListenerState {
    fn projection(&self) -> &'static str {
        match self {
            Self::StoppedForNative => "native",
            Self::Active => "active",
            Self::NeedsPermission(_) => "needs-permission",
            Self::Probing | Self::Unavailable(_) | Self::Failed(_) => "unavailable",
        }
    }

    fn error(&self) -> Option<&str> {
        match self {
            Self::NeedsPermission(error) | Self::Unavailable(error) | Self::Failed(error) => {
                Some(error)
            }
            _ => None,
        }
    }
}

static EVDEV_LISTENER_STATE: OnceLock<Arc<Mutex<EvdevListenerState>>> = OnceLock::new();

fn evdev_state_cell() -> &'static Arc<Mutex<EvdevListenerState>> {
    EVDEV_LISTENER_STATE.get_or_init(|| Arc::new(Mutex::new(EvdevListenerState::StoppedForNative)))
}

fn evdev_listener_state() -> EvdevListenerState {
    evdev_state_cell()
        .lock()
        .expect("evdev listener status lock")
        .clone()
}

fn set_evdev_listener_state(state: EvdevListenerState) {
    *evdev_state_cell()
        .lock()
        .expect("evdev listener status lock") = state;
}

struct HoldListenerHandle {
    key: String,
    cancel: echo::audio::CancellationToken,
    thread: JoinHandle<()>,
}

static HOLD_LISTENER: Mutex<Option<HoldListenerHandle>> = Mutex::new(None);
static EVDEV_RECONCILE: Mutex<()> = Mutex::new(());

fn stop_evdev_handle(handle: Option<HoldListenerHandle>) {
    if let Some(handle) = handle {
        handle.cancel.cancel();
        if handle.thread.join().is_err() {
            set_evdev_listener_state(EvdevListenerState::Failed(
                "evdev hold listener panicked during teardown".to_string(),
            ));
        }
    }
}

fn stop_evdev_listener() {
    let old = HOLD_LISTENER.lock().expect("hold listener lock").take();
    stop_evdev_handle(old);
}

/// Start, stop, or rekey the evdev fallback. Native probing/ownership always
/// cancels it; a settled native failure enables a capability-filtered
/// supervisor that reports permission/device health and reconnects on hotplug.
fn reconcile_evdev_fallback(native: NativeHoldState) {
    let _reconcile = EVDEV_RECONCILE.lock().expect("evdev reconcile lock");
    match evdev_fallback_plan(native) {
        EvdevFallbackPlan::StopForNative => {
            stop_evdev_listener();
            set_evdev_listener_state(if native == NativeHoldState::Healthy {
                EvdevListenerState::StoppedForNative
            } else {
                EvdevListenerState::Probing
            });
            return;
        }
        EvdevFallbackPlan::Start => {}
    }

    let spec = match echo::hotkey::hold_key() {
        Ok(spec) => spec,
        Err(err) => {
            stop_evdev_listener();
            set_evdev_listener_state(EvdevListenerState::Failed(format!(
                "invalid evdev hold shortcut: {err}"
            )));
            return;
        }
    };
    let old = {
        let mut guard = HOLD_LISTENER.lock().expect("hold listener lock");
        if guard
            .as_ref()
            .is_some_and(|running| running.key == spec.name && !running.thread.is_finished())
        {
            return;
        }
        guard.take()
    };
    stop_evdev_handle(old);

    let cancel = echo::audio::CancellationToken::new();
    let thread_cancel = cancel.clone();
    let ready = Arc::new(Barrier::new(2));
    let thread_ready = ready.clone();
    let spawned = std::thread::Builder::new()
        .name("echo-hold-listener".to_string())
        .spawn(move || {
            thread_ready.wait();
            let mut recording = None;
            echo::hotkey::run_evdev_supervisor(
                spec.code,
                &thread_cancel,
                |health| {
                    set_evdev_listener_state(match health {
                        echo::hotkey::EvdevListenerHealth::Active => EvdevListenerState::Active,
                        echo::hotkey::EvdevListenerHealth::NeedsPermission(detail) => {
                            EvdevListenerState::NeedsPermission(detail)
                        }
                        echo::hotkey::EvdevListenerHealth::Unavailable(detail) => {
                            EvdevListenerState::Unavailable(detail)
                        }
                        echo::hotkey::EvdevListenerHealth::Degraded(detail) => {
                            EvdevListenerState::Failed(detail)
                        }
                    });
                },
                |edge| dispatch_hold_edge(edge, &mut recording),
            );
            stop_held_recording(&mut recording);
            if !thread_cancel.is_cancelled() {
                set_evdev_listener_state(EvdevListenerState::Failed(
                    "evdev hold listener terminated unexpectedly".to_string(),
                ));
            }
        });
    match spawned {
        Ok(thread) => {
            *HOLD_LISTENER.lock().expect("hold listener lock") = Some(HoldListenerHandle {
                key: spec.name,
                cancel,
                thread,
            });
            set_evdev_listener_state(EvdevListenerState::Probing);
            ready.wait();
        }
        Err(err) => set_evdev_listener_state(EvdevListenerState::Failed(format!(
            "cannot spawn evdev hold listener: {err}"
        ))),
    }
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
    toggle: String,
    hold: String,
    cancel: echo::audio::CancellationToken,
    thread: JoinHandle<()>,
}

static NATIVE_SHORTCUT: Mutex<Option<NativeShortcutHandle>> = Mutex::new(None);

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum TestShortcutAction {
    Edge(String, echo::hotkey::HotkeyEvent),
    Toggle,
    HoldStart,
    HoldStop,
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

enum HeldRecording {
    Live {
        _recording: echo::rec::ManagedRecording,
    },
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
    let _reconcile = NATIVE_RECONCILE
        .lock()
        .expect("native shortcut reconcile lock");
    let settings = match read_settings() {
        Ok(settings) => settings,
        Err(err) => {
            stop_native_shortcuts();
            update_native_status(|status| {
                status.backend = ShortcutBackendName::Unsupported;
                status.healthy = false;
                status.global_shortcuts_absent = false;
                status.error = Some(format!("cannot read shortcut settings: {err}"));
                status.effective_toggle = None;
                status.effective_hold = None;
            });
            reconcile_evdev_fallback(NativeHoldState::Failed);
            return;
        }
    };
    let toggle = settings.toggle_shortcut.effective;
    let hold = settings.hold_key.effective;

    let old = {
        let mut guard = NATIVE_SHORTCUT.lock().expect("native shortcut lock");
        if guard.as_ref().is_some_and(|running| {
            running.toggle == toggle && running.hold == hold && !running.thread.is_finished()
        }) {
            return;
        }
        guard.take()
    };
    stop_native_handle(old);
    reconcile_evdev_fallback(NativeHoldState::Probing);

    let session = echo::hotkey::DesktopSession::from_xdg_session_type(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    update_native_status(|status| {
        status.backend = match session {
            echo::hotkey::DesktopSession::X11 => ShortcutBackendName::X11,
            _ => ShortcutBackendName::Unsupported,
        };
        status.healthy = false;
        status.global_shortcuts_absent = false;
        status.error = Some(match session {
            echo::hotkey::DesktopSession::Wayland => {
                "probing org.freedesktop.portal.GlobalShortcuts".to_string()
            }
            echo::hotkey::DesktopSession::X11 => "registering X11 shortcuts".to_string(),
            echo::hotkey::DesktopSession::Unknown => {
                "unknown or headless desktop session".to_string()
            }
        });
        status.requested_toggle.clone_from(&toggle);
        status.requested_hold.clone_from(&hold);
        status.effective_toggle = None;
        status.effective_hold = None;
    });

    if session == echo::hotkey::DesktopSession::Unknown {
        reconcile_evdev_fallback(NativeHoldState::Failed);
        return;
    }

    let cancel = echo::audio::CancellationToken::new();
    let thread_cancel = cancel.clone();
    let thread_toggle = toggle.clone();
    let thread_hold = hold.clone();
    let spawned = std::thread::Builder::new()
        .name(
            match session {
                echo::hotkey::DesktopSession::Wayland => "echo-shortcuts-portal",
                _ => "echo-shortcuts-x11",
            }
            .to_string(),
        )
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| match session {
                echo::hotkey::DesktopSession::Wayland => {
                    run_portal_shortcuts(&thread_toggle, &thread_hold, &thread_cancel)
                }
                echo::hotkey::DesktopSession::X11 => {
                    run_x11_shortcuts(&thread_toggle, &thread_hold, &thread_cancel)
                }
                echo::hotkey::DesktopSession::Unknown => Ok(()),
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => mark_native_failure(err),
                Err(_) => mark_native_failure("native shortcut listener panicked".to_string()),
            }
        });
    match spawned {
        Ok(thread) => {
            *NATIVE_SHORTCUT.lock().expect("native shortcut lock") = Some(NativeShortcutHandle {
                toggle,
                hold,
                cancel,
                thread,
            });
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
    update_native_status(|status| {
        status.healthy = false;
        status.global_shortcuts_absent = false;
        status.error = Some(error);
        status.effective_toggle = None;
        status.effective_hold = None;
    });
    reconcile_evdev_fallback(NativeHoldState::Failed);
}

fn mark_native_unsupported(error: String, global_shortcuts_absent: bool) {
    update_native_status(|status| {
        status.backend = ShortcutBackendName::Unsupported;
        status.healthy = false;
        status.global_shortcuts_absent = global_shortcuts_absent;
        status.error = Some(error);
        status.effective_toggle = None;
        status.effective_hold = None;
    });
    reconcile_evdev_fallback(NativeHoldState::Failed);
}

fn dispatch_shortcut_edge(
    id: &str,
    edge: echo::hotkey::HotkeyEvent,
    toggle: &mut echo::hotkey::ToggleDriver,
    hold: &mut Option<HeldRecording>,
) {
    #[cfg(test)]
    report_test_shortcut(TestShortcutAction::Edge(id.to_string(), edge));
    match id {
        TOGGLE_SHORTCUT_ID if toggle.on_edge(edge) => match start_recording_thread() {
            Ok(()) => {
                if let Err(err) = echo::status::mark_shortcut_activation("native-toggle") {
                    eprintln!("toggle shortcut: cannot record provenance: {err}");
                }
                #[cfg(test)]
                report_test_shortcut(TestShortcutAction::Toggle);
            }
            Err(err) => eprintln!("toggle shortcut: cannot change recording: {err}"),
        },
        HOLD_SHORTCUT_ID => dispatch_hold_edge(edge, hold),
        _ => {}
    }
}

fn dispatch_hold_edge(edge: echo::hotkey::HotkeyEvent, recording: &mut Option<HeldRecording>) {
    match edge {
        echo::hotkey::HotkeyEvent::Down if recording.is_none() => {
            match echo::rec::start_managed_recording() {
                Ok(Some(started)) => {
                    *recording = Some(HeldRecording::Live {
                        _recording: started,
                    });
                    #[cfg(test)]
                    report_test_shortcut(TestShortcutAction::HoldStart);
                }
                Ok(None) => {}
                Err(err) => eprintln!("push-to-talk: cannot start recording: {err}"),
            }
        }
        echo::hotkey::HotkeyEvent::Up => {
            stop_held_recording(recording);
        }
        _ => {}
    }
}

fn stop_held_recording(recording: &mut Option<HeldRecording>) {
    if recording.take().is_some() {
        #[cfg(test)]
        report_test_shortcut(TestShortcutAction::HoldStop);
    }
}

fn run_x11_shortcuts(
    requested_toggle: &str,
    requested_hold: &str,
    cancel: &echo::audio::CancellationToken,
) -> Result<(), String> {
    let mut toggle = echo::hotkey::ToggleDriver::new();
    let mut hold = None;
    let result = run_x11_event_loop(requested_toggle, requested_hold, cancel, |id, edge| {
        dispatch_shortcut_edge(id, edge, &mut toggle, &mut hold);
    });
    toggle.terminate();
    stop_held_recording(&mut hold);
    result
}

fn run_x11_event_loop(
    requested_toggle: &str,
    requested_hold: &str,
    cancel: &echo::audio::CancellationToken,
    mut on_edge: impl FnMut(&'static str, echo::hotkey::HotkeyEvent),
) -> Result<(), String> {
    let decision = echo::hotkey::select_native_backend(echo::hotkey::DesktopSession::X11, None);
    debug_assert_eq!(decision.backend, echo::hotkey::NativeBackend::X11);
    let toggle_key = x11_hotkey(requested_toggle)?;
    let hold_key = x11_hotkey(requested_hold)?;
    if toggle_key.id() == hold_key.id() {
        return Err("toggle and push-to-talk shortcuts conflict".to_string());
    }

    let manager = GlobalHotKeyManager::new()
        .map_err(|err| format!("cannot create X11 global-hotkey manager: {err}"))?;
    while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
    manager
        .register(toggle_key)
        .map_err(|err| format!("X11 toggle shortcut conflict: {err}"))?;
    if let Err(err) = manager.register(hold_key) {
        let _ = manager.unregister(toggle_key);
        return Err(format!("X11 push-to-talk shortcut conflict: {err}"));
    }
    update_native_status(|status| {
        status.backend = ShortcutBackendName::X11;
        status.healthy = true;
        status.global_shortcuts_absent = false;
        status.error = None;
        set_effective_shortcuts(
            status,
            requested_toggle.to_string(),
            requested_hold.to_string(),
        );
    });
    reconcile_evdev_fallback(NativeHoldState::Healthy);

    while !cancel.is_cancelled() {
        match GlobalHotKeyEvent::receiver().recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                let id = if event.id == toggle_key.id() {
                    TOGGLE_SHORTCUT_ID
                } else if event.id == hold_key.id() {
                    HOLD_SHORTCUT_ID
                } else {
                    continue;
                };
                let edge = match event.state {
                    HotKeyState::Pressed => echo::hotkey::HotkeyEvent::Down,
                    HotKeyState::Released => echo::hotkey::HotkeyEvent::Up,
                };
                on_edge(id, edge);
            }
            Err(err) if err.is_timeout() => {}
            Err(err) => {
                let _ = manager.unregister_all(&[toggle_key, hold_key]);
                return Err(format!("X11 shortcut listener terminated: {err}"));
            }
        }
    }
    manager
        .unregister_all(&[toggle_key, hold_key])
        .map_err(|err| format!("cannot unregister X11 shortcuts: {err}"))
}

fn run_portal_shortcuts(
    requested_toggle: &str,
    requested_hold: &str,
    cancel: &echo::audio::CancellationToken,
) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("cannot start portal runtime: {err}"))?;
    runtime.block_on(async {
        let connection = ashpd::zbus::Connection::session()
            .await
            .map_err(|err| format!("cannot connect to the session bus: {err}"))?;
        run_portal_shortcuts_async(requested_toggle, requested_hold, cancel, connection).await
    })
}

async fn run_portal_shortcuts_async(
    requested_toggle: &str,
    requested_hold: &str,
    cancel: &echo::audio::CancellationToken,
    connection: ashpd::zbus::Connection,
) -> Result<(), String> {
    let app_id = APP_ID
        .parse::<ashpd::AppID>()
        .map_err(|err| format!("invalid portal application id: {err}"))?;
    if let Err(err) = ashpd::register_host_app_with_connection(connection.clone(), app_id).await {
        if is_legacy_registry_absence(&err) {
            eprintln!("native shortcuts: host portal Registry is unavailable; probing legacy GlobalShortcuts support");
        } else {
            mark_native_unsupported(
                format!("portal host registry handshake failed: {err}"),
                false,
            );
            return Ok(());
        }
    }

    // The Registry attempt above intentionally precedes every portal proxy,
    // session and bind operation. New stacks attribute permissions to APP_ID;
    // legacy stacks without Registry can still expose GlobalShortcuts.
    let portal = match GlobalShortcuts::with_connection(connection).await {
        Ok(portal) => portal,
        Err(err) => {
            mark_native_unsupported(
                format!("Wayland GlobalShortcuts interface is unavailable: {err}"),
                is_global_shortcuts_absence(&err),
            );
            return Ok(());
        }
    };
    let decision = echo::hotkey::select_native_backend(
        echo::hotkey::DesktopSession::Wayland,
        Some(portal.version()),
    );
    if decision.backend != echo::hotkey::NativeBackend::Portal {
        mark_native_unsupported(
            decision
                .reason
                .unwrap_or_else(|| "Wayland GlobalShortcuts interface is unavailable".to_string()),
            false,
        );
        return Ok(());
    }
    update_native_status(|status| {
        status.backend = ShortcutBackendName::Portal;
        status.global_shortcuts_absent = false;
        status.error = Some("registering portal shortcuts".to_string());
    });

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

    let toggle_trigger = match portal_trigger(requested_toggle) {
        Ok(trigger) => trigger,
        Err(err) => return Err(close_portal_after_failure(&session, err).await),
    };
    let hold_trigger = match portal_trigger(requested_hold) {
        Ok(trigger) => trigger,
        Err(err) => return Err(close_portal_after_failure(&session, err).await),
    };
    let shortcuts = [
        NewShortcut::new(TOGGLE_SHORTCUT_ID, "Start or stop recording")
            .preferred_trigger(toggle_trigger.as_str()),
        NewShortcut::new(HOLD_SHORTCUT_ID, "Hold to record")
            .preferred_trigger(hold_trigger.as_str()),
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
    let (effective_toggle, effective_hold) = match effective_portal_shortcuts(response.shortcuts())
    {
        Ok(effective) => effective,
        Err(err) => return Err(close_portal_after_failure(&session, err).await),
    };
    update_native_status(|status| {
        status.backend = ShortcutBackendName::Portal;
        status.healthy = true;
        status.global_shortcuts_absent = false;
        status.error = None;
        set_effective_shortcuts(status, effective_toggle, effective_hold);
    });
    reconcile_evdev_fallback(NativeHoldState::Healthy);

    let mut toggle = echo::hotkey::ToggleDriver::new();
    let mut hold = None;
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
                        &mut hold,
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
                        &mut hold,
                    );
                }
                Some(_) => {}
                None => break Some("portal Deactivated listener terminated".to_string()),
            },
            event = changed.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => {
                    match effective_portal_shortcuts(event.shortcuts()) {
                        Ok((effective_toggle, effective_hold)) => update_native_status(|status| {
                            set_effective_shortcuts(status, effective_toggle, effective_hold);
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
    stop_held_recording(&mut hold);
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

fn effective_portal_shortcuts(shortcuts: &[Shortcut]) -> Result<(String, String), String> {
    let effective = |id: &str| {
        shortcuts
            .iter()
            .find(|shortcut| shortcut.id() == id)
            .map(Shortcut::trigger_description)
            .filter(|trigger| !trigger.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("portal did not assign an effective trigger for {id}"))
    };
    Ok((effective(TOGGLE_SHORTCUT_ID)?, effective(HOLD_SHORTCUT_ID)?))
}

fn x11_hotkey(shortcut: &str) -> Result<HotKey, String> {
    let canonical = echo_core::Config::canonical_shortcut(shortcut)
        .map_err(|err| format!("cannot parse X11 shortcut {shortcut}: {err}"))?;
    let parts = canonical.split('+').collect::<Vec<_>>();
    let terminal = parts.last().copied().ok_or("shortcut has no key")?;
    let mut modifiers = Modifiers::empty();
    for modifier in parts.iter().take(parts.len().saturating_sub(1)) {
        modifiers |= match *modifier {
            "Super" => Modifiers::SUPER,
            "Ctrl" => Modifiers::CONTROL,
            "Alt" => Modifiers::ALT,
            "Shift" => Modifiers::SHIFT,
            other => return Err(format!("unsupported X11 modifier {other}")),
        };
    }
    let key = shortcut_code(terminal)
        .ok_or_else(|| format!("unsupported X11 shortcut key {terminal}"))?;
    Ok(HotKey::new(Some(modifiers), key))
}

fn shortcut_code(name: &str) -> Option<Code> {
    Some(match name {
        "A" => Code::KeyA,
        "B" => Code::KeyB,
        "C" => Code::KeyC,
        "D" => Code::KeyD,
        "E" => Code::KeyE,
        "F" => Code::KeyF,
        "G" => Code::KeyG,
        "H" => Code::KeyH,
        "I" => Code::KeyI,
        "J" => Code::KeyJ,
        "K" => Code::KeyK,
        "L" => Code::KeyL,
        "M" => Code::KeyM,
        "N" => Code::KeyN,
        "O" => Code::KeyO,
        "P" => Code::KeyP,
        "Q" => Code::KeyQ,
        "R" => Code::KeyR,
        "S" => Code::KeyS,
        "T" => Code::KeyT,
        "U" => Code::KeyU,
        "V" => Code::KeyV,
        "W" => Code::KeyW,
        "X" => Code::KeyX,
        "Y" => Code::KeyY,
        "Z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "Space" => Code::Space,
        "Enter" => Code::Enter,
        "Tab" => Code::Tab,
        "Backspace" => Code::Backspace,
        "Delete" => Code::Delete,
        "Insert" => Code::Insert,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "ArrowUp" => Code::ArrowUp,
        "ArrowDown" => Code::ArrowDown,
        "ArrowLeft" => Code::ArrowLeft,
        "ArrowRight" => Code::ArrowRight,
        "Escape" => Code::Escape,
        "Minus" => Code::Minus,
        "Equal" => Code::Equal,
        "BracketLeft" => Code::BracketLeft,
        "BracketRight" => Code::BracketRight,
        "Backslash" => Code::Backslash,
        "Semicolon" => Code::Semicolon,
        "Quote" => Code::Quote,
        "Backquote" => Code::Backquote,
        "Comma" => Code::Comma,
        "Period" => Code::Period,
        "Slash" => Code::Slash,
        "CapsLock" => Code::CapsLock,
        "Menu" => Code::ContextMenu,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "Super" => Code::MetaLeft,
        "RightSuper" => Code::MetaRight,
        "LeftCtrl" => Code::ControlLeft,
        "RightCtrl" => Code::ControlRight,
        "Alt" => Code::AltLeft,
        "RightAlt" => Code::AltRight,
        "LeftShift" => Code::ShiftLeft,
        "RightShift" => Code::ShiftRight,
        _ => return None,
    })
}

fn portal_trigger(shortcut: &str) -> Result<String, String> {
    let canonical = echo_core::Config::canonical_shortcut(shortcut)
        .map_err(|err| format!("cannot parse portal shortcut {shortcut}: {err}"))?;
    let parts = canonical.split('+').collect::<Vec<_>>();
    let terminal = parts.last().copied().ok_or("shortcut has no key")?;
    let mut trigger = Vec::new();
    for modifier in parts.iter().take(parts.len().saturating_sub(1)) {
        trigger.push(match *modifier {
            "Super" => "LOGO",
            "Ctrl" => "CTRL",
            "Alt" => "ALT",
            "Shift" => "SHIFT",
            other => return Err(format!("unsupported portal modifier {other}")),
        });
    }
    trigger.push(
        portal_key_name(terminal)
            .ok_or_else(|| format!("unsupported portal shortcut key {terminal}"))?,
    );
    Ok(trigger.join("+"))
}

fn portal_key_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "A" => "a",
        "B" => "b",
        "C" => "c",
        "D" => "d",
        "E" => "e",
        "F" => "f",
        "G" => "g",
        "H" => "h",
        "I" => "i",
        "J" => "j",
        "K" => "k",
        "L" => "l",
        "M" => "m",
        "N" => "n",
        "O" => "o",
        "P" => "p",
        "Q" => "q",
        "R" => "r",
        "S" => "s",
        "T" => "t",
        "U" => "u",
        "V" => "v",
        "W" => "w",
        "X" => "x",
        "Y" => "y",
        "Z" => "z",
        "0" => "0",
        "1" => "1",
        "2" => "2",
        "3" => "3",
        "4" => "4",
        "5" => "5",
        "6" => "6",
        "7" => "7",
        "8" => "8",
        "9" => "9",
        "Space" => "space",
        "Enter" => "Return",
        "Tab" => "Tab",
        "Backspace" => "BackSpace",
        "Delete" => "Delete",
        "Insert" => "Insert",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "Page_Up",
        "PageDown" => "Page_Down",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "Escape" => "Escape",
        "Minus" => "minus",
        "Equal" => "equal",
        "BracketLeft" => "bracketleft",
        "BracketRight" => "bracketright",
        "Backslash" => "backslash",
        "Semicolon" => "semicolon",
        "Quote" => "apostrophe",
        "Backquote" => "grave",
        "Comma" => "comma",
        "Period" => "period",
        "Slash" => "slash",
        "CapsLock" => "Caps_Lock",
        "Menu" => "Menu",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        "Super" => "Super_L",
        "RightSuper" => "Super_R",
        "LeftCtrl" => "Control_L",
        "RightCtrl" => "Control_R",
        "Alt" => "Alt_L",
        "RightAlt" => "Alt_R",
        "LeftShift" => "Shift_L",
        "RightShift" => "Shift_R",
        _ => return None,
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
    let args: Vec<String> = env::args().skip(1).collect();
    if let Some(code) = try_cli(&args) {
        std::process::exit(code);
    }
    run_desktop();
}

/// Compositor shortcuts and hold-to-talk run these subcommands without
/// starting the webview.
fn try_cli(args: &[String]) -> Option<i32> {
    match args.first().map(String::as_str) {
        None => None,
        Some("rec") => Some(rec(args.get(1..).unwrap_or(&[]))),
        Some("--hud-demo") => Some(match echo::ui::hud::run_hud_demo() {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("hud-demo: {err}");
                1
            }
        }),
        Some("--version" | "-V") => {
            println!("echo-desktop {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        Some("--help" | "-h") => {
            print_cli_usage();
            Some(0)
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_cli_usage();
            Some(2)
        }
    }
}

fn rec(args: &[String]) -> i32 {
    // Bare CLI process: failures must reach the user where they are looking,
    // which is not the journal. The GUI's in-process sessions leave this off.
    echo::notify::enable_failure_notifications();
    match args {
        [arg] if arg == "--once" => echo::rec::run_rec_once(),
        [arg] if arg == "--toggle" => echo::rec::run_rec_toggle(),
        [arg] if arg == "--hold" => echo::rec::run_rec_hold(),
        _ => {
            eprintln!("usage: echo-desktop rec --once|--toggle|--hold");
            2
        }
    }
}

fn print_cli_usage() {
    eprintln!("usage: echo-desktop");
    eprintln!("       echo-desktop rec --once");
    eprintln!("       echo-desktop rec --toggle");
    eprintln!("       echo-desktop rec --hold");
    eprintln!("       echo-desktop --hud-demo");
    eprintln!("       echo-desktop --version");
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
            repair_legacy_shortcut,
            get_history,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            toggle_recording,
            get_recording_level,
            copy_text,
            remove_stale_installs,
            get_settings,
            set_settings,
            list_models,
            list_languages,
            list_model_offers,
            download_model,
            cancel_download,
            list_input_devices,
            test_input_device,
        ])
        .run(context);
    stop_native_shortcuts();
    stop_evdev_listener();
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
        assert_eq!(
            gnome_accelerator("alt+super+space").unwrap(),
            "<Super><Alt>space"
        );
        assert_eq!(
            gnome_accelerator("Ctrl+Shift+F9").unwrap(),
            "<Ctrl><Shift>F9"
        );
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
        let binding = gnome_accelerator(DEFAULT_TOGGLE_SHORTCUT).unwrap();
        update_native_status(|status| *status = NativeShortcutStatus::default());
        run_portal_shortcuts(
            DEFAULT_TOGGLE_SHORTCUT,
            "RightCtrl",
            &echo::audio::CancellationToken::new(),
        )
        .unwrap();
        assert!(native_shortcut_status().global_shortcuts_absent);

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
            stale["legacyShortcut"]["state"]
        );
        assert_eq!(stale["legacyShortcut"]["state"], "stale");

        let repaired = invoke_test_command(&webview, "repair_legacy_shortcut");
        assert_eq!(repaired["state"], "ready");
        let ready = invoke_test_command(&webview, "get_app_status");
        assert_eq!(ready["legacyShortcut"]["state"], "ready");
        assert_eq!(ready["shortcut"], DEFAULT_TOGGLE_SHORTCUT);
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
    #[ignore = "requires the current GNOME host without readable raw keyboard input"]
    fn denied_evdev_host_status_keeps_the_gnome_toggle_ready() {
        assert_eq!(
            echo::hotkey::DesktopSession::from_xdg_session_type(
                env::var("XDG_SESSION_TYPE").ok().as_deref()
            ),
            echo::hotkey::DesktopSession::Wayland
        );
        assert!(env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("gnome"));
        let hold = echo::hotkey::parse_hold_key("RightCtrl").unwrap();
        assert!(!matches!(
            echo::hotkey::probe_hold_devices(hold.code),
            echo::hotkey::EvdevAvailability::Ready(_)
        ));

        update_native_status(|status| {
            *status = NativeShortcutStatus::default();
            status.backend = ShortcutBackendName::Unsupported;
            status.error = Some(
                "Wayland session has no org.freedesktop.portal.GlobalShortcuts interface"
                    .to_string(),
            );
            status.global_shortcuts_absent = true;
        });
        *HEALTH.lock().expect("health cache lock") = Some((
            Instant::now(),
            Health {
                microphone_ready: false,
                engine_name: String::new(),
                engine_ready: false,
                injection_name: String::new(),
                injection_ready: false,
                current_exe: "/usr/bin/echo-desktop".to_string(),
                first_path_hit: None,
                stale_installs: Vec::new(),
            },
        ));
        reconcile_evdev_fallback(NativeHoldState::Failed);
        let deadline = Instant::now() + Duration::from_secs(2);
        while matches!(evdev_listener_state(), EvdevListenerState::Probing)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            evdev_listener_state(),
            EvdevListenerState::NeedsPermission(_) | EvdevListenerState::Unavailable(_)
        ));

        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![get_app_status])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let status = invoke_test_command(&webview, "get_app_status");
        assert!(matches!(
            status["holdListener"].as_str(),
            Some("needs-permission" | "unavailable")
        ));
        assert_eq!(status["legacyShortcut"]["state"], "ready");
        assert_eq!(status["shortcut"], DEFAULT_TOGGLE_SHORTCUT);
        let serialized = status.to_string().to_ascii_lowercase();
        assert!(!serialized.contains("sudo"));
        assert!(!serialized.contains("usermod"));
        assert!(!serialized.contains("input group"));
        stop_evdev_listener();
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
            toggle_shortcut: SettingField {
                value: Some("Ctrl+Alt+T".into()),
                effective: DEFAULT_TOGGLE_SHORTCUT.into(),
                source: SettingSource::Default,
            },
            hold_key: SettingField {
                value: Some("RightShift".into()),
                effective: "RightCtrl".into(),
                source: SettingSource::Default,
            },
            record_seconds: SettingField {
                value: Some(8),
                effective: 3,
                source: SettingSource::Default,
            },
            microphone: SettingField {
                value: Some("USB Mic".into()),
                effective: String::new(),
                source: SettingSource::Default,
            },
            language: SettingField {
                value: Some("de".into()),
                effective: "en".into(),
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
        assert_eq!(got.toggle_shortcut.value.as_deref(), Some("Ctrl+Alt+T"));
        assert_eq!(got.toggle_shortcut.effective, "Ctrl+Alt+T");
        assert_eq!(got.hold_key.value.as_deref(), Some("RightShift"));
        assert_eq!(got.record_seconds.value, Some(8));
        assert_eq!(got.record_seconds.effective, 8);
        assert_eq!(got.record_seconds.source, SettingSource::File);
        assert_eq!(got.microphone.value.as_deref(), Some("USB Mic"));
        assert_eq!(got.microphone.effective, "USB Mic");
        assert_eq!(got.microphone.source, SettingSource::File);
        assert_eq!(got.language.value.as_deref(), Some("de"));
        assert_eq!(got.language.effective, "de");
        assert_eq!(got.language.source, SettingSource::File);
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
    fn record_seconds_env_above_u32_max_clamps_like_recorder() {
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
        assert_eq!(
            settings.record_seconds.effective,
            echo::rec::MAX_RECORD_SECONDS as u32
        );
        assert_eq!(settings.record_seconds.source, SettingSource::Env);
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
    fn shortcut_projection_uses_env_file_default_precedence() {
        let env = SettingsEnv {
            toggle_shortcut: Some("alt+space+meta".into()),
            hold_key: Some("control+a".into()),
            ..SettingsEnv::default()
        };
        let file = Config {
            toggle_shortcut: Some("Ctrl+T".into()),
            hold_key: Some("RightCtrl".into()),
            ..Config::default()
        };
        let settings = settings_from(&env, &file, "en").unwrap();
        assert_eq!(settings.toggle_shortcut.effective, "Super+Alt+Space");
        assert_eq!(settings.toggle_shortcut.value.as_deref(), Some("Ctrl+T"));
        assert_eq!(settings.toggle_shortcut.source, SettingSource::Env);
        assert_eq!(settings.hold_key.effective, "Ctrl+A");
        assert_eq!(settings.hold_key.value.as_deref(), Some("RightCtrl"));
        assert_eq!(settings.hold_key.source, SettingSource::Env);

        let defaults = settings_from(&SettingsEnv::default(), &Config::default(), "en").unwrap();
        assert_eq!(defaults.toggle_shortcut.effective, DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(defaults.hold_key.effective, "RightCtrl");
    }

    #[test]
    fn invalid_shortcuts_are_rejected_before_config_save() {
        let mut settings =
            settings_from(&SettingsEnv::default(), &Config::default(), "en").unwrap();
        settings.toggle_shortcut.value = Some("F8".into());
        assert!(config_from_values(&settings).is_err());
        settings.toggle_shortcut.value = Some("Ctrl+Ctrl+T".into());
        assert!(config_from_values(&settings).is_err());
        settings.toggle_shortcut.value = None;
        settings.hold_key.value = Some(String::new());
        assert!(config_from_values(&settings).is_err());
    }

    #[test]
    fn invalid_shortcut_sources_fail_settings_projection() {
        let env = SettingsEnv {
            toggle_shortcut: Some(String::new()),
            ..SettingsEnv::default()
        };
        assert!(settings_from(&env, &Config::default(), "en").is_err());

        let file = Config {
            hold_key: Some("Ctrl+Ctrl+A".into()),
            ..Config::default()
        };
        assert!(settings_from(&SettingsEnv::default(), &file, "en").is_err());
    }

    #[test]
    fn native_adapters_parse_canonical_chords_and_single_right_ctrl() {
        let toggle = x11_hotkey("alt+space+meta").unwrap();
        assert_eq!(toggle.key, Code::Space);
        assert!(toggle.mods.contains(Modifiers::SUPER));
        assert!(toggle.mods.contains(Modifiers::ALT));

        let hold = x11_hotkey("RightCtrl").unwrap();
        assert_eq!(hold.key, Code::ControlRight);
        assert!(hold.mods.is_empty());
        assert_eq!(portal_trigger("Super+Alt+Space").unwrap(), "LOGO+ALT+space");
        assert_eq!(portal_trigger("RightCtrl").unwrap(), "Control_R");
        assert!(x11_hotkey("Ctrl+Ctrl+A").is_err());
    }

    #[test]
    fn fake_reconcile_is_idempotent_and_tears_down_before_registering() {
        #[derive(Default)]
        struct FakeAdapter {
            running: Option<(String, String)>,
            calls: Vec<&'static str>,
        }

        fn reconcile(adapter: &mut FakeAdapter, toggle: &str, hold: &str) {
            let desired = (toggle.to_string(), hold.to_string());
            if adapter.running.as_ref() == Some(&desired) {
                return;
            }
            if adapter.running.take().is_some() {
                adapter.calls.push("unregister");
            }
            adapter.calls.push("register");
            adapter.running = Some(desired);
        }

        let mut adapter = FakeAdapter::default();
        reconcile(&mut adapter, "Ctrl+T", "RightCtrl");
        reconcile(&mut adapter, "Ctrl+T", "RightCtrl");
        assert_eq!(adapter.calls, ["register"]);
        reconcile(&mut adapter, "Ctrl+Shift+T", "RightCtrl");
        assert_eq!(adapter.calls, ["register", "unregister", "register"]);
    }

    #[test]
    fn effective_trigger_change_never_mutates_requested_settings() {
        let mut status = NativeShortcutStatus {
            requested_toggle: "Super+Alt+Space".to_string(),
            requested_hold: "RightCtrl".to_string(),
            ..NativeShortcutStatus::default()
        };
        set_effective_shortcuts(
            &mut status,
            "Ctrl+Alt+T".to_string(),
            "Ctrl+Space".to_string(),
        );
        assert_eq!(status.requested_toggle, "Super+Alt+Space");
        assert_eq!(status.requested_hold, "RightCtrl");
        assert_eq!(status.effective_toggle.as_deref(), Some("Ctrl+Alt+T"));
        assert_eq!(status.effective_hold.as_deref(), Some("Ctrl+Space"));
    }

    #[test]
    fn ready_legacy_shortcut_projects_the_requested_toggle() {
        let native = NativeShortcutStatus {
            requested_toggle: "Super+Alt+Space".to_string(),
            ..NativeShortcutStatus::default()
        };
        let ready = LegacyShortcutSetup {
            state: LegacyShortcutState::Ready,
            detail: String::new(),
            command: "/usr/bin/echo-desktop rec --toggle".to_string(),
            binding: "<Super><Alt>space".to_string(),
        };
        assert_eq!(
            projected_toggle_shortcut(&native, Some(&ready)),
            "Super+Alt+Space"
        );
        assert_eq!(
            projected_toggle_shortcut(&native, None),
            "Unavailable (Super+Alt+Space)"
        );
    }

    #[test]
    fn fallback_plan_stops_for_native_and_supervises_failed_native_hold() {
        for native in [NativeHoldState::Probing, NativeHoldState::Healthy] {
            assert_eq!(
                evdev_fallback_plan(native),
                EvdevFallbackPlan::StopForNative
            );
        }
        assert_eq!(
            evdev_fallback_plan(NativeHoldState::Failed),
            EvdevFallbackPlan::Start
        );
        assert_eq!(EvdevListenerState::Active.projection(), "active");
        assert_eq!(EvdevListenerState::StoppedForNative.projection(), "native");
        assert_eq!(EvdevListenerState::Probing.projection(), "unavailable");
        assert_eq!(
            EvdevListenerState::NeedsPermission("denied".to_string()).projection(),
            "needs-permission"
        );
        let unavailable = EvdevListenerState::Unavailable("no keyboard".to_string());
        assert_eq!(unavailable.projection(), "unavailable");
        assert_eq!(unavailable.error(), Some("no keyboard"));
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

        let unavailable = NativeShortcutStatus::default();
        assert!(legacy_shortcut_setup(&unavailable, "/usr/bin/echo-desktop").is_none());
    }

    #[test]
    #[ignore = "needs an isolated X11 display"]
    fn x11_runtime_reports_conflicts_and_releases_grabs() {
        let toggle = "Ctrl+Alt+F12";
        let hold = "Ctrl+Alt+F11";
        let cancel = echo::audio::CancellationToken::new();
        let listener_cancel = cancel.clone();
        let listener =
            std::thread::spawn(move || run_x11_shortcuts(toggle, hold, &listener_cancel));

        let deadline = Instant::now() + Duration::from_secs(3);
        while !native_shortcut_status().healthy && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            native_shortcut_status().healthy,
            "X11 listener did not become healthy"
        );

        let competing = GlobalHotKeyManager::new().unwrap();
        assert!(
            competing.register(x11_hotkey(toggle).unwrap()).is_err(),
            "a competing X11 grab should be rejected"
        );

        cancel.cancel();
        listener.join().unwrap().unwrap();
        let after = GlobalHotKeyManager::new().unwrap();
        let released = x11_hotkey(toggle).unwrap();
        after.register(released).unwrap();
        after.unregister(released).unwrap();
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

        update_native_status(|status| status.healthy = false);
        let cancel = echo::audio::CancellationToken::new();
        let listener_cancel = cancel.clone();
        let (actions, received) = std::sync::mpsc::channel();
        *TEST_SHORTCUT_ACTIONS
            .lock()
            .expect("test shortcut observer lock") = Some(actions);
        let listener = std::thread::spawn(move || {
            run_x11_shortcuts("Ctrl+Shift+9", "Ctrl+Shift+8", &listener_cancel)
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while !native_shortcut_status().healthy && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(native_shortcut_status().healthy, "X11 listener not ready");

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

        send_shortcut("ctrl+shift+9");
        for expected in [
            TestShortcutAction::Edge(
                TOGGLE_SHORTCUT_ID.to_string(),
                echo::hotkey::HotkeyEvent::Down,
            ),
            TestShortcutAction::Toggle,
            TestShortcutAction::Edge(
                TOGGLE_SHORTCUT_ID.to_string(),
                echo::hotkey::HotkeyEvent::Up,
            ),
        ] {
            receive_expected(expected);
        }
        recording_env.assert_active();

        send_shortcut("ctrl+shift+9");
        for expected in [
            TestShortcutAction::Edge(
                TOGGLE_SHORTCUT_ID.to_string(),
                echo::hotkey::HotkeyEvent::Down,
            ),
            TestShortcutAction::Toggle,
            TestShortcutAction::Edge(
                TOGGLE_SHORTCUT_ID.to_string(),
                echo::hotkey::HotkeyEvent::Up,
            ),
        ] {
            receive_expected(expected);
        }
        recording_env.wait_until_inactive();

        send_shortcut("ctrl+shift+8");
        for expected in [
            TestShortcutAction::Edge(
                HOLD_SHORTCUT_ID.to_string(),
                echo::hotkey::HotkeyEvent::Down,
            ),
            TestShortcutAction::HoldStart,
            TestShortcutAction::Edge(HOLD_SHORTCUT_ID.to_string(), echo::hotkey::HotkeyEvent::Up),
            TestShortcutAction::HoldStop,
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
