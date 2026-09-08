use std::path::{Path, PathBuf};
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

#[test]
fn rec_once_silence_does_not_write_history() {
    let root = std::env::temp_dir().join(format!(
        "echo-hist-silence-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let wav = root.join("silence.wav");
    write_silence_wav(&wav);
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let rec = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", &wav)
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
    let stdout = String::from_utf8_lossy(&rec.stdout);
    assert!(!stdout.contains("session Injecting"), "stdout={stdout:?}");
    let history = echo_core::History::load_from(data.join("history.json")).unwrap();
    assert!(
        history.rows().is_empty(),
        "history={:?}",
        history
            .rows()
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(root);
}

fn write_silence_wav(path: &Path) {
    let sample_rate = 16_000u32;
    let samples = sample_rate / 5;
    let data_len = samples * 2;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend(b"RIFF");
    buf.extend(&(36 + data_len).to_le_bytes());
    buf.extend(b"WAVEfmt ");
    buf.extend(&16u32.to_le_bytes());
    buf.extend(&1u16.to_le_bytes());
    buf.extend(&1u16.to_le_bytes());
    buf.extend(&sample_rate.to_le_bytes());
    buf.extend(&(sample_rate * 2).to_le_bytes());
    buf.extend(&2u16.to_le_bytes());
    buf.extend(&16u16.to_le_bytes());
    buf.extend(b"data");
    buf.extend(&data_len.to_le_bytes());
    buf.extend(std::iter::repeat_n(0u8, data_len as usize));
    std::fs::write(path, buf).unwrap();
}
