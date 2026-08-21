use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use echo::audio::AudioCapture;
use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{DictEntry, Dictionary, History};
use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

/// Keyboard binding the README recommends. Echo never registers it; the
/// desktop environment owns global shortcuts.
const SUGGESTED_SHORTCUT: &str = "Super+Alt+Space";

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
    hold_key: SettingField<String>,
    record_seconds: SettingField<u32>,
    microphone: SettingField<String>,
}

#[derive(Debug, Default, Clone)]
struct SettingsEnv {
    engine: Option<String>,
    whisper_model: Option<String>,
    cleanup: Option<String>,
    hud: Option<String>,
    hold_key: Option<String>,
    record_seconds: Option<String>,
    microphone: Option<String>,
}

#[derive(Debug, Clone)]
struct Health {
    microphone_ready: bool,
    engine_name: String,
    engine_ready: bool,
    injection_name: String,
    injection_ready: bool,
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
    let health = Health {
        microphone_ready: AudioCapture::open_default().is_ok(),
        engine_name,
        engine_ready,
        injection_name,
        injection_ready,
    };
    *cache = Some((Instant::now(), health.clone()));
    health
}

fn health_invalidate() {
    *HEALTH.lock().expect("health cache lock") = None;
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
    AppStatus {
        recording: status.state == "Recording",
        phase: status.state,
        last_transcript: status.last,
        microphone_ready: health.microphone_ready,
        engine_name: health.engine_name,
        engine_ready: health.engine_ready,
        injection_name: health.injection_name,
        injection_ready: health.injection_ready,
        shortcut: SUGGESTED_SHORTCUT.to_string(),
        cleanup_name: echo::cleanup::mode_name(),
        hud_enabled: echo::ui::hud::enabled(),
        max_record_seconds: echo::rec::MAX_RECORD_SECONDS,
        settings_path: echo_core::config_path().to_string_lossy().into_owned(),
    }
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

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    SysClipboard.set(&text)
}

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    read_settings()
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
    let capture = echo::audio::AudioCapture::open(name.as_deref()).map_err(|err| err.to_string())?;
    let result = capture
        .record(std::time::Duration::from_secs(1))
        .map_err(|err| err.to_string())?;
    Ok(result.peak_rms)
}

fn read_settings() -> Result<Settings, String> {
    Ok(settings_from(
        &process_settings_env(),
        &echo::settings::file_config(),
    ))
}

fn write_settings(settings: Settings) -> Result<Settings, String> {
    config_from_values(&settings)?.save()?;
    echo::settings::reload();
    health_invalidate();
    read_settings()
}

fn process_settings_env() -> SettingsEnv {
    SettingsEnv {
        engine: env::var("ECHO_ENGINE").ok(),
        whisper_model: env::var("ECHO_WHISPER_MODEL").ok(),
        cleanup: env::var("ECHO_CLEANUP").ok(),
        hud: env::var("ECHO_HUD").ok(),
        hold_key: env::var("ECHO_HOLD_KEY").ok(),
        record_seconds: env::var("ECHO_RECORD_SECONDS").ok(),
        microphone: env::var("ECHO_MICROPHONE").ok(),
    }
}

fn settings_from(env: &SettingsEnv, file: &echo_core::Config) -> Settings {
    let env_cleanup = env
        .cleanup
        .as_deref()
        .and_then(|raw| echo_core::CleanupMode::parse(raw).ok())
        .map(|mode| cleanup_name(&mode));
    Settings {
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
            "base.en".to_string(),
        ),
        cleanup: setting_field(
            env_cleanup,
            file.cleanup.as_ref().map(cleanup_name),
            "rules".to_string(),
        ),
        hud: hud_field(env.hud.as_deref(), file.hud),
        hold_key: setting_field(
            env.hold_key.clone(),
            file.hold_key.clone(),
            "RightCtrl".to_string(),
        ),
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
    }
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
    config.hold_key = nonempty(settings.hold_key.value.clone());
    config.record_seconds = settings
        .record_seconds
        .value
        .map(|secs| secs.clamp(1, echo::rec::MAX_RECORD_SECONDS as u32));
    config.microphone = nonempty(settings.microphone.value.clone());
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
    std::thread::Builder::new()
        .name("echo-record-toggle".to_string())
        .spawn(|| {
            let _ = echo::rec::run_rec_toggle();
        })
        .map(|_| ())
        .map_err(|err| err.to_string())
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
}

fn run_desktop() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.tray_icon = None;
    tauri::Builder::default()
        .setup(|app| {
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
            get_history,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            toggle_recording,
            copy_text,
            get_settings,
            set_settings,
            list_input_devices,
            test_input_device,
        ])
        .run(context)
        .expect("error while running Echo");
}

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
        let settings = settings_from(&env, &file);
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
        };
        config_from_values(&incoming)
            .unwrap()
            .save_to(&path)
            .unwrap();
        let loaded = Config::load_from(&path).unwrap();
        let got = settings_from(&SettingsEnv::default(), &loaded);
        assert_eq!(got.engine.value.as_deref(), Some("parakeet"));
        assert_eq!(got.engine.effective, "parakeet");
        assert_eq!(got.engine.source, SettingSource::File);
        assert_eq!(got.whisper_model.value.as_deref(), Some("tiny.en"));
        assert_eq!(got.whisper_model.effective, "tiny.en");
        assert_eq!(got.cleanup.value.as_deref(), Some("off"));
        assert_eq!(got.cleanup.effective, "off");
        assert_eq!(got.hud.value, Some(false));
        assert!(!got.hud.effective);
        assert_eq!(got.hold_key.value.as_deref(), Some("RightShift"));
        assert_eq!(got.record_seconds.value, Some(8));
        assert_eq!(got.record_seconds.effective, 8);
        assert_eq!(got.record_seconds.source, SettingSource::File);
        assert_eq!(got.microphone.value.as_deref(), Some("USB Mic"));
        assert_eq!(got.microphone.effective, "USB Mic");
        assert_eq!(got.microphone.source, SettingSource::File);
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
        let settings = settings_from(&env, &file);
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
            let settings = settings_from(&env, &file);
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
            let settings = settings_from(&env, &enabled);
            assert!(!settings.hud.effective, "token {token}");
            assert_eq!(settings.hud.source, SettingSource::Env, "token {token}");
        }
        let unknown = SettingsEnv {
            hud: Some("maybe".into()),
            ..SettingsEnv::default()
        };
        assert!(!settings_from(&unknown, &disabled).hud.effective);
        assert_eq!(
            settings_from(&unknown, &disabled).hud.source,
            SettingSource::File
        );
        assert!(settings_from(&unknown, &enabled).hud.effective);
    }
}
