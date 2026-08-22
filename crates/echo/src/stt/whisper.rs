use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use echo_core::{
    strip_nonspeech, Config, Engine, EngineError, EngineId, Language, LanguageChoice, Pcm16kMono,
    RunDetail, Transcript,
};
use serde::Deserialize;

use super::cache::{parse_whisper_filename, ModelCache, ModelInventory};
use super::write_temp_wav;
use crate::settings::file_config;
use crate::which::path_of;

/// The model Echo runs: `ECHO_WHISPER_MODEL` wins over the config file, and
/// with neither set the best installed model runs instead of a hardcoded
/// name, so dropping a better model into the directory never silently changes
/// the weights under a pinned choice, and a better download is used at once.
fn resolved_whisper_model(
    env: Option<String>,
    file: &Config,
    inventory: &ModelInventory,
) -> Option<String> {
    env.filter(|name| !name.is_empty())
        .or_else(|| file.whisper_model.clone())
        .or_else(|| inventory.best_whisper().map(|model| model.name.clone()))
}

pub struct WhisperEngine {
    cache: ModelCache,
    model: Option<String>,
    language: LanguageChoice,
}

impl Default for WhisperEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperEngine {
    #[must_use]
    pub fn new() -> Self {
        let cache = ModelCache::from_env();
        let file = file_config();
        let model = resolved_whisper_model(
            std::env::var("ECHO_WHISPER_MODEL").ok(),
            &file,
            &cache.inventory(),
        );
        let mut engine = Self {
            cache,
            model,
            language: LanguageChoice::default(),
        };
        // The model-aware default: a multilingual model auto-detects, an
        // English-only model pins English. selected_model covers both the
        // scanned inventory and the probe fallback.
        let multilingual = engine.selected_model().map(|(_, multilingual)| multilingual);
        engine.language = super::resolved_language(
            std::env::var("ECHO_LANGUAGE").ok().as_deref(),
            &file,
            multilingual,
        );
        engine
    }

    #[must_use]
    pub fn with_cache(cache: ModelCache, model: impl Into<String>) -> Self {
        Self {
            cache,
            model: Some(model.into()),
            language: LanguageChoice::default(),
        }
    }

    #[must_use]
    pub fn with_language(mut self, language: LanguageChoice) -> Self {
        self.language = language;
        self
    }

    /// True when both the runner binary and a model file are installed.
    /// A metadata check only; no inference runs.
    #[must_use]
    pub fn available(&self) -> bool {
        self.model_file().is_some() && Self::binary().is_some()
    }

    #[must_use]
    pub fn model_name(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn model_file(&self) -> Option<PathBuf> {
        self.selected_model().map(|(path, _)| path)
    }

    /// The model file plus its filename-derived multilingual flag. The flag
    /// is a pre-flight guess used to refuse impossible language choices; the
    /// authoritative value is `model.multilingual` in the engine's JSON.
    pub(crate) fn selected_model(&self) -> Option<(PathBuf, bool)> {
        let model = self.model.as_deref()?;
        let inventory = self.cache.inventory();
        if let Some(installed) = inventory.whisper.iter().find(|m| m.name == model) {
            return Some((installed.path.clone(), installed.multilingual));
        }
        // A configured name outside the GGML convention still resolves, so a
        // fine-tuned file the scanner ignores remains usable when pinned.
        let candidates = [
            format!("ggml-{model}.bin"),
            format!("{model}.bin"),
            format!("ggml-{model}.gguf"),
        ];
        candidates
            .into_iter()
            .map(|name| self.cache.path(&name))
            .find(|path| path.is_file())
            .map(|path| {
                let multilingual = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(parse_whisper_filename)
                    .map(|(_, _, multilingual, _)| multilingual)
                    .unwrap_or(true);
                (path, multilingual)
            })
    }

    pub(crate) fn binary() -> Option<PathBuf> {
        ["whisper-cli", "whisper-cpp", "whisper"]
            .into_iter()
            .find_map(path_of)
    }
}

impl Engine for WhisperEngine {
    fn id(&self) -> EngineId {
        EngineId::Whisper {
            model: self.model.clone().unwrap_or_default(),
        }
    }

