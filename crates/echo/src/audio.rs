use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample, I24, U24};
use echo_core::{MicrophoneSelection, Pcm16kMono, SAMPLE_RATE_HZ};
use rubato::{FftFixedInOut, Resampler};

use crate::microphone::{
    is_system_default_proxy, resolve_selection, selectable_inputs, selection_from_sources,
    AudioHost, InputDeviceInfo, InputSelectionStatus, MicrophoneFailure, MicrophoneId,
    MicrophoneSnapshot, RawInputDescriptor,
};

/// The microphone's RMS level, shared between the capture callback and
/// whoever renders it. f32 bits in one atomic; publishing is a few
/// instructions per callback buffer and touches no lock.
#[derive(Debug, Clone, Default)]
pub struct LevelMeter {
    bits: Arc<AtomicU32>,
}

impl LevelMeter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, rms: f32) {
        let rms = if rms.is_finite() { rms } else { 0.0 };
        self.bits
            .store(rms.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    #[must_use]
    pub fn level(&self) -> f32 {
        f32::from_bits(self.bits.load(Ordering::Relaxed))
    }

    fn publish_samples(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let sum_sq: f32 = samples.iter().map(|sample| sample * sample).sum();
        self.publish((sum_sq / samples.len() as f32).sqrt());
    }
}

/// The meter for this process's own recording session. The GUI reads it for
/// its live level bars; a session started by a compositor shortcut lives in
/// another process and this meter stays parked at zero.
static PROCESS_METER: std::sync::LazyLock<LevelMeter> = std::sync::LazyLock::new(LevelMeter::new);

#[must_use]
pub fn process_meter() -> LevelMeter {
    PROCESS_METER.clone()
}

/// Publish a fixture's per-chunk RMS at real-time cadence, so HUD demos and
/// CI screenshots show the WAV's actual loudness instead of a synthetic wave.
pub fn play_fixture_meter(
    pcm: &Pcm16kMono,
    meter: LevelMeter,
    cancel: CancellationToken,
) -> std::thread::JoinHandle<usize> {
    const CHUNK: usize = SAMPLE_RATE_HZ as usize / 33;
    let samples: Vec<f32> = pcm
        .samples()
        .iter()
        .map(|sample| *sample as f32 / -f32::from(i16::MIN))
        .collect();
    std::thread::spawn(move || {
        let mut played = 0;
        for chunk in samples.chunks(CHUNK) {
            if cancel.is_cancelled() {
                break;
            }
            meter.publish_samples(chunk);
            std::thread::sleep(Duration::from_millis(30));
            played += chunk.len();
        }
        played
    })
}

#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub struct AudioCapture {
    device: cpal::Device,
    pub device_id: MicrophoneId,
    pub device_name: String,
    pub fallback_from: Option<String>,
    pub cancel: CancellationToken,
}

struct DiscoveredInput {
    info: InputDeviceInfo,
    handle: cpal::Device,
}

