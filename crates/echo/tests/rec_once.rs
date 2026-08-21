use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav")
}

#[test]
fn rec_once_drives_recording_then_transcribing() {
    let data = std::env::temp_dir().join(format!("echo-rec-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&data);
    let bin = env!("CARGO_BIN_EXE_echo");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .output()
        .expect("run echo rec --once");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("session Recording"),
        "stdout={stdout:?} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("session Transcribing"), "stdout={stdout:?}");
}

#[test]
fn rec_once_without_mic_names_permission() {
    let bin = env!("CARGO_BIN_EXE_echo");
    let mut cmd = Command::new(bin);
    cmd.args(["rec", "--once"]);
    cmd.env_remove("ECHO_AUDIO_FIXTURE");
    let out = cmd.output().expect("run echo rec --once");
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
            text.contains("microphone") || text.contains("evdev"),
            "expected a permission reason, got {text:?}"
        );
    }
}
