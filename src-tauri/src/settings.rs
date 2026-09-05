use std::collections::VecDeque;
use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use echo_desktop::ipc::{
    ChannelReply, Readiness, SettingField, SettingSource, Settings, SettingsChange,
    SettingsSnapshot,
};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};

static SNAPSHOT_RESPONSE_REVISION: AtomicU64 = AtomicU64::new(0);

struct ConfigJob {
    run: Box<dyn FnOnce() + Send>,
    fail: Box<dyn FnOnce(String) + Send>,
}

impl ConfigJob {
    fn fire_and_forget(run: impl FnOnce() + Send + 'static) -> Self {
        Self {
            run: Box::new(run),
            fail: Box::new(|error| eprintln!("configuration queue: {error}")),
        }
    }

    fn reply<T>(
        reply: Channel<ChannelReply<T>>,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Self
    where
        T: serde::Serialize + Send + 'static,
    {
        let reply = Arc::new(Mutex::new(Some(reply)));
        let run_reply = Arc::clone(&reply);
        let fail_reply = Arc::clone(&reply);
        Self {
            run: Box::new(move || {
                let result = match panic::catch_unwind(AssertUnwindSafe(work)) {
                    Ok(result) => result,
                    Err(_) => Err("configuration request failed unexpectedly".to_string()),
                };
                send_reply_once(&run_reply, result);
            }),
            fail: Box::new(move |error| {
                send_reply_once(&fail_reply, Err(error));
            }),
        }
    }
}

#[derive(Default)]
struct ConfigJobQueue {
    active: bool,
    pending: VecDeque<ConfigJob>,
}

impl ConfigJobQueue {
    fn enqueue(&mut self, job: ConfigJob) -> bool {
        self.pending.push_back(job);
        if self.active {
            false
        } else {
            self.active = true;
            true
        }
    }

    fn next(&mut self) -> Option<ConfigJob> {
        let next = self.pending.pop_front();
        if next.is_none() {
            self.active = false;
        }
        next
    }
}

#[derive(Clone, Default)]
pub(crate) struct ConfigMutationService {
    queue: Arc<Mutex<ConfigJobQueue>>,
}

impl ConfigMutationService {
    fn enqueue(&self, job: ConfigJob) -> Result<(), String> {
        let start_worker = self
            .queue
            .lock()
            .map(|mut queue| queue.enqueue(job))
            .map_err(|_| "configuration queue is unavailable".to_string())?;
        if start_worker {
            let service = self.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("echo-config-owner".to_string())
                .spawn(move || service.drain())
            {
                let message = format!("configuration worker could not start: {error}");
                self.fail_pending_jobs(message.clone());
                return Err(message);
            }
        }
        Ok(())
    }

    fn fail_pending_jobs(&self, error: String) {
        let jobs = match self.queue.lock() {
            Ok(mut queue) => {
                queue.active = false;
                queue.pending.drain(..).collect::<Vec<_>>()
            }
            Err(_) => return,
        };
        for job in jobs {
            (job.fail)(error.clone());
        }
    }

    fn drain(self) {
        loop {
            let job = match self.queue.lock() {
                Ok(mut queue) => queue.next(),
                Err(_) => {
                    eprintln!("configuration queue is unavailable");
                    return;
                }
            };
            let Some(job) = job else {
                return;
            };
            let fail = job.fail;
            if panic::catch_unwind(AssertUnwindSafe(job.run)).is_err() {
                fail("configuration request failed unexpectedly".to_string());
            }
        }
    }

    pub(crate) fn request_settings_snapshot(
        &self,
        mut readiness: impl FnMut() -> Readiness + Send + 'static,
        reply: Channel<ChannelReply<SettingsSnapshot>>,
    ) -> Result<(), String> {
        self.enqueue(ConfigJob::reply(reply, move || {
            snapshot_with_revision(&mut readiness).map(|(_, snapshot)| snapshot)
        }))
    }