    fn transcribe(&self, pcm: &Pcm16kMono) -> Result<Transcript, EngineError> {
        let (model, multilingual) = self.selected_model().ok_or(EngineError::Missing)?;
        refuse_impossible_language(&model, multilingual, self.language)?;
        let bin = Self::binary().ok_or(EngineError::Missing)?;
        let started = Instant::now();
        let wav = write_temp_wav(pcm).map_err(EngineError::Infer)?;
        let vad = self.cache.vad_model();
        let first = Command::new(&bin)
            .args(whisper_args(&model, &wav, vad.as_deref(), self.language))
            .output()
            .map_err(|err| EngineError::Infer(err.to_string()))?;
        let (status, vad_active) = if !first.status.success() && vad.is_some() {
            let retry = Command::new(&bin)
                .args(whisper_args(&model, &wav, None, self.language))
                .output()
                .map_err(|err| EngineError::Infer(err.to_string()))?;
            (retry, false)
        } else {
            (first, vad.is_some())
        };
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
            language: parsed.language.clone(),
            audio_ms: pcm.duration_ms(),
            infer_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            detail: RunDetail {
                binary: Some(bin.to_string_lossy().into_owned()),
                model_path: Some(model.to_string_lossy().into_owned()),
                multilingual: Some(parsed.multilingual),
                vad: Some(vad_active),
                language: parsed.language.clone(),
                language_probability: parsed.language_probability,
            },
        })
    }
}

/// Refuse before spawning when the model cannot honour the language choice.
/// Measured upstream: an `.en` model given `-l de` prints a warning, resets
/// to English, transcribes English, and exits 0, so passing the flag through
/// would return confident English text for German speech. `-dl` bypasses that
/// guard through an upstream bug and is never invoked.
fn refuse_impossible_language(
    model: &Path,
    multilingual: bool,
    choice: LanguageChoice,
) -> Result<(), EngineError> {
    if multilingual {
        return Ok(());
    }
    let wants = match choice {
        LanguageChoice::Pinned(Language::ENGLISH) => return Ok(()),
        LanguageChoice::Pinned(language) => language.english_name().to_string(),
        LanguageChoice::Auto => "automatic language detection".to_string(),
    };
    let name = model
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "The selected model".to_string());
    Err(EngineError::Infer(format!(
        "{name} is an English-only model and cannot do {wants}. \
         Choose a multilingual model or set the language to English."
    )))
}

fn whisper_args(
    model: &Path,
    wav: &Path,
    vad: Option<&Path>,
    language: LanguageChoice,
) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-nt".into(),
        "-oj".into(),
        "-of".into(),
        "-".into(),
        "-l".into(),
        match language {
            LanguageChoice::Auto => "auto".to_string(),
            LanguageChoice::Pinned(language) => language.code().to_string(),
        },
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

#[derive(Debug, Clone, PartialEq)]
struct WhisperParse {
    text: String,
    language: Option<String>,
    model: String,
    multilingual: bool,
    language_probability: Option<f32>,
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
    let mut parsed = parse_whisper_stdout(stdout)?;
    parsed.language_probability = parse_detection_probability(stderr);
    Ok(parsed)
}

