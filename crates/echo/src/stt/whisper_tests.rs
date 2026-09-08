use super::*;
use crate::stt::VulkanRuntimeSelector;
use echo_core::{Dictionary, RecognitionHints};
use std::fs;

fn whisper_args(
    model: &Path,
    wav: &Path,
    vad: Option<&Path>,
    options: &DecodeOptions,
) -> Vec<String> {
    super::whisper_args_with_tuning(
        model,
        wav,
        vad,
        options,
        WhisperTuning::runtime_defaults(),
        false,
    )
}

fn options(language: LanguageChoice) -> DecodeOptions {
    DecodeOptions {
        language,
        hints: RecognitionHints::default(),
    }
}

fn options_with_hint(language: LanguageChoice, written: &str) -> DecodeOptions {
    let dir = std::env::temp_dir().join(format!(
        "echo-whisper-hints-{}-{}",
        std::process::id(),
        written.len()
    ));
    let _ = fs::create_dir_all(&dir);
    let mut dictionary = Dictionary::load_from(dir.join("dictionary.json")).unwrap();
    dictionary.add("misheard", written).unwrap();
    DecodeOptions {
        language,
        hints: RecognitionHints::from_dictionary(&dictionary),
    }
}

#[test]
fn missing_model_is_engine_missing() {
    // Process-scoped and pre-cleaned: a stray .bin left by anything else
    // would turn Missing into a different error.
    let dir =
        std::env::temp_dir().join(format!("echo-empty-whisper-models-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let engine = WhisperEngine::configured(ModelCache::at(&dir), "base.en");
    let pcm = Pcm16kMono::from_samples(vec![0; 16]);
    assert_eq!(
        engine.transcribe(&pcm, &options(LanguageChoice::default())),
        Err(EngineError::Missing)
    );
}

#[test]
fn blank_audio_raw_is_empty() {
    assert!(raw_text("[BLANK_AUDIO]").is_empty());
}

#[test]
fn explicit_launch_contract_replaces_loader_and_cache_state() {
    let launch = WhisperRuntimeLaunch {
        library_dir: Some(PathBuf::from("/runtime")),
        vulkan_driver_files: Some(PathBuf::from("/driver.json")),
        mesa_shader_cache_dir: Some(PathBuf::from("/shader-cache")),
        vulkan_selector: Some(
            VulkanRuntimeSelector::parse("1".repeat(32), "2".repeat(32)).unwrap(),
        ),
        identity_sha256: Some("a".repeat(64)),
        cancel_on_recording: None,
    };
    let command = command_for_runtime(Path::new("whisper-cli"), Some(&launch));
    let value = |name: &str| {
        command
            .get_envs()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    };
    assert_eq!(
        value("LD_LIBRARY_PATH").flatten(),
        Some(std::ffi::OsStr::new("/runtime"))
    );
    assert_eq!(
        value("VK_DRIVER_FILES").flatten(),
        Some(std::ffi::OsStr::new("/driver.json"))
    );
    assert_eq!(
        value("MESA_SHADER_CACHE_DIR").flatten(),
        Some(std::ffi::OsStr::new("/shader-cache"))
    );
    assert_eq!(
        value("ECHO_WHISPER_VULKAN_DEVICE_UUID").flatten(),
        Some(std::ffi::OsStr::new("11111111111111111111111111111111"))
    );
    assert_eq!(
        value("ECHO_WHISPER_VULKAN_DRIVER_UUID").flatten(),
        Some(std::ffi::OsStr::new("22222222222222222222222222222222"))
    );
    assert_eq!(value("LD_PRELOAD"), Some(None));
    assert_eq!(value("VK_ICD_FILENAMES"), Some(None));
    assert!(command_for_runtime(Path::new("whisper-cli"), None)
        .get_envs()
        .next()
        .is_none());
}

#[test]
fn launch_contract_recognizes_loader_and_device_selector_namespaces() {
    for name in [
        "LD_DEBUG",
        "MESA_VK_DEVICE_SELECT",
        "DRI_PRIME",
        "VK_LOADER_LAYERS_ENABLE",
        "RADV_PERFTEST",
        "__GLX_VENDOR_LIBRARY_NAME",
        "CUDA_VISIBLE_DEVICES",
        "GGML_VK_VISIBLE_DEVICES",
        "HSA_OVERRIDE_GFX_VERSION",
    ] {
        assert!(
            is_inference_environment_selector(std::ffi::OsStr::new(name)),
            "{name}"
        );
    }
    for name in ["ECHO_MODEL_DIR", "LANG", "PATH"] {
        assert!(!is_inference_environment_selector(std::ffi::OsStr::new(
            name
        )));
    }
}

#[test]
fn calibration_child_is_reaped_when_recording_starts() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "echo-whisper-calibration-cancel-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let binary = root.join("whisper-cli");
    fs::write(&binary, "#!/bin/sh\nsleep 5\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let model = root.join("model.bin");
    fs::write(&model, b"model").unwrap();
    let lock = root.join("recording.lock");
    fs::write(&lock, format!("{}\ntest-token\n", std::process::id())).unwrap();
    let mut launch = whisper_runtime_launch(&binary);
    launch.cancel_on_recording = Some(lock);
    let plan = WhisperExecutionPlan::one_shot(
        WhisperRuntimeCandidate {
            source: WhisperRuntimeSource::Managed,
            backend: WhisperRuntimeBackend::Cpu,
            cli: binary,
            server: None,
            launch,
        },
        WhisperModelAsset {
            name: "small".to_string(),
            path: model,
            multilingual: true,
        },
        None,
    );
    let started = Instant::now();
    let error = WhisperEngine::with_plan(plan)
        .transcribe(
            &Pcm16kMono::from_samples(vec![0; 160]),
            &options(LanguageChoice::Pinned(Language::ENGLISH)),
        )
        .unwrap_err();
    assert!(error.as_str().contains("canceled because recording"));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn effective_launch_rehashes_the_runtime_immediately_before_inference() {
    let root = std::env::temp_dir().join(format!(
        "echo-whisper-effective-launch-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let cli = root.join("whisper-cli");
    let library = root.join("libwhisper.so");
    fs::write(&cli, b"cli").unwrap();
    fs::write(&library, b"library-v1").unwrap();
    let mut candidate = WhisperRuntimeCandidate {
        source: WhisperRuntimeSource::System,
        backend: WhisperRuntimeBackend::Unknown,
        cli: cli.clone(),
        server: None,
        launch: whisper_runtime_launch(&cli),
    };
    candidate.launch.vulkan_driver_files = Some(root.join("driver.json"));
    candidate.launch.mesa_shader_cache_dir = Some(root.join("cache"));
    let original_identity = candidate.launch.identity_sha256.clone();
    let engine = WhisperEngine::with_plan(WhisperExecutionPlan::one_shot(
        candidate,
        WhisperModelAsset {
            name: "small".to_string(),
            path: root.join("model.bin"),
            multilingual: true,
        },
        None,
    ));
    fs::write(&library, b"library-v2").unwrap();
    let effective = engine.effective_runtime_launch(&cli).unwrap();
    assert_ne!(effective.identity_sha256, original_identity);
    assert_eq!(
        effective.vulkan_driver_files,
        Some(root.join("driver.json"))
    );
    assert_eq!(effective.mesa_shader_cache_dir, Some(root.join("cache")));
}

fn args_for_cache(dir: &Path) -> Vec<String> {
    let cache = ModelCache::at(dir);
    whisper_args(
        Path::new("model.bin"),
        Path::new("in.wav"),
        cache.vad_model().as_deref(),
        &options(LanguageChoice::default()),
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
        &options(LanguageChoice::default()),
    );
    assert!(with_vad.iter().any(|arg| arg == "--vad"));
    assert_eq!(vm_path(&with_vad), Some("ggml-silero-v6.2.0.bin"));

    let without_vad = whisper_args(
        Path::new("model.bin"),
        Path::new("in.wav"),
        None,
        &options(LanguageChoice::default()),
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
fn model_file_finds_scanned_and_unconventional_names() {
    let dir = std::env::temp_dir().join(format!("echo-model-file-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("ggml-small.en-q5_1.bin"), []).unwrap();
    fs::write(dir.join("my-finetune.bin"), []).unwrap();
    let scanned = WhisperEngine::configured(ModelCache::at(&dir), "small.en-q5_1");
    assert_eq!(
        scanned.model_file().as_deref(),
        Some(dir.join("ggml-small.en-q5_1.bin").as_path())
    );
    let custom = WhisperEngine::configured(ModelCache::at(&dir), "my-finetune");
    assert_eq!(
        custom.model_file().as_deref(),
        Some(dir.join("my-finetune.bin").as_path())
    );
    let missing = WhisperEngine::configured(ModelCache::at(&dir), "large-v3");
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
        &options(german),
    );
    assert_eq!(language_arg(&args), Some("de"));
}

#[test]
fn auto_language_yields_dash_l_auto() {
    let args = whisper_args(
        Path::new("model.bin"),
        Path::new("in.wav"),
        None,
        &options(LanguageChoice::Auto),
    );
    assert_eq!(language_arg(&args), Some("auto"));
}

#[test]
fn prompt_is_one_argument_and_survives_vad_retry_args() {
    let options = options_with_hint(LanguageChoice::Auto, "Claude Code");
    let first = whisper_args(
        Path::new("model.bin"),
        Path::new("in.wav"),
        Some(Path::new("vad.bin")),
        &options,
    );
    let retry = whisper_args(Path::new("model.bin"), Path::new("in.wav"), None, &options);
    for args in [&first, &retry] {
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--prompt")
                .map(|pair| pair[1].as_str()),
            Some("Claude Code")
        );
        assert_eq!(language_arg(args), Some("auto"));
    }
    assert!(first.iter().any(|arg| arg == "--vad"));
    assert!(retry.iter().all(|arg| arg != "--vad"));
}

#[test]
fn empty_hints_omit_prompt() {
    let args = whisper_args(
        Path::new("model.bin"),
        Path::new("in.wav"),
        None,
        &options(LanguageChoice::Auto),
    );
    assert!(args.iter().all(|arg| arg != "--prompt"));
}

#[test]
fn normal_runs_preserve_runtime_tuning_defaults() {
    let args = whisper_args(
        Path::new("model.bin"),
        Path::new("in.wav"),
        None,
        &options(LanguageChoice::Auto),
    );
    assert!(args
        .iter()
        .all(|arg| !matches!(arg.as_str(), "-t" | "-bs" | "-bo" | "-nf")));
    assert!(args.iter().all(|arg| arg != "--no-gpu"));
}

#[test]
fn cpu_only_benchmark_plan_passes_the_upstream_flag() {
    let args = whisper_args_with_tuning(
        Path::new("model.bin"),
        Path::new("in.wav"),
        None,
        &options(LanguageChoice::Auto),
        WhisperTuning::runtime_defaults(),
        true,
    );
    assert!(args.iter().any(|arg| arg == "--no-gpu"));
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
        let args = whisper_args(
            Path::new("m.bin"),
            Path::new("in.wav"),
            None,
            &options(choice),
        );
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
    let engine = WhisperEngine::configured(ModelCache::at(&dir), "base.en");
    // The refusal must fire even with no whisper binary on PATH.
    match engine.transcribe(&pcm, &options(german)) {
        Err(EngineError::Infer(message)) => {
            assert!(message.contains("ggml-base.en.bin"), "msg={message}");
            assert!(message.contains("german"), "msg={message}");
        }
        other => panic!("expected refusal, got {other:?}"),
    }

    let auto = WhisperEngine::configured(ModelCache::at(&dir), "base.en");
    match auto.transcribe(&pcm, &options(LanguageChoice::Auto)) {
        Err(EngineError::Infer(message)) => {
            assert!(
                message.contains("automatic language detection"),
                "msg={message}"
            )
        }
        other => panic!("expected refusal, got {other:?}"),
    }

    // Pinned English on an .en model is accepted by the preflight check.
    assert!(refuse_impossible_language(
        &dir.join("ggml-base.en.bin"),
        false,
        LanguageChoice::Pinned(Language::ENGLISH),
    )
    .is_ok());

    // A multilingual model takes any pinned language.
    assert!(refuse_impossible_language(&dir.join("ggml-small.bin"), true, german).is_ok());
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
    assert_eq!(parse_detection_probability(stderr), Some(0.958_162));
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
fn malformed_successful_json_includes_bounded_stderr_diagnostics() {
    let err = finish_whisper(
        true,
        b"not json at all",
        "model loaded\ndecoder emitted malformed output",
    )
    .unwrap_err();
    let message = err.as_str();
    assert!(message.starts_with("whisper json:"), "{message}");
    assert!(
        message.contains("Whisper stderr:\nmodel loaded"),
        "{message}"
    );
    assert!(
        message.contains("decoder emitted malformed output"),
        "{message}"
    );

    let without_stderr = finish_whisper(true, b"not json at all", "").unwrap_err();
    assert_eq!(
        without_stderr,
        parse_whisper_json("not json at all").unwrap_err()
    );
}

#[test]
fn nonzero_exit_is_an_error_even_with_text() {
    let err =
        finish_whisper(false, fixture("english.json").as_bytes(), "decoder crashed").unwrap_err();
    assert_eq!(err, EngineError::Infer("decoder crashed".into()));
}

#[test]
fn bounded_stderr_retains_head_and_tail_with_an_omission_marker() {
    let mut raw = b"diagnostic-head\n".to_vec();
    raw.extend(std::iter::repeat_n(b'x', STDERR_CAPTURE_LIMIT * 2));
    raw.extend_from_slice(b"\ndiagnostic-tail");
    let mut capture = BoundedStderr::new();
    for chunk in raw.chunks(997) {
        capture.extend(chunk);
    }
    let bounded = capture.finish();

    assert_eq!(bounded.len(), STDERR_CAPTURE_LIMIT);
    assert!(bounded.starts_with(b"diagnostic-head\n"));
    assert!(bounded.ends_with(b"\ndiagnostic-tail"));
    assert!(bounded
        .windows(STDERR_OMISSION_MARKER.len())
        .any(|window| window == STDERR_OMISSION_MARKER));
}

#[cfg(unix)]
#[test]
fn process_stderr_is_bounded_while_the_pipe_is_fully_drained() {
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "yes x | head -c 131072 >&2; printf '\\ndiagnostic-tail' >&2",
    ]);
    let output = run_process_group_with_diagnostics(
        command,
        Instant::now() + Duration::from_secs(5),
        &|| false,
        true,
    )
    .unwrap()
    .output;

    assert!(output.status.success());
    assert_eq!(output.stderr.len(), STDERR_CAPTURE_LIMIT);
    assert!(output.stderr.ends_with(b"\ndiagnostic-tail"));
    assert!(output
        .stderr
        .windows(STDERR_OMISSION_MARKER.len())
        .any(|window| window == STDERR_OMISSION_MARKER));
}

#[test]
fn only_vad_failures_allow_the_no_vad_retry() {
    assert!(should_retry_without_vad("failed to load VAD model"));
    assert!(should_retry_without_vad(
        "whisper_vad: failed to initialize VAD context"
    ));
    assert!(should_retry_without_vad("error: unknown argument: --vad"));
    assert!(!should_retry_without_vad("decoder crashed"));
    assert!(!should_retry_without_vad("model allocation failed"));
    assert!(!should_retry_without_vad(
        "decoder crashed after loading ggml-silero-v6.2.0.bin"
    ));
}

#[cfg(unix)]
#[test]
fn runtime_timeout_kills_and_reaps_a_hung_process() {
    let started = Instant::now();
    let error = run_attempt(
        Path::new("/bin/sh"),
        None,
        vec!["-c".to_string(), "sleep 5".to_string()],
        false,
        std::time::Duration::from_millis(30),
        Instant::now() + std::time::Duration::from_millis(30),
        &|| false,
    )
    .unwrap_err();
    assert!(error.as_str().contains("timed out"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[cfg(unix)]
#[test]
fn runtime_timeout_surfaces_stderr_diagnostics() {
    let error = run_attempt(
        Path::new("/bin/sh"),
        None,
        vec![
            "-c".to_string(),
            "printf timeout-diagnostic >&2; sleep 5".to_string(),
        ],
        false,
        Duration::from_millis(500),
        Instant::now() + Duration::from_millis(500),
        &|| false,
    )
    .unwrap_err();
    let message = error.as_str();
    assert!(message.contains("timed out"), "{message}");
    assert!(message.contains("timeout-diagnostic"), "{message}");
}

#[cfg(unix)]
#[test]
fn runtime_cancellation_surfaces_stderr_diagnostics() {
    let root = std::env::temp_dir().join(format!(
        "echo-whisper-cancel-diagnostic-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let marker = root.join("ready");
    let script = format!(
        "printf cancel-diagnostic >&2; touch '{}'; sleep 5",
        marker.display()
    );
    let error = run_attempt(
        Path::new("/bin/sh"),
        None,
        vec!["-c".to_string(), script],
        false,
        Duration::from_secs(5),
        Instant::now() + Duration::from_secs(5),
        &|| marker.exists(),
    )
    .unwrap_err();
    let message = error.as_str();
    assert!(message.contains("canceled"), "{message}");
    assert!(message.contains("cancel-diagnostic"), "{message}");
}