    pub(crate) fn request_settings_change(
        &self,
        settings_change: SettingsChange,
        mut readiness: impl FnMut() -> Readiness + Send + 'static,
        app: tauri::AppHandle,
        tray_request: crate::tray::LanguageMenuRequest,
        reply: Channel<ChannelReply<SettingsSnapshot>>,
    ) -> Result<(), String> {
        self.enqueue(ConfigJob::reply(reply, move || {
            change(settings_change)
                .and_then(|_| snapshot_with_revision(&mut readiness))
                .map(|(revision, snapshot)| {
                    crate::tray::sync(&app, tray_request, revision, &snapshot);
                    snapshot
                })
        }))
    }

    pub(crate) fn request_microphone_snapshot(
        &self,
        reply: Channel<ChannelReply<echo_desktop::ipc::MicrophoneSnapshot>>,
    ) -> Result<(), String> {
        self.enqueue(ConfigJob::reply(reply, move || {
            Ok(revisioned_microphone_snapshot())
        }))
    }

    pub(crate) fn request_microphone_selection(
        &self,
        id: Option<String>,
        app: tauri::AppHandle,
        reply: Channel<ChannelReply<echo_desktop::ipc::MicrophoneSnapshot>>,
    ) -> Result<(), String> {
        self.enqueue(ConfigJob::reply(reply, move || {
            set_microphone_selection(id).map(|()| {
                let snapshot = revisioned_microphone_snapshot();
                let _ = app.emit("settings-event", ());
                snapshot
            })
        }))
    }

    pub(crate) fn request_tray_language(
        &self,
        value: String,
        app: tauri::AppHandle,
        tray_request: crate::tray::LanguageMenuRequest,
    ) -> Result<(), String> {
        let service = app.state::<crate::setup::SetupService>().inner().clone();
        self.enqueue(ConfigJob::fire_and_forget(move || {
            let outcome = change(SettingsChange::Language { value: Some(value) })
                .and_then(|_| snapshot_with_revision(|| service.snapshot()));
            match outcome {
                Ok((revision, snapshot)) => {
                    crate::tray::sync(&app, tray_request, revision, &snapshot);
                    let _ = app.emit("settings-event", ());
                }
                Err(error) => {
                    eprintln!("tray language: {error}");
                    crate::tray::restore(&app);
                }
            }
        }))
    }

    pub(crate) fn request_tray_refresh(
        &self,
        app: tauri::AppHandle,
        tray_request: crate::tray::LanguageMenuRequest,
    ) -> Result<(), String> {
        let service = app.state::<crate::setup::SetupService>().inner().clone();
        self.enqueue(ConfigJob::fire_and_forget(
            move || match snapshot_with_revision(|| service.snapshot()) {
                Ok((settings, snapshot)) => crate::tray::sync_requested(
                    &app,
                    crate::tray::LanguageMenuRevision {
                        settings,
                        request: tray_request.0,
                    },
                    &snapshot,
                ),
                Err(error) => eprintln!("tray language: failed to read settings: {error}"),
            },
        ))
    }

    pub(crate) fn apply_setup_plan_blocking(
        &self,
        plan_id: echo::install::SetupPlanId,
    ) -> Result<(), echo::install::InstallError> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let fail_sender = sender.clone();
        self.enqueue(ConfigJob {
            run: Box::new(move || {
                let result = match panic::catch_unwind(AssertUnwindSafe(|| {
                    update_file_config(|config| apply_plan_config(config, plan_id))
                })) {
                    Ok(result) => result.map_err(echo::install::InstallError::IoMessage),
                    Err(_) => Err(echo::install::InstallError::IoMessage(
                        "configuration request failed unexpectedly".to_string(),
                    )),
                };
                let _ = sender.send(result);
            }),
            fail: Box::new(move |error| {
                let _ = fail_sender.send(Err(echo::install::InstallError::IoMessage(error)));
            }),
        })
        .map_err(echo::install::InstallError::IoMessage)?;
        receiver
            .recv()
            .map_err(|error| echo::install::InstallError::IoMessage(error.to_string()))?
    }
}