struct InputDiscovery {
    host: AudioHost,
    devices: Vec<DiscoveredInput>,
    warning: Option<String>,
    authoritative: bool,
    native_seen: bool,
    diagnostics: Vec<InputDiagnostic>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDiagnostic {
    pub device: InputDeviceInfo,
    pub rejection: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneInventory {
    pub snapshot: MicrophoneSnapshot,
    pub diagnostics: Vec<InputDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub pcm: Pcm16kMono,
    pub duration: Duration,
    pub peak_rms: f32,
    pub dropped_samples: u64,
}

impl CaptureResult {
    #[must_use]
    pub fn from_pcm(pcm: Pcm16kMono) -> Self {
        Self::from_pcm_with_dropped_samples(pcm, 0)
    }

    #[must_use]
    pub fn from_pcm_with_dropped_samples(pcm: Pcm16kMono, dropped_samples: u64) -> Self {
        Self {
            duration: Duration::from_millis(pcm.duration_ms()),
            peak_rms: pcm.peak_rms(),
            dropped_samples,
            pcm,
        }
    }
}

#[derive(Debug)]
struct CaptureBuffer {
    samples: Mutex<Vec<f32>>,
    dropped_samples: AtomicU64,
}

impl CaptureBuffer {
    fn with_capacity(capacity: usize) -> Result<Self, AudioError> {
        let mut samples = Vec::new();
        samples.try_reserve_exact(capacity).map_err(|error| {
            AudioError::Stream(format!("could not reserve capture buffer: {error}"))
        })?;
        Ok(Self {
            samples: Mutex::new(samples),
            dropped_samples: AtomicU64::new(0),
        })
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped_samples.load(Ordering::Relaxed)
    }

    fn account_dropped(&self, count: usize) {
        self.dropped_samples
            .fetch_add(count as u64, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub enum AudioError {
    NoDevice,
    Selection(String),
    Permission(String),
    Busy(String),
    Disconnected(String),
    Unsupported(String),
    Host(String),
    Stream(String),
    Wav(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no input device"),
            Self::Selection(msg)
            | Self::Permission(msg)
            | Self::Busy(msg)
            | Self::Disconnected(msg)
            | Self::Unsupported(msg)
            | Self::Host(msg) => f.write_str(msg),
            Self::Stream(msg) | Self::Wav(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for AudioError {}

impl AudioError {
    #[must_use]
    pub fn category(&self) -> MicrophoneFailure {
        match self {
            Self::NoDevice | Self::Disconnected(_) => MicrophoneFailure::Disconnected,
            Self::Selection(_) => MicrophoneFailure::Selection,
            Self::Permission(_) => MicrophoneFailure::Permission,
            Self::Busy(_) => MicrophoneFailure::Busy,
            Self::Unsupported(_) => MicrophoneFailure::Unsupported,
            Self::Host(_) => MicrophoneFailure::Host,
            Self::Stream(_) | Self::Wav(_) => MicrophoneFailure::Failed,
        }
    }
}

enum CaptureStreamState {
    Capturing(Option<AudioError>),
    Stopping,
}

impl Default for CaptureStreamState {
    fn default() -> Self {
        Self::Capturing(None)
    }
}

impl CaptureStreamState {
    fn report_error(&mut self, error: AudioError) {
        if let Self::Capturing(slot) = self {
            *slot = Some(error);
        }
    }

    fn begin_shutdown(&mut self) -> Option<AudioError> {
        match std::mem::replace(self, Self::Stopping) {
            Self::Capturing(error) => error,
            Self::Stopping => None,
        }
    }
}

fn map_cpal_error(error: cpal::Error) -> AudioError {
    use cpal::ErrorKind;
    let detail = error.to_string();
    match error.kind() {
        ErrorKind::PermissionDenied => AudioError::Permission(detail),
        ErrorKind::DeviceBusy => AudioError::Busy(detail),
        ErrorKind::DeviceNotAvailable | ErrorKind::DeviceChanged => {
            AudioError::Disconnected(detail)
        }
        ErrorKind::HostUnavailable => AudioError::Host(detail),
        ErrorKind::UnsupportedConfig | ErrorKind::UnsupportedOperation => {
            AudioError::Unsupported(detail)
        }
        _ => AudioError::Stream(detail),
    }
}

fn describe_device(
    device: &cpal::Device,
    host: AudioHost,
    is_default: bool,
) -> Result<InputDeviceInfo, AudioError> {
    let id = device.id().map_err(map_cpal_error)?.to_string();
    let description = device.description().ok();
    let label = description
        .as_ref()
        .map(|value| value.name().to_string())
        .unwrap_or_else(|| id.clone());
    Ok(RawInputDescriptor {
        id: MicrophoneId::parse(id).map_err(AudioError::Selection)?,
        host,
        label,
        is_default,
        manufacturer: description
            .as_ref()
            .and_then(|value| value.manufacturer().map(str::to_string)),
        device_type: description.as_ref().and_then(|value| {
            let text = value.device_type().to_string();
            (text != "Unknown").then_some(text)
        }),
        interface_type: description.as_ref().and_then(|value| {
            let text = value.interface_type().to_string();
            (text != "Unknown").then_some(text)
        }),
        address: description
            .as_ref()
            .and_then(|value| value.address().map(str::to_string)),
        driver: description
            .as_ref()
            .and_then(|value| value.driver().map(str::to_string)),
        extended: description
            .as_ref()
            .map(|value| value.extended().map(str::to_string).collect())
            .unwrap_or_default(),
    }
    .into())
}

fn merge_default_handle<T>(
    mut enumerated: Vec<T>,
    default: Option<T>,
    id: impl Fn(&T) -> Option<String>,
) -> Vec<T> {
    if let Some(default) = default {
        let default_id = id(&default);
        let present = default_id.is_some_and(|expected| {
            enumerated
                .iter()
                .any(|candidate| id(candidate).as_deref() == Some(expected.as_str()))
        });
        if !present {
            enumerated.push(default);
        }
    }
    enumerated
}

fn discover_inputs(host_id: cpal::HostId) -> InputDiscovery {
    let audio_host = AudioHost::from_cpal_name(host_id.name());
    let mut discovery = InputDiscovery {
        host: audio_host,
        devices: Vec::new(),
        warning: None,
        authoritative: false,
        native_seen: false,
        diagnostics: Vec::new(),
    };
    #[cfg(target_os = "linux")]
    let native = crate::microphone::availability::snapshot(audio_host);
    #[cfg(target_os = "linux")]
    if let Some(native) = &native {
        use crate::microphone::availability::BackendHealth;
        discovery.warning.clone_from(&native.warning);
        discovery.native_seen = native.health != BackendHealth::Unreachable;
        if native.health != BackendHealth::Reachable || native.endpoints.is_empty() {
            discovery.authoritative =
                native.health == BackendHealth::Reachable && native.warning.is_none();
            return discovery;
        }
    }
    let host = match cpal::host_from_id(host_id) {
        Ok(host) => host,
        Err(error) => {
            discovery.warning = Some(error.to_string());
            return discovery;
        }
    };
    let default = host.default_input_device();
    let default_id = default
        .as_ref()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    #[cfg(target_os = "linux")]
    let default_id = match &native {
        Some(native) => default_input_id(default_id, native.default_source.as_ref()),
        None => default_id,
    };
    let handles = match host.input_devices() {
        Ok(devices) => {
            discovery.authoritative = true;
            devices.collect::<Vec<_>>()
        }
        Err(error) => {
            discovery.warning = Some(error.to_string());
            return discovery;
        }
    };
    let handles = merge_default_handle(handles, default, |device| {
        device.id().ok().map(|id| id.to_string())
    });
    for handle in handles {
        let is_default = handle
            .id()
            .ok()
            .is_some_and(|id| default_id.as_deref() == Some(id.to_string().as_str()));
        let info = match describe_device(&handle, audio_host, is_default) {
            Ok(info) => info,
            Err(error) => {
                discovery.warning.get_or_insert_with(|| error.to_string());
                continue;
            }
        };
        if discovery
            .diagnostics
            .iter()
            .any(|known| known.device.id == info.id)
        {
            continue;
        }
        let mut rejection = non_source_reason(&info).map(str::to_owned);
        #[cfg(target_os = "linux")]
        if rejection.is_none() {
            if let Some(native) = &native {
                rejection = match native.endpoints.get(&info.id) {
                    Some(metadata) => metadata.rejection().map(str::to_owned),
                    None => {
                        let reason =
                            "microphone metadata is unavailable; refresh and try again".to_owned();
                        discovery.warning.get_or_insert_with(|| reason.clone());
                        Some(reason)
                    }
                };
            }
        }
        if rejection.is_none() {
            rejection = capture_config(&handle).err().map(|error| error.to_string());
            if let Some(reason) = &rejection {
                discovery.warning.get_or_insert_with(|| reason.clone());
            }
        }
        discovery.diagnostics.push(InputDiagnostic {
            device: info.clone(),
            rejection: rejection.clone(),
        });
        if rejection.is_none() {
            discovery.devices.push(DiscoveredInput { info, handle });
        }
    }
    discovery.devices.sort_by(|left, right| {
        right
            .info
            .is_default
            .cmp(&left.info.is_default)
            .then_with(|| left.info.label.cmp(&right.info.label))
            .then_with(|| left.info.id.as_str().cmp(right.info.id.as_str()))
    });
    discovery
}

#[cfg(any(target_os = "linux", test))]
fn default_input_id(
    host_default: Option<String>,
    native_default: Option<&MicrophoneId>,
) -> Option<String> {
    native_default
        .map(|id| id.as_str().to_owned())
        .or(host_default)
}

fn non_source_reason(device: &InputDeviceInfo) -> Option<&'static str> {
    if is_system_default_proxy(device) {
        return Some("system default is a preference, not a microphone device");
    }
    let id = device.id.as_str();
    if id == "pipewire:sink_default" || id == "pipewire:output_default" || id == "alsa:null" {
        return Some("not a microphone source");
    }
    None
}

fn capture_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, AudioError> {
    let config = device.default_input_config().map_err(map_cpal_error)?;
    validate_capture_config(
        config.channels(),
        config.sample_rate(),
        config.sample_format(),
    )?;
    Ok(config)
}

fn validate_capture_config(
    channels: u16,
    sample_rate: u32,
    format: SampleFormat,
) -> Result<(), AudioError> {
    if channels == 0 || sample_rate == 0 {
        return Err(AudioError::Unsupported(
            "microphone has no valid capture channels or sample rate".to_owned(),
        ));
    }
    validate_sample_rate(sample_rate)?;
    match format {
        SampleFormat::I8
        | SampleFormat::I16
        | SampleFormat::I24
        | SampleFormat::I32
        | SampleFormat::I64
        | SampleFormat::U8
        | SampleFormat::U16
        | SampleFormat::U24
        | SampleFormat::U32
        | SampleFormat::U64
        | SampleFormat::F32
        | SampleFormat::F64 => Ok(()),
        other => Err(AudioError::Unsupported(format!(
            "unsupported microphone sample format {other:?}"
        ))),
    }
}

fn process_snapshot_from(discovery: &InputDiscovery) -> MicrophoneSnapshot {
    let discovered: Vec<_> = discovery
        .devices
        .iter()
        .map(|device| device.info.clone())
        .collect();
    let system_default = discovered.iter().find(|device| device.is_default).cloned();
    let system_default_is_proxy = system_default.as_ref().is_some_and(is_system_default_proxy);
    let devices = selectable_inputs(&discovered);
    let (file, config_error) = crate::settings::config_for_display();
    let environment = std::env::var("ECHO_MICROPHONE").ok();
    let (selection, source) =
        selection_from_sources(environment.as_deref(), file.microphone.as_ref(), &devices);
    let selection = match selection {
        None => InputSelectionStatus::SystemDefault {
            active: system_default.clone().or_else(|| devices.first().cloned()),
        },
        Some(ref requested) => {
            let resolved = resolve_selection(Some(requested), &discovered);
            match resolved {
                InputSelectionStatus::Selected { ref device }
                    if is_system_default_proxy(device) =>
                {
                    InputSelectionStatus::SystemDefault {
                        active: Some(device.clone()),
                    }
                }
                other => other,
            }
        }
    };
    MicrophoneSnapshot {
        host: discovery.host,
        source,
        system_default,
        system_default_is_proxy,
        selection,
        devices,
        enumeration_warning: config_error.or_else(|| discovery.warning.clone()),
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_host_priority(name: &str) -> usize {
    match name {
        "PipeWire" => 0,
        "PulseAudio" => 1,
        "ALSA" => 2,
        _ => usize::MAX,
    }
}

#[cfg(target_os = "linux")]
fn choose_discovery(
    hosts: impl IntoIterator<Item = cpal::HostId>,
    mut discover: impl FnMut(cpal::HostId) -> InputDiscovery,
) -> Option<InputDiscovery> {
    let mut first: Option<InputDiscovery> = None;
    let mut native_seen = false;
    for host in hosts {
        if host == cpal::HostId::Alsa && native_seen {
            break;
        }
        let candidate = discover(host);
        native_seen |= candidate.native_seen;
        if candidate.authoritative && (candidate.warning.is_none() || !candidate.devices.is_empty())
        {
            return Some(candidate);
        }
        if first
            .as_ref()
            .is_none_or(|previous| !previous.native_seen && candidate.native_seen)
        {
            first = Some(candidate);
        }
    }
    first
}

fn preferred_discovery() -> InputDiscovery {
    #[cfg(target_os = "linux")]
    {
        let mut available = cpal::available_hosts();
        available.sort_by_key(|host| linux_host_priority(host.name()));
        let available = available
            .into_iter()
            .filter(|host| linux_host_priority(host.name()) != usize::MAX);
        if let Some(discovery) = choose_discovery(available, discover_inputs) {
            return discovery;
        }
    }
    discover_inputs(cpal::default_host().id())
}

#[must_use]
pub fn microphone_inventory() -> MicrophoneInventory {
    let discovery = preferred_discovery();
    MicrophoneInventory {
        snapshot: process_snapshot_from(&discovery),
        diagnostics: discovery.diagnostics,
    }
}

#[must_use]
pub fn microphone_snapshot() -> MicrophoneSnapshot {
    process_snapshot_from(&preferred_discovery())
}

impl AudioCapture {
    pub fn default_input_ready() -> Result<(), AudioError> {
        let capture = Self::open_default()?;
        capture_config(&capture.device).map(|_| ())
    }

    pub fn open_default() -> Result<Self, AudioError> {
        let discovery = preferred_discovery();
        let snapshot = process_snapshot_from(&discovery);
        Self::open_snapshot(discovery, &snapshot.selection, true)
    }

    pub fn open(requested: Option<&str>) -> Result<Self, AudioError> {
        let discovery = preferred_discovery();
        let devices: Vec<_> = discovery
            .devices
            .iter()
            .map(|device| device.info.clone())
            .collect();
        let selection = requested.map(|raw| {
            devices
                .iter()
                .find(|device| device.id.as_str() == raw)
                .map(|device| MicrophoneSelection::Device {
                    id: raw.to_string(),
                    last_seen_label: device.label.clone(),
                })
                .unwrap_or_else(|| MicrophoneSelection::LegacyName {
                    name: raw.to_string(),
                })
        });
        let status = resolve_selection(selection.as_ref(), &devices);
        Self::open_snapshot(discovery, &status, true)
    }

    pub fn open_exact(id: Option<&MicrophoneId>) -> Result<Self, AudioError> {
        let discovery = preferred_discovery();
        let devices: Vec<_> = discovery
            .devices
            .iter()
            .map(|device| device.info.clone())
            .collect();
        let status = match id {
            None => resolve_selection(None, &devices),
            Some(id) => resolve_selection(
                Some(&MicrophoneSelection::Device {
                    id: id.as_str().to_string(),
                    last_seen_label: id.as_str().to_string(),
                }),
                &devices,
            ),
        };
        Self::open_snapshot(discovery, &status, false)
    }

    fn open_snapshot(
        discovery: InputDiscovery,
        status: &InputSelectionStatus,
        allow_fallback: bool,
    ) -> Result<Self, AudioError> {
        let (chosen, fallback_from) = match status {
            InputSelectionStatus::SystemDefault {
                active: Some(device),
            }
            | InputSelectionStatus::Selected { device }
            | InputSelectionStatus::LegacyMatch { device, .. } => (device, None),
            InputSelectionStatus::MissingWithFallback {
                requested_label,
                fallback,
                ..
            } if allow_fallback => (fallback, Some(requested_label.clone())),
            InputSelectionStatus::AmbiguousLegacyName {
                name,
                fallback: Some(fallback),
                ..
            } if allow_fallback => (fallback, Some(name.clone())),
            InputSelectionStatus::SystemDefault { active: None } => {
                return Err(AudioError::NoDevice)
            }
            InputSelectionStatus::MissingWithFallback {
                requested_label, ..
            }
            | InputSelectionStatus::MissingWithoutFallback {
                requested_label, ..
            } => {
                return Err(AudioError::Selection(format!(
                    "selected microphone {requested_label} is unavailable"
                )));
            }
            InputSelectionStatus::AmbiguousLegacyName { name, .. } => {
                return Err(AudioError::Selection(format!(
                    "more than one microphone is named {name}; select one by ID"
                )));
            }
        };
        let handle = discovery
            .devices
            .into_iter()
            .find(|device| device.info.id == chosen.id)
            .map(|device| device.handle)
            .ok_or(AudioError::NoDevice)?;
        Ok(Self {
            device: handle,
            device_id: chosen.id.clone(),
            device_name: chosen.label.clone(),
            fallback_from,
            cancel: CancellationToken::new(),
        })
    }

    pub fn record(
        &self,
        max: Duration,
        meter: Option<&LevelMeter>,
    ) -> Result<CaptureResult, AudioError> {
        let config = capture_config(&self.device)?;
        let src_hz = config.sample_rate();
        let channels = config.channels();
        let collected = Arc::new(CaptureBuffer::with_capacity(capture_sample_capacity(
            max, src_hz, channels,
        ))?);
        let stream_state = Arc::new(Mutex::new(CaptureStreamState::default()));
        let stream = match config.sample_format() {
            SampleFormat::I8 => build_stream::<i8>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::I24 => build_stream::<I24>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::I32 => build_stream::<i32>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::I64 => build_stream::<i64>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::U8 => build_stream::<u8>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::U24 => build_stream::<U24>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::U32 => build_stream::<u32>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::U64 => build_stream::<u64>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::F32 => build_stream::<f32>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::F64 => build_stream::<f64>(
                &self.device,
                config.into(),
                &collected,
                &stream_state,
                meter,
            )?,
            SampleFormat::DsdU8 | SampleFormat::DsdU16 | SampleFormat::DsdU32 => {
                return Err(AudioError::Stream(format!(
                    "unsupported DSD sample format {0:?}",
                    config.sample_format()
                )))
            }
            other => {
                return Err(AudioError::Stream(format!(
                    "unsupported sample format {other:?}"
                )))
            }
        };
        stream.play().map_err(map_cpal_error)?;
        let started = Instant::now();
        while !self.cancel.is_cancelled() && started.elapsed() < max {
            std::thread::sleep(Duration::from_millis(10));
        }
        finish_capture_stream(stream, &stream_state)?;
        let dropped_samples = collected.dropped_samples();
        let samples = std::mem::take(
            &mut *collected
                .samples
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if samples.is_empty() {
            return Err(AudioError::Stream(
                "microphone did not deliver any audio frames".to_owned(),
            ));
        }
        Ok(CaptureResult::from_pcm_with_dropped_samples(
            resample_to_16k_mono(&samples, src_hz, channels)?,
            dropped_samples,
        ))
    }
}

fn capture_sample_capacity(max: Duration, sample_rate: u32, channels: u16) -> usize {
    const NANOS_PER_SECOND: u128 = 1_000_000_000;
    let sample_nanos = max
        .as_nanos()
        .saturating_mul(u128::from(sample_rate))
        .saturating_mul(u128::from(channels));
    let samples = sample_nanos
        .saturating_add(NANOS_PER_SECOND - 1)
        .saturating_div(NANOS_PER_SECOND);
    samples.min(usize::MAX as u128) as usize
}

fn finish_capture_stream<T>(
    stream: T,
    state: &Arc<Mutex<CaptureStreamState>>,
) -> Result<(), AudioError> {
    let result = match state.lock() {
        Ok(mut state) => match state.begin_shutdown() {
            Some(error) => Err(error),
            None => Ok(()),
        },
        Err(_) => Err(AudioError::Stream(
            "capture stream state lock poisoned".to_string(),
        )),
    };
    drop(stream);
    result
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    collected: &Arc<CaptureBuffer>,
    stream_state: &Arc<Mutex<CaptureStreamState>>,
    meter: Option<&LevelMeter>,
) -> Result<cpal::Stream, AudioError>
where
    T: Sample + SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let collected = Arc::clone(collected);
    let stream_state = Arc::clone(stream_state);
    let meter = meter.cloned();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                capture_input(&collected, data, meter.as_ref());
            },
            move |err| {
                if let Ok(mut state) = stream_state.lock() {
                    state.report_error(map_cpal_error(err));
                }
            },
            None,
        )
        .map_err(map_cpal_error)
}

fn capture_input<T>(collected: &CaptureBuffer, data: &[T], meter: Option<&LevelMeter>)
where
    T: Sample,
    f32: cpal::FromSample<T>,
{
    let mut sum_sq = 0.0f32;
    match collected.samples.try_lock() {
        Ok(mut samples) => {
            let retained = data.len().min(samples.capacity() - samples.len());
            for (index, sample) in data.iter().enumerate() {
                let value = sample.to_sample::<f32>();
                sum_sq += value * value;
                if index < retained {
                    samples.push(value);
                }
            }
            collected.account_dropped(data.len() - retained);
        }
        Err(TryLockError::Poisoned(poisoned)) => {
            let mut samples = poisoned.into_inner();
            let retained = data.len().min(samples.capacity() - samples.len());
            for (index, sample) in data.iter().enumerate() {
                let value = sample.to_sample::<f32>();
                sum_sq += value * value;
                if index < retained {
                    samples.push(value);
                }
            }
            collected.account_dropped(data.len() - retained);
        }
        Err(TryLockError::WouldBlock) => {
            for sample in data {
                let value = sample.to_sample::<f32>();
                sum_sq += value * value;
            }
            collected.account_dropped(data.len());
        }
    }
    if let Some(meter) = meter {
        if !data.is_empty() {
            meter.publish((sum_sq / data.len() as f32).sqrt());
        }
    }
}

// Bound FFT scratch even for coprime rates declared by untrusted WAV headers.
// 384 kHz includes high-rate PCM interfaces while limiting alignment to 768k frames.
fn validate_sample_rate(sample_rate: u32) -> Result<(), AudioError> {
    if !(1..=384_000).contains(&sample_rate) {
        return Err(AudioError::Unsupported(format!(
            "sample rate {sample_rate} Hz is outside the supported range 1..=384000 Hz"
        )));
    }
    Ok(())
}

pub fn resample_to_16k_mono(
    interleaved: &[f32],
    src_hz: u32,
    channels: u16,
) -> Result<Pcm16kMono, AudioError> {
    validate_sample_rate(src_hz)?;
    let ch = usize::from(channels.max(1));
    let frames = interleaved.len() / ch;
    let average_frame = |frame: usize| {
        let mut sum = 0.0f32;
        for channel in 0..ch {
            sum += interleaved[frame * ch + channel];
        }
        sum / ch as f32
    };
    if src_hz == SAMPLE_RATE_HZ {
        return Ok(Pcm16kMono::from_samples(
            (0..frames)
                .map(|frame| f32_to_i16(average_frame(frame)))
                .collect(),
        ));
    }
    let out_len = (frames as u64)
        .saturating_mul(u64::from(SAMPLE_RATE_HZ))
        .saturating_div(u64::from(src_hz)) as usize;
    if out_len == 0 {
        return Ok(Pcm16kMono::from_samples(Vec::new()));
    }

    // Conversion runs after capture (and for WAV imports), never in the callback.
    // Reuse bounded scratch buffers instead of materializing the whole mono track.
    // Even input/output block lengths make the filter's midpoint and reported
    // output delay exact, including the fractional 44.1 kHz conversion ratio.
    let (mut gcd, mut remainder) = (src_hz, SAMPLE_RATE_HZ);
    while remainder != 0 {
        (gcd, remainder) = (remainder, gcd % remainder);
    }
    let alignment = 2 * (src_hz / gcd) as usize;
    let chunk_size = 1024_usize.div_ceil(alignment) * alignment;
    let mut resampler =
        FftFixedInOut::<f32>::new(src_hz as usize, SAMPLE_RATE_HZ as usize, chunk_size, 1)
            .expect("normalized sample rates are nonzero");
    let mut input = vec![0.0; resampler.input_frames_next()];
    let mut output = vec![0.0; resampler.output_frames_max()];
    let mut delay = resampler.output_delay();
    let mut frame = 0;
    let mut out = Vec::with_capacity(out_len);
    while out.len() < out_len {
        let available = input.len().min(frames - frame);
        for (offset, sample) in input[..available].iter_mut().enumerate() {
            *sample = average_frame(frame + offset);
        }
        frame += available;
        // Pad the final block and continue with silence to flush the filter tail.
        // Unlike process_partial_into_buffer, this reuses the input allocation.
        input[available..].fill(0.0);
        let (_, written) = resampler
            .process_into_buffer(&[input.as_slice()], &mut [output.as_mut_slice()], None)
            .expect("mono buffers match the resampler's fixed frame counts");
        let skip = delay.min(written);
        delay -= skip;
        let take = (written - skip).min(out_len - out.len());
        out.extend(output[skip..skip + take].iter().copied().map(f32_to_i16));
    }
    Ok(Pcm16kMono::from_samples(out))
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

pub fn load_wav(path: &Path) -> Result<CaptureResult, AudioError> {
    let mut reader =
        hound::WavReader::open(path).map_err(|err| AudioError::Wav(err.to_string()))?;
    let spec = reader.spec();
    validate_sample_rate(spec.sample_rate)?;
    let samples = decode_wav_samples(&mut reader)?;
    let pcm = resample_to_16k_mono(&samples, spec.sample_rate, spec.channels)?;
    Ok(CaptureResult::from_pcm(pcm))
}

fn decode_wav_samples<R: std::io::Read>(
    reader: &mut hound::WavReader<R>,
) -> Result<Vec<f32>, AudioError> {
    let spec = reader.spec();
    let samples: Result<Vec<f32>, AudioError> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|err| AudioError::Wav(err.to_string())))
            .collect(),
        hound::SampleFormat::Int => {
            let denominator = signed_pcm_denominator(spec.bits_per_sample)?;
            reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|value| (value as f32 / denominator).clamp(-1.0, 1.0))
                        .map_err(|err| AudioError::Wav(err.to_string()))
                })
                .collect()
        }
    };
    samples
}

fn signed_pcm_denominator(bits_per_sample: u16) -> Result<f32, AudioError> {
    let magnitude_bits = bits_per_sample
        .checked_sub(1)
        .ok_or_else(|| AudioError::Wav("integer WAV has invalid zero-bit samples".to_string()))?;
    if magnitude_bits >= 32 {
        return Err(AudioError::Wav(format!(
            "unsupported integer WAV bit depth {bits_per_sample}"
        )));
    }
    Ok((1_u64 << magnitude_bits) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct ShutdownError(Arc<Mutex<CaptureStreamState>>);

    impl Drop for ShutdownError {
        fn drop(&mut self) {
            self.0
                .lock()
                .unwrap()
                .report_error(AudioError::Disconnected("shutdown".to_string()));
        }
    }

    #[test]
    fn shutdown_only_stream_error_is_ignored() {
        let state = Arc::new(Mutex::new(CaptureStreamState::default()));
        assert!(finish_capture_stream(ShutdownError(Arc::clone(&state)), &state).is_ok());
    }

    #[test]
    fn errors_reported_before_shutdown_still_fail_capture() {
        let state = Arc::new(Mutex::new(CaptureStreamState::default()));
        state
            .lock()
            .unwrap()
            .report_error(AudioError::Busy("busy".to_string()));
        assert!(matches!(
            finish_capture_stream((), &state),
            Err(AudioError::Busy(_))
        ));
    }

    #[test]
    fn poisoned_shutdown_protocol_fails_explicitly() {
        let state = Arc::new(Mutex::new(CaptureStreamState::default()));
        let poison = Arc::clone(&state);
        let _ = std::thread::spawn(move || {
            let _guard = poison.lock().unwrap();
            panic!("poison stream state");
        })
        .join();

        assert!(matches!(
            finish_capture_stream((), &state),
            Err(AudioError::Stream(message)) if message.contains("poisoned")
        ));
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav")
    }

    fn wav_bytes(sample_format: u16, bits: u16, samples: &[i32]) -> Vec<u8> {
        let bytes_per_sample = usize::from(bits / 8);
        let data_len = samples.len() * bytes_per_sample;
        let mut bytes = Vec::with_capacity(44 + data_len);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36_u32 + data_len as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&sample_format.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_RATE_HZ.to_le_bytes());
        bytes.extend_from_slice(&(SAMPLE_RATE_HZ * bytes_per_sample as u32).to_le_bytes());
        bytes.extend_from_slice(&(bytes_per_sample as u16).to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes()[..bytes_per_sample]);
        }
        bytes
    }

    fn decode_bytes(bytes: Vec<u8>) -> Vec<f32> {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).expect("wav reader");
        decode_wav_samples(&mut reader).expect("wav samples")
    }

