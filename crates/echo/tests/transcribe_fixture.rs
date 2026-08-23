use echo::audio::load_wav;
use echo::stt::{ModelCache, ParakeetEngine, WhisperEngine};
use echo_core::{DecodeOptions, Engine, LanguageChoice, RecognitionHints};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav")
}

fn has_known_word(raw: &str) -> bool {
    let hay = raw.to_lowercase();
    ["claude", "code", "clawed"]
        .into_iter()
        .any(|word| hay.contains(word))
}

#[test]
#[ignore = "needs cached models under ECHO_MODEL_DIR or $XDG_CACHE_HOME/echo"]
fn transcribe_fixture() {
    let capture = load_wav(&fixture()).expect("fixture wav");
    let cache = ModelCache::from_env();
    let model = cache.inventory().best_whisper().unwrap().name.clone();
    let engines: Vec<Box<dyn Engine>> = vec![
        Box::new(ParakeetEngine::new()),
        Box::new(WhisperEngine::configured(cache, model)),
    ];
    let options = DecodeOptions {
        language: LanguageChoice::Auto,
        hints: RecognitionHints::default(),
    };
    for engine in engines {
        let transcript = engine
            .transcribe(&capture.pcm, &options)
            .expect("cached model and runner");
        eprintln!(
            "engine={} infer_ms={} raw={}",
            transcript.engine, transcript.infer_ms, transcript.raw
        );
        assert!(
            has_known_word(&transcript.raw),
            "expected a known word in {:?}",
            transcript.raw
        );
    }
}
