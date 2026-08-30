use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use echo::audio::AudioCapture;
use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{Dictionary, History, RunDetail, WhisperAccelerationSkip};
#[cfg(test)]
use echo_core::{
    WhisperRunMode, WhisperRuntimeBackend, WhisperRuntimeSource, WhisperTuningTelemetry,
};
use echo_desktop::ipc::{
    AccelerationSkipReason, AppStatus, DictionaryItem, EngineAvailability, HistoryItem,
    LanguageGroup, LanguageMode, LanguageOption, LanguageOptions, LastRun, LastRunPerformance,
    LegacyShortcutSetup, ModelInventory, RecordingPolicy, SettingField, SettingSource, Settings,
    ShortcutStatus, WhisperModelInfo,
};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

mod cli;
mod setup;
mod shortcuts;

const APP_ID: &str = "io.github.ddv1982.echo";
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
fn recording_policy_dto() -> RecordingPolicy {
    RecordingPolicy {
        minimum_seconds: echo_core::RecordingLimit::MIN.seconds(),
        default_seconds: echo_core::RecordingLimit::DEFAULT.seconds(),
        maximum_seconds: echo_core::RecordingLimit::MAX.seconds(),
        presets_seconds: echo_core::RecordingLimit::PRESETS
            .map(echo_core::RecordingLimit::seconds)
            .to_vec(),
    }
}

fn project_acceleration_skip(
    whisper: &echo_core::WhisperRunTelemetry,
) -> Option<AccelerationSkipReason> {
    if let Some(skip) = whisper.skipped_acceleration {
        return Some(match skip {
            WhisperAccelerationSkip::RuntimeMissing => AccelerationSkipReason::RuntimeMissing,
            WhisperAccelerationSkip::NoDeviceEnumerated => {
                AccelerationSkipReason::NoDeviceEnumerated
            }
            WhisperAccelerationSkip::PinnedDeviceAbsent => {
                AccelerationSkipReason::PinnedDeviceAbsent
            }
            WhisperAccelerationSkip::DeviceQuarantined => AccelerationSkipReason::DeviceQuarantined,
            WhisperAccelerationSkip::CpuFallbackMissing => {
                AccelerationSkipReason::CpuFallbackMissing
            }
            WhisperAccelerationSkip::DeviceNotReady => AccelerationSkipReason::DeviceNotReady,
        });
    }
    // A recovery row does not on its own mean the GPU ran. A quarantine that
    // lands between preparation and execution retreats without attempting
    // anything, so accelerated_attempted is what separates a run that lost
    // from a run that never started.
    let recovery = whisper.recovery.as_ref()?;
    recovery.fallback_reason?;
    Some(if recovery.accelerated_attempted {
        AccelerationSkipReason::RecoveredToCpu
    } else {
        AccelerationSkipReason::DeviceQuarantined
    })
}