    #[test]
    fn integer_wav_uses_actual_16_24_and_32_bit_depth() {
        for (bits, samples) in [
            (
                16,
                vec![i16::MIN as i32, -16_384, 0, 16_384, i16::MAX as i32],
            ),
            (24, vec![-8_388_608, -4_194_304, 0, 4_194_304, 8_388_607]),
            (
                32,
                vec![i32::MIN, -1_073_741_824, 0, 1_073_741_824, i32::MAX],
            ),
        ] {
            let decoded = decode_bytes(wav_bytes(1, bits, &samples));
            let denominator = (1_u64 << (bits - 1)) as f32;
            let expected: Vec<_> = samples
                .iter()
                .map(|sample| (*sample as f32 / denominator).clamp(-1.0, 1.0))
                .collect();

            assert_eq!(decoded, expected, "{bits}-bit PCM");
            assert_eq!(decoded[0], -1.0, "{bits}-bit minimum");
            assert_eq!(decoded[1], -0.5, "{bits}-bit ordinary negative");
            assert_eq!(decoded[3], 0.5, "{bits}-bit ordinary positive");
            assert!(decoded.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        }
    }

    #[test]
    fn float_wav_samples_are_not_integer_normalized_or_clamped() {
        let samples = [-1.25_f32, -0.25, 0.0, 0.5, 1.25];
        let encoded: Vec<i32> = samples
            .iter()
            .map(|sample| i32::from_le_bytes(sample.to_le_bytes()))
            .collect();
        assert_eq!(decode_bytes(wav_bytes(3, 32, &encoded)), samples);
    }

    #[test]
    fn fixture_is_16k_and_not_silent() {
        let capture = load_wav(&fixture()).expect("fixture wav");
        assert!(capture.pcm.duration_ms() >= 300);
        assert!(capture.peak_rms > 0.05);
        assert_eq!(
            capture.pcm.len(),
            (capture.pcm.duration_ms() * u64::from(SAMPLE_RATE_HZ) / 1000) as usize
        );
    }

    fn sample_to_f32<T>(sample: T) -> f32
    where
        T: Sample,
        f32: cpal::FromSample<T>,
    {
        sample.to_sample::<f32>()
    }

    #[test]
    fn every_pcm_sample_type_converts_to_capture_f32() {
        assert_eq!(sample_to_f32(0i8), 0.0);
        assert_eq!(sample_to_f32(0i16), 0.0);
        assert_eq!(sample_to_f32(I24::new(0).unwrap()), 0.0);
        assert_eq!(sample_to_f32(0i32), 0.0);
        assert_eq!(sample_to_f32(0i64), 0.0);
        assert_eq!(sample_to_f32(128u8), 0.0);
        assert_eq!(sample_to_f32(32_768u16), 0.0);
        assert_eq!(sample_to_f32(U24::new(1 << 23).unwrap()), 0.0);
        assert_eq!(sample_to_f32(1u32 << 31), 0.0);
        assert_eq!(sample_to_f32(1u64 << 63), 0.0);
        assert_eq!(sample_to_f32(0.0f32), 0.0);
        assert_eq!(sample_to_f32(0.0f64), 0.0);
    }

    #[test]
    fn poisoned_capture_buffer_is_recovered() {
        let collected = Arc::new(CaptureBuffer::with_capacity(4).unwrap());
        let poison = Arc::clone(&collected);
        let _ = std::thread::spawn(move || {
            let _guard = poison.samples.lock().unwrap();
            panic!("poison capture samples");
        })
        .join();

        capture_input(&collected, &[0.25_f32, -0.5], None);

        let samples = collected
            .samples
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(&*samples, &[0.25, -0.5]);
        assert_eq!(collected.dropped_samples(), 0);
    }

    #[test]
    fn unavailable_or_full_capture_buffer_counts_dropped_samples() {
        let collected = CaptureBuffer::with_capacity(2).unwrap();
        capture_input(&collected, &[0.1_f32, 0.2, 0.3], None);
        assert_eq!(collected.dropped_samples(), 1);

        let guard = collected.samples.lock().unwrap();
        capture_input(&collected, &[0.4_f32, 0.5], None);
        drop(guard);
        assert_eq!(collected.dropped_samples(), 3);
    }

    #[test]
    fn capture_result_exposes_dropped_samples() {
        let result =
            CaptureResult::from_pcm_with_dropped_samples(Pcm16kMono::from_samples(vec![0]), 17);
        assert_eq!(result.dropped_samples, 17);
        assert_eq!(
            CaptureResult::from_pcm(Pcm16kMono::from_samples(vec![])).dropped_samples,
            0
        );
    }

    #[test]
    fn meter_projection_is_finite_and_bounded() {
        let meter = LevelMeter::new();
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 2.0] {
            meter.publish(value);
            assert!(meter.level().is_finite());
            assert!((0.0..=1.0).contains(&meter.level()));
        }
        meter.publish_samples(&[-1.0, 1.0]);
        assert_eq!(meter.level(), 1.0);
    }

