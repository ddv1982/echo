use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn scratch(label: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "echo-cli-{label}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/echo/tests/fixtures/claude_code.wav")
}

fn run(root: &Path, args: &[&str]) -> Output {
    let config = root.join("config");
    let data = root.join("data");
    let models = root.join("models");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&models).unwrap();
    Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(args)
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_CONFIG_DIR", config)
        .env("ECHO_DATA_DIR", data)
        .env("ECHO_MODEL_DIR", models)
        .env_remove("ECHO_LANGUAGE")
        .env_remove("ECHO_WHISPER_MODEL")
        .output()
        .unwrap()
}

#[test]
fn text_applies_dictionary_while_raw_preserves_engine_output() {
    let root = scratch("fake-output");
    let wav = fixture();
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("dictionary.json"),
        r#"{"entries":[{"spoken":"claude code","written":"Claude Code","created_at":1}]}"#,
    )
    .unwrap();
    let clean = run(&root, &["transcribe", wav.to_str().unwrap()]);
    assert!(
        clean.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert_eq!(clean.stdout, b"Claude Code\n");
    assert!(clean.stderr.is_empty());

    let raw = run(&root, &["transcribe", wav.to_str().unwrap(), "--raw"]);
    assert!(raw.status.success());
    assert_eq!(raw.stdout, b"claude code\n");

    let json = run(
        &root,
        &["transcribe", wav.to_str().unwrap(), "--format", "json"],
    );
    assert!(json.status.success());
    assert_eq!(json.stdout.last(), Some(&b'\n'));
    assert!(!json.stdout[..json.stdout.len() - 1].ends_with(b"\n"));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["text"], "Claude Code");
    assert_eq!(value["raw"], "claude code");
    assert_eq!(value["audioMs"], 400);
    assert_eq!(value["engine"]["id"], "fake");
    assert_eq!(value["engine"]["model"], "fake");
    assert_eq!(value["language"]["requested"], "en");
    assert!(value["language"]["observed"].is_null());
    assert!(value["language"]["probability"].is_null());
    assert_eq!(value["hintCount"], 0);
    assert!(value.get("confidence").is_none());

    let exact = root.join("result.data");
    let written = run(
        &root,
        &[
            "transcribe",
            wav.to_str().unwrap(),
            "--output",
            exact.to_str().unwrap(),
        ],
    );
    assert!(written.status.success());
    assert!(written.stdout.is_empty());
    assert_eq!(std::fs::read(&exact).unwrap(), b"Claude Code\n");
    assert!(!root.join("result.data.txt").exists());

    let relative = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .current_dir(&root)
        .args([
            "transcribe",
            wav.to_str().unwrap(),
            "--output",
            "relative.data",
        ])
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_CONFIG_DIR", root.join("config-relative"))
        .env("ECHO_DATA_DIR", root.join("data-relative"))
        .env("ECHO_MODEL_DIR", root.join("models-relative"))
        .output()
        .unwrap();
    assert!(
        relative.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&relative.stderr)
    );
    assert_eq!(
        std::fs::read(root.join("relative.data")).unwrap(),
        b"claude code\n"
    );
}