fn send_reply_once<T>(
    reply: &Arc<Mutex<Option<Channel<ChannelReply<T>>>>>,
    result: Result<T, String>,
) where
    T: serde::Serialize,
{
    let Ok(mut reply) = reply.lock() else {
        return;
    };
    let Some(reply) = reply.take() else {
        return;
    };
    let message = match result {
        Ok(value) => ChannelReply::Ok { value },
        Err(error) => ChannelReply::Err { error },
    };
    let _ = reply.send(message);
}

fn next_snapshot_response_revision() -> u64 {
    SNAPSHOT_RESPONSE_REVISION.fetch_add(1, Ordering::SeqCst) + 1
}

fn with_snapshot_revision(mut snapshot: SettingsSnapshot) -> SettingsSnapshot {
    snapshot.revision = next_snapshot_response_revision();
    snapshot
}

fn revisioned_microphone_snapshot() -> echo_desktop::ipc::MicrophoneSnapshot {
    let mut snapshot: echo_desktop::ipc::MicrophoneSnapshot =
        echo::audio::microphone_snapshot().into();
    snapshot.revision = next_snapshot_response_revision();
    snapshot
}

fn update_file_config(
    update: impl FnOnce(&mut echo_core::Config) -> Result<(), String>,
) -> Result<(), String> {
    update_file_config_at(&echo_core::config_path(), update)?;
    echo::settings::reload();
    crate::status::health_invalidate();
    Ok(())
}

fn update_file_config_at(
    path: &Path,
    update: impl FnOnce(&mut echo_core::Config) -> Result<(), String>,
) -> Result<(), String> {
    let mut config = load_preferences_for_update(path)?;
    update(&mut config)?;
    config.save_to(path)
}

fn load_preferences_for_update(path: &Path) -> Result<echo_core::Config, String> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A dangling symlink also reports NotFound, but it is an existing
            // preference entry and must not be replaced as if it were absent.
            match std::fs::symlink_metadata(path) {
                Err(metadata_error) if metadata_error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(echo_core::Config::default());
                }
                _ => return Err(preferences_read_error(path, &error)),
            }
        }
        Err(error) => return Err(preferences_read_error(path, &error)),
    };
    serde_json::from_slice(&raw).map_err(|error| {
        format!(
            "Preferences at {} are invalid and were left unchanged: {error}",
            path.display()
        )
    })
}

fn preferences_read_error(path: &Path, error: &std::io::Error) -> String {
    format!(
        "Could not read existing preferences at {}; changes were not saved: {error}",
        path.display()
    )
}

#[derive(Debug, Default, Clone)]
struct SettingsEnv {
    engine: Option<String>,
    whisper_model: Option<String>,
    hud: Option<String>,
    record_seconds: Option<String>,
    language: Option<String>,
    whisper_acceleration: Option<String>,
}

fn snapshot(readiness: Readiness) -> Result<SettingsSnapshot, String> {
    let file = load_preferences_for_update(&echo_core::config_path())?;
    let preferences = read_from_file(&file)?;
    Ok(crate::speech::snapshot(preferences, &file, readiness))
}

fn snapshot_with_revision(
    mut readiness: impl FnMut() -> Readiness,
) -> Result<(u64, SettingsSnapshot), String> {
    let snapshot = with_snapshot_revision(snapshot(readiness())?);
    Ok((snapshot.revision, snapshot))
}

fn read_from_file(file: &echo_core::Config) -> Result<Settings, String> {
    let catalog = echo::transcribe::language_catalog(None, file);
    let language_default = match catalog.selection {
        echo::transcribe::LanguageSelection::EnglishOnly => "en",
        echo::transcribe::LanguageSelection::AutoOrPinned if catalog.model.is_none() => "en",
        echo::transcribe::LanguageSelection::AutoOrPinned
        | echo::transcribe::LanguageSelection::AutomaticOnly => "auto",
    };
    settings_from(&process_settings_env(), file, language_default)
}

