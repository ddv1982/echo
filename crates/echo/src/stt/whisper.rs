use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use echo_core::{
    strip_nonspeech, Config, Engine, EngineError, EngineId, Pcm16kMono, Transcript,
};
use serde::Deserialize;

use super::cache::ModelCache;
use super::write_temp_wav;
use crate::settings::file_config;
use crate::which::on_path;

const DEFAULT_MODEL: &str = "base.en";

fn resolved_whisper_model(env: Option<String>, file: &Config) -> String {
    echo_core::resolve(
        env.filter(|name| !name.is_empty()),
        file.whisper_model.clone(),
        DEFAULT_MODEL.to_string(),
    )
}

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
        Self::with_cache(
            ModelCache::from_env(),
            resolved_whisper_model(std::env::var("ECHO_WHISPER_MODEL").ok(), &file_config()),
        )
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
        let vad = self.cache.vad_model();
        let mut status = Command::new(bin)
            .args(whisper_args(&model, &wav, vad.as_deref()))
            .output()
            .map_err(|err| EngineError::Infer(err.to_string()))?;
        if !status.status.success() && vad.is_some() {
            status = Command::new(bin)
                .args(whisper_args(&model, &wav, None))
                .output()
                .map_err(|err| EngineError::Infer(err.to_string()))?;
        }
        let _ = fs::remove_file(&wav);
        let parsed = finish_whisper(
            status.status.success(),
            &status.stdout,
            &String::from_utf8_lossy(&status.stderr),
        )?;
        Ok(Transcript {
            raw: raw_text(&parsed.text),
            engine: EngineId::Whisper {
                model: parsed.model,
            },
            language: parsed.language,
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

fn whisper_args(model: &Path, wav: &Path, vad: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-nt".into(),
        "-oj".into(),
        "-of".into(),
        "-".into(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct WhisperParse {
    text: String,
    language: Option<String>,
    model: String,
    #[allow(dead_code)]
    multilingual: bool,
}

#[derive(Debug, Deserialize)]
struct WhisperOutput {
    model: ModelInfo,
    #[serde(default)]
    result: ResultInfo,
    #[serde(default)]
    transcription: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    #[serde(rename = "type")]
    model_type: String,
    multilingual: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ResultInfo {
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Segment {
    text: String,
}

fn finish_whisper(success: bool, stdout: &[u8], stderr: &str) -> Result<WhisperParse, EngineError> {
    if !success {
        return Err(EngineError::Infer(stderr.to_string()));
    }
    parse_whisper_stdout(stdout)
}

fn parse_whisper_stdout(stdout: &[u8]) -> Result<WhisperParse, EngineError> {
    parse_whisper_json(&String::from_utf8_lossy(stdout))
}

fn parse_whisper_json(raw: &str) -> Result<WhisperParse, EngineError> {
    let output: WhisperOutput = serde_json::from_str(raw.trim()).map_err(|err| {
        EngineError::Infer(format!("whisper json: {err}"))
    })?;
    let text = output
        .transcription
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>()
        .trim()
        .to_string();
    let language = if output.transcription.is_empty() {
        None
    } else {
        output.result.language.filter(|code| !code.is_empty())
    };
    Ok(WhisperParse {
        text,
        language,
        model: output.model.model_type,
        multilingual: output.model.multilingual,
    })
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
        let with_vad = whisper_args(Path::new("model.bin"), Path::new("in.wav"), Some(vad));
        assert!(with_vad.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&with_vad), Some("ggml-silero-v6.2.0.bin"));

        let without_vad = whisper_args(Path::new("model.bin"), Path::new("in.wav"), None);
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

    #[test]
    fn whisper_model_prefers_env_then_file_then_default() {
        let file = Config {
            whisper_model: Some("tiny.en".into()),
            ..Config::default()
        };
        assert_eq!(
            resolved_whisper_model(Some("small.en".into()), &file),
            "small.en"
        );
        assert_eq!(resolved_whisper_model(None, &file), "tiny.en");
        assert_eq!(
            resolved_whisper_model(None, &Config::default()),
            DEFAULT_MODEL
        );
        assert_eq!(
            resolved_whisper_model(Some(String::new()), &file),
            "tiny.en"
        );
    }

    fn fixture(name: &str) -> String {
        fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/whisper")
                .join(name),
        )
        .unwrap_or_else(|err| panic!("read {name}: {err}"))
    }

    #[test]
    fn parses_multilingual_result() {
        let parsed = parse_whisper_json(&fixture("multilingual.json")).unwrap();
        assert_eq!(parsed.text, "Claude Code.");
        assert_eq!(parsed.language.as_deref(), Some("de"));
        assert_eq!(parsed.model, "base");
        assert!(parsed.multilingual);
    }

    #[test]
    fn parses_english_only_result() {
        let parsed = parse_whisper_json(&fixture("english.json")).unwrap();
        assert_eq!(parsed.text, "Claude Code.");
        assert_eq!(parsed.language.as_deref(), Some("en"));
        assert_eq!(parsed.model, "base");
        assert!(!parsed.multilingual);
    }

    #[test]
    fn empty_transcription_hides_stale_language() {
        let parsed = parse_whisper_json(&fixture("empty_transcription.json")).unwrap();
        assert!(parsed.text.is_empty());
        assert_eq!(parsed.language, None);
        assert!(!parsed.multilingual);
    }

    #[test]
    fn malformed_json_is_a_named_error() {
        let err = parse_whisper_json("not json at all").unwrap_err();
        match err {
            EngineError::Infer(msg) => assert!(msg.starts_with("whisper json:"), "msg={msg}"),
            other => panic!("expected Infer, got {other:?}"),
        }
    }

    #[test]
    fn nonzero_exit_is_an_error_even_with_text() {
        let err = finish_whisper(false, fixture("english.json").as_bytes(), "decoder crashed")
            .unwrap_err();
        assert_eq!(err, EngineError::Infer("decoder crashed".into()));
    }
}
