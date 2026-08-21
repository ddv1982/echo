use std::time::Instant;

use echo_core::{Engine, EngineError, EngineId, Pcm16kMono, Transcript};

/// Deterministic stand-in. Silence is empty. Non-silent PCM yields `spoken`.
pub struct FakeEngine {
    pub spoken: String,
}

impl Default for FakeEngine {
    fn default() -> Self {
        Self {
            spoken: "claude code".to_string(),
        }
    }
}

impl FakeEngine {
    #[must_use]
    pub fn new(spoken: impl Into<String>) -> Self {
        Self {
            spoken: spoken.into(),
        }
    }
}

impl Engine for FakeEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: "fake".to_string(),
        }
    }

    fn transcribe(&self, pcm: &Pcm16kMono) -> Result<Transcript, EngineError> {
        let started = Instant::now();
        let raw = if pcm.is_empty() || pcm.peak_rms() < 0.01 {
            String::new()
        } else {
            self.spoken.clone()
        };
        Ok(Transcript {
            raw,
            engine: self.id(),
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_core::SAMPLE_RATE_HZ;

    #[test]
    fn silence_is_empty() {
        let engine = FakeEngine::default();
        let pcm = Pcm16kMono::from_samples(vec![0; SAMPLE_RATE_HZ as usize / 5]);
        let transcript = engine.transcribe(&pcm).unwrap();
        assert!(transcript.raw.is_empty());
    }

    #[test]
    fn fixture_length_is_deterministic() {
        let engine = FakeEngine::default();
        let pcm = Pcm16kMono::from_samples(vec![8_000; SAMPLE_RATE_HZ as usize / 4]);
        let transcript = engine.transcribe(&pcm).unwrap();
        assert_eq!(transcript.raw, "claude code");
    }
}