    #[test]
    fn default_missing_from_enumeration_is_kept_once() {
        let merged = merge_default_handle(vec!["usb"], Some("default"), |value| {
            Some((*value).to_string())
        });
        assert_eq!(merged, vec!["usb", "default"]);
        let already_present = merge_default_handle(vec!["default"], Some("default"), |value| {
            Some((*value).to_string())
        });
        assert_eq!(already_present, vec!["default"]);
    }

    #[test]
    fn linux_host_priority_is_pipewire_then_pulse_then_alsa() {
        assert!(linux_host_priority("PipeWire") < linux_host_priority("PulseAudio"));
        assert!(linux_host_priority("PulseAudio") < linux_host_priority("ALSA"));
        assert_eq!(linux_host_priority("JACK"), usize::MAX);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_host_failures_can_fall_back_without_reviving_alsa_aliases() {
        let hosts = [
            cpal::HostId::PipeWire,
            cpal::HostId::PulseAudio,
            cpal::HostId::Alsa,
        ];
        let empty = |host: cpal::HostId, authoritative, native_seen| InputDiscovery {
            host: AudioHost::from_cpal_name(host.name()),
            devices: Vec::new(),
            warning: None,
            authoritative,
            native_seen,
            diagnostics: Vec::new(),
        };
        let healthy_empty = choose_discovery(hosts, |host| {
            assert_eq!(host, cpal::HostId::PipeWire);
            empty(host, true, true)
        })
        .unwrap();
        assert_eq!(healthy_empty.host, AudioHost::PipeWire);
        assert!(healthy_empty.devices.is_empty());
        let native_fallback = choose_discovery(hosts, |host| {
            assert_ne!(host, cpal::HostId::Alsa);
            empty(host, host == cpal::HostId::PulseAudio, true)
        })
        .unwrap();
        assert_eq!(native_fallback.host, AudioHost::PulseAudio);
        let incomplete_metadata = choose_discovery(hosts, |host| {
            assert_ne!(host, cpal::HostId::Alsa);
            let mut discovery = empty(host, true, true);
            if host == cpal::HostId::PipeWire {
                discovery.warning = Some("route metadata query failed".into());
            }
            discovery
        })
        .unwrap();
        assert_eq!(incomplete_metadata.host, AudioHost::PulseAudio);
        let failed_native = choose_discovery(hosts, |host| {
            assert_ne!(host, cpal::HostId::Alsa);
            empty(host, false, host == cpal::HostId::PipeWire)
        })
        .unwrap();
        assert_eq!(failed_native.host, AudioHost::PipeWire);
        let alsa_only =
            choose_discovery(hosts, |host| empty(host, host == cpal::HostId::Alsa, false)).unwrap();
        assert_eq!(alsa_only.host, AudioHost::Alsa);
    }

    #[test]
    fn capture_configuration_rejects_non_capture_formats() {
        assert!(validate_capture_config(0, 48_000, SampleFormat::F32).is_err());
        assert!(validate_capture_config(2, 0, SampleFormat::F32).is_err());
        assert!(validate_capture_config(2, 48_000, SampleFormat::DsdU8).is_err());
        for format in [
            SampleFormat::I16,
            SampleFormat::I24,
            SampleFormat::F32,
            SampleFormat::U24,
        ] {
            assert!(validate_capture_config(2, 48_000, format).is_ok());
        }
    }

    #[test]
    fn missing_native_default_preserves_the_host_default() {
        let host = "pulseaudio:usb-microphone".to_owned();
        assert_eq!(default_input_id(Some(host.clone()), None), Some(host));
        let native = MicrophoneId::parse("pulseaudio:native-default").unwrap();
        assert_eq!(
            default_input_id(Some("pulseaudio:other".into()), Some(&native)),
            Some(native.as_str().to_owned())
        );
        assert_eq!(default_input_id(None, None), None);
    }

    #[test]
    fn cpal_error_categories_name_actionable_failures() {
        use cpal::ErrorKind;
        for (kind, category) in [
            (ErrorKind::PermissionDenied, MicrophoneFailure::Permission),
            (ErrorKind::DeviceBusy, MicrophoneFailure::Busy),
            (
                ErrorKind::DeviceNotAvailable,
                MicrophoneFailure::Disconnected,
            ),
            (ErrorKind::UnsupportedConfig, MicrophoneFailure::Unsupported),
            (ErrorKind::HostUnavailable, MicrophoneFailure::Host),
        ] {
            assert_eq!(map_cpal_error(cpal::Error::new(kind)).category(), category);
        }
    }

    #[test]
    fn silence_is_legal() {
        let pcm = Pcm16kMono::from_samples(vec![0; SAMPLE_RATE_HZ as usize / 10]);
        let capture = CaptureResult::from_pcm(pcm);
        assert!(capture.peak_rms == 0.0);
        assert!(capture.duration > Duration::ZERO);
        assert_eq!(capture.dropped_samples, 0);
    }

    fn tone(src_hz: u32, frequency: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| 0.5 * (std::f32::consts::TAU * frequency * i as f32 / src_hz as f32).sin())
            .collect()
    }