fn change(change: SettingsChange) -> Result<(), String> {
    if matches!(&change, SettingsChange::EnableWhisperGpu) {
        if let Some(variable) = whisper_gpu_environment_override(&process_settings_env()) {
            return Err(format!(
                "{variable} controls this setting; remove the environment override to use Whisper with GPU"
            ));
        }
    }
    update_file_config(|config| apply_change(config, change))
}

fn set_microphone_selection(id: Option<String>) -> Result<(), String> {
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
    })
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

pub(crate) fn apply_plan_config(
    config: &mut echo_core::Config,
    plan_id: echo::install::SetupPlanId,
) -> Result<(), String> {
    match plan_id {
        echo::install::SetupPlanId::Parakeet => {
            config.engine = Some(echo_core::EngineChoice::Parakeet);
            config.whisper_model = None;
        }
        echo::install::SetupPlanId::Recommended
        | echo::install::SetupPlanId::WhisperBase
        | echo::install::SetupPlanId::WhisperSmall
        | echo::install::SetupPlanId::WhisperLargeV3Turbo => {
            config.engine = Some(echo_core::EngineChoice::Whisper);
            let model = match plan_id {
                echo::install::SetupPlanId::Recommended => {
                    echo::install::catalog::recommended_model()
                }
                echo::install::SetupPlanId::WhisperBase => {
                    echo::install::ComponentId::WhisperBaseQ51
                }
                echo::install::SetupPlanId::WhisperSmall => {
                    echo::install::ComponentId::WhisperSmall
                }
                echo::install::SetupPlanId::WhisperLargeV3Turbo => {
                    echo::install::ComponentId::WhisperLargeV3TurboQ50
                }
                echo::install::SetupPlanId::Parakeet => unreachable!(),
            };
            config.whisper_model = Some(
                match model {
                    echo::install::ComponentId::WhisperBaseQ51 => "base-q5_1",
                    echo::install::ComponentId::WhisperSmall => "small",
                    echo::install::ComponentId::WhisperLargeV3TurboQ50 => "large-v3-turbo-q5_0",
                    _ => return Err("invalid Whisper model plan".to_string()),
                }
                .to_string(),
            );
        }
    }
    Ok(())
}

fn whisper_gpu_environment_override(env: &SettingsEnv) -> Option<&'static str> {
    if env
        .engine
        .as_deref()
        .and_then(echo_core::EngineChoice::from_env_var)
        .is_some_and(|engine| engine != echo_core::EngineChoice::Whisper)
    {
        return Some("ECHO_ENGINE");
    }
    env.whisper_acceleration
        .as_deref()
        .and_then(echo_core::WhisperAccelerationPreference::parse)
        .filter(|preference| *preference != echo_core::WhisperAccelerationPreference::Gpu)
        .map(|_| "ECHO_WHISPER_ACCELERATION")
}

