use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/echo/tests/fixtures/claude_code.wav")
}

#[test]
fn rec_once_drives_recording_then_transcribing() {
    let data = std::env::temp_dir().join(format!("echo-rec-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&data);
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .output()
        .expect("run echo-desktop rec --once");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("session Recording"),
        "stdout={stdout:?} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("session Transcribing"), "stdout={stdout:?}");
}

#[test]
fn rec_once_uses_config_file_engine_when_env_unset() {
    let root = std::env::temp_dir().join(format!("echo-config-engine-{}", std::process::id()));
    let config_dir = root.join("config");
    let data = root.join("data");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(config_dir.join("config.json"), r#"{"engine":"fake"}"#).unwrap();
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env_remove("ECHO_ENGINE")
        .output()
        .expect("run echo-desktop rec --once");
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let history = echo_core::History::load_from(data.join("history.json")).unwrap();
    let text = history
        .rows()
        .iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        text.to_ascii_lowercase().contains("claude code"),
        "history={text:?}"
    );
}

#[test]
fn rec_once_without_engine_fails_engine_missing() {
    let data = std::env::temp_dir().join(format!("echo-rec-noengine-{}", std::process::id()));
    let models = data.join("models");
    let config_dir = data.join("config");
    let _ = std::fs::create_dir_all(&models);
    let _ = std::fs::create_dir_all(&config_dir);
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_MODEL_DIR", &models)
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env_remove("ECHO_ENGINE")
        .output()
        .expect("run echo-desktop rec --once");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "expected failure, stdout={stdout:?}");
    assert!(
        stdout.contains("session Failed speech engine or model missing"),
        "stdout={stdout:?} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rec_once_missing_microphone_name_still_records() {
    let root = std::env::temp_dir().join(format!("echo-config-mic-{}", std::process::id()));
    let config_dir = root.join("config");
    let data = root.join("data");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"engine":"fake","microphone":"no-such-device"}"#,
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env_remove("ECHO_ENGINE")
        .output()
        .expect("run echo-desktop rec --once");
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rec_once_without_mic_names_permission() {
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let mut cmd = Command::new(bin);
    cmd.args(["rec", "--once"]);
    cmd.env_remove("ECHO_AUDIO_FIXTURE");
    cmd.env("ECHO_RECORD_SECONDS", "1");
    let out = cmd.output().expect("run echo-desktop rec --once");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("session Recording"),
        "expected a Recording edge, got {text:?}"
    );
    if !text.contains("session Transcribing") {
        assert!(
            text.contains("microphone"),
            "expected a permission reason, got {text:?}"
        );
    }
}
