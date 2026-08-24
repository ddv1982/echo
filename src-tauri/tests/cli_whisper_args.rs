use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/echo/tests/fixtures/claude_code.wav")
}

#[test]
fn fake_whisper_proves_model_language_prompt_and_vad_retry_arguments() {
    let root = std::env::temp_dir().join(format!("echo-cli-whisper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
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
printf '%s\n' 'ggml_vulkan: 0 = Test Vulkan GPU (driver) | uma: 0' >&2
printf '%s\n' 'whisper_backend_init_gpu: using Vulkan0 backend' >&2
printf '%s\n' 'whisper_full: auto-detected language: de (p = 0.958162)' >&2
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).unwrap();
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
            "--whisper-threads",
            "2",
            "--whisper-beam-size",
            "1",
            "--whisper-best-of",
            "3",
            "--whisper-no-fallback",
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_ARGV_LOG", &log)
        .env("ECHO_ATTEMPT_FILE", &attempt)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let argv = std::fs::read_to_string(log).unwrap();
    let runs = argv
        .split("BEGIN\n")
        .filter(|run| !run.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(runs.len(), 2, "argv={argv}");
    for run in &runs {
        assert!(run.contains("ggml-small.bin\n"), "run={run}");
        assert!(run.contains("-l\nde\n"), "run={run}");
        assert!(run.contains("-t\n"), "run={run}");
        assert!(run.contains("-t\n2\n"), "run={run}");
        assert!(run.contains("-bs\n1\n"), "run={run}");
        assert!(run.contains("-bo\n3\n"), "run={run}");
        assert!(run.contains("-nf\n"), "run={run}");
        assert!(run.contains("--prompt\nClaude Code\n"), "run={run}");
        assert!(!run.contains("clawed code"), "run={run}");
    }
    assert!(runs[0].contains("--vad\n"));
    assert!(runs[0].contains("-vm\n"));
    assert!(!runs[1].contains("--vad\n"));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["engine"]["id"], "whisper");
    assert_eq!(json["engine"]["model"], "small");
    assert_eq!(json["engine"]["vad"], false);
    assert_eq!(json["language"]["requested"], "de");
    assert_eq!(json["language"]["observed"], "de");
    assert_eq!(json["language"]["probability"], 0.958_162);
    assert_eq!(json["hintCount"], 1);
    assert_eq!(json["whisper"]["mode"], "coldFallback");
    assert_eq!(json["whisper"]["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(json["whisper"]["attempts"][0]["vad"], true);
    assert_eq!(json["whisper"]["attempts"][0]["retryReason"], "vadRejected");
    assert_eq!(json["whisper"]["attempts"][1]["vad"], false);
    assert_eq!(
        json["whisper"]["runtime"]["binary"],
        runner.display().to_string()
    );
    assert_eq!(json["whisper"]["runtime"]["source"], "system");
    assert_eq!(json["whisper"]["runtime"]["backend"], "vulkan");
    assert_eq!(
        json["whisper"]["runtime"]["device"],
        "Test Vulkan GPU (driver)"
    );
    assert_eq!(json["whisper"]["tuning"]["threads"], 2);
    assert_eq!(json["whisper"]["tuning"]["beamSize"], 1);
    assert_eq!(json["whisper"]["tuning"]["bestOf"], 3);
    assert_eq!(json["whisper"]["tuning"]["noFallback"], true);
}