fn apply_change(config: &mut echo_core::Config, change: SettingsChange) -> Result<(), String> {
    match change {
        SettingsChange::Engine { value } => {
            config.engine = value
                .as_deref()
                .map(|raw| {
                    echo_core::EngineChoice::from_env_var(raw)
                        .ok_or_else(|| format!("unknown engine {raw}"))
                })
                .transpose()?;
            if config.engine == Some(echo_core::EngineChoice::Parakeet) {
                config.whisper_model = None;
            }
        }
        SettingsChange::WhisperModel { value } => {
            config.whisper_model = nonempty(value);
        }
        SettingsChange::Hud { value } => {
            config.hud = value;
        }
        SettingsChange::RecordSeconds { value } => {
            config.record_seconds = value
                .map(|seconds| echo_core::RecordingLimit::clamped(u64::from(seconds)).seconds());
        }
        SettingsChange::Language { value } => {
            config.language = value
                .as_deref()
                .map(|raw| {
                    echo_core::LanguageChoice::parse(raw)
                        .ok_or_else(|| format!("unknown language {raw}"))
                })
                .transpose()?;
        }
        SettingsChange::WhisperAcceleration { value } => {
            config.whisper_acceleration = value
                .as_deref()
                .map(|raw| {
                    echo_core::WhisperAccelerationPreference::parse(raw)
                        .ok_or_else(|| format!("unknown Whisper acceleration {raw}"))
                })
                .transpose()?;
        }
        SettingsChange::WhisperGpuDevice { value } => {
            config.whisper_gpu_device = match value.as_deref() {
                None | Some("") => None,
                Some(raw) => {
                    Some(parse_gpu_device(raw).ok_or_else(|| format!("unknown GPU device {raw}"))?)
                }
            };
        }
        SettingsChange::EnableWhisperGpu => {
            config.engine = Some(echo_core::EngineChoice::Whisper);
            config.whisper_acceleration = Some(echo_core::WhisperAccelerationPreference::Gpu);
        }
    }
    Ok(())
}

