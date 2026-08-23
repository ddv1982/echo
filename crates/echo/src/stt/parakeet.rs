use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use echo_core::{
    strip_nonspeech, DecodeOptions, Engine, EngineError, EngineId, LanguageChoice, Pcm16kMono,
    RunDetail, Transcript,
};

use super::cache::ModelCache;
use super::write_temp_wav;
use crate::which::path_of;

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
        self.cache.parakeet_root()
    }

    pub(crate) fn binary() -> Option<PathBuf> {
        ["sherpa-onnx-offline", "sherpa-onnx"]
            .into_iter()
            .find_map(path_of)
    }
}

impl Engine for ParakeetEngine {
    fn id(&self) -> EngineId {
        EngineId::ParakeetTdt06bV3
    }

    fn transcribe(
        &self,
        pcm: &Pcm16kMono,
        options: &DecodeOptions,
    ) -> Result<Transcript, EngineError> {
        if !matches!(options.language, LanguageChoice::Auto) {
            return Err(EngineError::Infer(
                "Parakeet supports automatic language selection only".to_string(),
            ));
        }
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
        let status = Command::new(&bin)
            .arg(format!("--encoder={}", encoder.display()))
            .arg(format!("--decoder={}", decoder.display()))
            .arg(format!("--joiner={}", joiner.display()))
            .arg(format!("--tokens={}", tokens.display()))
            .arg(wav.as_os_str())
            .output()
            .map_err(|err| EngineError::Infer(err.to_string()))?;
        let _ = fs::remove_file(&wav);
        let raw = finish_output(status.status.success(), &status.stdout, &status.stderr)?;
        Ok(Transcript {
            raw: raw_text(&raw),
            engine: self.id(),
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            detail: RunDetail {
                binary: Some(bin.to_string_lossy().into_owned()),
                ..RunDetail::default()
            },
        })
    }
}

fn finish_output(success: bool, stdout: &[u8], stderr: &[u8]) -> Result<String, EngineError> {
    let raw = String::from_utf8_lossy(stdout).trim().to_string();
    if success {
        return Ok(raw);
    }
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let message = if stderr.is_empty() {
        if raw.is_empty() {
            "Parakeet inference failed".to_string()
        } else {
            raw
        }
    } else {
        stderr
    };
    Err(EngineError::Infer(message))
}

fn raw_text(text: &str) -> String {
    strip_nonspeech(text.trim()).to_string()
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
    use echo_core::{Language, RecognitionHints};

    fn options(language: LanguageChoice) -> DecodeOptions {
        DecodeOptions {
            language,
            hints: RecognitionHints::default(),
        }
    }

    #[test]
    fn missing_model_is_engine_missing() {
        let dir = std::env::temp_dir().join("echo-empty-parakeet-models");
        let _ = fs::create_dir_all(&dir);
        let engine = ParakeetEngine::with_cache(ModelCache::at(&dir));
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);
        assert_eq!(
            engine.transcribe(&pcm, &options(LanguageChoice::Auto)),
            Err(EngineError::Missing)
        );
    }

    #[test]
    fn pinned_language_is_rejected_before_model_lookup() {
        let engine = ParakeetEngine::with_cache(ModelCache::at("/missing"));
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);
        let german = LanguageChoice::Pinned(Language::from_code("de").unwrap());
        assert!(matches!(
            engine.transcribe(&pcm, &options(german)),
            Err(EngineError::Infer(message))
                if message.contains("automatic language selection only")
        ));
    }

    #[test]
    fn blank_audio_raw_is_empty() {
        assert!(raw_text("[BLANK_AUDIO]").is_empty());
    }

    #[test]
    fn nonzero_exit_rejects_partial_stdout() {
        assert_eq!(
            finish_output(false, b"partial result\n", b"decoder failed\n"),
            Err(EngineError::Infer("decoder failed".to_string()))
        );
    }
}
