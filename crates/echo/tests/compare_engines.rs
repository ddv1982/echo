use echo::audio::load_wav;
use echo::stt::{ParakeetEngine, WhisperEngine};
use echo_core::Engine;
use std::path::PathBuf;

#[test]
#[ignore = "needs cached models under ECHO_MODEL_DIR or $XDG_CACHE_HOME/echo"]
fn compare_engines() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
    let capture = load_wav(&path).expect("fixture wav");
    let engines: Vec<Box<dyn Engine>> = vec![
        Box::new(ParakeetEngine::new()),
        Box::new(WhisperEngine::new()),
    ];
    for engine in engines {
        match engine.transcribe(&capture.pcm) {
            Ok(transcript) => {
                println!(
                    "engine={} infer_ms={} raw={}",
                    transcript.engine, transcript.infer_ms, transcript.raw
                );
            }
            Err(err) => println!("engine={} error={err}", engine.id()),
        }
    }
}
