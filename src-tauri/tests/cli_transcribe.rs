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
        .env_remove("ECHO_CLEANUP")
        .output()
        .unwrap()
}

#[test]
fn fake_text_raw_json_and_exact_output_are_stable() {
    let root = scratch("fake-output");
    let wav = fixture();
    let clean = run(&root, &["transcribe", wav.to_str().unwrap()]);
    assert!(
        clean.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert_eq!(clean.stdout, b"Claude code.\n");
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
    assert_eq!(value["text"], "Claude code.");
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
    assert_eq!(std::fs::read(&exact).unwrap(), b"Claude code.\n");
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
        b"Claude code.\n"
    );
}

#[test]
fn file_transcription_is_read_only_and_has_no_recorder_side_effects() {
    let root = scratch("side-effects");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = config_dir.join("config.json");
    let dictionary = data_dir.join("dictionary.json");
    std::fs::write(&config, "corrupt config sentinel").unwrap();
    std::fs::write(&dictionary, "corrupt dictionary sentinel").unwrap();

    let wav = fixture();
    let output = run(&root, &["transcribe", wav.to_str().unwrap()]);
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "corrupt config sentinel"
    );
    assert_eq!(
        std::fs::read_to_string(dictionary).unwrap(),
        "corrupt dictionary sentinel"
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
fn audio_setup_inference_cleanup_and_output_failures_exit_one() {
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

    let cleanup = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["transcribe", wav.to_str().unwrap()])
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_CLEANUP", "local:/definitely/missing-echo-cleaner")
        .env("ECHO_CONFIG_DIR", root.join("cleanup-config"))
        .env("ECHO_DATA_DIR", root.join("cleanup-data"))
        .env("ECHO_MODEL_DIR", root.join("cleanup-models"))
        .output()
        .unwrap();
    assert_eq!(cleanup.status.code(), Some(1));
    assert!(cleanup.stdout.is_empty());
    assert!(!cleanup.stderr.is_empty());

    let raw_without_cleanup = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["transcribe", wav.to_str().unwrap(), "--raw"])
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_CLEANUP", "local:/definitely/missing-echo-cleaner")
        .env("ECHO_CONFIG_DIR", root.join("raw-cleanup-config"))
        .env("ECHO_DATA_DIR", root.join("raw-cleanup-data"))
        .env("ECHO_MODEL_DIR", root.join("raw-cleanup-models"))
        .output()
        .unwrap();
    assert!(
        raw_without_cleanup.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&raw_without_cleanup.stderr)
    );
    assert_eq!(raw_without_cleanup.stdout, b"claude code\n");

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
fn automatic_language_without_observation_disables_english_cleanup() {
    let root = scratch("unknown-language-cleanup");
    let wav = fixture();
    let config = root.join("config");
    let data = root.join("data");
    let models = root.join("models");
    for dir in [&config, &data, &models] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["transcribe", wav.to_str().unwrap(), "--language", "auto"])
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_CONFIG_DIR", config)
        .env("ECHO_DATA_DIR", data)
        .env("ECHO_MODEL_DIR", models)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"claude code\n");
}
