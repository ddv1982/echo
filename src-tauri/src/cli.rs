use std::ffi::OsString;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use echo::transcribe::{
    CompletedTranscription, LanguageCatalog, LanguageSelection, PrepareError, ResolvedEngine,
    RunOverrides,
};
use echo_core::{Config, Dictionary, EngineChoice, LanguageChoice};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "echo-desktop", version, disable_help_subcommand = true)]
struct Cli {
    #[arg(long)]
    hud_demo: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Rec(RecArgs),
    Transcribe(TranscribeArgs),
    Languages(LanguagesArgs),
    #[command(hide = true)]
    WhisperCalibrate(WhisperCalibrateArgs),
}

#[derive(Debug, Args)]
struct WhisperCalibrateArgs {
    #[arg(long)]
    job: PathBuf,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
struct RecArgs {
    #[arg(long)]
    once: bool,
    #[arg(long)]
    toggle: bool,
}

#[derive(Debug, Args)]
struct TranscribeArgs {
    file: PathBuf,
    #[arg(long, value_enum)]
    engine: Option<CliEngine>,
    #[arg(long, value_parser = nonempty_model)]
    model: Option<String>,
    #[arg(long, value_parser = parse_language)]
    language: Option<LanguageChoice>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    #[arg(long, default_value = "-")]
    output: PathBuf,
    #[arg(long)]
    raw: bool,
    #[arg(long)]
    whisper_threads: Option<NonZeroUsize>,
    #[arg(long, value_parser = positive_u8)]
    whisper_beam_size: Option<u8>,
    #[arg(long, value_parser = positive_u8)]
    whisper_best_of: Option<u8>,
    #[arg(long)]
    whisper_no_fallback: bool,
    #[arg(long, value_enum)]
    whisper_acceleration: Option<CliWhisperAcceleration>,
    #[arg(long, hide = true)]
    whisper_no_gpu: bool,
    #[arg(long, hide = true)]
    whisper_vulkan_driver_files: Option<PathBuf>,
    #[arg(long, hide = true)]
    whisper_mesa_shader_cache_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct LanguagesArgs {
    #[arg(long, value_enum)]
    engine: Option<CatalogEngine>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliEngine {
    Auto,
    Whisper,
    Parakeet,
}

impl From<CliEngine> for EngineChoice {
    fn from(value: CliEngine) -> Self {
        match value {
            CliEngine::Auto => Self::Auto,
            CliEngine::Whisper => Self::Whisper,
            CliEngine::Parakeet => Self::Parakeet,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CatalogEngine {
    Whisper,
    Parakeet,
}

impl From<CatalogEngine> for EngineChoice {
    fn from(value: CatalogEngine) -> Self {
        match value {
            CatalogEngine::Whisper => Self::Whisper,
            CatalogEngine::Parakeet => Self::Parakeet,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliWhisperAcceleration {
    Auto,
    Gpu,
    Cpu,
}

impl From<CliWhisperAcceleration> for echo_core::WhisperAccelerationPreference {
    fn from(value: CliWhisperAcceleration) -> Self {
        match value {
            CliWhisperAcceleration::Auto => Self::Auto,
            CliWhisperAcceleration::Gpu => Self::Gpu,
            CliWhisperAcceleration::Cpu => Self::Cpu,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
    let args = args.into_iter().collect::<Vec<_>>();
    if args.first().and_then(|arg| arg.to_str()) == Some("rec") {
        let rec_args = args
            .get(1..)
            .unwrap_or_default()
            .iter()
            .filter_map(|arg| arg.to_str())
            .collect::<Vec<_>>();
        if !matches!(
            rec_args.as_slice(),
            ["--once"] | ["--toggle"] | ["--help"] | ["-h"]
        ) {
            eprintln!("usage: echo-desktop rec --once|--toggle");
            return 2;
        }
    }
    let parsed =
        match Cli::try_parse_from(std::iter::once(OsString::from("echo-desktop")).chain(args)) {
            Ok(parsed) => parsed,
            Err(error) => {
                let code = error.exit_code();
                let _ = error.print();
                return code;
            }
        };
    if parsed.hud_demo {
        if parsed.command.is_some() {
            eprintln!("error: --hud-demo cannot be combined with a command");
            return 2;
        }
        return match echo::ui::hud::run_hud_demo() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("hud-demo: {error}");
                1
            }
        };
    }
    match parsed.command {
        Some(Command::Rec(args)) => run_rec(args),
        Some(Command::Transcribe(args)) => match run_transcribe(args) {
            Ok(()) => 0,
            Err(failure) => {
                eprintln!("{}", failure.message);
                failure.code
            }
        },
        Some(Command::Languages(args)) => match run_languages(args) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                1
            }
        },
        Some(Command::WhisperCalibrate(args)) => match echo::stt::run_calibration_job(&args.job) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("whisper-calibrate: {message}");
                1
            }
        },
        None => {
            eprintln!("error: a command is required");
            2
        }
    }
}

fn run_rec(args: RecArgs) -> i32 {
    echo::notify::enable_failure_notifications();
    if args.once {
        echo::rec::run_rec_once()
    } else if args.toggle {
        echo::rec::run_rec_toggle()
    } else {
        2
    }
}

struct CliFailure {
    code: i32,
    message: String,
}

impl CliFailure {
    fn argument(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: format!("error: {}", message.into()),
        }
    }

    fn runtime(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: format!("transcribe: {}", message.into()),
        }
    }
}

fn run_transcribe(args: TranscribeArgs) -> Result<(), CliFailure> {
    if args.raw && args.format == OutputFormat::Json {
        return Err(CliFailure::argument(
            "--raw cannot be combined with --format json",
        ));
    }
    if args.engine == Some(CliEngine::Parakeet) && args.model.is_some() {
        return Err(CliFailure::argument("--model requires the Whisper engine"));
    }
    if args.engine == Some(CliEngine::Parakeet)
        && matches!(args.language, Some(LanguageChoice::Pinned(_)))
    {
        return Err(CliFailure::argument(
            "Parakeet supports automatic language selection only",
        ));
    }
    let has_whisper_performance_options = args.whisper_threads.is_some()
        || args.whisper_beam_size.is_some()
        || args.whisper_best_of.is_some()
        || args.whisper_no_fallback
        || args.whisper_acceleration.is_some()
        || args.whisper_no_gpu
        || args.whisper_vulkan_driver_files.is_some()
        || args.whisper_mesa_shader_cache_dir.is_some();
    if args.engine == Some(CliEngine::Parakeet) && has_whisper_performance_options {
        return Err(CliFailure::argument(
            "Whisper performance options require the Whisper engine",
        ));
    }
    if args.output != Path::new("-") && paths_alias(&args.file, &args.output) {
        return Err(CliFailure::argument(
            "the output path must differ from the input WAV",
        ));
    }

    let config = Config::load_read_only().map_err(CliFailure::runtime)?;
    let dictionary = Dictionary::load_read_only().map_err(CliFailure::runtime)?;
    let whisper_tuning = (args.whisper_threads.is_some()
        || args.whisper_beam_size.is_some()
        || args.whisper_best_of.is_some()
        || args.whisper_no_fallback)
        .then_some(echo::stt::WhisperTuningOverride {
            threads: args.whisper_threads,
            beam_size: args.whisper_beam_size,
            best_of: args.whisper_best_of,
            no_fallback: args.whisper_no_fallback.then_some(true),
        });
    let overrides = RunOverrides {
        engine: args.engine.map(Into::into),
        whisper_model: args.model,
        language: args.language,
        whisper_tuning,
        whisper_force_cpu: args.whisper_no_gpu,
        whisper_acceleration: args.whisper_acceleration.map(Into::into),
        whisper_vulkan_driver_files: args.whisper_vulkan_driver_files,
        whisper_mesa_shader_cache_dir: args.whisper_mesa_shader_cache_dir,
    };
    let prepared =
        echo::transcribe::prepare_with_config(overrides, &config).map_err(|error| match error {
            PrepareError::InvalidRequest(message) => CliFailure::argument(message),
            PrepareError::Configuration(message) | PrepareError::EngineMissing(message) => {
                CliFailure::runtime(message)
            }
        })?;
    let cleanup_policy = if args.raw {
        echo::transcribe::CleanupPolicy::Skip
    } else {
        echo::transcribe::CleanupPolicy::Strict
    };
    let result =
        echo::transcribe::transcribe_file(&args.file, &prepared, &dictionary, cleanup_policy)
            .map_err(|error| CliFailure::runtime(error.to_string()))?;
    let payload = render_transcription(&result, prepared.resolved(), args.format, args.raw)
        .map_err(CliFailure::runtime)?;
    write_payload(&args.output, &payload).map_err(CliFailure::runtime)
}

fn paths_alias(left: &Path, right: &Path) -> bool {
    match (normalize_path(left), normalize_path(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn normalize_path(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => {
                normalized.push(part);
                if normalized.exists() {
                    normalized = normalized.canonicalize()?;
                }
            }
        }
    }
    Ok(normalized)
}

fn render_transcription(
    result: &CompletedTranscription,
    resolved: &echo::transcribe::ResolvedRun,
    format: OutputFormat,
    raw: bool,
) -> Result<Vec<u8>, String> {
    let body = match format {
        OutputFormat::Text => {
            if raw {
                result.raw.clone()
            } else {
                result.text.clone()
            }
        }
        OutputFormat::Json => serde_json::to_string(&TranscriptionJsonV1::new(result, resolved))
            .map_err(|error| error.to_string())?,
    };
    Ok(one_trailing_newline(body).into_bytes())
}

fn one_trailing_newline(mut body: String) -> String {
    while body.ends_with(['\n', '\r']) {
        body.pop();
    }
    body.push('\n');
    body
}

fn write_payload(output: &Path, payload: &[u8]) -> Result<(), String> {
    if output == Path::new("-") {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(payload)
            .map_err(|error| error.to_string())?;
        stdout.flush().map_err(|error| error.to_string())
    } else {
        echo_core::write_atomic(output, payload)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionJsonV1<'a> {
    schema_version: u8,
    text: &'a str,
    raw: &'a str,
    audio_ms: u64,
    infer_ms: u64,
    engine: EngineJsonV1<'a>,
    language: LanguageJsonV1<'a>,
    hint_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    whisper: Option<&'a echo_core::WhisperRunTelemetry>,
}

impl<'a> TranscriptionJsonV1<'a> {
    fn new(
        result: &'a CompletedTranscription,
        resolved: &'a echo::transcribe::ResolvedRun,
    ) -> Self {
        let (id, model) = match &resolved.engine {
            ResolvedEngine::Whisper { model, .. } => ("whisper", Some(model.as_str())),
            ResolvedEngine::ParakeetTdt06bV3 => ("parakeet", Some("tdt-0.6b-v3")),
            ResolvedEngine::Fake => ("fake", Some("fake")),
        };
        Self {
            schema_version: 1,
            text: &result.text,
            raw: &result.raw,
            audio_ms: result.audio_ms,
            infer_ms: result.infer_ms,
            engine: EngineJsonV1 {
                id,
                model,
                binary: result.detail.binary.as_deref(),
                model_path: result.detail.model_path.as_deref(),
                vad_path: result.detail.vad_path.as_deref(),
                multilingual: result.detail.multilingual,
                vad: result.detail.vad,
            },
            language: LanguageJsonV1 {
                requested: result.requested_language.as_str(),
                observed: result.detail.language.as_deref(),
                probability: result.detail.language_probability,
            },
            hint_count: result.hint_count,
            whisper: result.detail.whisper.as_ref(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineJsonV1<'a> {
    id: &'static str,
    model: Option<&'a str>,
    binary: Option<&'a str>,
    model_path: Option<&'a str>,
    vad_path: Option<&'a str>,
    multilingual: Option<bool>,
    vad: Option<bool>,
}

#[derive(Serialize)]
struct LanguageJsonV1<'a> {
    requested: &'static str,
    observed: Option<&'a str>,
    probability: Option<f32>,
}

fn run_languages(args: LanguagesArgs) -> Result<(), String> {
    let config = Config::load_read_only()?;
    let catalog = echo::transcribe::language_catalog(args.engine.map(Into::into), &config);
    let payload = match args.format {
        OutputFormat::Text => render_languages_text(&catalog),
        OutputFormat::Json => serde_json::to_string(&LanguagesJsonV1::from(&catalog))
            .map_err(|error| error.to_string())?,
    };
    write_payload(Path::new("-"), one_trailing_newline(payload).as_bytes())
}

fn render_languages_text(catalog: &LanguageCatalog) -> String {
    let selection = selection_name(catalog.selection);
    let mut lines = vec![format!("engine\t{}", catalog.engine)];
    if let Some(model) = &catalog.model {
        lines.push(format!("model\t{model}"));
    }
    lines.push(format!("selection\t{selection}"));
    lines.extend(
        catalog
            .languages
            .iter()
            .map(|language| format!("{}\t{}", language.code(), language.english_name())),
    );
    lines.join("\n")
}

fn selection_name(selection: LanguageSelection) -> &'static str {
    match selection {
        LanguageSelection::AutoOrPinned => "auto-or-pinned",
        LanguageSelection::EnglishOnly => "english-only",
        LanguageSelection::AutomaticOnly => "automatic-only",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguagesJsonV1<'a> {
    schema_version: u8,
    engine: &'a str,
    model: Option<&'a str>,
    selection: &'static str,
    languages: Vec<CatalogLanguageJsonV1>,
}

impl<'a> From<&'a LanguageCatalog> for LanguagesJsonV1<'a> {
    fn from(catalog: &'a LanguageCatalog) -> Self {
        Self {
            schema_version: 1,
            engine: catalog.engine,
            model: catalog.model.as_deref(),
            selection: selection_name(catalog.selection),
            languages: catalog
                .languages
                .iter()
                .map(|language| CatalogLanguageJsonV1 {
                    code: language.code(),
                    name: language.english_name(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct CatalogLanguageJsonV1 {
    code: &'static str,
    name: &'static str,
}

fn parse_language(raw: &str) -> Result<LanguageChoice, String> {
    LanguageChoice::parse(raw).ok_or_else(|| format!("unknown language {raw}"))
}

fn nonempty_model(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        Err("model name cannot be empty".to_string())
    } else {
        Ok(raw.to_string())
    }
}

fn positive_u8(raw: &str) -> Result<u8, String> {
    let value = raw
        .parse::<u8>()
        .map_err(|_| format!("{raw} is not an integer from 1 through 255"))?;
    if value == 0 {
        Err("value must be at least 1".to_string())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn trailing_newline_is_exact() {
        assert_eq!(one_trailing_newline("hello".into()), "hello\n");
        assert_eq!(one_trailing_newline("hello\n\n".into()), "hello\n");
        assert_eq!(one_trailing_newline(String::new()), "\n");
    }

    #[test]
    fn calibration_owner_is_callable_but_hidden_from_product_help() {
        let help = Cli::command().render_long_help().to_string();
        assert!(!help.contains("whisper-calibrate"));
        assert!(matches!(
            Cli::try_parse_from([
                "echo-desktop",
                "whisper-calibrate",
                "--job",
                "/tmp/job.json"
            ])
            .unwrap()
            .command,
            Some(Command::WhisperCalibrate(_))
        ));
    }
}
