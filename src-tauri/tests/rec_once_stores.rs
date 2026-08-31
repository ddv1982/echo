use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/echo/tests/fixtures/claude_code.wav")
}

#[test]
fn rec_once_writes_transcript_stores() {
    let data = std::env::temp_dir().join(format!("echo-hist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
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
    let status_raw = std::fs::read_to_string(data.join("status")).unwrap();
    assert!(status_raw.contains("state="), "status={status_raw:?}");
}
