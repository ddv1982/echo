use echo::audio::load_wav;
use echo::stt::{ModelCache, ParakeetEngine, WhisperEngine};
use echo_core::{DecodeOptions, Engine, LanguageChoice, RecognitionHints};
use std::path::PathBuf;

#[test]
#[ignore = "needs cached models under ECHO_MODEL_DIR or $XDG_CACHE_HOME/echo"]
fn compare_engines() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
    let capture = load_wav(&path).expect("fixture wav");
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
        match engine.transcribe(&capture.pcm, &options) {
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
