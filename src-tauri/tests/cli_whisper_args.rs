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
    let shader_cache_dir = root.join("shader-cache");
    for dir in [
        &bin_dir,
        &config_dir,
        &data_dir,
        &model_dir,
        &shader_cache_dir,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let driver_manifest = root.join("intel_icd.json");
    std::fs::write(&driver_manifest, "{}").unwrap();
    std::fs::write(model_dir.join("ggml-small.bin"), []).unwrap();
    std::fs::write(model_dir.join("ggml-silero-v6.2.0.bin"), []).unwrap();
    std::fs::write(bin_dir.join("libwhisper-fake.so"), b"fake library").unwrap();
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
  printf 'ENV_LD=%s\n' "${LD_LIBRARY_PATH-unset}"
  printf 'ENV_VK=%s\n' "${VK_DRIVER_FILES-unset}"
  printf 'ENV_MESA_DEVICE=%s\n' "${MESA_VK_DEVICE_SELECT-unset}"
  printf 'ENV_DRI=%s\n' "${DRI_PRIME-unset}"
  printf 'ENV_CUDA=%s\n' "${CUDA_VISIBLE_DEVICES-unset}"
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
            "--whisper-vulkan-driver-files",
            driver_manifest.to_str().unwrap(),
            "--whisper-mesa-shader-cache-dir",
            shader_cache_dir.to_str().unwrap(),
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_ARGV_LOG", &log)
        .env("ECHO_ATTEMPT_FILE", &attempt)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .env("LD_LIBRARY_PATH", "/poison")
        .env("VK_DRIVER_FILES", "/poison.json")
        .env("MESA_VK_DEVICE_SELECT", "8086:9a49!")
        .env("DRI_PRIME", "1")
        .env("CUDA_VISIBLE_DEVICES", "0")
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
        assert!(
            run.contains(&format!("ENV_LD={}\n", bin_dir.display())),
            "run={run}"
        );
        assert!(
            run.contains(&format!("ENV_VK={}\n", driver_manifest.display())),
            "run={run}"
        );
        assert!(run.contains("ENV_MESA_DEVICE=unset\n"), "run={run}");
        assert!(run.contains("ENV_DRI=unset\n"), "run={run}");
        assert!(run.contains("ENV_CUDA=unset\n"), "run={run}");
        assert!(!run.contains("clawed code"), "run={run}");
    }
    assert!(runs[0].contains("--vad\n"));
    assert!(runs[0].contains("-vm\n"));
    assert!(!runs[1].contains("--vad\n"));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["engine"]["id"], "whisper");
    assert_eq!(json["engine"]["model"], "small");
    assert_eq!(json["engine"]["vad"], false);
    assert_eq!(
        json["engine"]["vadPath"],
        model_dir
            .join("ggml-silero-v6.2.0.bin")
            .display()
            .to_string()
    );
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
    assert_eq!(
        json["whisper"]["runtime"]["libraryPath"],
        bin_dir.display().to_string()
    );
    assert_eq!(
        json["whisper"]["runtime"]["identitySha256"]
            .as_str()
            .map(str::len),
        Some(64)
    );
    assert_eq!(
        json["whisper"]["runtime"]["vulkanDriverFiles"],
        driver_manifest.display().to_string()
    );
    assert_eq!(
        json["whisper"]["runtime"]["mesaShaderCacheDir"],
        shader_cache_dir.display().to_string()
    );
    assert_eq!(json["whisper"]["tuning"]["threads"], 2);
    assert_eq!(json["whisper"]["tuning"]["beamSize"], 1);
    assert_eq!(json["whisper"]["tuning"]["bestOf"], 3);
    assert_eq!(json["whisper"]["tuning"]["noFallback"], true);
}