fn process_settings_env() -> SettingsEnv {
    SettingsEnv {
        engine: env::var("ECHO_ENGINE").ok(),
        whisper_model: env::var("ECHO_WHISPER_MODEL").ok(),
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

#[cfg(test)]
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

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|raw| !raw.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_core::{Config, EngineChoice};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

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
    fn config_owner_runs_delayed_jobs_in_enqueue_order() {
        let owner = ConfigMutationService::default();
        let (started_sender, started_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let (order_sender, order_receiver) = mpsc::channel();
        let first_order = order_sender.clone();

        owner
            .enqueue(ConfigJob::fire_and_forget(move || {
                started_sender.send(()).unwrap();
                release_receiver.recv().unwrap();
                first_order.send(1).unwrap();
            }))
            .unwrap();
        started_receiver.recv().unwrap();
        owner
            .enqueue(ConfigJob::fire_and_forget(move || {
                order_sender.send(2).unwrap();
            }))
            .unwrap();

        release_sender.send(()).unwrap();

        assert_eq!(order_receiver.recv().unwrap(), 1);
        assert_eq!(order_receiver.recv().unwrap(), 2);
    }

    #[test]
    fn config_owner_continues_after_failed_job_result() {
        let owner = ConfigMutationService::default();
        let (sender, receiver) = mpsc::channel();
        let first_sender = sender.clone();

        owner
            .enqueue(ConfigJob::fire_and_forget(move || {
                first_sender
                    .send(Result::<(), &str>::Err("failed"))
                    .unwrap();
            }))
            .unwrap();
        owner
            .enqueue(ConfigJob::fire_and_forget(move || {
                sender.send(Ok(())).unwrap();
            }))
            .unwrap();

        assert_eq!(receiver.recv().unwrap(), Err("failed"));
        assert_eq!(receiver.recv().unwrap(), Ok(()));
    }

    #[test]
    fn config_owner_continues_after_panicked_job() {
        let owner = ConfigMutationService::default();
        let (sender, receiver) = mpsc::channel();
        let healthy_sender = sender.clone();

        owner
            .enqueue(ConfigJob {
                run: Box::new(move || panic!("boom")),
                fail: Box::new(move |error| sender.send(error).unwrap()),
            })
            .unwrap();
        owner
            .enqueue(ConfigJob::fire_and_forget(move || {
                healthy_sender.send("healthy".to_string()).unwrap();
            }))
            .unwrap();

        assert_eq!(
            receiver.recv().unwrap(),
            "configuration request failed unexpectedly"
        );
        assert_eq!(receiver.recv().unwrap(), "healthy");
    }

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

    #[test]
    fn snapshot_response_revisions_are_unique_for_reads() {
        let first = next_snapshot_response_revision();
        let second = next_snapshot_response_revision();

        assert!(second > first);
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
    fn missing_preferences_can_be_created_by_an_update() {
        let path = scratch_path("missing-update");
        assert!(!path.exists());

        update_file_config_at(&path, |config| {
            config.engine = Some(EngineChoice::Fake);
            Ok(())
        })
        .unwrap();

        assert_eq!(
            Config::load_from(&path).unwrap().engine,
            Some(EngineChoice::Fake)
        );
    }

    #[test]
    fn corrupt_preferences_remain_byte_for_byte_unchanged_after_an_update() {
        let path = scratch_path("corrupt-update");
        let original = b"{\"engine\":\"Whisper\",\"partial\":\xff}".to_vec();
        std::fs::write(&path, &original).unwrap();
        let mut update_called = false;

        let error = update_file_config_at(&path, |_| {
            update_called = true;
            Ok(())
        })
        .unwrap_err();

        assert!(!update_called);
        assert!(error.contains("invalid"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!path.with_file_name("config.json.corrupt").exists());
    }

    #[test]
    fn unreadable_preferences_abort_before_the_mutation() {
        let path = scratch_path("unreadable-update");
        std::fs::create_dir(&path).unwrap();
        let mut update_called = false;

        let error = update_file_config_at(&path, |_| {
            update_called = true;
            Ok(())
        })
        .unwrap_err();

        assert!(!update_called);
        assert!(error.contains("changes were not saved"), "{error}");
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_preferences_symlink_aborts_without_replacing_the_link() {
        let path = scratch_path("dangling-symlink-update");
        let target = path.with_file_name("missing-target.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let mut update_called = false;

        let error = update_file_config_at(&path, |_| {
            update_called = true;
            Ok(())
        })
        .unwrap_err();

        assert!(!update_called);
        assert!(error.contains("changes were not saved"), "{error}");
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        assert!(!target.exists());
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
    fn field_changes_preserve_unrelated_config_and_engine_rules() {
        let microphone = echo_core::MicrophoneSelection::Device {
            id: "alsa:buds".into(),
            last_seen_label: "Earbuds".into(),
        };
        let mut config = Config {
            engine: Some(EngineChoice::Whisper),
            whisper_model: Some("small".into()),
            microphone: Some(microphone.clone()),
            ..Config::default()
        };

        apply_change(
            &mut config,
            SettingsChange::Engine {
                value: Some("parakeet".into()),
            },
        )
        .unwrap();
        assert_eq!(config.engine, Some(EngineChoice::Parakeet));
        assert_eq!(config.whisper_model, None);
        assert_eq!(config.microphone, Some(microphone));
    }

    #[test]
    fn enabling_whisper_gpu_changes_both_preferences_atomically() {
        let mut config = Config {
            engine: Some(EngineChoice::Parakeet),
            ..Config::default()
        };

        apply_change(&mut config, SettingsChange::EnableWhisperGpu).unwrap();

        assert_eq!(config.engine, Some(EngineChoice::Whisper));
        assert_eq!(
            config.whisper_acceleration,
            Some(echo_core::WhisperAccelerationPreference::Gpu)
        );
    }

    #[test]
    fn environment_overrides_block_the_combined_whisper_gpu_change() {
        let engine = SettingsEnv {
            engine: Some("parakeet".into()),
            ..SettingsEnv::default()
        };
        assert_eq!(
            whisper_gpu_environment_override(&engine),
            Some("ECHO_ENGINE")
        );

        let acceleration = SettingsEnv {
            whisper_acceleration: Some("cpu".into()),
            ..SettingsEnv::default()
        };
        assert_eq!(
            whisper_gpu_environment_override(&acceleration),
            Some("ECHO_WHISPER_ACCELERATION")
        );

        let desired = SettingsEnv {
            engine: Some("whisper".into()),
            whisper_acceleration: Some("gpu".into()),
            ..SettingsEnv::default()
        };
        assert_eq!(whisper_gpu_environment_override(&desired), None);
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
    fn recording_limit_settings_preserve_sources_and_clamping() {
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
}
