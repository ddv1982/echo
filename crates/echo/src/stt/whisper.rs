use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use echo_core::{Engine, EngineError, EngineId, Pcm16kMono, Transcript};

use super::cache::ModelCache;
use super::write_temp_wav;
use crate::which::on_path;

const DEFAULT_MODEL: &str = "base.en";

pub struct WhisperEngine {
    cache: ModelCache,
    model: String,
}

impl Default for WhisperEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: ModelCache::from_env(),
            model: DEFAULT_MODEL.to_string(),
        }
    }

    #[must_use]
    pub fn with_cache(cache: ModelCache, model: impl Into<String>) -> Self {
        Self {
            cache,
            model: model.into(),
        }
    }

    /// True when both the runner binary and a model file are installed.
    /// A metadata check only; no inference runs.
    #[must_use]
    pub fn available(&self) -> bool {
        self.model_file().is_some() && Self::binary().is_some()
    }

    #[must_use]
    pub fn model_name(&self) -> &str {
        &self.model
    }

    fn model_file(&self) -> Option<PathBuf> {
        let candidates = [
            format!("ggml-{}.bin", self.model),
            format!("{}.bin", self.model),
            format!("ggml-{}.gguf", self.model),
        ];
        candidates
            .into_iter()
            .map(|name| self.cache.path(&name))
            .find(|path| path.is_file())
    }

    fn binary() -> Option<&'static str> {
        ["whisper-cli", "whisper-cpp", "whisper"]
            .into_iter()
            .find(|name| on_path(name))
    }
}

impl Engine for WhisperEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: self.model.clone(),
        }
    }

    fn transcribe(&self, pcm: &Pcm16kMono) -> Result<Transcript, EngineError> {
        let model = self.model_file().ok_or(EngineError::Missing)?;
        let bin = Self::binary().ok_or(EngineError::Missing)?;
        let started = Instant::now();
        let wav = write_temp_wav(pcm).map_err(EngineError::Infer)?;
        let out_prefix = wav.with_extension("");
        let status = Command::new(bin)
            .args(["-m", &model.to_string_lossy(), "-f", &wav.to_string_lossy()])
            .args(["-nt", "-otxt", "-of", &out_prefix.to_string_lossy()])
            .output()
            .map_err(|err| EngineError::Infer(err.to_string()))?;
        let txt = out_prefix.with_extension("txt");
        let raw = read_transcript(&txt, &status.stdout);
        let _ = fs::remove_file(&wav);
        let _ = fs::remove_file(&txt);
        if !status.status.success() && raw.is_empty() {
            return Err(EngineError::Infer(
                String::from_utf8_lossy(&status.stderr).into_owned(),
            ));
        }
        Ok(Transcript {
            raw: raw.trim().to_string(),
            engine: self.id(),
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

fn read_transcript(path: &Path, stdout: &[u8]) -> String {
    fs::read_to_string(path)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| String::from_utf8_lossy(stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_engine_missing() {
        let dir = std::env::temp_dir().join("echo-empty-whisper-models");
        let _ = fs::create_dir_all(&dir);
        let engine = WhisperEngine::with_cache(ModelCache::at(&dir), "base.en");
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);
        assert_eq!(engine.transcribe(&pcm), Err(EngineError::Missing));
    }
}