    fn normalized_rms(samples: &[i16]) -> f32 {
        (samples
            .iter()
            .map(|&sample| (f32::from(sample) / f32::from(i16::MAX)).powi(2))
            .sum::<f32>()
            / samples.len() as f32)
            .sqrt()
    }

    #[test]
    fn resampling_rejects_stopband_and_retains_speech_band() {
        for src_hz in [44_100, 48_000] {
            for frequency in [1_000.0, 6_000.0, 12_000.0] {
                let pcm =
                    resample_to_16k_mono(&tone(src_hz, frequency, src_hz as usize), src_hz, 1)
                        .unwrap();
                assert_eq!(pcm.len(), SAMPLE_RATE_HZ as usize);
                // Ignore only edge transients, not block boundaries within the recording.
                let rms = normalized_rms(&pcm.samples()[160..pcm.len() - 160]);
                if frequency < 8_000.0 {
                    let expected = 0.5 / std::f32::consts::SQRT_2;
                    assert!(
                        (rms - expected).abs() < 0.01,
                        "{src_hz} Hz, {frequency} Hz: {rms}"
                    );
                } else {
                    // At least 50 dB rejection versus the input tone's RMS.
                    assert!(rms < 0.001, "{src_hz} Hz, {frequency} Hz: {rms}");
                }
            }
        }
    }

