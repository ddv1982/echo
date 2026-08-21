mod cache;
mod fake;
mod parakeet;
mod whisper;

pub use cache::ModelCache;
pub use fake::FakeEngine;
pub use parakeet::ParakeetEngine;
pub use whisper::WhisperEngine;

use std::fs;
use std::path::PathBuf;

use echo_core::{Engine, Pcm16kMono, SAMPLE_RATE_HZ};

/// The engine `ECHO_ENGINE` names, or the first installed real engine.
/// `None` when nothing is installed and no engine was requested; the fake
/// engine never runs unless asked for by name.
#[must_use]
pub fn resolve_engine() -> Option<Box<dyn Engine>> {
    match std::env::var("ECHO_ENGINE").ok().as_deref() {
        Some("whisper") => Some(Box::new(WhisperEngine::new())),
        Some("parakeet") => Some(Box::new(ParakeetEngine::new())),
        Some("fake") => Some(Box::new(FakeEngine::default())),
        _ => {
            let parakeet = ParakeetEngine::new();
            if parakeet.available() {
                return Some(Box::new(parakeet));
            }
            let whisper = WhisperEngine::new();
            if whisper.available() {
                return Some(Box::new(whisper));
            }
            None
        }
    }
}

fn write_temp_wav(pcm: &Pcm16kMono) -> Result<PathBuf, String> {
    let path =
        std::env::temp_dir().join(format!("echo-stt-{}-{}.wav", std::process::id(), pcm.len()));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE_HZ,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).map_err(|err| err.to_string())?;
    for sample in pcm.samples() {
        writer
            .write_sample(*sample)
            .map_err(|err| err.to_string())?;
    }
    writer.finalize().map_err(|err| err.to_string())?;
    let _ = fs::metadata(&path);
    Ok(path)
}