#[test]
fn cpu_only_benchmark_flag_reaches_whisper_and_reports_cpu() {
    let root = std::env::temp_dir().join(format!("echo-cli-whisper-cpu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin_dir = root.join("bin");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    let model_dir = root.join("models");
    for dir in [&bin_dir, &config_dir, &data_dir, &model_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(model_dir.join("ggml-base.bin"), []).unwrap();
    let runner = bin_dir.join("whisper-cli");
    std::fs::write(
        &runner,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$ECHO_ARGV_LOG"
printf '%s\n' '{"model":{"type":"base","multilingual":true},"result":{"language":"en"},"transcription":[{"text":" cpu works"}]}'
printf '%s\n' 'whisper_backend_init_gpu: no GPU found' >&2
printf '%s\n' 'whisper_model_load: CPU total size = 59.12 MB' >&2
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).unwrap();
    let log = root.join("argv.log");
    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "whisper",
            "--model",
            "base",
            "--language",
            "en",
            "--format",
            "json",
            "--whisper-no-gpu",
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_ARGV_LOG", &log)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .any(|arg| arg == "--no-gpu"));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["whisper"]["runtime"]["backend"], "cpu");
    assert!(json["whisper"]["runtime"].get("device").is_none());
}

#[test]
fn whisper_acceleration_cpu_forces_no_gpu() {
    let root =
        std::env::temp_dir().join(format!("echo-cli-whisper-accel-cpu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin_dir = root.join("bin");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    let model_dir = root.join("models");
    for dir in [&bin_dir, &config_dir, &data_dir, &model_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(model_dir.join("ggml-small.bin"), []).unwrap();
    let runner = bin_dir.join("whisper-cli");
    std::fs::write(
        &runner,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$ECHO_ARGV_LOG"
printf '%s\n' '{"model":{"type":"small","multilingual":true},"result":{"language":"en"},"transcription":[{"text":" cpu mode"}]}'
printf '%s\n' 'whisper_model_load: CPU total size = 1 MB' >&2
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).unwrap();
    let log = root.join("argv.log");
    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "whisper",
            "--model",
            "small",
            "--language",
            "en",
            "--format",
            "json",
            "--whisper-acceleration",
            "cpu",
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_ARGV_LOG", &log)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .any(|arg| arg == "--no-gpu"));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["whisper"]["runtime"]["backend"], "cpu");
}

#[test]
fn whisper_acceleration_rejects_an_explicit_non_whisper_engine() {
    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "parakeet",
            "--whisper-acceleration",
            "gpu",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Whisper performance options require the Whisper engine"));
}

#[test]
fn cpu_only_benchmark_flag_rejects_an_explicit_non_whisper_engine() {
    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "parakeet",
            "--whisper-no-gpu",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Whisper performance options require the Whisper engine"));
}

#[test]
fn a_refused_gpu_run_still_forces_no_gpu_on_a_system_runtime() {
    // The GPU gate needs the managed CPU runtime as the path a failed
    // accelerated run retreats to, so with only a system whisper-cli it
    // refuses. force_cpu is decided before that refusal and is false for a
    // system runtime, and distributions ship Vulkan-capable whisper.cpp
    // builds, so a refusal used to hand the run to a GPU with no pinned
    // device, no receipt check, and no quarantine.
    let root =
        std::env::temp_dir().join(format!("echo-cli-whisper-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin_dir = root.join("bin");
    let config_dir = root.join("config");
    let data_dir = root.join("data");
    let model_dir = root.join("models");
    for dir in [&bin_dir, &config_dir, &data_dir, &model_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(model_dir.join("ggml-small.bin"), []).unwrap();
    let runner = bin_dir.join("whisper-cli");
    std::fs::write(
        &runner,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$ECHO_ARGV_LOG"
printf '%s\n' '{"model":{"type":"small","multilingual":true},"result":{"language":"en"},"transcription":[{"text":" refused"}]}'
printf '%s\n' 'whisper_model_load: CPU total size = 1 MB' >&2
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).unwrap();
    let log = root.join("argv.log");
    let output = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args([
            "transcribe",
            fixture().to_str().unwrap(),
            "--engine",
            "whisper",
            "--model",
            "small",
            "--language",
            "en",
            "--format",
            "json",
            "--whisper-acceleration",
            "gpu",
        ])
        .env("PATH", &bin_dir)
        .env("ECHO_ARGV_LOG", &log)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env("ECHO_DATA_DIR", &data_dir)
        .env("ECHO_MODEL_DIR", &model_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .any(|arg| arg == "--no-gpu"),
        "a refused GPU run must not leave the runtime free to pick a device"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["whisper"]["runtime"]["backend"], "cpu");
    // No GPU runtime is installed here at all, so that is the reason reported.
    // What matters is that the refusal is stated rather than implied by a
    // backend the user never chose.
    assert_eq!(json["whisper"]["skippedAcceleration"], "runtimeMissing");
}