    #[test]
    fn resampling_preserves_stereo_mix_duration_and_tail_alignment() {
        for src_hz in [44_100, 48_000] {
            let frames = src_hz as usize / 10 + 7;
            let mono = tone(src_hz, 1_000.0, frames);
            let mut stereo = Vec::with_capacity(frames * 2 + 1);
            for sample in mono {
                // The average is a half-amplitude tone; channel bias must cancel.
                stereo.extend([sample + 0.25, -0.25]);
            }
            stereo.push(1.0); // An incomplete final frame must not change duration.
            let pcm = resample_to_16k_mono(&stereo, src_hz, 2).unwrap();
            let expected_len = frames * SAMPLE_RATE_HZ as usize / src_hz as usize;
            assert_eq!(pcm.len(), expected_len);
            assert_eq!(
                pcm.duration_ms(),
                expected_len as u64 * 1000 / u64::from(SAMPLE_RATE_HZ)
            );
            // Check phase all the way into the final partial block: an untrimmed
            // filter delay or an unflushed tail shifts or erases the signal.
            for (i, &sample) in pcm
                .samples()
                .iter()
                .enumerate()
                .take(expected_len - 32)
                .skip(32)
            {
                let expected = 0.25
                    * (std::f32::consts::TAU * 1_000.0 * i as f32 / SAMPLE_RATE_HZ as f32).sin();
                let actual = f32::from(sample) / f32::from(i16::MAX);
                assert!(
                    (actual - expected).abs() < 0.01,
                    "{src_hz} Hz, frame {i}: {actual} != {expected}"
                );
            }
        }
    }

