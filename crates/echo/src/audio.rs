use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample, I24, U24};
use echo_core::{MicrophoneSelection, Pcm16kMono, SAMPLE_RATE_HZ};

use crate::microphone::{
    is_system_default_proxy, resolve_selection, selectable_inputs, selection_from_sources,
    AudioHost, InputDeviceInfo, InputSelectionStatus, MicrophoneFailure, MicrophoneId,
    MicrophoneSnapshot, RawInputDescriptor,
};
#[cfg(any(target_os = "linux", test))]
use crate::microphone::EndpointTier;

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
        .map(|sample| *sample as f32 / f32::from(i16::MAX))
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
}

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub pcm: Pcm16kMono,
    pub duration: Duration,
    pub peak_rms: f32,
}

impl CaptureResult {
    #[must_use]
    pub fn from_pcm(pcm: Pcm16kMono) -> Self {
        Self {
            duration: Duration::from_millis(pcm.duration_ms()),
            peak_rms: pcm.peak_rms(),
            pcm,
        }
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

fn discover_inputs(host: &cpal::Host) -> InputDiscovery {
    let audio_host = AudioHost::from_cpal_name(host.id().name());
    let default = host.default_input_device();
    let default_id = default
        .as_ref()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let mut warning = None;
    let handles = match host.input_devices() {
        Ok(devices) => devices.collect::<Vec<_>>(),
        Err(error) => {
            warning = Some(error.to_string());
            Vec::new()
        }
    };
    let handles = merge_default_handle(handles, default, |device| {
        device.id().ok().map(|id| id.to_string())
    });
    let mut devices = Vec::new();
    for handle in handles {
        let is_default = handle
            .id()
            .ok()
            .is_some_and(|id| default_id.as_deref() == Some(id.to_string().as_str()));
        match describe_device(&handle, audio_host, is_default) {
            Ok(info)
                if !devices
                    .iter()
                    .any(|known: &DiscoveredInput| known.info.id == info.id) =>
            {
                devices.push(DiscoveredInput { info, handle });
            }
            Ok(_) => {}
            Err(error) => {
                warning.get_or_insert_with(|| error.to_string());
            }
        }
    }
    devices.sort_by(|left, right| {
        right
            .info
            .is_default
            .cmp(&left.info.is_default)
            .then_with(|| left.info.label.cmp(&right.info.label))
            .then_with(|| left.info.id.as_str().cmp(right.info.id.as_str()))
    });
    InputDiscovery {
        host: audio_host,
        devices,
        warning,
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
    let file = crate::settings::file_config();
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
        enumeration_warning: discovery.warning.clone(),
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

#[cfg(any(target_os = "linux", test))]
fn first_usable_or_first<T>(
    candidates: impl IntoIterator<Item = T>,
    mut is_usable: impl FnMut(&T) -> bool,
) -> Option<T> {
    let mut candidates = candidates.into_iter();
    let first = candidates.next()?;
    if is_usable(&first) {
        return Some(first);
    }
    candidates.find(|candidate| is_usable(candidate)).or(Some(first))
}

#[cfg(any(target_os = "linux", test))]
fn has_usable_input<'a>(devices: impl IntoIterator<Item = &'a InputDeviceInfo>) -> bool {
    devices.into_iter().any(|device| {
        !is_system_default_proxy(device)
            && (device.host == AudioHost::Alsa || device.tier == EndpointTier::Primary)
    })
}

fn preferred_discovery() -> InputDiscovery {
    #[cfg(target_os = "linux")]
    {
        let mut available = cpal::available_hosts();
        available.sort_by_key(|host| linux_host_priority(host.name()));
        let discoveries = available
            .into_iter()
            .filter(|host| linux_host_priority(host.name()) != usize::MAX)
            .filter_map(|host| cpal::host_from_id(host).ok())
            .map(|host| discover_inputs(&host));
        if let Some(discovery) = first_usable_or_first(discoveries, |candidate| {
            has_usable_input(candidate.devices.iter().map(|device| &device.info))
        }) {
            return discovery;
        }
    }
    discover_inputs(&cpal::default_host())
}

#[must_use]
pub fn microphone_snapshot() -> MicrophoneSnapshot {
    process_snapshot_from(&preferred_discovery())
}

impl AudioCapture {
    pub fn default_input_ready() -> Result<(), AudioError> {
        let capture = Self::open_default()?;
        capture
            .device
            .default_input_config()
            .map(|_| ())
            .map_err(map_cpal_error)
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
        let config = self.device.default_input_config().map_err(map_cpal_error)?;
        let src_hz = config.sample_rate();
        let channels = config.channels();
        let collected = Arc::new(Mutex::new(Vec::<f32>::new()));
        let err_slot: Arc<Mutex<Option<AudioError>>> = Arc::new(Mutex::new(None));
        let stream = match config.sample_format() {
            SampleFormat::I8 => build_stream::<i8>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::I24 => build_stream::<I24>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::I32 => build_stream::<i32>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::I64 => build_stream::<i64>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::U8 => build_stream::<u8>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::U24 => build_stream::<U24>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::U32 => build_stream::<u32>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::U64 => build_stream::<u64>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::F32 => build_stream::<f32>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
                meter,
            )?,
            SampleFormat::F64 => build_stream::<f64>(
                &self.device,
                config.into(),
                &collected,
                &err_slot,
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
        finish_capture_stream(stream, &err_slot)?;
        let samples = std::mem::take(&mut *collected.lock().expect("pcm lock"));
        Ok(CaptureResult::from_pcm(resample_to_16k_mono(
            &samples, src_hz, channels,
        )))
    }
}

fn finish_capture_stream<T>(
    stream: T,
    err_slot: &Arc<Mutex<Option<AudioError>>>,
) -> Result<(), AudioError> {
    drop(stream);
    match err_slot.lock().expect("stream error lock").take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    collected: &Arc<Mutex<Vec<f32>>>,
    err_slot: &Arc<Mutex<Option<AudioError>>>,
    meter: Option<&LevelMeter>,
) -> Result<cpal::Stream, AudioError>
where
    T: Sample + SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let collected = Arc::clone(collected);
    let err_slot = Arc::clone(err_slot);
    let meter = meter.cloned();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut sum_sq = 0.0f32;
                {
                    let mut buf = collected.lock().expect("pcm lock");
                    for sample in data {
                        let value = sample.to_sample::<f32>();
                        sum_sq += value * value;
                        buf.push(value);
                    }
                }
                if let Some(meter) = &meter {
                    if !data.is_empty() {
                        meter.publish((sum_sq / data.len() as f32).sqrt());
                    }
                }
            },
            move |err| {
                *err_slot.lock().expect("stream error lock") = Some(map_cpal_error(err));
            },
            None,
        )
        .map_err(map_cpal_error)
}

#[must_use]
pub fn resample_to_16k_mono(interleaved: &[f32], src_hz: u32, channels: u16) -> Pcm16kMono {
    let ch = usize::from(channels.max(1));
    let frames = interleaved.len() / ch;
    let average_frame = |frame: usize| {
        let mut sum = 0.0f32;
        for channel in 0..ch {
            sum += interleaved[frame * ch + channel];
        }
        sum / ch as f32
    };
    let src_hz = src_hz.max(1);
    if src_hz == SAMPLE_RATE_HZ {
        return Pcm16kMono::from_samples(
            (0..frames)
                .map(|frame| f32_to_i16(average_frame(frame)))
                .collect(),
        );
    }
    let out_len = (frames as u64)
        .saturating_mul(u64::from(SAMPLE_RATE_HZ))
        .saturating_div(u64::from(src_hz)) as usize;
    if frames == 0 {
        return Pcm16kMono::from_samples(Vec::new());
    }
    let mut out = Vec::with_capacity(out_len);
    let last = frames - 1;
    for i in 0..out_len {
        let src_pos = i as f64 * f64::from(src_hz) / f64::from(SAMPLE_RATE_HZ);
        let idx = src_pos.floor() as usize;
        let frac = src_pos.fract() as f32;
        let a = average_frame(idx.min(last));
        let b = average_frame((idx + 1).min(last));
        out.push(f32_to_i16(a + (b - a) * frac));
    }
    Pcm16kMono::from_samples(out)
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

pub fn load_wav(path: &Path) -> Result<CaptureResult, AudioError> {
    let mut reader =
        hound::WavReader::open(path).map_err(|err| AudioError::Wav(err.to_string()))?;
    let spec = reader.spec();
    let samples: Result<Vec<f32>, AudioError> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|err| AudioError::Wav(err.to_string())))
            .collect(),
        hound::SampleFormat::Int => {
            let denom = f32::from(i16::MAX);
            reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|v| v as f32 / denom)
                        .map_err(|err| AudioError::Wav(err.to_string()))
                })
                .collect()
        }
    };
    let pcm = resample_to_16k_mono(&samples?, spec.sample_rate, spec.channels);
    Ok(CaptureResult::from_pcm(pcm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct ShutdownError(Arc<Mutex<Option<AudioError>>>);

    impl Drop for ShutdownError {
        fn drop(&mut self) {
            *self.0.lock().unwrap() = Some(AudioError::Disconnected("shutdown".to_string()));
        }
    }

    #[test]
    fn intentional_stream_shutdown_does_not_discard_captured_audio() {
        let error = Arc::new(Mutex::new(None));
        assert!(finish_capture_stream(ShutdownError(Arc::clone(&error)), &error).is_ok());
    }

    #[test]
    fn errors_reported_before_shutdown_still_fail_capture() {
        let error = Arc::new(Mutex::new(Some(AudioError::Busy("busy".to_string()))));
        assert!(matches!(
            finish_capture_stream((), &error),
            Err(AudioError::Busy(_))
        ));
    }

    fn previous_resample_to_16k_mono(
        interleaved: &[f32],
        src_hz: u32,
        channels: u16,
    ) -> Pcm16kMono {
        let ch = usize::from(channels.max(1));
        let frames = interleaved.len() / ch;
        let mut mono = Vec::with_capacity(frames);
        for frame in 0..frames {
            let mut sum = 0.0f32;
            for channel in 0..ch {
                sum += interleaved[frame * ch + channel];
            }
            mono.push(sum / ch as f32);
        }
        let src_hz = src_hz.max(1);
        if src_hz == SAMPLE_RATE_HZ {
            return Pcm16kMono::from_samples(mono.into_iter().map(f32_to_i16).collect());
        }
        let out_len = (frames as u64)
            .saturating_mul(u64::from(SAMPLE_RATE_HZ))
            .saturating_div(u64::from(src_hz)) as usize;
        if frames == 0 {
            return Pcm16kMono::from_samples(Vec::new());
        }
        let mut out = Vec::with_capacity(out_len);
        let last = frames - 1;
        for i in 0..out_len {
            let src_pos = i as f64 * f64::from(src_hz) / f64::from(SAMPLE_RATE_HZ);
            let idx = src_pos.floor() as usize;
            let frac = src_pos.fract() as f32;
            let a = mono[idx.min(last)];
            let b = mono[(idx + 1).min(last)];
            out.push(f32_to_i16(a + (b - a) * frac));
        }
        Pcm16kMono::from_samples(out)
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav")
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

    #[test]
    fn linux_host_falls_through_empty_discoveries() {
        assert_eq!(
            first_usable_or_first([0, 0, 3], |device_count| *device_count > 0),
            Some(3)
        );
        assert_eq!(
            first_usable_or_first([0, 0, 0], |device_count| *device_count > 0),
            Some(0)
        );
    }

    #[test]
    fn synthetic_default_alone_does_not_stop_host_fallback() {
        let proxy: InputDeviceInfo = RawInputDescriptor {
            id: MicrophoneId::parse("pipewire:input_default").unwrap(),
            host: AudioHost::PipeWire,
            label: "System default".to_string(),
            is_default: true,
            manufacturer: None,
            device_type: None,
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
        }
        .into();
        let playback: InputDeviceInfo = RawInputDescriptor {
            id: MicrophoneId::parse("pipewire:alsa_output.pci-card.analog-stereo").unwrap(),
            host: AudioHost::PipeWire,
            label: "Built-in Audio".to_string(),
            is_default: false,
            manufacturer: None,
            device_type: Some("Speaker".to_string()),
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
        }
        .into();
        let real: InputDeviceInfo = RawInputDescriptor {
            id: MicrophoneId::parse("alsa:hw:CARD=USB,DEV=0").unwrap(),
            host: AudioHost::Alsa,
            label: "USB Microphone".to_string(),
            is_default: true,
            manufacturer: None,
            device_type: None,
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
        }
        .into();
        let alsa_alias: InputDeviceInfo = RawInputDescriptor {
            id: MicrophoneId::parse("alsa:default").unwrap(),
            host: AudioHost::Alsa,
            label: "Default ALSA input".to_string(),
            is_default: true,
            manufacturer: None,
            device_type: None,
            interface_type: None,
            address: None,
            driver: None,
            extended: Vec::new(),
        }
        .into();

        assert!(!has_usable_input([&proxy]));
        assert!(!has_usable_input([&proxy, &playback]));
        assert!(has_usable_input([&proxy, &real]));
        assert!(has_usable_input([&alsa_alias]));
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
    }

    #[test]
    fn resamples_48k_stereo_to_16k_mono() {
        let src_hz = 48_000u32;
        let frames = src_hz as usize / 10;
        let mut interleaved = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (i as f32 / frames as f32) * 0.5;
            interleaved.push(s);
            interleaved.push(s);
        }
        let pcm = resample_to_16k_mono(&interleaved, src_hz, 2);
        assert!((pcm.len() as i32 - (SAMPLE_RATE_HZ as i32 / 10)).abs() <= 1);
        assert!(pcm.peak_rms() > 0.0);
        assert!(pcm.samples().iter().all(|s| s.abs() < i16::MAX));
    }

    #[test]
    fn direct_conversion_matches_previous_short_recordings_exactly() {
        for (interleaved, src_hz, channels) in [
            (Vec::new(), 48_000, 2),
            (vec![-1.0, -0.5, 0.0, 0.25, 0.5, 1.0], 16_000, 1),
            (
                vec![-1.0, 1.0, -0.5, 0.25, 0.0, 0.75, 0.5, -0.25],
                48_000,
                2,
            ),
            (
                vec![
                    -1.0, -0.5, 0.0, 0.5, -0.75, -0.25, 0.25, 0.75, -0.5, 0.0, 0.5, 1.0,
                ],
                44_100,
                4,
            ),
        ] {
            let expected = previous_resample_to_16k_mono(&interleaved, src_hz, channels);
            let actual = resample_to_16k_mono(&interleaved, src_hz, channels);
            assert_eq!(actual.samples(), expected.samples());
        }
    }

    #[test]
    fn ten_minute_stereo_capture_budget_is_249_6_mb() {
        let seconds = 600usize;
        let native_bytes = seconds
            .checked_mul(48_000)
            .and_then(|samples| samples.checked_mul(2))
            .and_then(|samples| samples.checked_mul(size_of::<f32>()))
            .unwrap();
        let output_bytes = seconds
            .checked_mul(SAMPLE_RATE_HZ as usize)
            .and_then(|samples| samples.checked_mul(size_of::<i16>()))
            .unwrap();

        assert_eq!(native_bytes, 230_400_000);
        assert_eq!(output_bytes, 19_200_000);
        assert_eq!(native_bytes + output_bytes, 249_600_000);
    }
}