#[test]
fn corrupt_dictionary_is_a_read_only_runtime_failure() {
    let root = scratch("side-effects");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = config_dir.join("config.json");
    let dictionary = data_dir.join("dictionary.json");
    std::fs::write(&config, "{}").unwrap();
    std::fs::write(&dictionary, "corrupt dictionary sentinel").unwrap();

    let wav = fixture();
    let output = run(&root, &["transcribe", wav.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid JSON"), "{stderr}");
    assert!(stderr.contains(dictionary.to_str().unwrap()), "{stderr}");
    assert_eq!(std::fs::read(&config).unwrap(), b"{}");
    assert_eq!(
        std::fs::read(&dictionary).unwrap(),
        b"corrupt dictionary sentinel"
    );
    for name in [
        "history.json",
        "status",
        "recording.lock",
        "recording.stop",
        "dictionary.json.corrupt",
    ] {
        assert!(!data_dir.join(name).exists(), "unexpected {name}");
    }
    assert!(!config_dir.join("config.json.corrupt").exists());
}

#[test]
fn syntax_and_runtime_failures_use_distinct_exit_codes_and_stderr() {
    let root = scratch("errors");
    let wav = fixture();
    for args in [
        vec![
            "transcribe",
            wav.to_str().unwrap(),
            "--raw",
            "--format",
            "json",
        ],
        vec!["transcribe", wav.to_str().unwrap(), "--language", "xx"],
        vec![
            "transcribe",
            wav.to_str().unwrap(),
            "--whisper-threads",
            "2",
        ],
        vec![
            "transcribe",
            wav.to_str().unwrap(),
            "--whisper-beam-size",
            "0",
        ],
        vec![
            "transcribe",
            wav.to_str().unwrap(),
            "--engine",
            "parakeet",
            "--language",
            "de",
        ],
    ] {
        let output = run(&root, &args);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        assert!(output.stdout.is_empty(), "args={args:?}");
        assert!(!output.stderr.is_empty(), "args={args:?}");
    }

    let missing = run(&root, &["transcribe", "/definitely/missing.wav"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert!(!missing.stderr.is_empty());
}

#[test]
fn output_alias_with_a_missing_parent_cannot_overwrite_the_input() {
    let root = scratch("output-alias");
    let input = root.join("audio.wav");
    std::fs::copy(fixture(), &input).unwrap();
    let original = std::fs::read(&input).unwrap();
    let output = root.join("scratch/../audio.wav");

    let result = run(
        &root,
        &[
            "transcribe",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ],
    );
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(std::fs::read(input).unwrap(), original);
    assert!(!root.join("scratch").exists());
}

#[test]
fn audio_setup_inference_and_output_failures_exit_one() {
    let root = scratch("runtime-failures");
    let wav = fixture();

    let bad_wav = root.join("bad.wav");
    std::fs::write(&bad_wav, "not a wav").unwrap();
    let audio = run(&root, &["transcribe", bad_wav.to_str().unwrap()]);
    assert_eq!(audio.status.code(), Some(1));
    assert!(audio.stdout.is_empty());
    assert!(!audio.stderr.is_empty());

    let setup = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["transcribe", wav.to_str().unwrap(), "--engine", "whisper"])
        .env("PATH", root.join("empty-path"))
        .env("ECHO_CONFIG_DIR", root.join("setup-config"))
        .env("ECHO_DATA_DIR", root.join("setup-data"))
        .env("ECHO_MODEL_DIR", root.join("setup-models"))
        .env_remove("ECHO_ENGINE")
        .output()
        .unwrap();
    assert_eq!(setup.status.code(), Some(1));
    assert!(setup.stdout.is_empty());
    assert!(!setup.stderr.is_empty());

    let bin_dir = root.join("failing-bin");
    let model_dir = root.join("failing-models");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::create_dir_all(&model_dir).unwrap();
    std::fs::write(model_dir.join("ggml-small.bin"), []).unwrap();
    let runner = bin_dir.join("whisper-cli");
    std::fs::write(
        &runner,
        "#!/bin/sh\nprintf 'decoder failed\\n' >&2\nexit 7\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).unwrap();
    let inference = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            wav.to_str().unwrap(),
            "--engine",
            "whisper",
            "--model",
            "small",
        ])
        .env("PATH", bin_dir)
        .env("ECHO_CONFIG_DIR", root.join("infer-config"))
        .env("ECHO_DATA_DIR", root.join("infer-data"))
        .env("ECHO_MODEL_DIR", model_dir)
        .output()
        .unwrap();
    assert_eq!(inference.status.code(), Some(1));
    assert!(inference.stdout.is_empty());
    assert!(String::from_utf8_lossy(&inference.stderr).contains("decoder failed"));

    let output = run(
        &root,
        &[
            "transcribe",
            wav.to_str().unwrap(),
            "--output",
            root.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn corrupt_config_is_left_in_place_without_side_effects() {
    let root = scratch("corrupt-config");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = config_dir.join("config.json");
    let dictionary = data_dir.join("dictionary.json");
    let dictionary_json =
        r#"{"entries":[{"spoken":"claude code","written":"Claude Code","created_at":1}]}"#;
    std::fs::write(&config, "corrupt config sentinel").unwrap();
    std::fs::write(&dictionary, dictionary_json).unwrap();

    let _ = run(&root, &["transcribe", fixture().to_str().unwrap()]);

    assert_eq!(std::fs::read(&config).unwrap(), b"corrupt config sentinel");
    assert_eq!(
        std::fs::read(&dictionary).unwrap(),
        dictionary_json.as_bytes()
    );
    assert!(!config_dir.join("config.json.corrupt").exists());
    for name in [
        "history.json",
        "status",
        "recording.lock",
        "recording.stop",
        "dictionary.json.corrupt",
    ] {
        assert!(!data_dir.join(name).exists(), "unexpected {name}");
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn count_lines(haystack: &str, needle: &str) -> usize {
    haystack.lines().filter(|line| *line == needle).count()
}

#[cfg(unix)]
#[test]
fn whisper_stub_applies_written_hints_and_retries_without_vad() {
    let root = scratch("whisper-stub");
    let bin_dir = root.join("bin");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    let model_dir = root.join("models");
    for dir in [&bin_dir, &config_dir, &data_dir, &model_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(model_dir.join("ggml-small.bin"), []).unwrap();
    std::fs::write(model_dir.join("ggml-silero-v6.2.0.bin"), []).unwrap();
    std::fs::write(
        data_dir.join("dictionary.json"),
        r#"{"entries":[{"spoken":"clawed code","written":"Claude Code","created_at":1}]}"#,
    )
    .unwrap();
    let runner = bin_dir.join("whisper-cli");
    std::fs::write(
        &runner,
        r#"#!/bin/sh
{
  printf 'BEGIN\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$ECHO_ARGV_LOG"
if [ ! -f "$ECHO_ATTEMPT_FILE" ]; then
  : > "$ECHO_ATTEMPT_FILE"
  printf 'failed to initialize VAD context\n' >&2
  exit 1
fi
printf '%s\n' '{"model":{"type":"small","multilingual":true},"result":{"language":"de"},"transcription":[{"text":" claude code"}]}'
printf '%s\n' 'whisper_full: auto-detected language: de (p = 0.958162)' >&2
"#,
    )
    .unwrap();
    make_executable(&runner);
    let log = root.join("argv.log");
    let attempt = root.join("attempt");

    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "whisper",
            "--model",
            "small",
            "--language",
            "de",
            "--format",
            "json",
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_ARGV_LOG", &log)
        .env("ECHO_ATTEMPT_FILE", &attempt)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .env_remove("ECHO_ENGINE")
        .env_remove("ECHO_LANGUAGE")
        .env_remove("ECHO_WHISPER_MODEL")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let argv = std::fs::read_to_string(&log).unwrap();
    assert_eq!(count_lines(&argv, "BEGIN"), 2, "argv={argv}");
    assert_eq!(count_lines(&argv, "--prompt"), 2, "argv={argv}");
    assert_eq!(count_lines(&argv, "Claude Code"), 2, "argv={argv}");
    assert_eq!(count_lines(&argv, "-l"), 2, "argv={argv}");
    assert_eq!(count_lines(&argv, "de"), 2, "argv={argv}");
    assert_eq!(count_lines(&argv, "--vad"), 1, "argv={argv}");
    assert_eq!(
        argv.lines()
            .filter(|line| line.ends_with("ggml-small.bin"))
            .count(),
        2,
        "argv={argv}"
    );
    assert_eq!(count_lines(&argv, "clawed code"), 0, "argv={argv}");

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["engine"]["id"], "whisper");
    assert_eq!(value["engine"]["model"], "small");
    assert_eq!(value["engine"]["vad"], false);
    assert_eq!(value["language"]["requested"], "de");
    assert_eq!(value["language"]["observed"], "de");
    assert_eq!(value["language"]["probability"], 0.958_162);
    assert_eq!(value["hintCount"], 1);
}

#[test]
fn languages_json_lists_whisper_and_parakeet_catalogs() {
    let root = scratch("languages");
    let config = root.join("config");
    let data = root.join("data");
    let models = root.join("models");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&models).unwrap();

    let whisper = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["languages", "--engine", "whisper", "--format", "json"])
        .env("ECHO_CONFIG_DIR", &config)
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_MODEL_DIR", &models)
        .env_remove("ECHO_ENGINE")
        .env_remove("ECHO_LANGUAGE")
        .env_remove("ECHO_WHISPER_MODEL")
        .output()
        .unwrap();
    assert!(
        whisper.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&whisper.stderr)
    );
    let whisper_json: serde_json::Value = serde_json::from_slice(&whisper.stdout).unwrap();
    assert_eq!(whisper_json["schemaVersion"], 1);
    assert_eq!(whisper_json["engine"], "whisper");
    assert_eq!(whisper_json["languages"].as_array().unwrap().len(), 100);

    let parakeet = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["languages", "--engine", "parakeet", "--format", "json"])
        .env("ECHO_CONFIG_DIR", &config)
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_MODEL_DIR", &models)
        .env_remove("ECHO_ENGINE")
        .env_remove("ECHO_LANGUAGE")
        .env_remove("ECHO_WHISPER_MODEL")
        .output()
        .unwrap();
    assert!(
        parakeet.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&parakeet.stderr)
    );
    let parakeet_json: serde_json::Value = serde_json::from_slice(&parakeet.stdout).unwrap();
    assert_eq!(parakeet_json["selection"], "automatic-only");
    assert_eq!(parakeet_json["languages"].as_array().unwrap().len(), 25);
}

#[cfg(unix)]
#[test]
fn parakeet_stub_uses_nemo_transducer_and_reports_model_path() {
    let root = scratch("parakeet-stub");
    let bin_dir = root.join("bin");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    let model_dir = root.join("models");
    let parakeet_model = model_dir.join("parakeet-tdt-0.6b-v3");
    for dir in [&bin_dir, &config_dir, &data_dir, &parakeet_model] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for name in [
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
    ] {
        std::fs::write(parakeet_model.join(name), "fixture").unwrap();
    }
    let runner = bin_dir.join("sherpa-onnx-offline");
    std::fs::write(
        &runner,
        r#"#!/bin/sh
for arg in "$@"; do printf '%s\n' "$arg"; done > "$ECHO_PARAKEET_ARGV_LOG"
printf '%s\n' '{"lang":"","emotion":"","event":"","text":" parakeet transcript","timestamps":[],"tokens":[],"words":[]}'
"#,
    )
    .unwrap();
    make_executable(&runner);
    let log = root.join("parakeet-argv.log");

    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "parakeet",
            "--language",
            "auto",
            "--format",
            "json",
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_PARAKEET_ARGV_LOG", &log)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .env_remove("ECHO_ENGINE")
        .env_remove("ECHO_LANGUAGE")
        .env_remove("ECHO_WHISPER_MODEL")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let argv = std::fs::read_to_string(&log).unwrap();
    assert!(
        argv.lines()
            .any(|line| line == "--model-type=nemo_transducer"),
        "argv={argv}"
    );

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["raw"], "parakeet transcript");
    assert_eq!(value["text"], "parakeet transcript");
    assert_eq!(value["engine"]["id"], "parakeet");
    assert_eq!(
        value["engine"]["modelPath"],
        parakeet_model.display().to_string()
    );
    assert!(
        !value["raw"].as_str().unwrap().contains('{'),
        "raw={}",
        value["raw"]
    );
}