fn project_last_run_performance(detail: &RunDetail) -> Option<LastRunPerformance> {
    let whisper = detail.whisper.as_ref()?;
    Some(LastRunPerformance {
        mode: whisper.mode.into(),
        runtime_source: whisper.runtime.source.into(),
        backend: whisper.runtime.backend.into(),
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
        tuning: whisper.tuning.into(),
        acceleration_skip: project_acceleration_skip(whisper),
        recovery: whisper.recovery.clone().map(Into::into),
    })
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

#[tauri::command]
fn get_app_status() -> AppStatus {
    let status = echo::status::read();
    let recording_limit =
        project_recording_limit(&status, echo::rec::recording_limit_from_process().limit);
    let health = health_snapshot();
    let shortcut = shortcuts::status(&health.current_exe);
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

#[tauri::command]
fn get_shortcut_status() -> ShortcutStatus {
    shortcuts::status(&current_exe_string())
}

fn current_exe_string() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
fn retry_shortcut() -> ShortcutStatus {
    shortcuts::retry()
}

#[tauri::command]
fn repair_legacy_shortcut() -> Result<LegacyShortcutSetup, String> {
    shortcuts::repair(&current_exe_string())
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

#[tauri::command]
fn list_languages() -> LanguageOptions {
    match echo::stt::language_support() {
        echo::stt::LanguageSupport::WhisperMultilingual => LanguageOptions {
            mode: LanguageMode::Multilingual,
            model: None,
            options: echo_core::Language::all()
                .map(|language| LanguageOption {
                    code: language.code().to_string(),
                    english_name: language.english_name().to_string(),
                    group: if ["en", "de", "es", "fr"].contains(&language.code()) {
                        LanguageGroup::Common
                    } else {
                        LanguageGroup::All
                    },
                })
                .collect(),
        },
        echo::stt::LanguageSupport::WhisperEnglishOnly { model } => LanguageOptions {
            mode: LanguageMode::English,
            model: Some(model),
            options: vec![LanguageOption {
                code: "en".to_string(),
                english_name: "english".to_string(),
                group: LanguageGroup::Common,
            }],
        },
        echo::stt::LanguageSupport::Parakeet => LanguageOptions {
            mode: LanguageMode::Parakeet,
            model: None,
            options: echo_core::PARAKEET_LANGUAGES
                .iter()
                .filter_map(|code| echo_core::Language::from_code(code))
                .map(|language| LanguageOption {
                    code: language.code().to_string(),
                    english_name: language.english_name().to_string(),
                    group: LanguageGroup::All,
                })
                .collect(),
        },
    }
}

/// Enumeration spawns one probe subprocess per ICD manifest, so the result is
/// held for the process and refreshed only when the user asks.
static GPU_DEVICES: OnceLock<Mutex<Option<Vec<echo::stt::GpuDevice>>>> = OnceLock::new();

#[tauri::command]
fn list_gpu_devices(refresh: bool) -> Vec<echo_desktop::ipc::GpuDevice> {
    let cell = GPU_DEVICES.get_or_init(|| Mutex::new(None));
    let Ok(mut cached) = cell.lock() else {
        return echo::stt::list_gpu_devices()
            .into_iter()
            .map(Into::into)
            .collect();
    };
    if refresh {
        *cached = None;
    }
    cached
        .get_or_insert_with(echo::stt::list_gpu_devices)
        .clone()
        .into_iter()
        .map(Into::into)
        .collect()
}

#[tauri::command]
fn list_models() -> Result<ModelInventory, String> {
    let cache = echo::stt::ModelCache::from_env();
    let inventory = echo::stt::SpeechRuntimeInventory::from_cache(&cache).models;
    Ok(ModelInventory {
        whisper: inventory
            .whisper
            .iter()
            .map(|model| WhisperModelInfo {
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
            .map(|engine| EngineAvailability {
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
fn get_microphones() -> echo_desktop::ipc::MicrophoneSnapshot {
    echo::audio::microphone_snapshot().into()
}

#[tauri::command]
fn set_microphone(id: Option<String>) -> Result<echo_desktop::ipc::MicrophoneSnapshot, String> {
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
    Ok(echo::audio::microphone_snapshot().into())
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
fn test_input_device(
    id: Option<String>,
) -> Result<echo_desktop::ipc::MicrophoneTestResult, String> {
    let id = id.map(echo::microphone::MicrophoneId::parse).transpose()?;
    Ok(microphone_test(AudioCapture::open_exact(id.as_ref())).into())
}

#[tauri::command]
fn test_microphone_fallback() -> echo_desktop::ipc::MicrophoneTestResult {
    microphone_test(AudioCapture::open_default()).into()
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
        whisper_gpu_device: setting_field(
            None,
            file.whisper_gpu_device
                .as_deref()
                .and_then(parse_gpu_device),
            String::new(),
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
    config.whisper_gpu_device = match settings.whisper_gpu_device.value.as_deref() {
        None | Some("") => None,
        Some(raw) => {
            Some(parse_gpu_device(raw).ok_or_else(|| format!("unknown GPU device {raw}"))?)
        }
    };
    Ok(config)
}

/// A pinned device is `deviceUUID:driverUUID`, both nonzero lowercase 32-hex.
/// Anything else is refused at the boundary rather than carried inward.
fn parse_gpu_device(raw: &str) -> Option<String> {
    let (device, driver) = raw.split_once(':')?;
    [device, driver]
        .iter()
        .all(|uuid| {
            uuid.len() == 32
                && uuid
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && !uuid.bytes().all(|b| b == b'0')
        })
        .then(|| raw.to_string())
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
            shortcuts::reconcile();
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
            list_gpu_devices,
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
    shortcuts::shutdown();
    result.expect("error while running Echo");
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
            whisper_gpu_device: SettingField {
                value: Some(format!("{}:{}", "a".repeat(32), "b".repeat(32))),
                effective: String::new(),
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
        assert_eq!(got.whisper_acceleration.value.as_deref(), Some("gpu"));
        assert_eq!(got.whisper_acceleration.effective, "gpu");
        assert_eq!(got.whisper_acceleration.source, SettingSource::File);
        let pinned = format!("{}:{}", "a".repeat(32), "b".repeat(32));
        assert_eq!(
            got.whisper_gpu_device.value.as_deref(),
            Some(pinned.as_str())
        );
        assert_eq!(got.whisper_gpu_device.source, SettingSource::File);
    }

    #[test]
    fn legacy_auto_acceleration_settings_resolve_to_cpu() {
        let path = scratch_path("legacy-auto-acceleration");
        std::fs::write(&path, r#"{"whisper_acceleration":"auto"}"#).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        let got = settings_from(&SettingsEnv::default(), &loaded, "en").unwrap();
        assert_eq!(got.whisper_acceleration.value.as_deref(), Some("cpu"));
        assert_eq!(got.whisper_acceleration.effective, "cpu");
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
                skipped_acceleration: None,
            }),
            ..RunDetail::default()
        };
        let projected = project_last_run_performance(&detail).unwrap();
        assert_eq!(projected.mode, WhisperRunMode::ColdFallback.into());
        assert_eq!(projected.child_wall_ms, 1_210);
        assert_eq!(projected.attempt_count, 2);
        assert_eq!(projected.tuning.threads, Some(4));
        assert_eq!(projected.device.as_deref(), Some("Test CPU"));
        assert_eq!(projected.acceleration_skip, None);
    }

    fn cpu_telemetry() -> echo_core::WhisperRunTelemetry {
        echo_core::WhisperRunTelemetry {
            mode: WhisperRunMode::ColdCli,
            total_ms: 100,
            audio_encode_ms: 1,
            parse_ms: 1,
            runtime: echo_core::WhisperRuntimeTelemetry {
                binary: "/usr/bin/whisper-cli".to_string(),
                source: WhisperRuntimeSource::Managed,
                backend: WhisperRuntimeBackend::Cpu,
                device: None,
                library_path: None,
                vulkan_driver_files: None,
                mesa_shader_cache_dir: None,
                identity_sha256: None,
                vulkan_receipt: None,
            },
            tuning: WhisperTuningTelemetry {
                threads: None,
                beam_size: Some(3),
                best_of: Some(5),
                no_fallback: Some(false),
            },
            attempts: Vec::new(),
            recovery: None,
            skipped_acceleration: None,
        }
    }

    #[test]
    fn every_gate_refusal_reaches_the_readout() {
        for (skip, expected) in [
            (
                WhisperAccelerationSkip::RuntimeMissing,
                AccelerationSkipReason::RuntimeMissing,
            ),
            (
                WhisperAccelerationSkip::NoDeviceEnumerated,
                AccelerationSkipReason::NoDeviceEnumerated,
            ),
            (
                WhisperAccelerationSkip::PinnedDeviceAbsent,
                AccelerationSkipReason::PinnedDeviceAbsent,
            ),
            (
                WhisperAccelerationSkip::DeviceQuarantined,
                AccelerationSkipReason::DeviceQuarantined,
            ),
            (
                WhisperAccelerationSkip::CpuFallbackMissing,
                AccelerationSkipReason::CpuFallbackMissing,
            ),
            (
                WhisperAccelerationSkip::DeviceNotReady,
                AccelerationSkipReason::DeviceNotReady,
            ),
        ] {
            let mut whisper = cpu_telemetry();
            whisper.skipped_acceleration = Some(skip);
            assert_eq!(
                project_acceleration_skip(&whisper),
                Some(expected),
                "{skip:?}"
            );
        }
    }

    #[test]
    fn a_failed_accelerated_run_reports_the_retreat_not_its_diagnosis() {
        let mut whisper = cpu_telemetry();
        whisper.recovery = Some(echo_core::WhisperRecoveryTelemetry {
            identity_key: "accelerator".to_string(),
            accelerated_attempted: true,
            fallback_reason: Some(echo_core::WhisperRecoveryReason::Timeout),
        });
        assert_eq!(
            project_acceleration_skip(&whisper),
            Some(AccelerationSkipReason::RecoveredToCpu),
        );
    }

    #[test]
    fn a_quarantine_hit_is_not_reported_as_a_failed_gpu_run() {
        // RecoveringWhisperEngine re-checks the quarantine after preparation,
        // so an overlapping run that poisons it lands here having attempted
        // nothing. Collapsing that into RecoveredToCpu told the user "GPU ran
        // and failed" about a run the GPU never saw.
        for reason in [
            echo_core::WhisperRecoveryReason::Quarantined,
            echo_core::WhisperRecoveryReason::QuarantineUnreadable,
        ] {
            let mut whisper = cpu_telemetry();
            whisper.recovery = Some(echo_core::WhisperRecoveryTelemetry {
                identity_key: "accelerator".to_string(),
                accelerated_attempted: false,
                fallback_reason: Some(reason),
            });
            assert_eq!(
                project_acceleration_skip(&whisper),
                Some(AccelerationSkipReason::DeviceQuarantined),
                "{reason:?}"
            );
        }
    }

    #[test]
    fn an_accelerated_run_that_kept_the_gpu_reports_no_skip() {
        let mut whisper = cpu_telemetry();
        whisper.runtime.backend = WhisperRuntimeBackend::Vulkan;
        whisper.recovery = Some(echo_core::WhisperRecoveryTelemetry {
            identity_key: "accelerator".to_string(),
            accelerated_attempted: true,
            fallback_reason: None,
        });
        assert_eq!(project_acceleration_skip(&whisper), None);
    }
}
