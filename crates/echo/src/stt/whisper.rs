use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use echo_core::{strip_nonspeech, Engine, EngineError, EngineId, Pcm16kMono, Transcript};

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
        let vad = self.cache.vad_model();
        let mut status = Command::new(bin)
            .args(whisper_args(&model, &wav, &out_prefix, vad.as_deref()))
            .output()
            .map_err(|err| EngineError::Infer(err.to_string()))?;
        if !status.status.success() && vad.is_some() {
            let _ = fs::remove_file(out_prefix.with_extension("txt"));
            status = Command::new(bin)
                .args(whisper_args(&model, &wav, &out_prefix, None))
                .output()
                .map_err(|err| EngineError::Infer(err.to_string()))?;
        }
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
            raw: raw_text(&raw),
            engine: self.id(),
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

fn whisper_args(model: &Path, wav: &Path, out_prefix: &Path, vad: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-nt".into(),
        "-otxt".into(),
        "-of".into(),
        out_prefix.to_string_lossy().into_owned(),
    ];
    if let Some(vad) = vad {
        args.push("--vad".into());
        args.push("-vm".into());
        args.push(vad.to_string_lossy().into_owned());
    }
    args
}

fn raw_text(text: &str) -> String {
    strip_nonspeech(text.trim()).to_string()
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

    #[test]
    fn blank_audio_raw_is_empty() {
        assert!(raw_text("[BLANK_AUDIO]").is_empty());
    }

    fn args_for_cache(dir: &Path) -> Vec<String> {
        let cache = ModelCache::at(dir);
        whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            Path::new("out"),
            cache.vad_model().as_deref(),
        )
    }

    fn vm_path(args: &[String]) -> Option<&str> {
        args.windows(2)
            .find(|pair| pair[0] == "-vm")
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn whisper_args_include_vad_when_silero_v6_present() {
        let dir = std::env::temp_dir().join(format!("echo-vad-v6-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let vad = dir.join("ggml-silero-v6.2.0.bin");
        fs::write(&vad, []).expect("dummy vad model");
        let args = args_for_cache(&dir);
        assert!(args.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&args).map(Path::new), Some(vad.as_path()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn whisper_args_include_vad_flags_only_when_model_is_some() {
        let vad = Path::new("ggml-silero-v6.2.0.bin");
        let with_vad = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            Path::new("out"),
            Some(vad),
        );
        assert!(with_vad.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&with_vad), Some("ggml-silero-v6.2.0.bin"));

        let without_vad = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            Path::new("out"),
            None,
        );
        assert!(without_vad.iter().all(|arg| arg != "--vad" && arg != "-vm"));
    }

    #[test]
    fn whisper_args_omit_vad_when_cache_empty() {
        let dir = std::env::temp_dir().join(format!("echo-vad-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        let args = args_for_cache(&dir);
        assert!(args.iter().all(|arg| arg != "--vad" && arg != "-vm"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn vad_model_prefers_v6_when_both_exist() {
        let dir = std::env::temp_dir().join(format!("echo-vad-prefer-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let v6 = dir.join("ggml-silero-v6.2.0.bin");
        let v5 = dir.join("ggml-silero-v5.1.2.bin");
        fs::write(&v6, []).expect("dummy v6");
        fs::write(&v5, []).expect("dummy v5");
        let args = args_for_cache(&dir);
        assert!(args.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&args).map(Path::new), Some(v6.as_path()));
        let _ = fs::remove_dir_all(&dir);
    }
}
