use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use echo_core::{Engine, EngineError, EngineId, Pcm16kMono, Transcript};

use super::cache::ModelCache;
use super::write_temp_wav;
use crate::which::on_path;

pub struct ParakeetEngine {
    cache: ModelCache,
}

impl Default for ParakeetEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ParakeetEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: ModelCache::from_env(),
        }
    }

    #[must_use]
    pub fn with_cache(cache: ModelCache) -> Self {
        Self { cache }
    }

    /// True when both the runner binary and the model directory are installed.
    /// A metadata check only; no inference runs.
    #[must_use]
    pub fn available(&self) -> bool {
        self.model_root().is_some() && Self::binary().is_some()
    }

    fn model_root(&self) -> Option<PathBuf> {
        let nested = self.cache.path("parakeet-tdt-0.6b-v3");
        if tokens_present(&nested) {
            return Some(nested);
        }
        if tokens_present(self.cache.dir()) {
            return Some(self.cache.dir().to_path_buf());
        }
        None
    }

    fn binary() -> Option<&'static str> {
        ["sherpa-onnx-offline", "sherpa-onnx"]
            .into_iter()
            .find(|name| on_path(name))
    }
}

fn tokens_present(dir: &Path) -> bool {
    dir.join("tokens.txt").is_file()
}

impl Engine for ParakeetEngine {
    fn id(&self) -> EngineId {
        EngineId::ParakeetTdt06bV3
    }

    fn transcribe(&self, pcm: &Pcm16kMono) -> Result<Transcript, EngineError> {
        let root = self.model_root().ok_or(EngineError::Missing)?;
        let bin = Self::binary().ok_or(EngineError::Missing)?;
        let started = Instant::now();
        let wav = write_temp_wav(pcm).map_err(EngineError::Infer)?;
        let encoder = first_existing(&root, &["encoder.int8.onnx", "encoder.onnx"])
            .ok_or(EngineError::Missing)?;
        let decoder = first_existing(&root, &["decoder.int8.onnx", "decoder.onnx"])
            .ok_or(EngineError::Missing)?;
        let joiner = first_existing(&root, &["joiner.int8.onnx", "joiner.onnx"])
            .ok_or(EngineError::Missing)?;
        let tokens = root.join("tokens.txt");
        let status = Command::new(bin)
            .arg(format!("--encoder={}", encoder.display()))
            .arg(format!("--decoder={}", decoder.display()))
            .arg(format!("--joiner={}", joiner.display()))
            .arg(format!("--tokens={}", tokens.display()))
            .arg(wav.as_os_str())
            .output()
            .map_err(|err| EngineError::Infer(err.to_string()))?;
        let _ = fs::remove_file(&wav);
        let raw = String::from_utf8_lossy(&status.stdout).trim().to_string();
        if !status.status.success() && raw.is_empty() {
            return Err(EngineError::Infer(
                String::from_utf8_lossy(&status.stderr).into_owned(),
            ));
        }
        Ok(Transcript {
            raw,
            engine: self.id(),
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

fn first_existing(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_engine_missing() {
        let dir = std::env::temp_dir().join("echo-empty-parakeet-models");
        let _ = fs::create_dir_all(&dir);
        let engine = ParakeetEngine::with_cache(ModelCache::at(&dir));
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);
        assert_eq!(engine.transcribe(&pcm), Err(EngineError::Missing));
    }
}