    #[test]
    fn resampling_short_inputs_flushes_without_adding_duration() {
        for src_hz in [8_000, 44_100, 48_000] {
            for frames in [0, 1, 2, 3, 17] {
                let pcm = resample_to_16k_mono(&vec![0.5; frames], src_hz, 1).unwrap();
                assert_eq!(
                    pcm.len(),
                    frames * SAMPLE_RATE_HZ as usize / src_hz as usize
                );
                if !pcm.is_empty() {
                    assert!(pcm.samples().iter().any(|&sample| sample > 1_000));
                }
            }
        }
    }

    #[test]
    fn same_rate_downmix_preserves_pcm_and_clips_after_averaging() {
        let pcm = resample_to_16k_mono(
            &[-2.0, -2.0, -0.5, 0.0, 0.5, 0.5, 2.0, 2.0, 2.0, -1.0, 1.0],
            SAMPLE_RATE_HZ,
            2,
        )
        .unwrap();
        assert_eq!(pcm.samples(), &[-32_767, -8_191, 16_383, 32_767, 16_383]);
    }

    #[test]
    fn unsupported_wav_rates_fail_before_resampling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extreme-rate.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 50_000_003,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // Enough frames for one output sample: the old code would allocate
        // a 100-million-frame FFT despite this file being only a few kilobytes.
        for _ in 0..3126 {
            writer.write_sample(1000_i16).unwrap();
        }
        writer.finalize().unwrap();
        assert!(matches!(load_wav(&path), Err(AudioError::Unsupported(_))));
        assert!(matches!(
            resample_to_16k_mono(&[0.0], 384_001, 1),
            Err(AudioError::Unsupported(_))
        ));
        assert!(matches!(
            validate_capture_config(1, 50_000_003, SampleFormat::F32),
            Err(AudioError::Unsupported(_))
        ));
        let pcm = resample_to_16k_mono(&[0.5; 48], 384_000, 1).unwrap();
        assert_eq!(pcm.len(), 2);
        assert!(pcm.samples().iter().any(|&sample| sample > 1000));
    }
}
