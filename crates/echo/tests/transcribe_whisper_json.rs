use echo::audio::load_wav;
use echo::stt::WhisperEngine;
use echo_core::{Engine, EngineId};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav")
}

fn echo_stt_txt_count() -> usize {
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("echo-stt-") && name.ends_with(".txt"))
        })
        .count()
}

#[test]
#[ignore = "needs cached models under ECHO_MODEL_DIR or $XDG_CACHE_HOME/echo"]
fn transcribe_fixture_uses_stdout_json() {
    let capture = load_wav(&fixture()).expect("fixture wav");
    let engine = WhisperEngine::new();
    let before = echo_stt_txt_count();
    let transcript = engine
        .transcribe(&capture.pcm)
        .expect("cached whisper model and runner");
    let after = echo_stt_txt_count();
    assert_eq!(after, before, "whisper must not write a .txt sidecar");
    assert!(
        matches!(transcript.engine, EngineId::Whisper { .. }),
        "engine={:?}",
        transcript.engine
    );
    let hay = transcript.raw.to_lowercase();
    assert!(
        ["claude", "code", "clawed"]
            .into_iter()
            .any(|word| hay.contains(word)),
        "expected a known word in {:?}",
        transcript.raw
    );
}