/// whisper.cpp prints `auto-detected language: de (p = 0.973123)` on stderr
/// when detection runs; the JSON carries only the code, no probability.
fn parse_detection_probability(stderr: &str) -> Option<f32> {
    let line = stderr
        .lines()
        .find(|line| line.contains("auto-detected language:"))?;
    let after = line.split("p = ").nth(1)?;
    let end = after.find(')')?;
    after[..end].trim().parse().ok()
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
        language_probability: None,
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
            LanguageChoice::default(),
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
            Some(vad),
            LanguageChoice::default(),
        );
        assert!(with_vad.iter().any(|arg| arg == "--vad"));
        assert_eq!(vm_path(&with_vad), Some("ggml-silero-v6.2.0.bin"));

        let without_vad = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            LanguageChoice::default(),
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

    #[test]
    fn whisper_model_prefers_env_then_file_then_best_installed() {
        let dir = std::env::temp_dir().join(format!("echo-resolve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ggml-base.en.bin"), []).unwrap();
        fs::write(dir.join("ggml-small.bin"), []).unwrap();
        let inventory = ModelCache::at(&dir).inventory();
        let file = Config {
            whisper_model: Some("tiny.en".into()),
            ..Config::default()
        };
        assert_eq!(
            resolved_whisper_model(Some("small.en".into()), &file, &inventory).as_deref(),
            Some("small.en")
        );
        assert_eq!(
            resolved_whisper_model(None, &file, &inventory).as_deref(),
            Some("tiny.en")
        );
        assert_eq!(
            resolved_whisper_model(None, &Config::default(), &inventory).as_deref(),
            Some("small")
        );
        assert_eq!(
            resolved_whisper_model(Some(String::new()), &file, &inventory).as_deref(),
            Some("tiny.en")
        );
        let empty = ModelInventory::default();
        assert_eq!(
            resolved_whisper_model(None, &Config::default(), &empty),
            None
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_file_finds_scanned_and_unconventional_names() {
        let dir = std::env::temp_dir().join(format!("echo-model-file-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ggml-small.en-q5_1.bin"), []).unwrap();
        fs::write(dir.join("my-finetune.bin"), []).unwrap();
        let scanned = WhisperEngine::with_cache(ModelCache::at(&dir), "small.en-q5_1");
        assert_eq!(
            scanned.model_file().as_deref(),
            Some(dir.join("ggml-small.en-q5_1.bin").as_path())
        );
        let custom = WhisperEngine::with_cache(ModelCache::at(&dir), "my-finetune");
        assert_eq!(
            custom.model_file().as_deref(),
            Some(dir.join("my-finetune.bin").as_path())
        );
        let missing = WhisperEngine::with_cache(ModelCache::at(&dir), "large-v3");
        assert_eq!(missing.model_file(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    fn language_arg(args: &[String]) -> Option<&str> {
        args.windows(2)
            .find(|pair| pair[0] == "-l")
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn pinned_language_yields_dash_l_code() {
        let german = LanguageChoice::Pinned(Language::from_code("de").unwrap());
        let args = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            german,
        );
        assert_eq!(language_arg(&args), Some("de"));
    }

    #[test]
    fn auto_language_yields_dash_l_auto() {
        let args = whisper_args(
            Path::new("model.bin"),
            Path::new("in.wav"),
            None,
            LanguageChoice::Auto,
        );
        assert_eq!(language_arg(&args), Some("auto"));
    }

    #[test]
    fn args_never_contain_dl_or_a_translate_task() {
        // `-dl` bypasses the multilingual guard through an upstream bug, and
        // turbo models are not trained for translation and would silently
        // return the original language. Neither may ever be constructed.
        for choice in [
            LanguageChoice::Auto,
            LanguageChoice::Pinned(Language::ENGLISH),
            LanguageChoice::Pinned(Language::from_code("de").unwrap()),
        ] {
            let args = whisper_args(Path::new("m.bin"), Path::new("in.wav"), None, choice);
            assert!(!args.iter().any(|arg| arg == "-dl"), "{args:?}");
            assert!(!args.iter().any(|arg| arg == "--task"), "{args:?}");
        }
        let source = include_str!("whisper.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!production.contains(concat!("--", "task")));
        assert!(!production.contains(concat!("\"-d", "l\"")));
    }

    #[test]
    fn english_only_model_refuses_non_english_before_spawning() {
        let dir = std::env::temp_dir().join(format!("echo-refuse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ggml-base.en.bin"), []).unwrap();
        let pcm = Pcm16kMono::from_samples(vec![0; 16]);

        let german = LanguageChoice::Pinned(Language::from_code("de").unwrap());
        let engine = WhisperEngine::with_cache(ModelCache::at(&dir), "base.en").with_language(german);
        // The refusal must fire even with no whisper binary on PATH.
        match engine.transcribe(&pcm) {
            Err(EngineError::Infer(message)) => {
                assert!(message.contains("ggml-base.en.bin"), "msg={message}");
                assert!(message.contains("german"), "msg={message}");
            }
            other => panic!("expected refusal, got {other:?}"),
        }

        let auto = WhisperEngine::with_cache(ModelCache::at(&dir), "base.en")
            .with_language(LanguageChoice::Auto);
        match auto.transcribe(&pcm) {
            Err(EngineError::Infer(message)) => {
                assert!(message.contains("automatic language detection"), "msg={message}")
            }
            other => panic!("expected refusal, got {other:?}"),
        }

        // Pinned English on an .en model is the one combination that runs;
        // with no binary installed it reaches Missing instead of a refusal.
        let english = WhisperEngine::with_cache(ModelCache::at(&dir), "base.en");
        assert_eq!(english.transcribe(&pcm), Err(EngineError::Missing));

        // A multilingual model takes any pinned language.
        fs::write(dir.join("ggml-small.bin"), []).unwrap();
        let multi = WhisperEngine::with_cache(ModelCache::at(&dir), "small").with_language(german);
        assert_eq!(multi.transcribe(&pcm), Err(EngineError::Missing));
        let _ = fs::remove_dir_all(&dir);
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
    fn detection_probability_comes_from_stderr() {
        let stderr = "whisper_full: auto-detected language: de (p = 0.958162)\nmore noise";
        assert_eq!(
            parse_detection_probability(stderr),
            Some(0.958_162)
        );
        assert_eq!(parse_detection_probability("no detection here"), None);
        let parsed = finish_whisper(true, fixture("multilingual.json").as_bytes(), stderr)
            .expect("valid fixture");
        assert_eq!(parsed.language.as_deref(), Some("de"));
        assert_eq!(parsed.language_probability, Some(0.958_162));
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
