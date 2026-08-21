use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample};
use echo_core::{Pcm16kMono, SAMPLE_RATE_HZ};

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
    pub device_name: String,
    pub fallback_from: Option<String>,
    pub cancel: CancellationToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChoice {
    Found(InputDevice),
    Fallback {
        requested: String,
        device: InputDevice,
    },
    None,
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
    Stream(String),
    Wav(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no input device"),
            Self::Stream(msg) | Self::Wav(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for AudioError {}

#[must_use]
pub fn resolve_device(candidates: &[InputDevice], requested: Option<&str>) -> DeviceChoice {
    let default = candidates
        .iter()
        .find(|device| device.is_default)
        .or_else(|| candidates.first());
    match requested.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => {
            if let Some(found) = candidates.iter().find(|device| device.name == name) {
                DeviceChoice::Found(found.clone())
            } else if let Some(default) = default {
                DeviceChoice::Fallback {
                    requested: name.to_string(),
                    device: default.clone(),
                }
            } else {
                DeviceChoice::None
            }
        }
        None => default
            .cloned()
            .map(DeviceChoice::Found)
            .unwrap_or(DeviceChoice::None),
    }
}

pub fn list_input_devices() -> Vec<InputDevice> {
    list_host_devices(&cpal::default_host()).0
}

fn keep_default_device<T>(default: Option<(String, T)>) -> (Vec<InputDevice>, Vec<(String, T)>) {
    match default {
        Some((name, device)) => (
            vec![InputDevice {
                is_default: true,
                name: name.clone(),
            }],
            vec![(name, device)],
        ),
        None => (Vec::new(), Vec::new()),
    }
}

fn list_host_devices(host: &cpal::Host) -> (Vec<InputDevice>, Vec<(String, cpal::Device)>) {
    let default = host
        .default_input_device()
        .and_then(|device| device.name().ok().map(|name| (name, device)));
    let default_name = default.as_ref().map(|(name, _)| name.clone());
    let mut named = Vec::new();
    let Ok(devices) = host.input_devices() else {
        return keep_default_device(default);
    };
    for device in devices {
        let Ok(name) = device.name() else {
            continue;
        };
        named.push((name, device));
    }
    let list = named
        .iter()
        .map(|(name, _)| InputDevice {
            is_default: default_name.as_deref() == Some(name.as_str()),
            name: name.clone(),
        })
        .collect();
    (list, named)
}

impl AudioCapture {
    pub fn open_default() -> Result<Self, AudioError> {
        let env = std::env::var("ECHO_MICROPHONE")
            .ok()
            .filter(|name| !name.is_empty());
        let requested = echo_core::resolve(
            env,
            crate::settings::file_config().microphone,
            String::new(),
        );
        Self::open((!requested.is_empty()).then_some(requested).as_deref())
    }

    pub fn open(requested: Option<&str>) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let (candidates, named) = list_host_devices(&host);
        let (chosen, fallback_from) = match resolve_device(&candidates, requested) {
            DeviceChoice::Found(device) => (device, None),
            DeviceChoice::Fallback { requested, device } => {
                eprintln!("microphone {requested} is gone; using {}", device.name);
                (device, Some(requested))
            }
            DeviceChoice::None => return Err(AudioError::NoDevice),
        };
        let device = named
            .into_iter()
            .find(|(name, _)| *name == chosen.name)
            .map(|(_, device)| device)
            .ok_or(AudioError::NoDevice)?;
        Ok(Self {
            device,
            device_name: chosen.name,
            fallback_from,
            cancel: CancellationToken::new(),
        })
    }

    pub fn record(&self, max: Duration) -> Result<CaptureResult, AudioError> {
        let config = self
            .device
            .default_input_config()
            .map_err(|err| AudioError::Stream(err.to_string()))?;
        let src_hz = config.sample_rate().0;
        let channels = config.channels();
        let collected = Arc::new(Mutex::new(Vec::<f32>::new()));
        let err_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                build_stream::<f32>(&self.device, &config.into(), &collected, &err_slot)?
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&self.device, &config.into(), &collected, &err_slot)?
            }
            SampleFormat::I32 => {
                build_stream::<i32>(&self.device, &config.into(), &collected, &err_slot)?
            }
            SampleFormat::F64 => {
                build_stream::<f64>(&self.device, &config.into(), &collected, &err_slot)?
            }
            other => {
                return Err(AudioError::Stream(format!(
                    "unsupported sample format {other:?}"
                )))
            }
        };
        stream
            .play()
            .map_err(|err| AudioError::Stream(err.to_string()))?;
        let started = Instant::now();
        while !self.cancel.is_cancelled() && started.elapsed() < max {
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(stream);
        if let Some(msg) = err_slot.lock().expect("stream error lock").take() {
            return Err(AudioError::Stream(msg));
        }
        let samples = collected.lock().expect("pcm lock").clone();
        Ok(CaptureResult::from_pcm(resample_to_16k_mono(
            &samples, src_hz, channels,
        )))
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    collected: &Arc<Mutex<Vec<f32>>>,
    err_slot: &Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, AudioError>
where
    T: Sample + SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let collected = Arc::clone(collected);
    let err_slot = Arc::clone(err_slot);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut buf = collected.lock().expect("pcm lock");
                buf.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
            },
            move |err| {
                *err_slot.lock().expect("stream error lock") = Some(err.to_string());
            },
            None,
        )
        .map_err(|err| AudioError::Stream(err.to_string()))
}

#[must_use]
pub fn resample_to_16k_mono(interleaved: &[f32], src_hz: u32, channels: u16) -> Pcm16kMono {
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

    fn sample_devices() -> Vec<InputDevice> {
        vec![
            InputDevice {
                name: "Built-in".into(),
                is_default: true,
            },
            InputDevice {
                name: "USB Mic".into(),
                is_default: false,
            },
        ]
    }

    #[test]
    fn resolve_matches_requested_name() {
        assert_eq!(
            resolve_device(&sample_devices(), Some("USB Mic")),
            DeviceChoice::Found(InputDevice {
                name: "USB Mic".into(),
                is_default: false,
            })
        );
    }

    #[test]
    fn resolve_missing_name_falls_back_to_default() {
        assert_eq!(
            resolve_device(&sample_devices(), Some("Headset")),
            DeviceChoice::Fallback {
                requested: "Headset".into(),
                device: InputDevice {
                    name: "Built-in".into(),
                    is_default: true,
                },
            }
        );
    }

    #[test]
    fn resolve_empty_list_is_none() {
        assert_eq!(resolve_device(&[], Some("USB Mic")), DeviceChoice::None);
        assert_eq!(resolve_device(&[], None), DeviceChoice::None);
    }

    #[test]
    fn enum_failure_keeps_default_as_sole_candidate() {
        let (list, named) = keep_default_device(Some(("Built-in".into(), ())));
        assert_eq!(
            list,
            vec![InputDevice {
                name: "Built-in".into(),
                is_default: true,
            }]
        );
        assert_eq!(named, vec![("Built-in".into(), ())]);
        assert_eq!(
            resolve_device(&list, None),
            DeviceChoice::Found(list[0].clone())
        );
    }

    #[test]
    fn enum_failure_without_default_is_empty() {
        let (list, named): (Vec<InputDevice>, Vec<(String, ())>) = keep_default_device(None);
        assert!(list.is_empty());
        assert!(named.is_empty());
        assert_eq!(resolve_device(&list, None), DeviceChoice::None);
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
}
