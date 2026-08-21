# Phase 14: model catalog

Back to [overview](overview.md).

## Goal

Echo knows which models are on disk, what each one is capable of, and which one it will use. Backend only.

## Changes

Today `const DEFAULT_MODEL: &str = "base.en"` (`crates/echo/src/stt/whisper.rs:12`) is effectively compile-time. `model_file()` probes `ggml-{model}.bin`, then `{model}.bin`, then `ggml-{model}.gguf` and takes the **first** match (`:54-64`), and `WhisperEngine::with_cache` is the only override with no production caller.

That first-match behaviour is also the most likely explanation for the user's report that transcription quality changed with no code change. Dropping a new file into the cache directory silently changes which weights run, and different Whisper checkpoints produce different silence placeholders. Making the choice explicit and visible is the fix (**principle-fix-root-causes**).

**`crates/echo/src/stt/cache.rs`.** Scan the model directory and return what is there rather than probing for one name. Recognise the GGML naming convention and derive capability from the filename: family, whether the `.en` suffix is present, and the quantisation suffix.

The suffix is not uniform, which is why this needs a table rather than a regex. `tiny`, `base` and `small` use `q5_1`; `medium` and `large` use `q5_0`. There is no `large-v3-q8_0` and no `.en` turbo.

Also detect the Silero VAD models phase 4 introduced, and the Parakeet ONNX directory layout, so one scan answers "what is installed" for the whole app.

**`crates/echo/src/stt/whisper.rs`.** Delete `DEFAULT_MODEL`. The model comes from `Config.whisper_model`, falling back to the best installed model rather than a hardcoded name.

Define "best" explicitly and write the rule down: prefer multilingual over `.en`, then prefer the larger family. A user who has downloaded `small` should not keep getting `base.en` because it sorts first.

**`crates/echo/src/stt/parakeet.rs`.** Fix the availability lie while in the neighbourhood. `available()` gates on `tokens.txt` alone (`:60-62`) while `transcribe` also needs three ONNX files probed at `:74-79`, so `available()` can return true and `transcribe` immediately return `EngineError::Missing`. Check all four.

## Data structures

```
InstalledModel { path, family, multilingual: bool, quantisation: Option<String>, size_bytes }
ModelInventory { whisper: Vec<InstalledModel>, vad: Vec<PathBuf>, parakeet: Option<PathBuf> }
```

`multilingual` is a resolved boolean, not a filename to re-inspect. Deriving it at each use site is how the `n_langs` trap gets stepped in (**principle-model-the-domain**).

Note the ordering dependency with phase 13. Filename-derived `multilingual` is a pre-flight guess used to populate the picker and to refuse an impossible language choice. The **authoritative** value is `model.multilingual` from the JSON, available only after a run. Keep both, and name them differently in the code so nobody confuses them.

## Verification

**Static.** `cargo test --workspace`.

Table-driven unit tests over filenames, against a temp `ECHO_MODEL_DIR` populated with empty files. Cover every naming shape from the real catalog: `ggml-base.en.bin`, `ggml-base.bin`, `ggml-small.en-q5_1.bin`, `ggml-medium-q5_0.bin`, `ggml-large-v3-turbo-q5_0.bin`, `ggml-silero-v6.2.0.bin`. Plus: an empty directory yields an empty inventory rather than an error; an unrecognised filename is ignored, not guessed at.

Test the "best installed" rule directly. A directory with both `ggml-base.en.bin` and `ggml-small.bin` must select `small`.

**Runtime.** Via **control-cli**. With three real models in `ECHO_MODEL_DIR`, run `echo-desktop rec --once` and confirm from the phase 13 JSON that the selected model is the one the rule predicts. Then set `Config.whisper_model` explicitly and confirm it overrides the rule.

Then the case that motivated the phase. Add a second model file to the directory and confirm the selected model does **not** change, because it is now pinned by config rather than by directory-listing order.
