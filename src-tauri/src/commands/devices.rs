use std::env;
use std::sync::{Mutex, OnceLock};

use echo::audio::AudioCapture;
use echo_desktop::ipc::{
    EngineAvailability, LanguageGroup, LanguageMode, LanguageOption, LanguageOptions,
    ModelInventory, WhisperModelInfo,
};

#[tauri::command]
pub(crate) fn list_languages() -> LanguageOptions {
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

static GPU_DEVICES: OnceLock<Mutex<Option<Vec<echo::stt::GpuDevice>>>> = OnceLock::new();

#[tauri::command]
pub(crate) fn list_gpu_devices(refresh: bool) -> Vec<echo_desktop::ipc::GpuDevice> {
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
pub(crate) fn list_models() -> Result<ModelInventory, String> {
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
pub(crate) fn get_microphones() -> echo_desktop::ipc::MicrophoneSnapshot {
    echo::audio::microphone_snapshot().into()
}

#[tauri::command]
pub(crate) fn set_microphone(
    id: Option<String>,
) -> Result<echo_desktop::ipc::MicrophoneSnapshot, String> {
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
    crate::settings::update_file_config(|config| {
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
pub(crate) fn test_input_device(
    id: Option<String>,
) -> Result<echo_desktop::ipc::MicrophoneTestResult, String> {
    let id = id.map(echo::microphone::MicrophoneId::parse).transpose()?;
    Ok(microphone_test(AudioCapture::open_exact(id.as_ref())).into())
}

#[tauri::command]
pub(crate) fn test_microphone_fallback() -> echo_desktop::ipc::MicrophoneTestResult {
    microphone_test(AudioCapture::open_default()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_microphone_update_writes_id_and_clears_legacy_name() {
        let mut config = echo_core::Config {
            microphone: Some(echo_core::MicrophoneSelection::LegacyName {
                name: "USB Mic".into(),
            }),
            ..echo_core::Config::default()
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
}
