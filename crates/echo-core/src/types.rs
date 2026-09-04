use serde::{Deserialize, Serialize};

/// 16 kHz is the only sample rate a `Pcm16kMono` buffer may represent.
pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// 16-bit linear PCM at 16 kHz mono. Construction does not take a rate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm16kMono {
    samples: Vec<i16>,
}

impl Pcm16kMono {
    #[must_use]
    pub fn from_samples(samples: Vec<i16>) -> Self {
        Self { samples }
    }

    #[must_use]
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    #[must_use]
    pub fn into_samples(self) -> Vec<i16> {
        self.samples
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        (self.samples.len() as u64 * 1000) / u64::from(SAMPLE_RATE_HZ)
    }

    #[must_use]
    pub fn peak_rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f64 = self
            .samples
            .iter()
            .map(|s| {
                let v = f64::from(*s) / -f64::from(i16::MIN);
                v * v
            })
            .sum();
        ((sum_sq / self.samples.len() as f64).sqrt() as f32).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_is_bounded_for_signed_pcm_extrema() {
        let minimum = Pcm16kMono::from_samples(vec![i16::MIN]);
        let maximum = Pcm16kMono::from_samples(vec![i16::MAX]);
        let mixed = Pcm16kMono::from_samples(vec![i16::MIN, i16::MAX]);

        assert_eq!(minimum.peak_rms(), 1.0);
        assert!(maximum.peak_rms() <= 1.0);
        assert!(mixed.peak_rms() <= 1.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailReason {
    NoInputDevice,
    CaptureFailed,
    InjectPermission,
    EngineMissing,
    NoFocus,
    EngineError,
    InjectUnconfirmed,
}

impl FailReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoInputDevice => "no microphone input device",
            Self::CaptureFailed => "microphone capture failed",
            Self::InjectPermission => "inject permission denied",
            Self::EngineMissing => "speech engine or model missing",
            Self::NoFocus => "no focused window",
            Self::EngineError => "speech engine failed",
            Self::InjectUnconfirmed => "insert was not confirmed",
        }
    }
}

impl std::fmt::Display for FailReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineId {
    ParakeetTdt06bV3,
    Whisper { model: String },
    Fake,
}

impl EngineId {
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::ParakeetTdt06bV3 => "parakeet-tdt-0.6b-v3".to_string(),
            Self::Whisper { model } => format!("whisper-{model}"),
            Self::Fake => "fake".to_string(),
        }
    }
}

impl std::fmt::Display for EngineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}
