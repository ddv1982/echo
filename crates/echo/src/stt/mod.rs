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

use echo_core::{Pcm16kMono, SAMPLE_RATE_HZ};

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
