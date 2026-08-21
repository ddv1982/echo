use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav")
}

#[test]
fn history_survives_relaunch() {
    let data = std::env::temp_dir().join(format!("echo-hist-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let bin = env!("CARGO_BIN_EXE_echo");
    let rec = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(
        rec.status.success(),
        "{}",
        String::from_utf8_lossy(&rec.stderr)
    );
    let hist = Command::new(bin)
        .arg("history")
        .env("ECHO_DATA_DIR", &data)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&hist.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("claude code"),
        "history={stdout:?}"
    );
    let status = Command::new(bin)
        .arg("status")
        .env("ECHO_DATA_DIR", &data)
        .output()
        .unwrap();
    let status_txt = String::from_utf8_lossy(&status.stdout);
    assert!(status_txt.contains("state="), "{status_txt:?}");
}
